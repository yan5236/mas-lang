use inkwell::targets::FileType;

use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 校验模块并执行指定档位的优化管线，再输出目标平台的对象文件；可选写出 IR 与汇编，失败停止构建。
    pub(super) fn finish_object(
        self,
        options: &CompileOptions,
    ) -> Result<GeneratedObject, CompilerError> {
        self.module.verify().map_err(|error| {
            CompilerError::build(
                MessageId::BuildVerifyModuleFailed,
                vec![error.to_string()],
                Span::new(0, self.source.text().len()),
            )
        })?;

        let target_machine =
            create_target_machine(self.source, &self.module.get_triple(), options.opt_level)?;
        if let Some(dir) = &options.emit_dir {
            std::fs::create_dir_all(dir).map_err(|error| self.output_error(error))?;
            self.emit_ir(dir, "raw.ll")?;
        }
        if options.opt_level != OptLevel::O0 {
            self.elide_redundant_gc_polls();
            self.elide_scalar_gc();
        }
        let passes = inkwell::passes::PassBuilderOptions::create();
        passes.set_verify_each(true);
        self.module
            .run_passes(options.opt_level.pipeline(), &target_machine, passes)
            .map_err(|error| self.output_error(error))?;
        self.module
            .verify()
            .map_err(|error| self.output_error(error))?;
        if let Some(dir) = &options.emit_dir {
            self.emit_ir(dir, "opt.ll")?;
            target_machine
                .write_to_file(&self.module, FileType::Assembly, &self.dump_path(dir, "s"))
                .map_err(|error| self.output_error(error))?;
        }
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

    /// 为生成证据的失败添加源码范围；写盘或 PassManager 失败均停止构建。
    fn output_error(&self, error: impl std::fmt::Display) -> CompilerError {
        CompilerError::build(
            MessageId::BuildWriteIrFailed,
            vec![error.to_string()],
            Span::new(0, self.source.text().len()),
        )
    }

    /// 以源文件名生成证据路径；调用者指定独立目录以区分优化档位。
    fn dump_path(&self, dir: &std::path::Path, suffix: &str) -> std::path::PathBuf {
        let stem = std::path::Path::new(self.source.path())
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        dir.join(format!("{stem}.{suffix}"))
    }

    /// 写出当前阶段的完整 IR，不修改模块；失败时保留诊断并停止。
    fn emit_ir(&self, dir: &std::path::Path, suffix: &str) -> Result<(), CompilerError> {
        self.module
            .print_to_file(self.dump_path(dir, suffix))
            .map_err(|error| self.output_error(error))
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
