use mas_lang_lsp::analysis::AnalysisService;
use mas_lang_lsp::diagnostics::compiler_error_to_diagnostic;
use mas_lang_lsp::documents::OpenDocument;
use mascript_compiler_llvm::diagnostic::{CompilerError, ErrorStage, MessageId};
use mascript_compiler_llvm::settings::{CompilerSettings, UiLanguage};
use mascript_compiler_llvm::source::{SourceFile, Span};
use tower_lsp::lsp_types::{DiagnosticSeverity, Position, Range, Url};

/// 构造测试用打开文档，避免每个诊断用例重复拼接 URI 与路径。
fn open_document(text: &str) -> OpenDocument {
    let uri = Url::parse("file:///E:/xiangmu/mascript/mskaifa/yanzhenglsp/src/main.ms").unwrap();
    OpenDocument {
        uri,
        version: 1,
        path: "E:/xiangmu/mascript/mskaifa/yanzhenglsp/src/main.ms".into(),
        text: text.to_string(),
    }
}

/// 验证编译器错误会保留中文消息、阶段、严重级别与 UTF-16 源码范围。
#[test]
fn should_convert_compiler_error_to_lsp_diagnostic() {
    let text = "var 数量 = 1\nprint(未知)\n";
    let start = text.find("未知").unwrap();
    let source = SourceFile::new("main.ms", text.to_string());
    let error = CompilerError::semantic(
        MessageId::SemanticUnknownVariable,
        vec!["未知".to_string()],
        Span::new(start, start + "未知".len()),
    );

    let diagnostic =
        compiler_error_to_diagnostic(&error, &source, CompilerSettings::new(UiLanguage::ZhCn));

    assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostic.source, Some("mas-lang 语义错误".to_string()));
    assert!(diagnostic.message.contains("未声明变量 `未知`"));
    assert_eq!(
        diagnostic.range,
        Range {
            start: Position::new(1, 6),
            end: Position::new(1, 8),
        }
    );
}

/// 验证多行源码跨度会转换为覆盖首尾行的 LSP range，避免诊断只落在第一行。
#[test]
fn should_convert_multiline_error_span_to_lsp_range() {
    let text = "var value = 1\nprint(value)\nprint(value)\n";
    let source = SourceFile::new("main.ms", text.to_string());
    let start = text.find("print(value)").unwrap();
    let end = text.rfind("print(value)").unwrap() + "print(value)".len();
    let error = CompilerError::new(
        ErrorStage::Syntax,
        MessageId::CustomMessage,
        vec!["跨行语法错误".to_string()],
        Span::new(start, end),
    );

    let diagnostic =
        compiler_error_to_diagnostic(&error, &source, CompilerSettings::new(UiLanguage::ZhCn));

    assert_eq!(
        diagnostic.range,
        Range {
            start: Position::new(1, 0),
            end: Position::new(2, 12),
        }
    );
    assert!(diagnostic.message.contains("跨行语法错误"));
}

/// 验证分析服务会调用主力编译器前端，成功源码返回空诊断。
#[test]
fn should_return_empty_diagnostics_for_valid_source() {
    let service = AnalysisService::with_settings(CompilerSettings::new(UiLanguage::ZhCn));
    let analysis = service.analyze_document(&open_document(
        "func main():\n    var score = 1\n    print(score)\n",
    ));

    assert!(analysis.diagnostics.is_empty());
}

/// 验证分析服务会把编译器前端错误转换成 LSP 诊断，而不是让错误向外逃逸。
#[test]
fn should_return_diagnostic_for_compiler_frontend_error() {
    let service = AnalysisService::with_settings(CompilerSettings::new(UiLanguage::ZhCn));
    let analysis = service.analyze_document(&open_document("func main():\n    print(@)\n"));

    assert_eq!(analysis.diagnostics.len(), 1);
    let diagnostic = &analysis.diagnostics[0];
    assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
    assert!(diagnostic.message.contains("无法识别的字符"));
    assert!(
        diagnostic
            .source
            .as_deref()
            .unwrap_or("")
            .contains("词法错误")
    );
}

/// 验证模块装载或文件系统错误会被收敛成诊断，不会让分析服务 panic。
#[test]
fn should_return_diagnostic_for_missing_module_error() {
    let service = AnalysisService::with_settings(CompilerSettings::new(UiLanguage::ZhCn));
    let analysis = service.analyze_document(&open_document(
        "import missing.module { value }\nfunc main():\n    print(value)\n",
    ));

    assert_eq!(analysis.diagnostics.len(), 1);
    let diagnostic = &analysis.diagnostics[0];
    assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
    assert!(diagnostic.message.contains("找不到模块"));
    assert!(
        diagnostic
            .source
            .as_deref()
            .unwrap_or("")
            .contains("构建错误")
    );
}

/// 验证列表和固定数组下标赋值复用编译器规则：合法写入无诊断，错误下标及元素类型仍定位到赋值行。
#[test]
fn index_assignment_diagnostics_match_compiler() {
    let service = AnalysisService::with_settings(CompilerSettings::new(UiLanguage::ZhCn));
    let valid = "var xs = [1, 2]\nxs[0] = 3\nvar a = array<int,2>(1,2)\na[1] = 4\n";
    let analysis = service.analyze_document(&open_document(valid));
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    for (declaration, assignment, message) in [
        ("var xs = [1, 2]", "xs[true] = 3", "下标必须是整数"),
        ("var a = array<int,2>(1,2)", "a[0.5] = 3", "下标必须是整数"),
        ("var xs = [1, 2]", "xs[0] = false", "元素类型"),
        ("var a = array<int,2>(1,2)", "a[0] = false", "元素类型"),
    ] {
        let text = format!("{declaration}\n{assignment}\n");
        let analysis = service.analyze_document(&open_document(&text));
        assert_eq!(analysis.diagnostics.len(), 1);
        let diagnostic = &analysis.diagnostics[0];
        assert_eq!(diagnostic.range.start.line, 1, "{diagnostic:?}");
        assert!(diagnostic.message.contains(message), "{diagnostic:?}");
    }
}
