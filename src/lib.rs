pub mod ast;
pub mod cli;
pub mod diagnostic;
pub mod lexer;
pub mod linker;
pub mod llvm_backend;
pub mod parser;
pub mod runtime_bootstrap;
pub mod semantic;
pub mod settings;
pub mod source;
pub mod token;

use std::path::PathBuf;

use ast::Program;
use diagnostic::CompilerError;
use lexer::Lexer;
use parser::Parser;
use semantic::SemanticAnalyzer;
use settings::CompilerSettings;
use source::SourceFile;

/// 保存前端流水线产出的抽象语法树，供后端直接复用。
pub struct FrontendOutput {
    pub program: Program,
}

/// 保存构建阶段允许外部指定的输出选项。
pub struct CompileOptions {
    pub output_path: Option<PathBuf>,
}

impl CompileOptions {
    /// 使用可选输出路径创建新的编译选项对象。
    pub fn new(output_path: Option<PathBuf>) -> Self {
        Self { output_path }
    }
}

/// 按固定顺序执行词法、语法和语义分析，并返回可复用的前端结果。
pub fn analyze_source(
    source: &SourceFile,
    settings: &CompilerSettings,
) -> Result<FrontendOutput, CompilerError> {
    let tokens = Lexer::new(source, settings).tokenize()?;
    let program = Parser::new(tokens, settings).parse_program()?;
    SemanticAnalyzer::new(settings).analyze(&program)?;
    Ok(FrontendOutput { program })
}

/// 把源码编译为 Windows 可执行文件，并返回最终产物路径。
pub fn compile_to_executable(
    source: &SourceFile,
    settings: &CompilerSettings,
    options: &CompileOptions,
) -> Result<PathBuf, CompilerError> {
    let frontend = analyze_source(source, settings)?;
    linker::compile_to_executable(source, settings, &frontend.program, options)
}

/// 从用户目录读取编译器设置；当设置不存在或无效时自动回退到默认值。
pub fn load_settings() -> CompilerSettings {
    settings::load_settings()
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{CompileOptions, analyze_source, compile_to_executable};
    use crate::settings::{CompilerSettings, UiLanguage};
    use crate::source::SourceFile;

    /// 构造测试环境统一使用的中文设置，减少重复样板代码。
    fn zh_settings() -> CompilerSettings {
        CompilerSettings::new(UiLanguage::ZhCn)
    }

    /// 校验最小程序能够完整通过前端分析流水线。
    #[test]
    fn should_analyze_minimal_program() {
        let source = SourceFile::new("<test>", "var value = 1\nprint(value)\n".to_string());
        let output = analyze_source(&source, &zh_settings()).unwrap();
        assert_eq!(output.program.statements.len(), 2);
    }

    /// 校验原生后端能够把最小程序编译为可执行文件。
    #[test]
    fn should_compile_minimal_program() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let source_path = env::temp_dir()
            .join(format!("mascript-llvm-minimal-{unique}"))
            .join("minimal.ms");
        let source = SourceFile::new(
            source_path.display().to_string(),
            "print(\"Hello world.\")\n".to_string(),
        );
        let binary =
            compile_to_executable(&source, &zh_settings(), &CompileOptions::new(None)).unwrap();
        assert!(binary.extension().is_some_and(|ext| ext == "exe"));
    }
}
