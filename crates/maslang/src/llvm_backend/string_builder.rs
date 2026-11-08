use super::string_builder_analysis::{append_only, candidates, reads};
use super::*;
use crate::ast::Expr;
use inkwell::types::BasicMetadataTypeEnum;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 为只追加的局部字符串建立私有 GC 根；排除全局、中间读取、循环变量遮蔽和 O0，返回栈恢复位置。
    pub(super) fn begin_string_builders(
        &mut self,
        controls: &[&Expr],
        body: &[Stmt],
        binding: Option<&str>,
    ) -> Result<usize, CompilerError> {
        let mark = self.string_builders.len();
        if !self.optimize_strings {
            return Ok(mark);
        }
        let mut names = Vec::new();
        candidates(body, &mut names);
        for name in names {
            if binding == Some(name.as_str())
                || controls.iter().any(|x| reads(x, &name))
                || !append_only(body, &name)
            {
                continue;
            }
            let Some(variable) = self
                .scopes
                .iter()
                .rev()
                .find_map(|scope| scope.get(&name))
                .cloned()
            else {
                continue;
            };
            if variable.value_type != ValueType::String
                || !variable.is_mutable
                || self
                    .string_builders
                    .iter()
                    .any(|(slot, _)| *slot == variable.pointer)
            {
                continue;
            }
            let ptr_type = self.context.ptr_type(AddressSpace::default());
            let slot = self.allocate_entry_slot(ptr_type.into(), "string.builder.root")?;
            self.gc_handle_roots.borrow_mut().push(slot);
            let initial = self
                .map_builder(self.builder.build_load(
                    self.string_type,
                    variable.pointer,
                    "string.initial",
                ))?
                .into_struct_value();
            let create = self.builder_runtime(
                "mas_string_builder_create",
                &[ptr_type.into(), self.context.i64_type().into()],
                false,
            );
            let call = self.map_builder(
                self.builder.build_call(
                    create,
                    &[
                        self.extract_string_part(initial, 0, "builder.initial.ptr")?
                            .into(),
                        self.extract_string_part(initial, 1, "builder.initial.len")?
                            .into(),
                    ],
                    "string.builder",
                ),
            )?;
            self.map_builder(
                self.builder
                    .build_store(slot, call.try_as_basic_value().unwrap_basic()),
            )?;
            self.string_builders.push((variable.pointer, slot));
        }
        Ok(mark)
    }

    /// 重定向已证明安全的 +=；右值按原顺序求值，其他赋值返回 false 并沿用普通字符串路径。
    pub(super) fn append_string_builder(
        &mut self,
        variable: PointerValue<'ctx>,
        expr: &Expr,
    ) -> Result<bool, CompilerError> {
        let Some((_, slot)) = self
            .string_builders
            .iter()
            .find(|(p, _)| *p == variable)
            .copied()
        else {
            return Ok(false);
        };
        let compiled = self.compile_expr(expr)?;
        let value = self.expect_value(compiled)?;
        if value.value_type != ValueType::String {
            return Err(CompilerError::build_message(
                "字符串追加需要字符串右值",
                expr.span(),
            ));
        }
        let value = value.value.into_struct_value();
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let handle = self.map_builder(self.builder.build_load(ptr_type, slot, "builder.handle"))?;
        let append = self.builder_runtime(
            "mas_string_builder_append",
            &[
                ptr_type.into(),
                ptr_type.into(),
                self.context.i64_type().into(),
            ],
            true,
        );
        self.map_builder(
            self.builder.build_call(
                append,
                &[
                    handle.into(),
                    self.extract_string_part(value, 0, "builder.part.ptr")?
                        .into(),
                    self.extract_string_part(value, 1, "builder.part.len")?
                        .into(),
                ],
                "",
            ),
        )?;
        Ok(true)
    }

    /// 在循环公共出口（含 break）物化不可变字符串并解除私有根；continue 仍保留缓冲区。
    pub(super) fn finish_string_builders(&mut self, mark: usize) -> Result<(), CompilerError> {
        while self.string_builders.len() > mark {
            let (variable, slot) = self.string_builders.pop().expect("builder stack");
            let ptr_type = self.context.ptr_type(AddressSpace::default());
            let handle =
                self.map_builder(self.builder.build_load(ptr_type, slot, "builder.handle"))?;
            let length = self.allocate_local(&ValueType::Int, "builder.length")?;
            let finish = self.builder_runtime(
                "mas_string_builder_finish",
                &[ptr_type.into(), ptr_type.into()],
                false,
            );
            let call = self.map_builder(self.builder.build_call(
                finish,
                &[handle.into(), length.into()],
                "builder.result",
            ))?;
            let ptr = call
                .try_as_basic_value()
                .unwrap_basic()
                .into_pointer_value();
            let len = self
                .map_builder(self.builder.build_load(
                    self.context.i64_type(),
                    length,
                    "builder.bytes",
                ))?
                .into_int_value();
            let value = self.build_runtime_string(ptr, len)?;
            self.map_builder(self.builder.build_store(variable, value))?;
            self.map_builder(self.builder.build_store(slot, ptr_type.const_null()))?;
        }
        Ok(())
    }

    /// 按需声明内部构建器 ABI；三个符号仅返回句柄/字符串地址或无返回值，不改变语言字符串 ABI。
    fn builder_runtime(
        &self,
        name: &str,
        args: &[BasicMetadataTypeEnum<'ctx>],
        returns_void: bool,
    ) -> FunctionValue<'ctx> {
        self.module.get_function(name).unwrap_or_else(|| {
            let ty = if returns_void {
                self.context.void_type().fn_type(args, false)
            } else {
                self.context
                    .ptr_type(AddressSpace::default())
                    .fn_type(args, false)
            };
            self.module.add_function(name, ty, None)
        })
    }
}
