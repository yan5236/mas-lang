use maslang::ast::{BindingKind, ImportKind, Program, Stmt, TypeName};
use maslang::lexer::Lexer;
use maslang::settings::CompilerSettings;
use maslang::source::{SourceFile, Span};
use maslang::token::{Token, TokenKind};
use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind};

use crate::lsp_convert::span_to_range;

#[path = "symbol_scan.rs"]
mod symbol_scan;
use symbol_scan::scan_tokens_for_symbols;

/// 表示 LSP 能力共享的 mas-lang 符号模型。
#[derive(Clone, Debug, PartialEq)]
pub struct MasSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    pub detail: Option<String>,
}

/// 保存当前文档符号快照，并标记它来自完整 AST 还是降级扫描。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymbolSnapshot {
    symbols: Vec<MasSymbol>,
    fallback: bool,
}

impl SymbolSnapshot {
    /// 使用给定符号列表创建完整分析快照，供成功前端分析结果复用。
    pub fn precise(symbols: Vec<MasSymbol>) -> Self {
        Self {
            symbols,
            fallback: false,
        }
    }

    /// 使用给定符号列表创建降级快照，供语法错误或语义错误场景继续提供编辑能力。
    pub fn fallback(symbols: Vec<MasSymbol>) -> Self {
        Self {
            symbols,
            fallback: true,
        }
    }

    /// 返回快照中的符号列表，供补全、悬停和测试共享同一份符号数据。
    pub fn symbols(&self) -> &[MasSymbol] {
        &self.symbols
    }

    /// 返回当前快照是否来自降级扫描，便于测试和后续日志区分精度来源。
    pub fn is_fallback(&self) -> bool {
        self.fallback
    }
}

/// 从完整 AST 中提取文档符号，首批覆盖顶层和函数体内的声明类节点。
pub fn symbols_from_program(text: &str, program: &Program) -> SymbolSnapshot {
    let mut symbols = Vec::new();
    for statement in &program.statements {
        collect_statement_symbol(text, statement, &mut symbols);
    }
    SymbolSnapshot::precise(symbols)
}

/// 语法或语义分析失败时使用词法结果降级提取符号，避免编辑器能力完全消失。
pub fn fallback_symbols_from_source(
    source: &SourceFile,
    settings: &CompilerSettings,
) -> SymbolSnapshot {
    let tokens = Lexer::new(source, settings).tokenize().unwrap_or_default();
    if let Some(program) = crate::record_support::parse_editor_program(source.text()) {
        return SymbolSnapshot::fallback(symbols_from_program(source.text(), &program).symbols);
    }
    SymbolSnapshot::fallback(scan_tokens_for_symbols(source.text(), &tokens))
}

/// 将内部符号模型转换为 LSP 文档符号响应使用的结构。
#[allow(deprecated)]
pub fn document_symbols_for(snapshot: &SymbolSnapshot) -> Vec<DocumentSymbol> {
    snapshot
        .symbols()
        .iter()
        .map(|symbol| DocumentSymbol {
            name: symbol.name.clone(),
            detail: symbol.detail.clone(),
            kind: symbol.kind,
            tags: None,
            deprecated: None,
            range: symbol.range,
            selection_range: symbol.selection_range,
            children: None,
        })
        .collect()
}

