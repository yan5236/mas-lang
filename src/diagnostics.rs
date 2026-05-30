use mascript_compiler_llvm::diagnostic::{CompilerError, render_stage};
use mascript_compiler_llvm::settings::CompilerSettings;
use mascript_compiler_llvm::source::SourceFile;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::lsp_convert::span_to_range;

/// 把主力编译器错误转换为 LSP 诊断，保留阶段、中文消息和源码范围。
pub fn compiler_error_to_diagnostic(
    error: &CompilerError,
    source: &SourceFile,
    settings: CompilerSettings,
) -> Diagnostic {
    let stage = render_stage(settings.language, error.stage);
    Diagnostic {
        range: span_to_range(source.text(), error.span),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some(format!("mas-lang {stage}")),
        message: error.render(settings.language, Some(source)),
        ..Diagnostic::default()
    }
}
