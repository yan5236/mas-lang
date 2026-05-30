use tower_lsp::lsp_types::{Range, SymbolKind};

/// 表示 LSP 能力共享的 mas-lang 符号模型。
#[derive(Clone, Debug, PartialEq)]
pub struct MasSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    pub detail: Option<String>,
}
