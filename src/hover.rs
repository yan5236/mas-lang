use tower_lsp::lsp_types::{Hover, HoverContents, MarkedString, Position, SymbolKind};

use crate::lsp_convert::position_to_byte_offset;
use crate::symbols::{MasSymbol, SymbolSnapshot};

/// 根据光标位置查找当前单词，并返回符号声明种类、类型或函数签名。
pub fn hover_for(text: &str, snapshot: &SymbolSnapshot, position: Position) -> Option<Hover> {
    let word = word_at_position(text, position)?;
    let symbol = snapshot
        .symbols()
        .iter()
        .find(|symbol| symbol.name == word || symbol.name.ends_with(&format!(".{word}")))
        .or_else(|| {
            snapshot.symbols().iter().find(|symbol| {
                symbol
                    .detail
                    .as_deref()
                    .is_some_and(|detail| detail.contains(&word))
            })
        })?;

    Some(Hover {
        contents: HoverContents::Scalar(MarkedString::String(hover_text(symbol))),
        range: Some(symbol.selection_range),
    })
}

/// 兼容旧测试和占位调用的空悬停入口，实际 handler 使用 `hover_for`。
pub fn empty_hover() -> Option<Hover> {
    None
}

/// 组合悬停展示文本，避免协议层了解符号种类的中文渲染细节。
fn hover_text(symbol: &MasSymbol) -> String {
    let kind = match symbol.kind {
        SymbolKind::FUNCTION => "函数",
        SymbolKind::VARIABLE => "变量",
        SymbolKind::CONSTANT => "常量",
        SymbolKind::MODULE => "导入",
        _ => "符号",
    };
    match &symbol.detail {
        Some(detail) => format!("{kind}\n{detail}"),
        None => kind.to_string(),
    }
}

/// 提取光标所在的标识符文本，用于在符号快照中做名称匹配。
fn word_at_position(text: &str, position: Position) -> Option<String> {
    let offset = position_to_byte_offset(text, position);
    let bytes = text.as_bytes();
    let mut start = offset.min(text.len());
    while start > 0 && is_identifier_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset.min(text.len());
    while end < text.len() && is_identifier_byte(bytes[end]) {
        end += 1;
    }
    (start < end).then(|| text[start..end].to_string())
}

/// 判断字节是否可作为 mas-lang 标识符的 ASCII 部分，首批悬停定位按现有样例覆盖。
fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}
