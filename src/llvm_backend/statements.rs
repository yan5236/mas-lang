use inkwell::IntPredicate;

use super::*;
use crate::ast::{AssignOp, AssignTarget, BinaryOp, ConditionalBranch, Expr, Stmt};

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 顺序编译一个语句数组，并返回当前控制流是否已经终止。
    pub(super) fn compile_statements(
        &mut self,
        statements: &[Stmt],
    ) -> Result<bool, CompilerError> {
        let mut terminated = false;
        for statement in statements {
            if terminated {
                break;
            }
            terminated = self.compile_statement(statement)?;
        }
        Ok(terminated)
    }

    /// 编译单条语句，并返回当前基本块是否已经结束。
    pub(super) fn compile_statement(&mut self, statement: &Stmt) -> Result<bool, CompilerError> {
        match statement {
            Stmt::Import { .. } | Stmt::ExportImport { .. } => Ok(false),
            Stmt::FunctionDecl { .. } | Stmt::ExternFunctionDecl { .. } => Ok(false),
            Stmt::VarDecl {
                binding_kind,
                name,
                annotation,
                initializer,
                span,
                ..
            } => {
                let expected = annotation.as_ref().map(value_type_from_annotation);
                let value = match (initializer.as_ref(), expected.as_ref()) {
                    (
                        Some(Expr::ListLiteral { elements, .. }),
                        Some(ValueType::List(element_type)),
                    ) if elements.is_empty() => TypedValue {
                        value_type: ValueType::List(element_type.clone()),
                        value: self.build_empty_list(element_type)?.into(),
                    },
                    (
                        Some(Expr::MapLiteral { entries, .. }),
                        Some(ValueType::Map(key_type, value_type)),
                    ) if entries.is_empty() => TypedValue {
                        value_type: ValueType::Map(key_type.clone(), value_type.clone()),
                        value: self.build_empty_map(key_type, value_type)?.into(),
                    },
                    (Some(initializer), _) => {
                        let compiled = self.compile_expr(initializer)?;
                        self.expect_value(compiled)?
                    }
                    (None, Some(ValueType::Array(_, _))) => TypedValue {
                        value_type: expected.clone().expect("前一分支已经确认数组期望类型存在"),
                        value: self
                            .basic_type_for(expected.as_ref().expect("数组类型存在"))?
                            .const_zero(),
                    },
                    (None, _) => {
                        return Err(CompilerError::build_message(
                            "当前声明缺少可落地的初始化值",
                            *span,
                        ));
                    }
                };
                let expected = expected.unwrap_or_else(|| value.value_type.clone());
                if expected != value.value_type {
                    return Err(CompilerError::build_message(
                        format!(
                            "变量 `{name}` 的初始化值无法按 `{}` 落地",
                            expected.display_name()
                        ),
                        *span,
                    ));
                }

                if self.is_top_level_scope() {
                    let variable =
                        self.declare_global(name, expected.clone(), binding_kind.is_mutable())?;
                    self.store_variable(variable, value.value)?;
                } else {
                    let pointer = self.allocate_local(&expected, name)?;
                    self.map_builder(self.builder.build_store(pointer, value.value))?;
                    self.current_scope_mut().insert(
                        name.clone(),
                        VariableInfo {
                            value_type: expected,
                            pointer,
                            is_mutable: binding_kind.is_mutable(),
                        },
                    );
                }

                Ok(false)
            }
            Stmt::Assign {
                target,
                op,
                value,
                span,
            } => match target {
                AssignTarget::Identifier { name, .. } => {
                    let variable = self.lookup_variable(name).ok_or_else(|| {
                        CompilerError::build_message(
                            format!("代码生成阶段找不到变量 `{name}`"),
                            *span,
                        )
                    })?;
                    if !variable.is_mutable {
                        return Err(CompilerError::build_message(
                            format!("常量 `{name}` 不能重新赋值"),
                            *span,
                        ));
                    }

                    let assigned = match op {
                        AssignOp::Assign => {
                            let compiled = self.compile_expr(value)?;
                            self.expect_value(compiled)?
                        }
                        _ => {
                            let current = self.load_variable(variable.clone())?;
                            let compiled = self.compile_expr(value)?;
                            let right = self.expect_value(compiled)?;
                            let binary_op = match op {
                                AssignOp::AddAssign => BinaryOp::Add,
                                AssignOp::SubtractAssign => BinaryOp::Subtract,
                                AssignOp::MultiplyAssign => BinaryOp::Multiply,
                                AssignOp::DivideAssign => BinaryOp::Divide,
                                AssignOp::ModAssign => BinaryOp::Modulo,
                                AssignOp::Assign => unreachable!(),
                            };
                            self.compile_binary_values(binary_op, current, right, *span)?
                        }
                    };

                    if assigned.value_type != variable.value_type {
                        return Err(CompilerError::build_message(
                            "赋值类型无法写入目标变量",
                            *span,
                        ));
                    }
                    self.store_variable(variable, assigned.value)?;
                    Ok(false)
                }
                AssignTarget::Index { target, index, .. } => {
                    let compiled_target = self.compile_expr(target)?;
                    let target_value = self.expect_value(compiled_target)?;
                    let (map_value, key_type, value_type) =
                        self.expect_map_value(target_value, *span)?;
                    let key_compiled = self.compile_expr(index)?;
                    let key = self.expect_value(key_compiled)?;
                    let assigned_compiled = self.compile_expr(value)?;
                    let assigned = self.expect_value(assigned_compiled)?;
                    if key.value_type != key_type {
                        return Err(CompilerError::build_message(
                            "字典键类型与下标表达式不匹配",
                            index.span(),
                        ));
                    }
                    if assigned.value_type != value_type {
                        return Err(CompilerError::build_message(
                            "字典值类型与赋值表达式不匹配",
                            value.span(),
                        ));
                    }
                    self.build_map_set(map_value, &key_type, &value_type, key, assigned, *span)?;
                    Ok(false)
                }
            },
            Stmt::If {
                branches,
                else_branch,
                ..
            } => self.compile_if(branches, else_branch.as_ref()),
            Stmt::While {
                condition, body, ..
            } => self.compile_while(condition, body),
            Stmt::ForRange {
                name,
                start,
                end,
                body,
                span,
            } => self.compile_for_range(name, start, end, body, *span),
            Stmt::ForEach {
                key_name,
                value_name,
                iterable,
                body,
                span,
            } => self.compile_for_each(key_name, value_name.as_deref(), iterable, body, *span),
            Stmt::Break { span } => {
                let loop_context = self.loop_stack.last().ok_or_else(|| {
                    CompilerError::build_message("代码生成阶段遇到了非法的 break", *span)
                })?;
                self.map_builder(
                    self.builder
                        .build_unconditional_branch(loop_context.break_block),
                )?;
                Ok(true)
            }
            Stmt::Continue { span } => {
                let loop_context = self.loop_stack.last().ok_or_else(|| {
                    CompilerError::build_message("代码生成阶段遇到了非法的 continue", *span)
                })?;
                self.map_builder(
                    self.builder
                        .build_unconditional_branch(loop_context.continue_block),
                )?;
                Ok(true)
            }
            Stmt::Return { value, span } => {
                self.compile_return(value.as_ref(), *span)?;
                Ok(true)
            }
            Stmt::ExprStmt { expr, .. } => {
                self.compile_expr(expr)?;
                Ok(false)
            }
        }
    }

    /// 编译 `return` 语句，并对 `unit`/非 `unit` 返回值执行一致性检查。
    pub(super) fn compile_return(
        &mut self,
        value: Option<&Expr>,
        span: Span,
    ) -> Result<(), CompilerError> {
        match (self.current_return_type.clone(), value) {
            (ValueType::Unit, None) => self
                .map_builder(self.builder.build_return(None))
                .map(|_| ()),
            (ValueType::Unit, Some(_)) => Err(CompilerError::build_message(
                "当前函数没有返回值，return 后不能跟表达式",
                span,
            )),
            (_, None) => Err(CompilerError::build_message(
                "当前函数声明了返回值，return 后必须跟表达式",
                span,
            )),
            (expected, Some(expr)) => {
                let compiled = self.compile_expr(expr)?;
                let value = self.expect_value(compiled)?;
                if value.value_type != expected {
                    return Err(CompilerError::build_message("返回值类型不匹配", span));
                }
                self.map_builder(self.builder.build_return(Some(&value.value)))?;
                Ok(())
            }
        }
    }

    /// 编译 `if / elif / else` 控制流，并维护最终汇合块。
    pub(super) fn compile_if(
        &mut self,
        branches: &[ConditionalBranch],
        else_branch: Option<&Vec<Stmt>>,
    ) -> Result<bool, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let mut after_block = None;
        let mut next_test_block = None;
        let mut has_fallthrough = else_branch.is_none();

        for (index, branch) in branches.iter().enumerate() {
            if let Some(block) = next_test_block.take() {
                self.builder.position_at_end(block);
            }

            let compiled = self.compile_expr(&branch.condition)?;
            let condition = self.expect_value(compiled)?;
            let then_block = self.context.append_basic_block(function, "if.then");
            let false_block = if index + 1 < branches.len() || else_branch.is_some() {
                let block = self.context.append_basic_block(function, "if.next");
                next_test_block = Some(block);
                block
            } else {
                *after_block
                    .get_or_insert_with(|| self.context.append_basic_block(function, "if.after"))
            };

            self.map_builder(self.builder.build_conditional_branch(
                condition.value.into_int_value(),
                then_block,
                false_block,
            ))?;
            self.builder.position_at_end(then_block);
            self.push_scope();
            let terminated = self.compile_statements(&branch.body)?;
            self.pop_scope();
            if !terminated && !self.current_block_has_terminator() {
                let target = *after_block
                    .get_or_insert_with(|| self.context.append_basic_block(function, "if.after"));
                self.map_builder(self.builder.build_unconditional_branch(target))?;
                has_fallthrough = true;
            }
        }

        if let Some(else_branch) = else_branch {
            let else_block = next_test_block.expect("存在 else 时必须提前创建 else 块");
            self.builder.position_at_end(else_block);
            self.push_scope();
            let terminated = self.compile_statements(else_branch)?;
            self.pop_scope();
            if !terminated && !self.current_block_has_terminator() {
                let target = *after_block
                    .get_or_insert_with(|| self.context.append_basic_block(function, "if.after"));
                self.map_builder(self.builder.build_unconditional_branch(target))?;
                has_fallthrough = true;
            }
        }

        if has_fallthrough {
            let after_block = *after_block
                .get_or_insert_with(|| self.context.append_basic_block(function, "if.after"));
            self.builder.position_at_end(after_block);
            Ok(false)
        } else {
            Ok(true)
        }
    }

    /// 编译 `while` 循环，并设置 `break/continue` 的目标块。
    pub(super) fn compile_while(
        &mut self,
        condition: &Expr,
        body: &[Stmt],
    ) -> Result<bool, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let header_block = self.context.append_basic_block(function, "while.header");
        let body_block = self.context.append_basic_block(function, "while.body");
        let exit_block = self.context.append_basic_block(function, "while.exit");

        self.map_builder(self.builder.build_unconditional_branch(header_block))?;
        self.builder.position_at_end(header_block);
        let compiled = self.compile_expr(condition)?;
        let condition = self.expect_value(compiled)?;
        self.map_builder(self.builder.build_conditional_branch(
            condition.value.into_int_value(),
            body_block,
            exit_block,
        ))?;

        self.builder.position_at_end(body_block);
        self.loop_stack.push(LoopContext {
            break_block: exit_block,
            continue_block: header_block,
        });
        self.push_scope();
        let terminated = self.compile_statements(body)?;
        self.pop_scope();
        self.loop_stack.pop();
        if !terminated && !self.current_block_has_terminator() {
            self.map_builder(self.builder.build_unconditional_branch(header_block))?;
        }

        self.builder.position_at_end(exit_block);
        Ok(false)
    }

    /// 编译整数半开区间 `for` 循环。
    pub(super) fn compile_for_range(
        &mut self,
        name: &str,
        start: &Expr,
        end: &Expr,
        body: &[Stmt],
        span: Span,
    ) -> Result<bool, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let compiled_start = self.compile_expr(start)?;
        let start_value = self.expect_value(compiled_start)?;
        let compiled_end = self.compile_expr(end)?;
        let end_value = self.expect_value(compiled_end)?;
        if start_value.value_type != ValueType::Int || end_value.value_type != ValueType::Int {
            return Err(CompilerError::build_message("for 范围只支持整数边界", span));
        }

        self.push_scope();
        let iter_pointer = self.allocate_local(&ValueType::Int, &format!("{name}.iter"))?;
        let end_pointer = self.allocate_local(&ValueType::Int, &format!("{name}.end"))?;
        self.map_builder(self.builder.build_store(iter_pointer, start_value.value))?;
        self.map_builder(self.builder.build_store(end_pointer, end_value.value))?;
        self.current_scope_mut().insert(
            name.to_string(),
            VariableInfo {
                value_type: ValueType::Int,
                pointer: iter_pointer,
                is_mutable: true,
            },
        );

        let header_block = self.context.append_basic_block(function, "for.header");
        let body_block = self.context.append_basic_block(function, "for.body");
        let continue_block = self.context.append_basic_block(function, "for.continue");
        let exit_block = self.context.append_basic_block(function, "for.exit");

        self.map_builder(self.builder.build_unconditional_branch(header_block))?;
        self.builder.position_at_end(header_block);
        let current = self
            .map_builder(self.builder.build_load(
                self.context.i64_type(),
                iter_pointer,
                "for.current",
            ))?
            .into_int_value();
        let end_loaded = self
            .map_builder(
                self.builder
                    .build_load(self.context.i64_type(), end_pointer, "for.end"),
            )?
            .into_int_value();
        let condition = self.map_builder(self.builder.build_int_compare(
            IntPredicate::SLT,
            current,
            end_loaded,
            "for.cond",
        ))?;
        self.map_builder(
            self.builder
                .build_conditional_branch(condition, body_block, exit_block),
        )?;

        self.builder.position_at_end(body_block);
        self.loop_stack.push(LoopContext {
            break_block: exit_block,
            continue_block,
        });
        let terminated = self.compile_statements(body)?;
        self.loop_stack.pop();
        if !terminated && !self.current_block_has_terminator() {
            self.map_builder(self.builder.build_unconditional_branch(continue_block))?;
        }

        self.builder.position_at_end(continue_block);
        let iter_now = self
            .map_builder(self.builder.build_load(
                self.context.i64_type(),
                iter_pointer,
                "for.next.load",
            ))?
            .into_int_value();
        let next = self.map_builder(self.builder.build_int_add(
            iter_now,
            self.context.i64_type().const_int(1, false),
            "for.next",
        ))?;
        self.map_builder(self.builder.build_store(iter_pointer, next))?;
        self.map_builder(self.builder.build_unconditional_branch(header_block))?;

        self.builder.position_at_end(exit_block);
        self.pop_scope();
        Ok(false)
    }

    /// 编译 `for item in list` 或 `for key, value in map` 形式的容器遍历循环。
    pub(super) fn compile_for_each(
        &mut self,
        key_name: &str,
        value_name: Option<&str>,
        iterable: &Expr,
        body: &[Stmt],
        span: Span,
    ) -> Result<bool, CompilerError> {
        let compiled_iterable = self.compile_expr(iterable)?;
        let iterable_value = self.expect_value(compiled_iterable)?;
        match iterable_value.value_type.clone() {
            ValueType::List(_) => self.compile_for_each_list(key_name, iterable_value, body, span),
            ValueType::Map(_, _) => {
                let Some(value_name) = value_name else {
                    return Err(CompilerError::build_message(
                        "遍历字典时必须同时声明键和值变量",
                        span,
                    ));
                };
                self.compile_for_each_map(key_name, value_name, iterable_value, body, span)
            }
            _ => Err(CompilerError::build_message(
                "for 遍历当前只支持列表或字典类型",
                span,
            )),
        }
    }

    /// 编译 `for item in list` 形式的列表遍历循环。
    pub(super) fn compile_for_each_list(
        &mut self,
        name: &str,
        iterable_value: TypedValue<'ctx>,
        body: &[Stmt],
        span: Span,
    ) -> Result<bool, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let (list_value, element_type) = self.expect_list_value(iterable_value, span)?;
        self.push_scope();
        let iter_pointer = self.allocate_local(&ValueType::Int, &format!("{name}.iter"))?;
        let item_pointer = self.allocate_local(&element_type, name)?;
        self.map_builder(
            self.builder
                .build_store(iter_pointer, self.context.i64_type().const_zero()),
        )?;
        self.current_scope_mut().insert(
            name.to_string(),
            VariableInfo {
                value_type: element_type.clone(),
                pointer: item_pointer,
                is_mutable: true,
            },
        );

        let header_block = self.context.append_basic_block(function, "foreach.header");
        let body_block = self.context.append_basic_block(function, "foreach.body");
        let continue_block = self
            .context
            .append_basic_block(function, "foreach.continue");
        let exit_block = self.context.append_basic_block(function, "foreach.exit");

        self.map_builder(self.builder.build_unconditional_branch(header_block))?;
        self.builder.position_at_end(header_block);
        let current = self
            .map_builder(self.builder.build_load(
                self.context.i64_type(),
                iter_pointer,
                "foreach.current",
            ))?
            .into_int_value();
        let len_call = self.map_builder(self.builder.build_call(
            self.runtime.list_len,
            &[list_value.into()],
            "foreach.len",
        ))?;
        let len_value = len_call
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let condition = self.map_builder(self.builder.build_int_compare(
            IntPredicate::SLT,
            current,
            len_value,
            "foreach.cond",
        ))?;
        self.map_builder(
            self.builder
                .build_conditional_branch(condition, body_block, exit_block),
        )?;

        self.builder.position_at_end(body_block);
        let item_value = self.build_list_get(list_value, &element_type, current, span)?;
        self.map_builder(self.builder.build_store(item_pointer, item_value.value))?;
        self.loop_stack.push(LoopContext {
            break_block: exit_block,
            continue_block,
        });
        let terminated = self.compile_statements(body)?;
        self.loop_stack.pop();
        if !terminated && !self.current_block_has_terminator() {
            self.map_builder(self.builder.build_unconditional_branch(continue_block))?;
        }

        self.builder.position_at_end(continue_block);
        let iter_now = self
            .map_builder(self.builder.build_load(
                self.context.i64_type(),
                iter_pointer,
                "foreach.next.load",
            ))?
            .into_int_value();
        let next = self.map_builder(self.builder.build_int_add(
            iter_now,
            self.context.i64_type().const_int(1, false),
            "foreach.next",
        ))?;
        self.map_builder(self.builder.build_store(iter_pointer, next))?;
        self.map_builder(self.builder.build_unconditional_branch(header_block))?;

        self.builder.position_at_end(exit_block);
        self.pop_scope();
        Ok(false)
    }

    /// 编译 `for key, value in map` 形式的字典遍历循环。
    pub(super) fn compile_for_each_map(
        &mut self,
        key_name: &str,
        value_name: &str,
        iterable_value: TypedValue<'ctx>,
        body: &[Stmt],
        span: Span,
    ) -> Result<bool, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let (map_value, key_type, value_type) = self.expect_map_value(iterable_value, span)?;
        let keys_call = self.map_builder(self.builder.build_call(
            self.runtime.map_keys,
            &[map_value.into()],
            "foreach.map.keys",
        ))?;
        let keys_list = keys_call
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();

        self.push_scope();
        let iter_pointer = self.allocate_local(&ValueType::Int, &format!("{key_name}.iter"))?;
        let key_pointer = self.allocate_local(&key_type, key_name)?;
        let value_pointer = self.allocate_local(&value_type, value_name)?;
        self.map_builder(
            self.builder
                .build_store(iter_pointer, self.context.i64_type().const_zero()),
        )?;
        self.current_scope_mut().insert(
            key_name.to_string(),
            VariableInfo {
                value_type: key_type.clone(),
                pointer: key_pointer,
                is_mutable: true,
            },
        );
        self.current_scope_mut().insert(
            value_name.to_string(),
            VariableInfo {
                value_type: value_type.clone(),
                pointer: value_pointer,
                is_mutable: true,
            },
        );

        let header_block = self
            .context
            .append_basic_block(function, "foreach.map.header");
        let body_block = self
            .context
            .append_basic_block(function, "foreach.map.body");
        let continue_block = self
            .context
            .append_basic_block(function, "foreach.map.continue");
        let exit_block = self
            .context
            .append_basic_block(function, "foreach.map.exit");

        self.map_builder(self.builder.build_unconditional_branch(header_block))?;
        self.builder.position_at_end(header_block);
        let current = self
            .map_builder(self.builder.build_load(
                self.context.i64_type(),
                iter_pointer,
                "foreach.map.current",
            ))?
            .into_int_value();
        let len_call = self.map_builder(self.builder.build_call(
            self.runtime.list_len,
            &[keys_list.into()],
            "foreach.map.len",
        ))?;
        let len_value = len_call
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let condition = self.map_builder(self.builder.build_int_compare(
            IntPredicate::SLT,
            current,
            len_value,
            "foreach.map.cond",
        ))?;
        self.map_builder(
            self.builder
                .build_conditional_branch(condition, body_block, exit_block),
        )?;

        self.builder.position_at_end(body_block);
        let key_value = self.build_list_get(keys_list, &key_type, current, span)?;
        self.map_builder(self.builder.build_store(key_pointer, key_value.value))?;
        let value_value = self.build_map_get(map_value, &key_type, &value_type, key_value, span)?;
        self.map_builder(self.builder.build_store(value_pointer, value_value.value))?;
        self.loop_stack.push(LoopContext {
            break_block: exit_block,
            continue_block,
        });
        let terminated = self.compile_statements(body)?;
        self.loop_stack.pop();
        if !terminated && !self.current_block_has_terminator() {
            self.map_builder(self.builder.build_unconditional_branch(continue_block))?;
        }

        self.builder.position_at_end(continue_block);
        let iter_now = self
            .map_builder(self.builder.build_load(
                self.context.i64_type(),
                iter_pointer,
                "foreach.map.next.load",
            ))?
            .into_int_value();
        let next = self.map_builder(self.builder.build_int_add(
            iter_now,
            self.context.i64_type().const_int(1, false),
            "foreach.map.next",
        ))?;
        self.map_builder(self.builder.build_store(iter_pointer, next))?;
        self.map_builder(self.builder.build_unconditional_branch(header_block))?;

        self.builder.position_at_end(exit_block);
        self.pop_scope();
        Ok(false)
    }
}
