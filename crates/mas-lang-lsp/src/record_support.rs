use crate::lsp_convert::{position_to_byte_offset, span_to_range};
use crate::symbols::{MasSymbol, type_name_text};
use maslang::ast::{Expr, Literal, Parameter, Program, Stmt, TypeName};
use maslang::{
    lexer::Lexer,
    parser::Parser,
    settings::CompilerSettings,
    source::{SourceFile, Span},
};
use std::collections::HashMap;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Position, Range, SymbolKind};

/// 保存局部名称类型和有效作用域，避免同名局部变量串到其他函数。
struct Binding {
    name: String,
    ty: Option<TypeName>,
    start: usize,
    scope: Span,
}

/// 当前内存文档的结构体字段、枚举成员、函数返回值及局部类型索引；不读取磁盘上的旧编辑内容。
struct RecordIndex {
    records: HashMap<String, (Span, Vec<Parameter>)>,
    /// 枚举外部名到（声明区间，成员名与成员声明区间）的索引。
    enums: HashMap<String, (Span, Vec<(String, Span)>)>,
    functions: HashMap<String, TypeName>,
    bindings: Vec<Binding>,
}

/// 解析原始文档；对行末尚未输入成员名的点号作等长空格恢复，保留其余源码位置。
/// 恢复仅用于编辑能力，诊断始终来自未经修补的编译器前端。
pub(crate) fn parse_editor_program(text: &str) -> Option<Program> {
    let mut repaired = text.as_bytes().to_vec();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed.ends_with('.') && !trimmed.ends_with("..") {
            repaired[offset + trimmed.len() - 1] = b' ';
        }
        offset += line.len();
    }
    let source = SourceFile::new("<editor>", String::from_utf8(repaired).ok()?);
    let settings = CompilerSettings::default_settings();
    let tokens = Lexer::new(&source, &settings).tokenize().ok()?;
    Parser::new(tokens, &settings).parse_program().ok()
}

impl RecordIndex {
    /// 先登记顶层类型和函数返回类型，再按词法作用域收集参数及声明。
    fn new(text: &str) -> Option<Self> {
        let program = parse_editor_program(text)?;
        let mut index = Self {
            records: HashMap::new(),
            enums: HashMap::new(),
            functions: HashMap::new(),
            bindings: Vec::new(),
        };
        for stmt in &program.statements {
            match stmt {
                Stmt::StructDecl {
                    name, fields, span, ..
                } => {
                    index.records.insert(name.clone(), (*span, fields.clone()));
                }
                Stmt::EnumDecl {
                    name, members, span, ..
                } => {
                    index.enums.insert(
                        name.clone(),
                        (
                            *span,
                            members
                                .iter()
                                .map(|m| (m.name.clone(), m.span))
                                .collect(),
                        ),
                    );
                }
                Stmt::FunctionDecl {
                    name,
                    return_type: Some(ty),
                    ..
                }
                | Stmt::ExternFunctionDecl {
                    name,
                    return_type: Some(ty),
                    ..
                } => {
                    index.functions.insert(name.clone(), ty.clone());
                }
                _ => {}
            }
        }
        index.collect(&program.statements, Span::new(0, text.len()));
        Some(index)
    }

    /// 收集变量和函数参数；不能推导的声明也占据名称，避免错误穿透到外层同名绑定。
    fn collect(&mut self, body: &[Stmt], scope: Span) {
        for stmt in body {
            match stmt {
                Stmt::VarDecl {
                    name,
                    annotation,
                    initializer,
                    span,
                    ..
                } => {
                    let ty = annotation
                        .clone()
                        .or_else(|| initializer.as_ref().and_then(|e| self.infer(e, span.start)));
                    self.bindings.push(Binding {
                        name: name.clone(),
                        ty,
                        start: span.start,
                        scope,
                    });
                }
                Stmt::FunctionDecl {
                    parameters,
                    body,
                    span,
                    ..
                } => {
                    for p in parameters {
                        self.bindings.push(Binding {
                            name: p.name.clone(),
                            ty: Some(p.type_name.clone()),
                            start: span.start,
                            scope: *span,
                        });
                    }
                    self.collect(body, *span);
                }
                Stmt::If {
                    branches,
                    else_branch,
                    span,
                } => {
                    for branch in branches {
                        self.collect(&branch.body, branch.span);
                    }
                    if let Some(body) = else_branch {
                        self.collect(body, block_span(body, *span));
                    }
                }
                Stmt::While { body, span, .. } => self.collect(body, *span),
                Stmt::ForRange {
                    name, body, span, ..
                } => {
                    self.bindings.push(Binding {
                        name: name.clone(),
                        ty: Some(TypeName::Int),
                        start: span.start,
                        scope: *span,
                    });
                    self.collect(body, *span);
                }
                Stmt::ForEach {
                    key_name,
                    value_name,
                    body,
                    span,
                    ..
                } => {
                    for name in std::iter::once(key_name).chain(value_name.iter()) {
                        self.bindings.push(Binding {
                            name: name.clone(),
                            ty: None,
                            start: span.start,
                            scope: *span,
                        });
                    }
                    self.collect(body, *span);
                }
                _ => {}
            }
        }
    }

