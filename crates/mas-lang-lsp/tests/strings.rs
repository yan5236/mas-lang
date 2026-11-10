use mas_lang_lsp::{
    analysis::AnalysisService, completion::completion_items_for, documents::OpenDocument,
};
use tower_lsp::lsp_types::{Position, Url};

/// 构造仅内存的文档，避免补全测试依赖工作区样例或磁盘内容。
fn document(text: &str) -> OpenDocument {
    OpenDocument {
        uri: Url::parse("file:///E:/string-lsp/main.ms").unwrap(),
        path: "E:/string-lsp/main.ms".into(),
        version: 1,
        text: text.into(),
    }
}

/// 根据唯一片段和片段内字节偏移获取光标位置，支持前置中文按 UTF-16 计数。
fn at(text: &str, marker: &str, extra: usize) -> Position {
    let offset = text.find(marker).unwrap() + extra;
    let prefix = &text[..offset];
    Position::new(
        prefix.bytes().filter(|b| *b == b'\n').count() as u32,
        prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
    )
}

/// 字符串接收者上的成员补全列出全部内建字符串方法，包含新加入的 6 个方法。
#[test]
fn string_receiver_completes_all_builtin_methods() {
    let text = "var text = \"a,b\"\ntext.";
    let analysis = AnalysisService::new().analyze_document(&document(text));
    let items = completion_items_for(text, &analysis.symbols, at(text, "text.", 5));
    let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "trim",
            "contains",
            "starts_with",
            "ends_with",
            "replace",
            "split",
            "index_of",
            "substring",
            "to_lower",
            "to_upper",
            "repeat",
        ]
    );
    let split = items
        .iter()
        .find(|item| item.label == "split")
        .expect("split 补全项");
    assert_eq!(split.detail.as_deref(), Some("list<string>"));
}

/// 字符串参数的接收者也提供方法补全，未标注类型的接收者保持不猜测。
#[test]
fn string_parameter_receiver_completes_methods() {
    let text = "func show(text: string):\n    text.";
    let analysis = AnalysisService::new().analyze_document(&document(text));
    let items = completion_items_for(text, &analysis.symbols, at(text, "text.", 5));
    assert!(items.iter().any(|item| item.label == "to_upper"));
    let unknown = "func show(other):\n    other.";
    let analysis = AnalysisService::new().analyze_document(&document(unknown));
    let items = completion_items_for(unknown, &analysis.symbols, at(unknown, "other.", 6));
    assert!(items.is_empty());
}