/// 递归收集语句中的声明类符号，兼顾顶层声明与函数体局部变量。
fn collect_statement_symbol(text: &str, statement: &Stmt, symbols: &mut Vec<MasSymbol>) {
    match statement {
        Stmt::StructDecl {
            name, fields, span, ..
        } => {
            symbols.push(make_symbol(
                text,
                name,
                SymbolKind::STRUCT,
                *span,
                name_selection_span(text, *span, name),
                Some(format!("struct {name}")),
            ));
            for field in fields {
                symbols.push(make_symbol(
                    text,
                    &format!("{name}.{}", field.name),
                    SymbolKind::FIELD,
                    field.span,
                    field.span,
                    Some(format!(
                        "{}: {}",
                        field.name,
                        type_name_text(&field.type_name)
                    )),
                ));
            }
        }
        Stmt::EnumDecl {
            name, members, span, ..
        } => {
            let display_name = declared_name_in_span(text, *span, "enum").unwrap_or(name);
            symbols.push(make_symbol(
                text,
                display_name,
                SymbolKind::ENUM,
                *span,
                name_selection_span(text, *span, display_name),
                Some(format!("enum {display_name}")),
            ));
            for member in members {
                symbols.push(make_symbol(
                    text,
                    &format!("{display_name}.{}", member.name),
                    SymbolKind::ENUM_MEMBER,
                    member.span,
                    member.span,
                    Some(format!("{}（{} 成员）", member.name, display_name)),
                ));
            }
        }
        Stmt::FunctionDecl {
            name,
            parameters,
            return_type,
            body,
            span,
            ..
        } => {
            let display_name = declared_name_in_span(text, *span, "func").unwrap_or(name);
            let detail = format!(
                "func {display_name}({}){}",
                parameters
                    .iter()
                    .map(|parameter| format!(
                        "{}:{}",
                        parameter.name,
                        type_name_text(&parameter.type_name)
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                return_type
                    .as_ref()
                    .map(|name| format!(" -> {}", type_name_text(name)))
                    .unwrap_or_default()
            );
            symbols.push(make_symbol(
                text,
                display_name,
                SymbolKind::FUNCTION,
                *span,
                name_selection_span(text, *span, display_name),
                Some(detail),
            ));
            for child in body {
                collect_statement_symbol(text, child, symbols);
            }
        }
        Stmt::ExternFunctionDecl {
            name,
            parameters,
            return_type,
            span,
            ..
        } => {
            let display_name = declared_name_in_span(text, *span, "func").unwrap_or(name);
            let detail = format!(
                "extern func {display_name}({}){}",
                parameters
                    .iter()
                    .map(|parameter| format!(
                        "{}:{}",
                        parameter.name,
                        type_name_text(&parameter.type_name)
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                return_type
                    .as_ref()
                    .map(|name| format!(" -> {}", type_name_text(name)))
                    .unwrap_or_default()
            );
            symbols.push(make_symbol(
                text,
                display_name,
                SymbolKind::FUNCTION,
                *span,
                name_selection_span(text, *span, display_name),
                Some(detail),
            ));
        }
        Stmt::Import { path, kind, span } | Stmt::ExportImport { path, kind, span } => {
            let module = path
                .alias
                .clone()
                .unwrap_or_else(|| path.normalized_segments().join("."));
            match kind {
                ImportKind::Named(items) => {
                    for item in items {
                        let name = item.alias.as_ref().unwrap_or(&item.name);
                        symbols.push(make_symbol(
                            text,
                            &format!("{module}.{name}"),
                            SymbolKind::MODULE,
                            *span,
                            item.span,
                            Some("import".to_string()),
                        ));
                    }
                }
                ImportKind::All => symbols.push(make_symbol(
                    text,
                    &format!("{module}.*"),
                    SymbolKind::MODULE,
                    *span,
                    path.span,
                    Some("import *".to_string()),
                )),
            }
        }
        Stmt::VarDecl {
            binding_kind,
            name,
            annotation,
            span,
            ..
        } => {
            let kind = match binding_kind {
                BindingKind::Var => SymbolKind::VARIABLE,
                BindingKind::Const => SymbolKind::CONSTANT,
            };
            let detail = format!(
                "{}{}",
                match binding_kind {
                    BindingKind::Var => "var",
                    BindingKind::Const => "const",
                },
                annotation
                    .as_ref()
                    .map(|name| format!(" {}", type_name_text(name)))
                    .unwrap_or_default()
            );
            symbols.push(make_symbol(
                text,
                name,
                kind,
                *span,
                name_selection_span(text, *span, name),
                Some(detail),
            ));
        }
        _ => {}
    }
}

/// 创建统一的 `MasSymbol`，集中处理 Span 到 LSP Range 的转换。
fn make_symbol(
    text: &str,
    name: &str,
    kind: SymbolKind,
    range_span: Span,
    selection_span: Span,
    detail: Option<String>,
) -> MasSymbol {
    MasSymbol {
        name: name.to_string(),
        kind,
        range: span_to_range(text, range_span),
        selection_range: span_to_range(text, selection_span),
        detail,
    }
}

/// 在语句跨度内寻找声明名的真实源码跨度，避免 selectionRange 落在整条语句上。
fn name_selection_span(text: &str, span: Span, name: &str) -> Span {
    text.get(span.start..span.end)
        .and_then(|slice| {
            slice
                .match_indices(name)
                .find(|(offset, _)| {
                    let before = slice[..*offset].chars().next_back();
                    let after = slice[*offset + name.len()..].chars().next();
                    let identifier = |ch: char| ch.is_alphanumeric() || ch == '_';
                    !before.is_some_and(identifier) && !after.is_some_and(identifier)
                })
                .map(|(offset, _)| span.start + offset)
        })
        .map(|start| Span::new(start, start + name.len()))
        .unwrap_or(span)
}

/// 从源码声明片段中读取用户实际书写的名称，避免展示编译器模块装载后的内部重写名。
fn declared_name_in_span<'a>(text: &'a str, span: Span, keyword: &str) -> Option<&'a str> {
    let slice = text.get(span.start..span.end)?;
    let start = slice.find(keyword)? + keyword.len();
    let rest = slice[start..].trim_start();
    let end = rest
        .char_indices()
        .find_map(|(index, ch)| (!is_identifier_char(ch)).then_some(index))
        .unwrap_or(rest.len());
    (end > 0).then_some(&rest[..end])
}

/// 判断字符是否可以作为声明名的一部分，供源码声明名恢复使用。
fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// 将 AST 类型名转换为用户可读的 mas-lang 类型文本。
pub(crate) fn type_name_text(name: &TypeName) -> String {
    match name {
        TypeName::Named(name) => name.clone(),
        TypeName::Nullable(inner) => format!("{}?", type_name_text(inner)),
        TypeName::Int => "int".to_string(),
        TypeName::Float => "float".to_string(),
        TypeName::Bool => "bool".to_string(),
        TypeName::String => "string".to_string(),
        TypeName::Bytes => "bytes".to_string(),
        TypeName::List(inner) => format!("list<{}>", type_name_text(inner)),
        TypeName::Map(key, value) => {
            format!("map<{}, {}>", type_name_text(key), type_name_text(value))
        }
        TypeName::Array(inner, size) => format!("array<{}, {}>", type_name_text(inner), size),
    }
}
