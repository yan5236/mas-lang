use mas_lang_lsp::analysis::AnalysisService;
use mas_lang_lsp::completion::{completion_items_for, type_items};
use mas_lang_lsp::documents::OpenDocument;
use mas_lang_lsp::hover::hover_for;
use mas_lang_lsp::symbols::SymbolSnapshot;
use maslang::settings::{CompilerSettings, UiLanguage};
use tower_lsp::lsp_types::{Position, Url};

/// 构造基于编辑器内存文本的打开文档，供各语言能力共享同一份源码。
fn open_document(text: &str) -> OpenDocument {
    OpenDocument {
        uri: Url::parse("file:///E:/xiangmu/mas-lang/tmp/bytes/main.ms").unwrap(),
        version: 1,
        path: "E:/xiangmu/mas-lang/tmp/bytes/main.ms".into(),
        text: text.to_string(),
    }
}

/// 使用中文设置分析文档并返回符号快照与诊断。
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

/// 类型补全必须包含 `bytes` 和 `bytes?`，避免编辑器看不到新类型。
#[test]
fn type_completion_includes_bytes() {
    let labels: Vec<String> = type_items().into_iter().map(|item| item.label).collect();
    assert!(labels.iter().any(|label| label == "bytes"), "{labels:?}");
    assert!(labels.iter().any(|label| label == "bytes?"), "{labels:?}");
}

/// 内建函数补全必须包含字节构造与 UTF-8 转换入口。
#[test]
fn builtin_completion_includes_bytes_helpers() {
    let text = "func main():\n    \n";
    let (snapshot, _) = analyze(text);
    let items = completion_items_for(text, &snapshot, position_of(text, "    \n"));
    let labels: Vec<String> = items.into_iter().map(|item| item.label).collect();
    for expected in ["bytes_from_list", "encode_utf8", "decode_utf8"] {
        assert!(
            labels.iter().any(|label| label == expected),
            "补全缺少 `{expected}`：{labels:?}"
        );
    }
}

/// 字节类型的变量与函数返回类型必须出现在文档符号的 detail 中。
#[test]
fn document_symbols_show_bytes_types() {
    let text = "func payload() -> bytes:\n    return bytes_from_list([1]).value\nfunc main():\n    var data: bytes = payload()\n    print(len(data))\n";
    let (snapshot, diagnostics) = analyze(text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let payload = snapshot
        .symbols()
        .iter()
        .find(|symbol| symbol.name == "payload")
        .expect("函数符号必须存在");
    let detail = payload.detail.clone().unwrap_or_default();
    assert!(detail.contains("bytes"), "{detail}");
}

/// 字节变量的悬停提示必须显示 `bytes` 类型，而不是退化成结构体或未知类型。
#[test]
fn hover_reports_bytes_type() {
    let text =
        "func main():\n    var data: bytes = bytes_from_list([1]).value\n    print(data[0])\n";
    let document = open_document(text);
    let service = AnalysisService::with_settings(CompilerSettings::new(UiLanguage::ZhCn));
    let analysis = service.analyze_document(&document);
    let hover = hover_for(text, &analysis.symbols, position_of(text, "data[0]"))
        .expect("字节变量必须有悬停信息");
    let tower_lsp::lsp_types::HoverContents::Scalar(marked) = hover.contents else {
        panic!("悬停应返回纯文本");
    };
    let tower_lsp::lsp_types::MarkedString::String(text) = marked else {
        panic!("悬停应返回字符串");
    };
    assert!(text.contains("bytes"), "{text}");
}

/// 字节与字符串混用、非法索引等错误必须直接进入编辑器诊断。
#[test]
fn bytes_misuse_is_reported_as_diagnostics() {
    for text in [
        "func main():\n    var data = bytes_from_list([1]).value\n    print(data == \"a\")\n",
        "func main():\n    var data = bytes_from_list([1]).value\n    print(int(data))\n",
        "func main():\n    var data = bytes_from_list([\"a\"])\n    print(len(data))\n",
    ] {
        let (_, diagnostics) = analyze(text);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("bytes")
                    || diagnostic.message.contains("字节")
                    || diagnostic.message.contains("列表")),
            "{text} => {diagnostics:?}"
        );
    }
}
