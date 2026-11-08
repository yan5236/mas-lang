use maslang::diagnostic::{CompilerError, render_stage};
use maslang::settings::CompilerSettings;
use maslang::source::SourceFile;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Range, Url,
};

use crate::lsp_convert::span_to_range;

/// 把主力编译器错误转换为 LSP 诊断，保留阶段、中文消息和源码范围。
pub fn compiler_error_to_diagnostic(
    error: &CompilerError,
    source: &SourceFile,
    settings: CompilerSettings,
) -> Diagnostic {
    let stage = render_stage(settings.language, error.stage);
    let origin = error.source.as_ref().unwrap_or(source);
    let foreign = origin.path() != source.path();
    let related_information = if foreign {
        Url::from_file_path(origin.path()).ok().map(|uri| {
            vec![DiagnosticRelatedInformation {
                location: Location {
                    uri,
                    range: span_to_range(origin.text(), error.span),
                },
                message: error.render(settings.language, Some(origin)),
            }]
        })
    } else {
        None
    };
    Diagnostic {
        range: if foreign {
            Range::default()
        } else {
            span_to_range(source.text(), error.span)
        },
        related_information,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some(format!("mas-lang {stage}")),
        message: error.render(settings.language, Some(source)),
        ..Diagnostic::default()
    }
}
