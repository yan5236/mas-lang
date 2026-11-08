use crate::ast::{AssignOp, AssignTarget, BindingKind, Stmt};
use crate::diagnostic::CompilerError;

use super::{SemanticAnalyzer, SymbolInfo, ValueType};

impl<'a> SemanticAnalyzer<'a> {
    /// 分析单条语句并更新作用域与控制流上下文；赋值检查绑定或容器键值类型，不匹配时返回源码诊断。
    pub(super) fn analyze_statement(&mut self, statement: &Stmt) -> Result<(), CompilerError> {
        match statement {
            Stmt::Import { span, .. } | Stmt::ExportImport { span, .. } => Err(
                CompilerError::semantic_message("模块导入应在语义分析前完成展开", *span),
            ),
            Stmt::StructDecl { span, .. }
            | Stmt::FunctionDecl { span, .. }
            | Stmt::ExternFunctionDecl { span, .. } => Err(CompilerError::semantic_message(
                "函数和结构体定义只能出现在顶层",
                *span,
            )),
            Stmt::VarDecl {
                binding_kind,
                name,
                annotation,
                initializer,
                span,
                ..
            } => {
                let expected = if let Some(annotation) = annotation {
                    Some(self.value_type_from_annotation(annotation, *span)?)
                } else {
                    None
                };
                let value_type = if let Some(initializer) = initializer {
                    self.infer_expression_type_with_hint(initializer, expected.as_ref())?
                } else {
                    if *binding_kind == BindingKind::Const {
                        return Err(CompilerError::semantic_message(
                            "常量声明必须提供初始化表达式",
                            *span,
                        ));
                    }
                    match expected.clone() {
                        Some(ValueType::Array(_, _)) => {
                            expected.clone().expect("前一分支已经确认数组类型存在")
                        }
                        Some(_) => {
                            return Err(CompilerError::semantic_message(
                                "当前只有数组类型允许省略初始化表达式",
                                *span,
                            ));
                        }
                        None => {
                            return Err(CompilerError::semantic_message(
                                "省略初始化表达式时必须提供数组类型标注",
                                *span,
                            ));
                        }
                    }
                };
                if value_type == ValueType::Null {
                    return Err(CompilerError::semantic_message(
                        "null 需要可空类型标注，例如 `var age: int? = null`",
                        *span,
                    ));
                }
                if self.structs.contains_key(name) || self.current_scope().contains_key(name) {
                    return Err(CompilerError::semantic_message(
                        format!("{} `{name}` 在当前作用域中重复声明", binding_kind.keyword()),
                        *span,
                    ));
                }

                if let Some(expected) = expected {
                    if expected != value_type {
                        return Err(CompilerError::semantic_message(
                            format!(
                                "{} `{name}` 的类型与初始化表达式不匹配",
                                binding_kind.keyword()
                            ),
                            *span,
                        ));
                    }
                }

                self.current_scope_mut()
                    .insert(name.clone(), SymbolInfo::new(value_type, *binding_kind));
                Ok(())
            }
            Stmt::Assign {
                target,
                op,
                value,
                span,
            } => match target {
                AssignTarget::Field { target, name, .. } => {
                    let expected = self.infer_field_type(target, name, *span)?;
                    let actual = self.infer_expression_type_with_hint(value, Some(&expected))?;
                    if self.infer_assignment_type(&expected, &actual, *op, *span)? != expected {
                        return Err(CompilerError::semantic_message(
                            "结构体字段赋值类型不匹配",
                            *span,
                        ));
                    }
                    Ok(())
                }
                AssignTarget::Identifier { name, .. } => {
                    let symbol = self.lookup(name).ok_or_else(|| {
                        CompilerError::semantic_message(format!("未声明变量 `{name}`"), *span)
                    })?;
                    if !symbol.is_mutable {
                        return Err(CompilerError::semantic_message(
                            format!("常量 `{name}` 不能重新赋值"),
                            *span,
                        ));
                    }
                    let expected = symbol.value_type.clone();
                    let actual = self.infer_expression_type_with_hint(value, Some(&expected))?;
                    let result = self.infer_assignment_type(&expected, &actual, *op, *span)?;
                    if expected != result {
                        return Err(CompilerError::semantic_message(
                            format!("变量 `{name}` 的赋值类型不匹配"),
                            *span,
                        ));
                    }
                    Ok(())
                }
                AssignTarget::Index { target, index, .. } => {
                    if *op != AssignOp::Assign {
                        return Err(CompilerError::semantic_message(
                            "当前阶段下标赋值只支持 `=`",
                            *span,
                        ));
                    }
                    let target_type = self.infer_expression_type(target)?;
                    let (key_type, value_type, key_error, value_error) = match target_type {
                        ValueType::Map(key, value) => (
                            key,
                            value,
                            "字典键类型与下标表达式不匹配",
                            "字典值类型与赋值表达式不匹配",
                        ),
                        ValueType::List(element) | ValueType::Array(element, _) => (
                            Box::new(ValueType::Int),
                            element,
                            "列表或数组下标必须是整数",
                            "容器元素类型与赋值表达式不匹配",
                        ),
                        _ => {
                            return Err(CompilerError::semantic_message(
                                "下标赋值只支持列表、数组或字典类型",
                                *span,
                            ));
                        }
                    };
                    let actual_key =
                        self.infer_expression_type_with_hint(index, Some(key_type.as_ref()))?;
                    if actual_key != *key_type {
                        return Err(CompilerError::semantic_message(key_error, index.span()));
                    }
                    let actual_value =
                        self.infer_expression_type_with_hint(value, Some(value_type.as_ref()))?;
                    if actual_value != *value_type {
                        return Err(CompilerError::semantic_message(value_error, value.span()));
                    }
                    Ok(())
                }
            },
            Stmt::If {
                branches,
                else_branch,
                ..
            } => {
                for branch in branches {
                    let condition_type = self.infer_expression_type(&branch.condition)?;
                    if condition_type != ValueType::Bool {
                        return Err(CompilerError::semantic_message(
                            "条件表达式必须是布尔值",
                            branch.condition.span(),
                        ));
                    }
                    self.analyze_block(&branch.body)?;
                }

                if let Some(else_branch) = else_branch {
                    self.analyze_block(else_branch)?;
                }

                Ok(())
            }
            Stmt::While {
                condition,
                body,
                span,
                ..
            } => {
                if self.infer_expression_type(condition)? != ValueType::Bool {
                    return Err(CompilerError::semantic_message(
                        "条件表达式必须是布尔值",
                        *span,
                    ));
                }
                self.loop_depth += 1;
                let result = self.analyze_block(body);
                self.loop_depth -= 1;
                result
            }
            Stmt::ForRange {
                name,
                start,
                end,
                body,
                span,
            } => {
                if self.infer_expression_type(start)? != ValueType::Int
                    || self.infer_expression_type(end)? != ValueType::Int
                {
                    return Err(CompilerError::semantic_message(
                        "for 范围的起点和终点必须是整数",
                        *span,
                    ));
                }

                self.loop_depth += 1;
                self.push_scope();
                self.current_scope_mut().insert(
                    name.clone(),
                    SymbolInfo::new(ValueType::Int, BindingKind::Var),
                );
                let result = self.analyze_statements(body);
                self.pop_scope();
                self.loop_depth -= 1;
                result
            }
            Stmt::ForEach {
                key_name,
                value_name,
                iterable,
                body,
                span,
            } => {
                let iterable_type = self.infer_expression_type(iterable)?;
                self.loop_depth += 1;
                self.push_scope();
                match iterable_type {
                    ValueType::List(element_type) => {
                        if value_name.is_some() {
                            self.pop_scope();
                            self.loop_depth -= 1;
                            return Err(CompilerError::semantic_message(
                                "列表遍历只能声明一个循环变量",
                                *span,
                            ));
                        }
                        self.current_scope_mut().insert(
                            key_name.clone(),
                            SymbolInfo::new((*element_type).clone(), BindingKind::Var),
                        );
                    }
                    ValueType::Map(key_type, value_type) => {
                        let Some(value_name) = value_name else {
                            self.pop_scope();
                            self.loop_depth -= 1;
                            return Err(CompilerError::semantic_message(
                                "遍历字典时必须同时声明键和值变量",
                                *span,
                            ));
                        };
                        self.current_scope_mut().insert(
                            key_name.clone(),
                            SymbolInfo::new((*key_type).clone(), BindingKind::Var),
                        );
                        self.current_scope_mut().insert(
                            value_name.clone(),
                            SymbolInfo::new((*value_type).clone(), BindingKind::Var),
                        );
                    }
                    _ => {
                        self.pop_scope();
                        self.loop_depth -= 1;
                        return Err(CompilerError::semantic_message(
                            "for 遍历当前只支持列表或字典类型",
                            *span,
                        ));
                    }
                }
                let result = self.analyze_statements(body);
                self.pop_scope();
                self.loop_depth -= 1;
                result
            }
            Stmt::Break { span } | Stmt::Continue { span } => {
                if self.loop_depth == 0 {
                    return Err(CompilerError::semantic_message(
                        "break 和 continue 只能出现在循环体中",
                        *span,
                    ));
                }
                Ok(())
            }
            Stmt::Return { value, span } => {
                let Some(expected_return_type) = self.current_return_type.clone() else {
                    return Err(CompilerError::semantic_message(
                        "return 只能出现在函数体中",
                        *span,
                    ));
                };

                match (expected_return_type, value) {
                    (ValueType::Unit, None) => Ok(()),
                    (ValueType::Unit, Some(_)) => Err(CompilerError::semantic_message(
                        "当前函数没有返回值，return 后不能跟表达式",
                        *span,
                    )),
                    (_, None) => Err(CompilerError::semantic_message(
                        "当前函数声明了返回值，return 后必须跟表达式",
                        *span,
                    )),
                    (expected, Some(expr)) => {
                        let actual = self.infer_expression_type_with_hint(expr, Some(&expected))?;
                        if expected != actual {
                            return Err(CompilerError::semantic_message("返回值类型不匹配", *span));
                        }
                        Ok(())
                    }
                }
            }
            Stmt::ExprStmt { expr, .. } => {
                self.infer_expression_type(expr)?;
                Ok(())
            }
        }
    }

    /// 分析一组语句，并保持块级作用域的生命周期正确结束。
    pub(super) fn analyze_block(&mut self, statements: &[Stmt]) -> Result<(), CompilerError> {
        self.push_scope();
        let result = self.analyze_statements(statements);
        self.pop_scope();
        result
    }

    /// 顺序分析一个语句数组，直到全部通过或遇到第一条错误语句。
    pub(super) fn analyze_statements(&mut self, statements: &[Stmt]) -> Result<(), CompilerError> {
        for statement in statements {
            self.analyze_statement(statement)?;
        }
        Ok(())
    }
}
