use inkwell::IntPredicate;
use inkwell::values::{BasicMetadataValueEnum, IntValue, PointerValue};

use super::*;
use crate::ast::{Expr, MethodName};

/// 列表的可恢复方法：先判定下标/范围，再调用既有运行时代码，失败分支不改动容器。
/// 固定数组与结果槽构造位于同目录的 `safe_arrays` / `safe_result`。
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
    /// 下标实参先求值、再读取接收者当前长度，与下标赋值一致：实参缩容后按新长度拒绝，
    /// 扩容后按新长度放行，不会用旧长度放行越界弹出或错误拒绝新增元素。
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
        let (valid, index) = match arguments.first() {
            Some(argument) => {
                let index = self.compile_safe_index_argument(argument)?;
                let len = self.build_list_length(list)?;
                (
                    self.build_list_in_range_check(index, len, "safe.pop")?,
                    index,
                )
            }
            None => {
                let len = self.build_list_length(list)?;
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
            let call =
                this.map_builder(this.builder.build_call(function, &args, "call.list_index"))?;
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
        let call = self.map_builder(self.builder.build_call(
            function,
            args,
            "call.list_contains",
        ))?;
        let raw = call.try_as_basic_value().unwrap_basic().into_int_value();
        self.map_builder(self.builder.build_int_compare(
            IntPredicate::NE,
            raw,
            self.context.i8_type().const_zero(),
            "safe.contains.hit",
        ))
    }

    /// 读取列表当前长度；运行时的长度值以 i64 位模式返回。
    fn build_list_length(
        &mut self,
        list: PointerValue<'ctx>,
    ) -> Result<IntValue<'ctx>, CompilerError> {
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
        self.map_builder(self.builder.build_and(
            non_negative,
            in_bounds,
            &format!("{prefix}.valid"),
        ))
    }
}
