use super::*;
use crate::ast::{Expr, MethodName};
use inkwell::values::IntValue;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译字典安全方法（`try_get`/`try_remove`）；缺键时返回 `ok=false` 或 `false`，
    /// 不触发运行时错误，也不改变字典内容。
    /// 键是否存在统一复用 `map_contains_*`，键比较语义（身份/字节内容/整数位模式）与现有 `contains` 完全一致。
    pub(super) fn compile_map_safe_method(
        &mut self,
        target_value: TypedValue<'ctx>,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let (map_value, key_type, value_type) = self.expect_map_value(target_value, span)?;
        if arguments.len() != 1 {
            return Err(CompilerError::build_message(
                format!(
                    "字典方法 `{}` 需要且只接受一个键参数",
                    method.display_name()
                ),
                span,
            ));
        }
        let key_compiled = self.compile_expr(&arguments[0])?;
        let key = self.expect_value(key_compiled)?;
        if key.value_type != key_type {
            return Err(CompilerError::build_message(
                format!("字典方法 `{}` 的键类型不匹配", method.display_name()),
                arguments[0].span(),
            ));
        }
        let present = self.build_map_contains_flag(map_value, &key_type, &key, span)?;
        match method {
            MethodName::TryGet => {
                self.build_map_try_get(map_value, &key_type, &value_type, key, present, span)
            }
            MethodName::TryRemove => {
                self.build_map_try_remove(map_value, &key_type, key, present, span)
            }
            _ => Err(CompilerError::build_message(
                format!("字典类型不支持方法 `{}`", method.display_name()),
                span,
            )),
        }
    }

    /// 调用 `map_contains_*` 得到键是否存在；该入口不读取值，因此也覆盖键存在但值为 `null` 的情况。
    fn build_map_contains_flag(
        &mut self,
        map_value: PointerValue<'ctx>,
        key_type: &ValueType,
        key: &TypedValue<'ctx>,
        span: Span,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let function = self.map_runtime_function("map_contains", key_type, None)?;
        let mut args = vec![map_value.into()];
        args.extend(self.build_list_value_arguments(key.clone(), span)?);
        let call = self.map_builder(self.builder.build_call(
            function,
            &args,
            "call.map_contains",
        ))?;
        let raw = call.try_as_basic_value().unwrap_basic().into_int_value();
        self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            raw,
            self.context.i8_type().const_zero(),
            "map.try_get.contains",
        ))
    }

    /// 编译 `try_get`：命中时才调用原有读取入口，缺失分支返回 `ok=false` 与占位值；
    /// 两条路径写入同一个 `{ ok, value }` 槽位后汇合，保证结果载荷以正确布局登记为 GC 根。
    fn build_map_try_get(
        &mut self,
        map_value: PointerValue<'ctx>,
        key_type: &ValueType,
        value_type: &ValueType,
        key: TypedValue<'ctx>,
        present: IntValue<'ctx>,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let result_type = ValueType::SafeConvertResult(Box::new(value_type.clone()));
        let result_slot = self.allocate_local(&result_type, "map.try_get.result")?;
        let function = self.current_function.expect("function context");
        let found = self
            .context
            .append_basic_block(function, "map.try_get.found");
        let missing = self
            .context
            .append_basic_block(function, "map.try_get.missing");
        let merge = self
            .context
            .append_basic_block(function, "map.try_get.merge");
        self.map_builder(
            self.builder
                .build_conditional_branch(present, found, missing),
        )?;

        self.builder.position_at_end(found);
        let value = self.build_map_get(map_value, key_type, value_type, key, span)?;
        let found_result = self.build_safe_convert_result(
            value_type.clone(),
            self.context.bool_type().const_int(1, false),
            value.value,
        )?;
        self.map_builder(self.builder.build_store(result_slot, found_result.value))?;
        self.map_builder(self.builder.build_unconditional_branch(merge))?;

        self.builder.position_at_end(missing);
        let placeholder = self.build_safe_failure_placeholder(value_type)?;
        let missing_result = self.build_safe_convert_result(
            value_type.clone(),
            self.context.bool_type().const_zero(),
            placeholder,
        )?;
        self.map_builder(self.builder.build_store(result_slot, missing_result.value))?;
        self.map_builder(self.builder.build_unconditional_branch(merge))?;

        self.builder.position_at_end(merge);
        let loaded = self.map_builder(self.builder.build_load(
            self.basic_type_for(&result_type)?,
            result_slot,
            "load.map.try_get",
        ))?;
        Ok(CodegenValue::Value(TypedValue {
            value_type: result_type,
            value: loaded,
        }))
    }

    /// 编译 `try_remove`：仅当 `contains` 判定键存在时才调用原有删除入口，
    /// 缺失分支直接返回 `false`，字典长度、内容与 GC 引用关系都保持原样。
    fn build_map_try_remove(
        &mut self,
        map_value: PointerValue<'ctx>,
        key_type: &ValueType,
        key: TypedValue<'ctx>,
        present: IntValue<'ctx>,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let result_slot =
            self.allocate_entry_slot(self.context.bool_type().into(), "map.try_remove.result")?;
        let function = self.current_function.expect("function context");
        let removed = self
            .context
            .append_basic_block(function, "map.try_remove.removed");
        let missing = self
            .context
            .append_basic_block(function, "map.try_remove.missing");
        let merge = self
            .context
            .append_basic_block(function, "map.try_remove.merge");
        self.map_builder(
            self.builder
                .build_conditional_branch(present, removed, missing),
        )?;

        self.builder.position_at_end(removed);
        let remove = self.map_runtime_function("map_remove", key_type, None)?;
        let mut args = vec![map_value.into()];
        args.extend(self.build_list_value_arguments(key, span)?);
        args.extend(self.build_map_error_site_arguments(span)?);
        self.map_builder(self.builder.build_call(remove, &args, "call.map_remove"))?;
        self.map_builder(
            self.builder
                .build_store(result_slot, self.context.bool_type().const_int(1, false)),
        )?;
        self.map_builder(self.builder.build_unconditional_branch(merge))?;

        self.builder.position_at_end(missing);
        self.map_builder(
            self.builder
                .build_store(result_slot, self.context.bool_type().const_zero()),
        )?;
        self.map_builder(self.builder.build_unconditional_branch(merge))?;

        self.builder.position_at_end(merge);
        let loaded = self.map_builder(self.builder.build_load(
            self.context.bool_type(),
            result_slot,
            "load.map.try_remove",
        ))?;
        Ok(CodegenValue::Value(TypedValue {
            value_type: ValueType::Bool,
            value: loaded,
        }))
    }
}
