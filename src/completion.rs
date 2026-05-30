use std::collections::HashSet;

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat, Position};

use crate::lsp_convert::position_to_byte_offset;
use crate::symbols::SymbolSnapshot;

/// 返回一阶段可用于验证补全通道的静态关键字候选。
pub fn keyword_items() -> Vec<CompletionItem> {
    vec![
        snippet_keyword_item("func", "func ${1}(${2}) :\n    ${0}"),
        plain_keyword_item("var"),
        plain_keyword_item("const"),
        plain_keyword_item("return"),
        plain_keyword_item("import"),
        plain_keyword_item("export"),
        plain_keyword_item("pub"),
        snippet_keyword_item("main", "func main() :\n    ${0}"),
        snippet_keyword_item("if", "if ${1} :\n    ${0}"),
        snippet_keyword_item("elif", "elif ${1} :\n    ${0}"),
        snippet_keyword_item("else", "else :\n    ${0}"),
        snippet_keyword_item("while", "while ${1} :\n    ${0}"),
        snippet_keyword_item("for", "for ${1} in ${2}..${3} :\n    ${0}"),
    ]
}

/// 根据当前文本位置和符号快照构造补全项，组合语法、类型、内建函数与文件符号。
pub fn completion_items_for(
    text: &str,
    snapshot: &SymbolSnapshot,
    position: Position,
) -> Vec<CompletionItem> {
    let suppress_keywords = is_inside_string_or_comment(text, position);
    let statement_start = is_statement_start_context(text, position);
    let mut items = Vec::new();
    if !suppress_keywords && statement_start {
        items.extend(keyword_items());
        items.extend(type_items());
    }
    items.extend(builtin_items());
    items.extend(symbol_items(snapshot));
    dedupe_items(items)
}

/// 返回内建类型补全项，避免调用方重复维护类型关键字列表。
fn type_items() -> Vec<CompletionItem> {
    ["int", "float", "bool", "string"]
        .into_iter()
        .map(|label| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            ..CompletionItem::default()
        })
        .collect()
}

/// 返回首批内建函数补全项，覆盖当前编译器已有的基础运行时入口。
fn builtin_items() -> Vec<CompletionItem> {
    [
        ("print", "print(${1})"),
        ("string", "string(${1})"),
        ("int", "int(${1})"),
        ("float", "float(${1})"),
        ("bool", "bool(${1})"),
    ]
    .into_iter()
    .map(|(label, insert_text)| CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::FUNCTION),
        insert_text: Some(insert_text.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..CompletionItem::default()
    })
    .collect()
}

/// 将当前文件符号转换为补全项，保留 detail 以便客户端展示类型或签名。
fn symbol_items(snapshot: &SymbolSnapshot) -> Vec<CompletionItem> {
    snapshot
        .symbols()
        .iter()
        .map(|symbol| CompletionItem {
            label: symbol.name.clone(),
            kind: Some(match symbol.kind {
                tower_lsp::lsp_types::SymbolKind::FUNCTION => CompletionItemKind::FUNCTION,
                tower_lsp::lsp_types::SymbolKind::CONSTANT => CompletionItemKind::CONSTANT,
                tower_lsp::lsp_types::SymbolKind::MODULE => CompletionItemKind::MODULE,
                _ => CompletionItemKind::VARIABLE,
            }),
            detail: symbol.detail.clone(),
            insert_text: (symbol.kind == tower_lsp::lsp_types::SymbolKind::FUNCTION)
                .then(|| format!("{}(${{1}})", symbol.name)),
            insert_text_format: (symbol.kind == tower_lsp::lsp_types::SymbolKind::FUNCTION)
                .then_some(InsertTextFormat::SNIPPET),
            ..CompletionItem::default()
        })
        .collect()
}

/// 构造普通关键字补全项，适用于不需要插入代码骨架的语法词。
fn plain_keyword_item(label: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..CompletionItem::default()
    }
}

/// 构造 snippet 关键字补全项，让块级语法能一次插入常用结构和缩进占位。
fn snippet_keyword_item(label: &str, insert_text: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        insert_text: Some(insert_text.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..CompletionItem::default()
    }
}

/// 删除同名补全项，保证内建函数和类型重名时客户端列表不会重复膨胀。
fn dedupe_items(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.label.clone()))
        .collect()
}

/// 判断光标是否处于字符串字面量或行注释中，用于抑制语法关键字补全。
fn is_inside_string_or_comment(text: &str, position: Position) -> bool {
    let offset = position_to_byte_offset(text, position);
    let line_start = text[..offset]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let before_cursor = &text[line_start..offset];
    if before_cursor.contains("//") {
        return true;
    }

    let mut escaped = false;
    let mut quote_count = 0usize;
    for ch in before_cursor.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            quote_count += 1;
        }
    }
    quote_count % 2 == 1
}

/// 判断光标是否处于新语句开头，避免在函数名或表达式位置返回整段代码骨架。
fn is_statement_start_context(text: &str, position: Position) -> bool {
    let offset = position_to_byte_offset(text, position);
    let line_start = text[..offset]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let before_cursor = &text[line_start..offset];
    before_cursor
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch.is_whitespace())
        && !before_cursor.split_whitespace().any(|word| word == "func")
}
