use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 全局槽访问前检查声明是否执行；局部槽直接返回，失败分支输出带源码位置的语言错误。
    pub(super) fn check_global_initialized(
        &mut self,
        variable: &VariableInfo<'ctx>,
        span: Span,
    ) -> Result<(), CompilerError> {
        let Some((name, _)) = self
            .globals
            .iter()
            .find(|(_, global)| global.pointer == variable.pointer)
        else {
            return Ok(());
        };
        let flag = self.global_initialized[name];
        let initialized = self.map_builder(self.builder.build_load(
            self.context.i8_type(),
            flag,
            "global.initialized",
        ))?;
        let initialized = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            initialized.into_int_value(),
            self.context.i8_type().const_zero(),
            "global.ready",
        ))?;
        let function = self.current_function.expect("function context");
        let ready = self.context.append_basic_block(function, "global.ready");
        let failed = self
            .context
            .append_basic_block(function, "global.uninitialized");
        self.map_builder(
            self.builder
                .build_conditional_branch(initialized, ready, failed),
        )?;
        self.builder.position_at_end(failed);
        let args = self.build_map_error_site_arguments(span)?;
        let error = self
            .module
            .get_function("mas_global_uninitialized")
            .unwrap_or_else(|| {
                let ptr = self.context.ptr_type(AddressSpace::default()).into();
                let int = self.context.i64_type().into();
                self.module.add_function(
                    "mas_global_uninitialized",
                    self.context
                        .void_type()
                        .fn_type(&[ptr, int, ptr, int, int, int], false),
                    None,
                )
            });
        self.map_builder(self.builder.build_call(error, &args, ""))?;
        self.map_builder(self.builder.build_unreachable())?;
        self.builder.position_at_end(ready);
        Ok(())
    }
}
