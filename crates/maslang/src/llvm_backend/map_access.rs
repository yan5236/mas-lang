use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 将整数查询的六个只读诊断参数合并为模块常量指针；保留完整路径、源码与 span，仅缺失时解引用。
    pub(super) fn build_int_map_get(
        &mut self,
        map: PointerValue<'ctx>,
        key: TypedValue<'ctx>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        self.build_map_error_site_arguments(span)?;
        let site = self.context.const_struct(
            &[
                self.source_path_value
                    .expect("source path initialized")
                    .into(),
                self.source_text_value
                    .expect("source text initialized")
                    .into(),
                self.context
                    .i64_type()
                    .const_int(span.start as u64, false)
                    .into(),
                self.context
                    .i64_type()
                    .const_int(span.end as u64, false)
                    .into(),
            ],
            false,
        );
        let global = self
            .module
            .add_global(site.get_type(), None, "map.error.site");
        global.set_linkage(inkwell::module::Linkage::Private);
        global.set_constant(true);
        global.set_initializer(&site);
        let ptr = self.context.ptr_type(AddressSpace::default());
        let function = self
            .module
            .get_function("map_get_int_int_site")
            .unwrap_or_else(|| {
                self.module.add_function(
                    "map_get_int_int_site",
                    self.context.i64_type().fn_type(
                        &[ptr.into(), self.context.i64_type().into(), ptr.into()],
                        false,
                    ),
                    None,
                )
            });
        let call = self.map_builder(self.builder.build_call(
            function,
            &[
                map.into(),
                key.value.into(),
                global.as_pointer_value().into(),
            ],
            "call.map_get",
        ))?;
        Ok(TypedValue {
            value_type: ValueType::Int,
            value: call.try_as_basic_value().unwrap_basic(),
        })
    }
}
