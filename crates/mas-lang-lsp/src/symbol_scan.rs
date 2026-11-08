use super::*;

/// 使用词法单元序列识别已经完整书写的函数、变量和常量声明名。
pub(super) fn scan_tokens_for_symbols(text: &str, tokens: &[Token]) -> Vec<MasSymbol> {
    let mut symbols = Vec::new();
    for index in 0..tokens.len().saturating_sub(1) {
        match (&tokens[index].kind, &tokens[index + 1].kind) {
            (TokenKind::Import, _) => {
                if let Some(symbol) = scan_import_symbol(text, tokens, index) {
                    symbols.push(symbol);
                }
            }
            (TokenKind::Struct, TokenKind::Identifier(name)) => symbols.push(make_symbol(
                text,
                name,
                SymbolKind::STRUCT,
                declaration_range_span(tokens, index, tokens[index + 1].span),
                tokens[index + 1].span,
                Some(format!("struct {name}")),
            )),
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
                    let suffix = if matches!(
                        tokens.get(index + 3).map(|t| &t.kind),
                        Some(TokenKind::Question)
                    ) {
                        "?"
                    } else {
                        ""
                    };
                    parameters.push(format!("{parameter_name}:{type_name}{suffix}"));
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
                    let suffix = if matches!(
                        tokens.get(index + 2).map(|t| &t.kind),
                        Some(TokenKind::Question)
                    ) {
                        "?"
                    } else {
                        ""
                    };
                    return_type = Some(format!("{type_name}{suffix}"));
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
                let suffix = if matches!(
                    tokens.get(index + 2).map(|t| &t.kind),
                    Some(TokenKind::Question)
                ) {
                    "?"
                } else {
                    ""
                };
                return format!("{keyword} {type_name}{suffix}");
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