    /// 以有限深度推导具名变量、构造调用、用户函数返回和成员链，不替代编译器类型检查。
    fn infer(&self, expr: &Expr, position: usize) -> Option<TypeName> {
        match expr {
            Expr::Identifier { name, .. } => self
                .bindings
                .iter()
                .filter(|b| {
                    b.name == *name
                        && b.start <= position
                        && b.scope.start <= position
                        && position <= b.scope.end
                })
                .max_by_key(|b| (b.scope.start, b.start))
                .and_then(|b| b.ty.clone())
                .or_else(|| {
                    self.enums
                        .contains_key(name)
                        .then(|| TypeName::Named(name.clone()))
                }),
            Expr::Call { callee, .. } => {
                if let Expr::Identifier { name, .. } = callee.as_ref() {
                    if self.records.contains_key(name) {
                        Some(TypeName::Named(name.clone()))
                    } else {
                        self.functions.get(name).cloned()
                    }
                } else {
                    None
                }
            }
            Expr::Grouped { expression, .. } => self.infer(expression, position),
            Expr::FieldAccess { target, name, .. } => {
                self.member_type(&self.infer(target, position)?, name)
            }
            Expr::PropertyAccess {
                target, property, ..
            } => self.member_type(&self.infer(target, position)?, property.display_name()),
            Expr::Literal { value, .. } => match value {
                Literal::Int(_) => Some(TypeName::Int),
                Literal::Float(_) => Some(TypeName::Float),
                Literal::Bool(_) => Some(TypeName::Bool),
                Literal::String(_) => Some(TypeName::String),
                Literal::Null => None,
            },
            _ => None,
        }
    }

    /// 取得具名字段类型、枚举成员类型，或对可空类型 `.value` 去掉问号；不会直接展开可空对象的字段。
    fn member_type(&self, ty: &TypeName, member: &str) -> Option<TypeName> {
        match ty {
            TypeName::Nullable(inner) if member == "value" => Some(*inner.clone()),
            TypeName::Named(name) => self
                .records
                .get(name)
                .and_then(|(_, fields)| {
                    fields
                        .iter()
                        .find(|f| f.name == member)
                        .map(|f| f.type_name.clone())
                })
                .or_else(|| {
                    self.enums
                        .get(name)
                        .filter(|(_, members)| {
                            members.iter().any(|(member_name, _)| member_name == member)
                        })
                        .map(|_| TypeName::Named(name.clone()))
                }),
            _ => None,
        }
    }

    /// 解析点号前同一行中的接收者表达式；逐个尝试词边界，支持构造调用和多层字段链。
    fn receiver(&self, text: &str, dot: usize) -> Option<TypeName> {
        let start = text[..dot].rfind('\n').map_or(0, |i| i + 1);
        let line = &text[start..dot];
        for (i, ch) in line.char_indices() {
            if !(ch.is_ascii_alphabetic() || ch == '_' || ch == '(') {
                continue;
            }
            if i > 0 && identifier(line.as_bytes()[i - 1]) {
                continue;
            }
            if let Some(program) = parse_editor_program(&line[i..]) {
                if let [Stmt::ExprStmt { expr, .. }] = program.statements.as_slice() {
                    if let Some(ty) = self.infer(expr, dot) {
                        return Some(ty);
                    }
                }
            }
        }
        None
    }
}

/// 用块内首末语句定义作用域；空块回退到父语句位置。
fn block_span(body: &[Stmt], fallback: Span) -> Span {
    body.first()
        .zip(body.last())
        .map(|(a, b)| Span::merge(a.span(), b.span()))
        .unwrap_or(fallback)
}

