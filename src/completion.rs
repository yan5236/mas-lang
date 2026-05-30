use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};

/// 返回一阶段可用于验证补全通道的静态关键字候选。
pub fn keyword_items() -> Vec<CompletionItem> {
    ["func", "var", "return", "import", "export", "pub"]
        .into_iter()
        .map(|label| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..CompletionItem::default()
        })
        .collect()
}
