use inkwell::targets::FileType;

use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 把构建完成的模块校验后输出为 COFF 目标文件字节。
    pub(super) fn finish_object(self) -> Result<GeneratedObject, CompilerError> {
        self.module.verify().map_err(|error| {
            CompilerError::build(
                MessageId::BuildVerifyModuleFailed,
                vec![error.to_string()],
                Span::new(0, self.source.text().len()),
            )
        })?;

        let target_machine = create_target_machine(self.source, &self.module.get_triple())?;
        let buffer = target_machine
            .write_to_memory_buffer(&self.module, FileType::Object)
            .map_err(|error| {
                CompilerError::build(
                    MessageId::BuildWriteIrFailed,
                    vec![error.to_string()],
                    Span::new(0, self.source.text().len()),
                )
            })?;

        Ok(GeneratedObject {
            object_bytes: buffer.as_slice().to_vec(),
        })
    }

    /// 返回当前模块的 LLVM IR 字符串，供测试校验运行时符号绑定。
    #[cfg(test)]
    pub(super) fn finish_ir(self) -> Result<String, CompilerError> {
        self.module.verify().map_err(|error| {
            CompilerError::build(
                MessageId::BuildVerifyModuleFailed,
                vec![error.to_string()],
                Span::new(0, self.source.text().len()),
            )
        })?;
        Ok(self.module.print_to_string().to_string())
    }
}
