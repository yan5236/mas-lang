use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 原生返回值沿用结构体/可空值的指针 ABI；拒绝非托管地址和非空类型的 null，避免把悬空 C 地址带入语言堆。
    pub(super) fn validate_native_reference(
        &self,
        value: BasicValueEnum<'ctx>,
        value_type: &ValueType,
    ) -> Result<BasicValueEnum<'ctx>, CompilerError> {
        let ptr = self.list_pointer_type();
        let function = self
            .module
            .get_function("mas_native_require_record")
            .unwrap_or_else(|| {
                self.module.add_function(
                    "mas_native_require_record",
                    ptr.fn_type(&[ptr.into(), self.context.i8_type().into()], false),
                    None,
                )
            });
        let call = self.map_builder(
            self.builder.build_call(
                function,
                &[
                    value.into(),
                    self.context
                        .i8_type()
                        .const_int(
                            u64::from(matches!(value_type, ValueType::Nullable(_))),
                            false,
                        )
                        .into(),
                ],
                "native.reference",
            ),
        )?;
        Ok(call.try_as_basic_value().unwrap_basic())
    }
}
