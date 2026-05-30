use mascript_compiler_llvm::ast::{BindingKind, ImportKind, Program, Stmt, TypeName};
use mascript_compiler_llvm::lexer::Lexer;
use mascript_compiler_llvm::settings::CompilerSettings;
use mascript_compiler_llvm::source::{SourceFile, Span};
use mascript_compiler_llvm::token::{Token, TokenKind};
use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind};

use crate::lsp_convert::span_to_range;

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
            let module = path.normalized_segments().join(".");
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

/// 使用词法单元序列识别已经完整书写的函数、变量和常量声明名。
fn scan_tokens_for_symbols(text: &str, tokens: &[Token]) -> Vec<MasSymbol> {
    let mut symbols = Vec::new();
    for index in 0..tokens.len().saturating_sub(1) {
        match (&tokens[index].kind, &tokens[index + 1].kind) {
            (TokenKind::Import, _) => {
                if let Some(symbol) = scan_import_symbol(text, tokens, index) {
                    symbols.push(symbol);
                }
            }
            (TokenKind::Func, TokenKind::Identifier(name)) => symbols.push(make_symbol(
                text,
                name,
                SymbolKind::FUNCTION,
                declaration_range_span(tokens, index, tokens[index + 1].span),
                tokens[index + 1].span,
                Some(scan_function_detail(tokens, index, name)),
            )),
            (TokenKind::Var, TokenKind::Identifier(name)) => symbols.push(make_symbol(
                text,
                name,
                SymbolKind::VARIABLE,
                declaration_range_span(tokens, index, tokens[index + 1].span),
                tokens[index + 1].span,
                Some(scan_binding_detail(tokens, index, "var")),
            )),
            (TokenKind::Const, TokenKind::Identifier(name)) => symbols.push(make_symbol(
                text,
                name,
                SymbolKind::CONSTANT,
                declaration_range_span(tokens, index, tokens[index + 1].span),
                tokens[index + 1].span,
                Some(scan_binding_detail(tokens, index, "const")),
            )),
            _ => {}
        }
    }
    symbols
}

/// 从降级词法片段中读取函数参数和返回类型，尽量保持悬停签名与 AST 路径一致。
fn scan_function_detail(tokens: &[Token], start: usize, name: &str) -> String {
    let mut index = start + 2;
    let mut parameters = Vec::new();
    let mut return_type = None;
    while index < tokens.len() {
        match &tokens[index].kind {
            TokenKind::Identifier(parameter_name)
                if matches!(
                    tokens.get(index + 1).map(|token| &token.kind),
                    Some(TokenKind::Colon)
                ) =>
            {
                if let Some(Token {
                    kind: TokenKind::Identifier(type_name),
                    ..
                }) = tokens.get(index + 2)
                {
                    parameters.push(format!("{parameter_name}:{type_name}"));
                    index += 3;
                    continue;
                }
            }
            TokenKind::Arrow => {
                if let Some(Token {
                    kind: TokenKind::Identifier(type_name),
                    ..
                }) = tokens.get(index + 1)
                {
                    return_type = Some(type_name.clone());
                }
            }
            TokenKind::Colon | TokenKind::Newline | TokenKind::Eof => break,
            _ => {}
        }
        index += 1;
    }

    format!(
        "func {name}({}){}",
        parameters.join(", "),
        return_type
            .map(|name| format!(" -> {name}"))
            .unwrap_or_default()
    )
}

/// 从降级词法片段中读取变量或常量的可选类型标注，用于补全和文档符号展示。
fn scan_binding_detail(tokens: &[Token], start: usize, keyword: &str) -> String {
    let mut index = start + 2;
    while index + 1 < tokens.len() {
        match (&tokens[index].kind, &tokens[index + 1].kind) {
            (TokenKind::Colon, TokenKind::Identifier(type_name)) => {
                return format!("{keyword} {type_name}");
            }
            (TokenKind::Equal | TokenKind::Newline | TokenKind::Eof, _) => break,
            _ => index += 1,
        }
    }
    keyword.to_string()
}

/// 从 `import 模块 { 名称 }` 的词法片段中提取首个具名导入，供错误状态下仍展示导入符号。
fn scan_import_symbol(text: &str, tokens: &[Token], start: usize) -> Option<MasSymbol> {
    let mut index = start + 1;
    let mut module_parts = Vec::new();
    while index < tokens.len() {
        match &tokens[index].kind {
            TokenKind::Identifier(part) => {
                module_parts.push(part.clone());
                index += 1;
            }
            TokenKind::Dot => index += 1,
            TokenKind::LeftBrace => break,
            _ => return None,
        }
    }

    while index < tokens.len() {
        if let TokenKind::Identifier(name) = &tokens[index].kind {
            let module = module_parts.join(".");
            return Some(make_symbol(
                text,
                &format!("{module}.{name}"),
                SymbolKind::MODULE,
                declaration_range_span(tokens, start, tokens[index].span),
                tokens[index].span,
                Some("import".to_string()),
            ));
        }
        if matches!(tokens[index].kind, TokenKind::Newline | TokenKind::Eof) {
            break;
        }
        index += 1;
    }
    None
}

/// 计算降级扫描声明的完整范围，确保 LSP selectionRange 总是落在 range 内部。
fn declaration_range_span(tokens: &[Token], start: usize, selection_span: Span) -> Span {
    let mut end = selection_span.end;
    let mut index = start;
    while index < tokens.len() {
        let token = &tokens[index];
        if matches!(token.kind, TokenKind::Newline | TokenKind::Eof) {
            break;
        }
        end = token.span.end.max(end);
        index += 1;
    }
    Span::new(tokens[start].span.start, end)
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
        .and_then(|slice| slice.find(name).map(|offset| span.start + offset))
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
fn type_name_text(name: &TypeName) -> String {
    match name {
        TypeName::Int => "int".to_string(),
        TypeName::Float => "float".to_string(),
        TypeName::Bool => "bool".to_string(),
        TypeName::String => "string".to_string(),
        TypeName::List(inner) => format!("list<{}>", type_name_text(inner)),
        TypeName::Map(key, value) => {
            format!("map<{}, {}>", type_name_text(key), type_name_text(value))
        }
        TypeName::Array(inner, size) => format!("array<{}, {}>", type_name_text(inner), size),
    }
}
