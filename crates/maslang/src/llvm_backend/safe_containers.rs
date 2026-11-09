use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, IntValue, PointerValue};
use inkwell::IntPredicate;

use super::*;
use crate::ast::{Expr, MethodName};

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译列表安全方法（`try_get`/`try_set`/`try_pop`/`try_index`/`try_remove`/`try_slice`）。
    /// 失败必须返回 `ok=false` 或 `false`，不得触发运行时错误，也不得改动原列表。
    pub(super) fn compile_list_safe_method(
        &mut self,
        target_value: TypedValue<'ctx>,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let (list_value, element_type) = self.expect_list_value(target_value, span)?;
        match method {
            MethodName::TryGet => {
                self.compile_list_try_get(list_value, &element_type, arguments, span)
            }
            MethodName::TrySet => {
                self.compile_list_try_set(list_value, &element_type, arguments, span)
            }
            MethodName::TryPop => {
                self.compile_list_try_pop(list_value, &element_type, arguments, span)
            }
            MethodName::TryIndex => {
                self.compile_list_try_index(list_value, &element_type, arguments, span)
            }
            MethodName::TryRemove => {
                self.compile_list_try_remove(list_value, &element_type, arguments, span)
            }
            MethodName::TrySlice => {
                self.compile_list_try_slice(list_value, &element_type, arguments, span)
            }
            _ => Err(CompilerError::build_message(
                format!(
                    "列表安全方法分派收到不支持的方法 `{}`",
                    method.display_name()
                ),
                span,
            )),
        }
    }

    /// 编译固定数组安全方法（`try_get`/`try_set`）；越界返回失败状态且不访问元素地址。
    pub(super) fn compile_array_safe_method(
        &mut self,
        target: &Expr,
        target_value: TypedValue<'ctx>,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let ValueType::Array(element_type, size) = target_value.value_type.clone() else {
            return Err(CompilerError::build_message(
                "当前表达式不是固定数组类型",
                span,
            ));
        };
        match method {
            MethodName::TryGet => {
                self.expect_safe_argument_count(arguments, 1, MethodName::TryGet, span)?;
                let index = self.compile_safe_index_argument(&arguments[0])?;
                let pointer = self.resolve_array_pointer(target, target_value)?;
                let valid = self.build_array_in_range_check(index, size, "safe.array.get")?;
                let element = element_type.as_ref().clone();
                let result = self.build_guarded_result(
                    valid,
                    element_type.as_ref(),
                    "safe.array.get",
                    |this| {
                        let slot =
                            this.build_array_element_pointer(pointer, &element, size, index)?;
                        Ok(this.map_builder(this.builder.build_load(
                            this.basic_type_for(&element)?,
                            slot,
                            "load.array.try",
                        ))?)
                    },
                )?;
                Ok(CodegenValue::Value(result))
            }
            MethodName::TrySet => {
                self.expect_safe_argument_count(arguments, 2, MethodName::TrySet, span)?;
                // 先固定写入地址再求值实参，与下标赋值一致：右值里的重新赋值不会改变写入目标。
                let pointer = self.resolve_array_write_pointer(target, &target_value)?;
                let index = self.compile_safe_index_argument(&arguments[0])?;
                let assigned = self.compile_expected(&arguments[1], element_type.as_ref())?;
                let valid = self.build_array_in_range_check(index, size, "safe.array.set")?;
                let element = element_type.as_ref().clone();
                let result = self.build_guarded_action(valid, "safe.array.set", |this| {
                    let slot = this.build_array_element_pointer(pointer, &element, size, index)?;
                    this.map_builder(this.builder.build_store(slot, assigned.value))?;
                    Ok(())
                })?;
                Ok(CodegenValue::Value(result))
            }
            _ => Err(CompilerError::build_message(
                format!(
                    "固定数组安全方法分派收到不支持的方法 `{}`",
                    method.display_name()
                ),
                span,
            )),
        }
    }

    /// `list.try_get(i)`：`0 <= i < len` 才读取，失败返回 `ok=false` 且容器保持原样。
    fn compile_list_try_get(
        &mut self,
        list: PointerValue<'ctx>,
        element_type: &ValueType,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        self.expect_safe_argument_count(arguments, 1, MethodName::TryGet, span)?;
        let index = self.compile_safe_index_argument(&arguments[0])?;
        let len = self.build_list_length(list)?;
        let valid = self.build_list_in_range_check(index, len, "safe.get")?;
        let element = element_type.clone();
        let result = self.build_guarded_result(valid, element_type, "safe.get", |this| {
            let function = this.list_runtime_function("list_get", &element)?;
            let value = this.build_list_result_from_call(
                function,
                &[list.into(), index.into()],
                &element,
                "call.list_get",
            )?;
            Ok(value.value)
        })?;
        Ok(CodegenValue::Value(result))
    }

    /// `list.try_set(i, v)`：下标合法才写入，失败返回 `false` 且元素保持原值。
    /// 元素值先求值再检查长度，与下标赋值一致：右值合法扩容后仍按当前长度判断。
    fn compile_list_try_set(
        &mut self,
        list: PointerValue<'ctx>,
        element_type: &ValueType,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        self.expect_safe_argument_count(arguments, 2, MethodName::TrySet, span)?;
        let index = self.compile_safe_index_argument(&arguments[0])?;
        let assigned = self.compile_expected(&arguments[1], element_type)?;
        let len = self.build_list_length(list)?;
        let valid = self.build_list_in_range_check(index, len, "safe.set")?;
        let element = element_type.clone();
        let result = self.build_guarded_action(valid, "safe.set", |this| {
            let function = this.list_runtime_function("list_set", &element)?;
            let mut args = vec![list.into(), index.into()];
            args.extend(this.build_list_value_arguments(assigned, span)?);
            this.map_builder(this.builder.build_call(function, &args, "call.list_set"))?;
            Ok(())
        })?;
        Ok(CodegenValue::Value(result))
    }

    /// `list.try_pop()` 弹出末元素，`list.try_pop(i)` 弹出指定下标；空列表或越界返回 `ok=false` 且不删除。
    fn compile_list_try_pop(
        &mut self,
        list: PointerValue<'ctx>,
        element_type: &ValueType,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if arguments.len() > 1 {
            return Err(CompilerError::build_message(
                "列表方法 `try_pop` 最多接受一个整数下标",
                span,
            ));
        }
        let int = self.context.i64_type();
        let len = self.build_list_length(list)?;
        let (valid, index) = match arguments.first() {
            Some(argument) => {
                let index = self.compile_safe_index_argument(argument)?;
                (self.build_list_in_range_check(index, len, "safe.pop")?, index)
            }
            None => {
                // 空列表没有末元素：先用无符号比较排除长度为零，再计算 `len - 1`；
                // 减一结果只在合法分支被使用，不会以负下标调用运行时。
                let has_element = self.map_builder(self.builder.build_int_compare(
                    IntPredicate::UGT,
                    len,
                    int.const_zero(),
                    "safe.pop.has_element",
                ))?;
                let index = self.map_builder(self.builder.build_int_sub(
                    len,
                    int.const_int(1, false),
                    "safe.pop.last",
                ))?;
                (has_element, index)
            }
        };
        let element = element_type.clone();
        let result = self.build_guarded_result(valid, element_type, "safe.pop", |this| {
            let function = this.list_runtime_function("list_pop_at", &element)?;
            let value = this.build_list_result_from_call(
                function,
                &[list.into(), index.into()],
                &element,
                "call.list_pop_at",
            )?;
            Ok(value.value)
        })?;
        Ok(CodegenValue::Value(result))
    }

    /// `list.try_index(v)`：先用 `contains` 判断是否命中，命中后再取下标。
    /// 两次查找换来“未命中不报错”；扫描期间不修改列表，代价是命中时多遍历一遍。
    fn compile_list_try_index(
        &mut self,
        list: PointerValue<'ctx>,
        element_type: &ValueType,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        self.expect_safe_argument_count(arguments, 1, MethodName::TryIndex, span)?;
        let argument = self.compile_expected(&arguments[0], element_type)?;
        let mut args = vec![list.into()];
        args.extend(self.build_list_value_arguments(argument, span)?);
        let hit = self.build_list_contains_call(element_type, &args)?;
        let element = element_type.clone();
        let result = self.build_guarded_result(hit, &ValueType::Int, "safe.index", |this| {
            let function = this.list_runtime_function("list_index", &element)?;
            let call = this.map_builder(this.builder.build_call(function, &args, "call.list_index"))?;
            Ok(call.try_as_basic_value().unwrap_basic())
        })?;
        Ok(CodegenValue::Value(result))
    }

    /// `list.try_remove(v)`：未命中返回 `false` 且不删除；命中时删除第一个匹配元素。
    /// 与 `try_index` 相同，用 `contains` 预判后再执行会报错的删除入口。
    fn compile_list_try_remove(
        &mut self,
        list: PointerValue<'ctx>,
        element_type: &ValueType,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        self.expect_safe_argument_count(arguments, 1, MethodName::TryRemove, span)?;
        let argument = self.compile_expected(&arguments[0], element_type)?;
        let mut args = vec![list.into()];
        args.extend(self.build_list_value_arguments(argument, span)?);
        let hit = self.build_list_contains_call(element_type, &args)?;
        let element = element_type.clone();
        let result = self.build_guarded_action(hit, "safe.remove", |this| {
            let function = this.list_runtime_function("list_remove", &element)?;
            this.map_builder(this.builder.build_call(function, &args, "call.list_remove"))?;
            Ok(())
        })?;
        Ok(CodegenValue::Value(result))
    }

    /// `list.try_slice(a, b)`：`0 <= a <= b <= len` 才切出副本，失败返回 `ok=false`。
    /// 副本元素槽位被复制，结构体/容器元素仍共享原对象引用。
    fn compile_list_try_slice(
        &mut self,
        list: PointerValue<'ctx>,
        element_type: &ValueType,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        self.expect_safe_argument_count(arguments, 2, MethodName::TrySlice, span)?;
        let start = self.compile_safe_index_argument(&arguments[0])?;
        let end = self.compile_safe_index_argument(&arguments[1])?;
        let int = self.context.i64_type();
        let len = self.build_list_length(list)?;
        // 先排除负起点；起点非负时无符号比较 `a <= b <= len` 同时保证终点非负且不越界。
        let start_non_negative = self.map_builder(self.builder.build_int_compare(
            IntPredicate::SGE,
            start,
            int.const_zero(),
            "safe.slice.start_ok",
        ))?;
        let ordered = self.map_builder(self.builder.build_int_compare(
            IntPredicate::ULE,
            start,
            end,
            "safe.slice.ordered",
        ))?;
        let within = self.map_builder(self.builder.build_int_compare(
            IntPredicate::ULE,
            end,
            len,
            "safe.slice.within",
        ))?;
        let ordered_within =
            self.map_builder(self.builder.build_and(ordered, within, "safe.slice.range"))?;
        let valid = self.map_builder(self.builder.build_and(
            start_non_negative,
            ordered_within,
            "safe.slice.valid",
        ))?;
        let payload = ValueType::List(Box::new(element_type.clone()));
        let result = self.build_guarded_result(valid, &payload, "safe.slice", |this| {
            let call = this.map_builder(this.builder.build_call(
                this.runtime.list_slice,
                &[list.into(), start.into(), end.into()],
                "call.list_slice",
            ))?;
            Ok(call.try_as_basic_value().unwrap_basic())
        })?;
        Ok(CodegenValue::Value(result))
    }

    /// 调用运行时 `list_contains_*` 并返回内部布尔标记；实参已按元素类型拆包。
    fn build_list_contains_call(
        &mut self,
        element_type: &ValueType,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let function = self.list_runtime_function("list_contains", element_type)?;
        let call = self.map_builder(self.builder.build_call(function, args, "call.list_contains"))?;
        let raw = call.try_as_basic_value().unwrap_basic().into_int_value();
        self.map_builder(self.builder.build_int_compare(
            IntPredicate::NE,
            raw,
            self.context.i8_type().const_zero(),
            "safe.contains.hit",
        ))
    }

    /// 读取列表当前长度；运行时的长度值以 i64 位模式返回。
    fn build_list_length(&mut self, list: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CompilerError> {
        let call = self.map_builder(self.builder.build_call(
            self.runtime.list_len,
            &[list.into()],
            "safe.list.len",
        ))?;
        Ok(call.try_as_basic_value().unwrap_basic().into_int_value())
    }

    /// 判定列表下标可安全访问：先按有符号比较排除负数，再用无符号比较限制上界。
    fn build_list_in_range_check(
        &mut self,
        index: IntValue<'ctx>,
        len: IntValue<'ctx>,
        prefix: &str,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let int = self.context.i64_type();
        let non_negative = self.map_builder(self.builder.build_int_compare(
            IntPredicate::SGE,
            index,
            int.const_zero(),
            &format!("{prefix}.non_negative"),
        ))?;
        let in_bounds = self.map_builder(self.builder.build_int_compare(
            IntPredicate::ULT,
            index,
            len,
            &format!("{prefix}.in_bounds"),
        ))?;
        self.map_builder(
            self.builder
                .build_and(non_negative, in_bounds, &format!("{prefix}.valid")),
        )
    }

    /// 判定固定数组下标可安全访问：`0 <= i < N`，`N` 是编译期常量。
    fn build_array_in_range_check(
        &mut self,
        index: IntValue<'ctx>,
        size: usize,
        prefix: &str,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let int = self.context.i64_type();
        let non_negative = self.map_builder(self.builder.build_int_compare(
            IntPredicate::SGE,
            index,
            int.const_zero(),
            &format!("{prefix}.non_negative"),
        ))?;
        let in_bounds = self.map_builder(self.builder.build_int_compare(
            IntPredicate::ULT,
            index,
            int.const_int(size as u64, false),
            &format!("{prefix}.in_bounds"),
        ))?;
        self.map_builder(
            self.builder
                .build_and(non_negative, in_bounds, &format!("{prefix}.valid")),
        )
    }

    /// 编译安全方法的下标实参；语义阶段已校验为整数，这里保留显式检查避免绕过前端的调用。
    fn compile_safe_index_argument(
        &mut self,
        argument: &Expr,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let compiled = self.compile_expr(argument)?;
        let value = self.expect_value(compiled)?;
        if value.value_type != ValueType::Int {
            return Err(CompilerError::build_message(
                "安全方法的下标参数必须是整数",
                argument.span(),
            ));
        }
        Ok(value.value.into_int_value())
    }

    /// 校验安全方法的实参数量；语义阶段已检查，重复校验用于保护直接调用代码生成入口的场景。
    fn expect_safe_argument_count(
        &self,
        arguments: &[Expr],
        expected: usize,
        method: MethodName,
        span: Span,
    ) -> Result<(), CompilerError> {
        if arguments.len() == expected {
            return Ok(());
        }
        Err(CompilerError::build_message(
            format!(
                "安全方法 `{}` 需要 {expected} 个参数，但实际传入 {}",
                method.display_name(),
                arguments.len()
            ),
            span,
        ))
    }

    /// 在合法分支内生成一次元素访问，并把结果写入带 GC 根的结果槽。
    /// 失败分支只写零占位值，不接触容器地址、不删除元素，因此失败后容器状态完全不变。
    fn build_guarded_result<F>(
        &mut self,
        valid: IntValue<'ctx>,
        payload: &ValueType,
        prefix: &str,
        build_value: F,
    ) -> Result<TypedValue<'ctx>, CompilerError>
    where
        F: FnOnce(&mut Self) -> Result<BasicValueEnum<'ctx>, CompilerError>,
    {
        let slot = self.allocate_safe_result_slot(payload)?;
        let (ok_block, failed_block, done_block) = self.append_safe_blocks(prefix)?;
        self.map_builder(self.builder.build_conditional_branch(
            valid,
            ok_block,
            failed_block,
        ))?;

        self.builder.position_at_end(failed_block);
        let placeholder = self.basic_type_for(payload)?.const_zero();
        self.store_safe_result(
            slot,
            payload,
            self.context.bool_type().const_zero(),
            placeholder,
        )?;
        self.map_builder(self.builder.build_unconditional_branch(done_block))?;

        self.builder.position_at_end(ok_block);
        let value = build_value(self)?;
        self.store_safe_result(
            slot,
            payload,
            self.context.bool_type().const_int(1, false),
            value,
        )?;
        self.map_builder(self.builder.build_unconditional_branch(done_block))?;

        self.builder.position_at_end(done_block);
        self.load_safe_result(slot, payload)
    }

    /// 在合法分支内生成一次会报错的修改操作，返回与条件一致的布尔结果（`true` 表示已生效）。
    /// 失败分支不执行任何容器调用，因此长度、容量、元素与 GC 引用关系保持原样。
    fn build_guarded_action<F>(
        &mut self,
        valid: IntValue<'ctx>,
        prefix: &str,
        action: F,
    ) -> Result<TypedValue<'ctx>, CompilerError>
    where
        F: FnOnce(&mut Self) -> Result<(), CompilerError>,
    {
        let function = self.current_function.expect("function context");
        let action_block = self.context.append_basic_block(function, &format!("{prefix}.act"));
        let done_block = self.context.append_basic_block(function, &format!("{prefix}.done"));
        self.map_builder(
            self.builder
                .build_conditional_branch(valid, action_block, done_block),
        )?;

        self.builder.position_at_end(action_block);
        action(self)?;
        self.map_builder(self.builder.build_unconditional_branch(done_block))?;

        self.builder.position_at_end(done_block);
        Ok(TypedValue {
            value_type: ValueType::Bool,
            value: valid.into(),
        })
    }

    /// 追加安全结果的合法、失败与汇合基本块。
    fn append_safe_blocks(
        &self,
        prefix: &str,
    ) -> Result<
        (
            inkwell::basic_block::BasicBlock<'ctx>,
            inkwell::basic_block::BasicBlock<'ctx>,
            inkwell::basic_block::BasicBlock<'ctx>,
        ),
        CompilerError,
    > {
        let function = self.current_function.expect("function context");
        Ok((
            self.context
                .append_basic_block(function, &format!("{prefix}.ok")),
            self.context
                .append_basic_block(function, &format!("{prefix}.failed")),
            self.context
                .append_basic_block(function, &format!("{prefix}.done")),
        ))
    }

    /// 分配安全结果槽；槽按载荷类型登记 GC 根，托管载荷在语句边界之后仍可被追踪。
    fn allocate_safe_result_slot(
        &self,
        payload: &ValueType,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        self.allocate_local(
            &ValueType::SafeConvertResult(Box::new(payload.clone())),
            "safe.result",
        )
    }

    /// 用统一的 `{ ok, value }` 结构写入结果槽，不手工拼接聚合值。
    fn store_safe_result(
        &self,
        slot: PointerValue<'ctx>,
        payload: &ValueType,
        ok: IntValue<'ctx>,
        value: BasicValueEnum<'ctx>,
    ) -> Result<(), CompilerError> {
        let assembled = self.build_safe_convert_result(payload.clone(), ok, value)?;
        self.map_builder(self.builder.build_store(slot, assembled.value))?;
        Ok(())
    }

    /// 从结果槽读回安全结果，返回带 `try_result<T>` 类型标记的表达式值。
    fn load_safe_result(
        &self,
        slot: PointerValue<'ctx>,
        payload: &ValueType,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let result_type = ValueType::SafeConvertResult(Box::new(payload.clone()));
        let value = self.map_builder(self.builder.build_load(
            self.basic_type_for(&result_type)?,
            slot,
            "load.safe.result",
        ))?;
        Ok(TypedValue {
            value_type: result_type,
            value,
        })
    }

    /// 计算数组元素地址；调用方必须先确认下标合法，本函数不再插入终止型安全检查。
    fn build_array_element_pointer(
        &mut self,
        array_pointer: PointerValue<'ctx>,
        element_type: &ValueType,
        size: usize,
        index: IntValue<'ctx>,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        let zero = self.context.i64_type().const_zero();
        unsafe {
            self.map_builder(self.builder.build_in_bounds_gep(
                self.array_type_for(element_type, size)?,
                array_pointer,
                &[zero, index],
                "array.try.ptr",
            ))
        }
    }

    /// 解析数组写入位置：变量直接复用真实栈槽或全局槽位，结构体字段链在接收者也是变量引用时
    /// 直接取字段地址；没有可写地址的临时数组值或数组元素在编译期拒绝，避免写入副本后仍报告成功。
    /// 调用方在编译实参之前调用本函数，重新求值只涉及变量和字段读取（不含调用与赋值），
    /// 因此不会重复执行带副作用的表达式，也不会改变写入目标。
    fn resolve_array_write_pointer(
        &mut self,
        target: &Expr,
        value: &TypedValue<'ctx>,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        match target {
            Expr::Grouped { expression, .. } => {
                self.resolve_array_write_pointer(expression, value)
            }
            Expr::Identifier { name, span } => match self.lookup_variable(name) {
                Some(variable) if variable.value_type == value.value_type => Ok(variable.pointer),
                _ => Err(Self::build_array_storage_error(*span)),
            },
            Expr::FieldAccess {
                target: receiver,
                name,
                span,
            } => {
                if !Self::is_place_chain(receiver) {
                    return Err(Self::build_array_storage_error(*span));
                }
                // 变量引用的重新加载没有副作用，字段地址直接指向原结构体对象。
                let compiled = self.compile_expr(receiver)?;
                let receiver_value = self.expect_value(compiled)?;
                let (pointer, field_type) = self.field_pointer(&receiver_value, name, *span)?;
                if field_type != value.value_type {
                    return Err(Self::build_array_storage_error(*span));
                }
                Ok(pointer)
            }
            _ => Err(Self::build_array_storage_error(target.span())),
        }
    }

    /// 判断表达式是否只由变量引用和结构体字段组成；这类表达式重新求值不会改变程序状态。
    fn is_place_chain(expression: &Expr) -> bool {
        match expression {
            Expr::Identifier { .. } => true,
            Expr::Grouped { expression, .. } => Self::is_place_chain(expression),
            Expr::FieldAccess { target, .. } => Self::is_place_chain(target),
            _ => false,
        }
    }

    /// 报告数组写入位置缺失：固定数组是内联值，只有变量和结构体字段里的数组才有可写地址。
    fn build_array_storage_error(span: Span) -> CompilerError {
        CompilerError::build_message(
            "固定数组 `try_set` 需要可写入的存储位置：请把数组保存在变量或结构体字段中，\
             数组元素改用下标赋值，例如 `grid[0][1] = v`",
            span,
        )
    }
}