/// 判断 ASCII 标识符字符，与当前编译器的名称规则一致。
fn identifier(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 找到光标所在单词的字节区间，支持尚未输入任何成员名的补全位置。
fn word_span(text: &str, position: Position) -> Span {
    let offset = position_to_byte_offset(text, position);
    let mut start = offset;
    let mut end = offset;
    while start > 0 && identifier(text.as_bytes()[start - 1]) {
        start -= 1;
    }
    while end < text.len() && identifier(text.as_bytes()[end]) {
        end += 1;
    }
    Span::new(start, end)
}

/// 在成员上下文仅返回该接收者的字段或可空 `.value`；未知接收者返回空列表而不猜测字段。
pub fn member_completions(text: &str, position: Position) -> Option<Vec<CompletionItem>> {
    let span = word_span(text, position);
    let dot = span.start.checked_sub(1)?;
    if text.as_bytes()[dot] != b'.' {
        return None;
    }
    let Some(index) = RecordIndex::new(text) else {
        return Some(Vec::new());
    };
    let items = match index.receiver(text, dot) {
        Some(TypeName::Named(name)) => {
            if let Some((_, fields)) = index.records.get(&name) {
                fields
                    .iter()
                    .map(|f| CompletionItem {
                        label: f.name.clone(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(format!("{}: {}", f.name, type_name_text(&f.type_name))),
                        ..CompletionItem::default()
                    })
                    .collect()
            } else if let Some((_, members)) = index.enums.get(&name) {
                members
                    .iter()
                    .map(|(member, _)| CompletionItem {
                        label: member.clone(),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        detail: Some(format!("{name}.{member}")),
                        ..CompletionItem::default()
                    })
                    .collect()
            } else {
                Vec::new()
            }
        }
        Some(TypeName::String) => crate::completion::string_method_items(),
        Some(TypeName::Nullable(inner)) => vec![CompletionItem {
            label: "value".into(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(format!(
                "{}（先判断 != null；空值取值会报错）",
                type_name_text(&inner)
            )),
            ..CompletionItem::default()
        }],
        _ => Vec::new(),
    };
    Some(items)
}

/// 按接收者类型解析字段或当前文件结构体类型声明，用于精确悬停及定义跳转。
pub fn symbol_at(text: &str, position: Position) -> Option<MasSymbol> {
    let span = word_span(text, position);
    let word = text.get(span.start..span.end)?;
    let source = SourceFile::new("<editor>", text.into());
    let settings = CompilerSettings::default_settings();
    let tokens = Lexer::new(&source, &settings).tokenize().ok()?;
    if !tokens.iter().any(|token| {
        token.span == span
            && matches!(
                token.kind,
                maslang::token::TokenKind::Identifier(_)
            )
    }) {
        return None;
    }
    let index = RecordIndex::new(text)?;
    for (record, (_, fields)) in &index.records {
        if let Some(field) = fields.iter().find(|f| f.span == span) {
            return Some(MasSymbol {
                name: format!("{record}.{word}"),
                kind: SymbolKind::FIELD,
                range: span_to_range(text, span),
                selection_range: span_to_range(text, span),
                detail: Some(format!("{word}: {}", type_name_text(&field.type_name))),
            });
        }
    }
    for (enum_name, (_, members)) in &index.enums {
        if let Some((_, member_span)) = members.iter().find(|(_, member_span)| *member_span == span) {
            return Some(MasSymbol {
                name: format!("{enum_name}.{word}"),
                kind: SymbolKind::ENUM_MEMBER,
                range: span_to_range(text, *member_span),
                selection_range: span_to_range(text, *member_span),
                detail: Some(format!("{enum_name}.{word}")),
            });
        }
    }
    if span.start > 0 && text.as_bytes()[span.start - 1] == b'.' {
        let ty = index.receiver(text, span.start - 1)?;
        match ty {
            TypeName::Named(record) => {
                if let Some((_, fields)) = index.records.get(&record) {
                    if let Some(f) = fields.iter().find(|f| f.name == word) {
                        return Some(MasSymbol {
                            name: format!("{record}.{word}"),
                            kind: SymbolKind::FIELD,
                            range: span_to_range(text, f.span),
                            selection_range: span_to_range(text, f.span),
                            detail: Some(format!("{word}: {}", type_name_text(&f.type_name))),
                        });
                    }
                    return None;
                }
                let (_, members) = index.enums.get(&record)?;
                let (_, member_span) = members.iter().find(|(member, _)| member == word)?;
                return Some(MasSymbol {
                    name: format!("{record}.{word}"),
                    kind: SymbolKind::ENUM_MEMBER,
                    range: span_to_range(text, *member_span),
                    selection_range: span_to_range(text, *member_span),
                    detail: Some(format!("{record}.{word}（枚举成员）")),
                });
            }
            TypeName::Nullable(inner) if word == "value" => {
                return Some(MasSymbol {
                    name: "value".into(),
                    kind: SymbolKind::PROPERTY,
                    range: span_to_range(text, span),
                    selection_range: span_to_range(text, span),
                    detail: Some(format!("{}（取值前检查 != null）", type_name_text(&inner))),
                });
            }
            _ => return None,
        }
    }
    if let Some((declaration, _)) = index.enums.get(word) {
        let fragment = text.get(declaration.start..declaration.end)?;
        let start = declaration.start + fragment.find(word)?;
        return Some(MasSymbol {
            name: word.into(),
            kind: SymbolKind::ENUM,
            range: span_to_range(text, *declaration),
            selection_range: span_to_range(text, Span::new(start, start + word.len())),
            detail: Some(format!("enum {word}")),
        });
    }
    let (declaration, _) = index.records.get(word)?;
    let fragment = text.get(declaration.start..declaration.end)?;
    let start = declaration.start + fragment.find(word)?;
    Some(MasSymbol {
        name: word.into(),
        kind: SymbolKind::STRUCT,
        range: span_to_range(text, *declaration),
        selection_range: span_to_range(text, Span::new(start, start + word.len())),
        detail: Some(format!("struct {word}")),
    })
}

/// 返回当前文件结构体/字段的声明位置；内建 `.value` 没有源码定义，不伪造跳转。
pub fn definition_range(text: &str, position: Position) -> Option<Range> {
    let symbol = symbol_at(text, position)?;
    (symbol.kind != SymbolKind::PROPERTY).then_some(symbol.selection_range)
}
