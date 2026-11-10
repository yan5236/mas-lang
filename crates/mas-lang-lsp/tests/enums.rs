use mas_lang_lsp::analysis::AnalysisService;
use mas_lang_lsp::completion::{completion_items_for, keyword_items};
use mas_lang_lsp::documents::OpenDocument;
use mas_lang_lsp::hover::hover_for;
use mas_lang_lsp::symbols::SymbolSnapshot;
use maslang::settings::{CompilerSettings, UiLanguage};
use tower_lsp::lsp_types::{CompletionItemKind, Position, SymbolKind, Url};

/// 构造基于编辑器内存文本的打开文档，供各语言能力共享同一份源码。
fn open_document(text: &str) -> OpenDocument {
    OpenDocument {
        uri: Url::parse("file:///E:/xiangmu/mas-lang/tmp/enums/main.ms").unwrap(),
        version: 1,
        path: "E:/xiangmu/mas-lang/tmp/enums/main.ms".into(),
        text: text.to_string(),
    }
}

/// 使用中文设置分析文档并返回符号快照，统一各用例的样板代码。
fn analyze(text: &str) -> (SymbolSnapshot, Vec<tower_lsp::lsp_types::Diagnostic>) {
    let service = AnalysisService::with_settings(CompilerSettings::new(UiLanguage::ZhCn));
    let analysis = service.analyze_document(&open_document(text));
    (analysis.symbols, analysis.diagnostics)
}

/// 把源码中的唯一片段起点转换为 LSP 位置，测试文本均为单字节字符。
fn position_of(text: &str, needle: &str) -> Position {
    let offset = text.find(needle).expect("片段必须存在");
    let line = text[..offset].matches('\n').count();
    let line_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    Position::new(line as u32, (offset - line_start) as u32)
}

/// 关键字补全必须包含 `enum`，并给出缩进成员骨架。
#[test]
fn keyword_completion_includes_enum() {
    let items = keyword_items();
    let enum_item = items
        .iter()
        .find(|item| item.label == "enum")
        .expect("关键字补全必须包含 enum");
    assert_eq!(enum_item.kind, Some(CompletionItemKind::KEYWORD));
    let insert = enum_item.insert_text.as_deref().unwrap_or_default();
    assert!(insert.contains("enum ") && insert.contains(':'), "{insert}");
}

/// 文档符号和类型显示必须认识枚举声明及其成员。
#[test]
fn document_symbols_include_enum_and_members() {
    let text = "enum FileMode:\n    Read\n    Write\nfunc main():\n    print(FileMode.Read == FileMode.Write)\n";
    let (snapshot, diagnostics) = analyze(text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let symbols = snapshot.symbols();
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol.name == "FileMode" && symbol.kind == SymbolKind::ENUM)
    );
    assert!(symbols.iter().any(|symbol| {
        symbol.name == "FileMode.Read"
            && symbol.kind == SymbolKind::ENUM_MEMBER
            && symbol
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("Read"))
    }));
    assert!(
        !symbols.iter().any(|symbol| symbol.name.contains("__ms_")),
        "符号名不应泄漏模块内部重写前缀"
    );
}

/// `枚举名.` 后的成员补全给出该枚举的成员，而不是内建属性。
#[test]
fn member_completion_lists_enum_members() {
    let text = "enum FileMode:\n    Read\n    Write\nfunc main():\n    var mode = FileMode.\n";
    let (snapshot, _) = analyze(text);
    let position = Position::new(4, "    var mode = FileMode.".len() as u32);
    let items = completion_items_for(text, &snapshot, position);
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, vec!["Read", "Write"], "{labels:?}");
    assert!(
        items
            .iter()
            .all(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER)),
        "{items:?}"
    );
}

/// 悬停 `枚举名.成员` 时展示源码名和成员身份，不暴露内部前缀。
#[test]
fn hover_explains_enum_member() {
    let text = "enum FileMode:\n    Read\n    Write\nfunc main():\n    var mode = FileMode.Read\n";
    let (snapshot, _) = analyze(text);
    let base = position_of(text, "= FileMode.Read");
    let position = Position::new(base.line, base.character + "= FileMode.".len() as u32);
    let hover = hover_for(text, &snapshot, position).expect("成员悬停必须可用");
    let tower_lsp::lsp_types::HoverContents::Scalar(marked) = hover.contents else {
        panic!("悬停应返回纯文本");
    };
    let tower_lsp::lsp_types::MarkedString::String(text) = marked else {
        panic!("悬停应返回字符串");
    };
    assert!(text.contains("枚举成员"), "{text}");
    assert!(text.contains("FileMode.Read"), "{text}");
    assert!(!text.contains("__ms_"), "{text}");

    let definition = mas_lang_lsp::record_support::definition_range(
        "enum FileMode:\n    Read\n    Write\nfunc main():\n    var mode = FileMode.Read\n",
        position,
    )
    .expect("成员定义必须可用");
    assert_eq!(definition.start.line, 1, "定义应跳回成员声明行");
}

/// 枚举诊断由编译器前端提供，重复成员和未知成员在编辑器内可见且带源码位置。
#[test]
fn diagnostics_report_enum_errors_from_frontend() {
    for (text, expected) in [
        ("enum FileMode:\n    Read\n    Read\n", "重复"),
        (
            "enum FileMode:\n    Read\nfunc main():\n    print(FileMode.Bogus)\n",
            "没有成员",
        ),
    ] {
        let (_, diagnostics) = analyze(text);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "期望 `{expected}`，实际 {diagnostics:?}"
        );
    }
}
