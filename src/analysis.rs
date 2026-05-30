use mascript_compiler_llvm::settings::CompilerSettings;
use mascript_compiler_llvm::source::SourceFile;
use tower_lsp::lsp_types::Diagnostic;

use crate::diagnostics::compiler_error_to_diagnostic;
use crate::documents::OpenDocument;
use crate::symbols::{SymbolSnapshot, fallback_symbols_from_source, symbols_from_program};

/// 保存单个文档最近一次分析结果，供诊断发布和后续语言能力复用。
#[derive(Clone, Debug, Default)]
pub struct DocumentAnalysis {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: SymbolSnapshot,
}

/// 封装分析服务需要共享的编译器设置，并统一调用主力编译器前端。
#[derive(Debug)]
pub struct AnalysisService {
    settings: CompilerSettings,
}

impl AnalysisService {
    /// 创建默认分析服务，统一复用主力编译器的设置加载逻辑。
    pub fn new() -> Self {
        Self {
            settings: mascript_compiler_llvm::load_settings(),
        }
    }

    /// 使用指定编译器设置创建分析服务，供测试固定中文或英文诊断环境。
    pub fn with_settings(settings: CompilerSettings) -> Self {
        Self { settings }
    }

    /// 暴露当前编译器设置引用，便于后续诊断和分析阶段保持同一份配置。
    pub fn settings(&self) -> &CompilerSettings {
        &self.settings
    }

    /// 对打开文档执行完整前端分析，并把所有可恢复错误收敛为 LSP 诊断。
    pub fn analyze_document(&self, document: &OpenDocument) -> DocumentAnalysis {
        let source = SourceFile::new(document.path.display().to_string(), document.text.clone());
        match mascript_compiler_llvm::analyze_source(&source, &self.settings) {
            Ok(output) => DocumentAnalysis {
                diagnostics: Vec::new(),
                symbols: symbols_from_program(output.source.text(), &output.program),
            },
            Err(error) => DocumentAnalysis {
                diagnostics: vec![compiler_error_to_diagnostic(&error, &source, self.settings)],
                symbols: fallback_symbols_from_source(&source, &self.settings),
            },
        }
    }
}

impl Default for AnalysisService {
    /// 创建默认分析服务，方便服务器状态结构按默认值组合。
    fn default() -> Self {
        Self::new()
    }
}
