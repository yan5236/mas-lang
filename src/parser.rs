use crate::ast::{BinaryOp, Expr, Literal, Program, Stmt, TypeName, UnaryOp};
use crate::diagnostic::{CompilerError, MessageId};
use crate::settings::CompilerSettings;
use crate::source::Span;
use crate::token::{Token, TokenKind, TokenTag};

/// 根据词法单元序列构建第一阶段使用的抽象语法树。
pub struct Parser<'a> {
    tokens: Vec<Token>,
    current: usize,
    settings: &'a CompilerSettings,
}

impl<'a> Parser<'a> {
    /// 使用完整词法单元列表和设置创建新的语法分析器。
    pub fn new(tokens: Vec<Token>, settings: &'a CompilerSettings) -> Self {
        Self {
            tokens,
            current: 0,
            settings,
        }
    }

    /// 解析整个文件并生成顶层程序节点。
    pub fn parse_program(&mut self) -> Result<Program, CompilerError> {
        let _ = self.settings;
        let mut statements = Vec::new();
        self.skip_newlines();

        while !self.check(TokenTag::Eof) {
            statements.push(self.parse_statement()?);
            self.consume_statement_boundary()?;
        }

        Ok(Program { statements })
    }

    /// 根据当前词法单元判断应进入哪一种语句分支。
    fn parse_statement(&mut self) -> Result<Stmt, CompilerError> {
        if self.match_tag(TokenTag::Var) {
            return self.parse_variable_declaration();
        }

        if self.is_unsupported_keyword() {
            let token = self.advance().clone();
            return Err(CompilerError::syntax(
                MessageId::SyntaxUnsupportedKeyword,
                vec![self.keyword_name(&token.kind).to_string()],
                token.span,
            ));
        }

        self.parse_expression_statement()
    }

    /// 解析 `var name[:type] = expr` 形式的变量声明语句。
    fn parse_variable_declaration(&mut self) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        let (name, _) = self.consume_identifier()?;
        let annotation = if self.match_tag(TokenTag::Colon) {
            Some(self.parse_type_name()?)
        } else {
            None
        };
        self.consume(TokenTag::Equal, MessageId::SyntaxExpectedEquals)?;
        let initializer = self.parse_expression()?;

        Ok(Stmt::VarDecl {
            name,
            annotation,
            span: Span::merge(start, initializer.span()),
            initializer,
        })
    }

    /// 解析普通表达式语句，例如 `print(...)`。
    fn parse_expression_statement(&mut self) -> Result<Stmt, CompilerError> {
        let expr = self.parse_expression()?;
        Ok(Stmt::ExprStmt {
            span: expr.span(),
            expr,
        })
    }

    /// 解析表达式入口，并按优先级从低到高递归下降。
    fn parse_expression(&mut self) -> Result<Expr, CompilerError> {
        self.parse_or()
    }

    /// 解析逻辑或表达式。
    fn parse_or(&mut self) -> Result<Expr, CompilerError> {
        let mut expr = self.parse_and()?;

        while self.match_tag(TokenTag::Or) {
            let right = self.parse_and()?;
            let span = Span::merge(expr.span(), right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
                span,
            };
        }

        Ok(expr)
    }

    /// 解析逻辑与表达式。
    fn parse_and(&mut self) -> Result<Expr, CompilerError> {
        let mut expr = self.parse_equality()?;

        while self.match_tag(TokenTag::And) {
            let right = self.parse_equality()?;
            let span = Span::merge(expr.span(), right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
                span,
            };
        }

        Ok(expr)
    }

    /// 解析相等比较表达式。
    fn parse_equality(&mut self) -> Result<Expr, CompilerError> {
        let mut expr = self.parse_comparison()?;

        while let Some(op) =
            self.match_binary_operator(&[TokenTag::EqualEqual, TokenTag::BangEqual])
        {
            let right = self.parse_comparison()?;
            let span = Span::merge(expr.span(), right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }

        Ok(expr)
    }

    /// 解析大小比较表达式。
    fn parse_comparison(&mut self) -> Result<Expr, CompilerError> {
        let mut expr = self.parse_additive()?;

        while let Some(op) = self.match_binary_operator(&[
            TokenTag::Less,
            TokenTag::LessEqual,
            TokenTag::Greater,
            TokenTag::GreaterEqual,
        ]) {
            let right = self.parse_additive()?;
            let span = Span::merge(expr.span(), right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }

        Ok(expr)
    }

    /// 解析加减法表达式。
    fn parse_additive(&mut self) -> Result<Expr, CompilerError> {
        let mut expr = self.parse_multiplicative()?;

        while let Some(op) = self.match_binary_operator(&[TokenTag::Plus, TokenTag::Minus]) {
            let right = self.parse_multiplicative()?;
            let span = Span::merge(expr.span(), right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }

        Ok(expr)
    }

    /// 解析乘除法表达式。
    fn parse_multiplicative(&mut self) -> Result<Expr, CompilerError> {
        let mut expr = self.parse_unary()?;

        while let Some(op) = self.match_binary_operator(&[TokenTag::Star, TokenTag::Slash]) {
            let right = self.parse_unary()?;
            let span = Span::merge(expr.span(), right.span());
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }

        Ok(expr)
    }

    /// 解析一元负号和逻辑非表达式。
    fn parse_unary(&mut self) -> Result<Expr, CompilerError> {
        if self.match_tag(TokenTag::Minus) {
            let operator = self.previous().span;
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Negate,
                span: Span::merge(operator, operand.span()),
                operand: Box::new(operand),
            });
        }

        if self.match_tag(TokenTag::Not) {
            let operator = self.previous().span;
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                span: Span::merge(operator, operand.span()),
                operand: Box::new(operand),
            });
        }

        self.parse_call()
    }

    /// 解析函数调用后缀，支持 `callee(arg1, arg2)` 形式。
    fn parse_call(&mut self) -> Result<Expr, CompilerError> {
        let mut expr = self.parse_primary()?;

        while self.match_tag(TokenTag::LeftParen) {
            let open_span = self.previous().span;
            let mut arguments = Vec::new();

            if !self.check(TokenTag::RightParen) {
                loop {
                    arguments.push(self.parse_expression()?);
                    if !self.match_tag(TokenTag::Comma) {
                        break;
                    }
                }
            }

            let close = self.consume(TokenTag::RightParen, MessageId::SyntaxExpectedRightParen)?;
            expr = Expr::Call {
                callee: Box::new(expr),
                arguments,
                span: Span::merge(open_span, close.span),
            };
        }

        Ok(expr)
    }

    /// 解析字面量、标识符和分组表达式等基础节点。
    fn parse_primary(&mut self) -> Result<Expr, CompilerError> {
        let token = self.advance().clone();

        match token.kind {
            TokenKind::IntLiteral(value) => Ok(Expr::Literal {
                value: Literal::Int(value),
                span: token.span,
            }),
            TokenKind::FloatLiteral(value) => Ok(Expr::Literal {
                value: Literal::Float(value),
                span: token.span,
            }),
            TokenKind::BoolLiteral(value) => Ok(Expr::Literal {
                value: Literal::Bool(value),
                span: token.span,
            }),
            TokenKind::StringLiteral(value) => Ok(Expr::Literal {
                value: Literal::String(value),
                span: token.span,
            }),
            TokenKind::Identifier(name) => Ok(Expr::Identifier {
                name,
                span: token.span,
            }),
            TokenKind::LeftParen => {
                let expression = self.parse_expression()?;
                let close =
                    self.consume(TokenTag::RightParen, MessageId::SyntaxExpectedRightParen)?;
                Ok(Expr::Grouped {
                    span: Span::merge(token.span, close.span),
                    expression: Box::new(expression),
                })
            }
            _ => Err(CompilerError::syntax(
                MessageId::SyntaxExpectedExpression,
                vec![],
                token.span,
            )),
        }
    }

    /// 解析显式类型标注中的内建类型名称。
    fn parse_type_name(&mut self) -> Result<TypeName, CompilerError> {
        let (name, span) = self.consume_identifier()?;
        TypeName::parse(&name).ok_or_else(|| {
            CompilerError::syntax(MessageId::SyntaxExpectedTypeName, vec![name], span)
        })
    }

    /// 校验并消费语句边界，允许连续换行或文件结束。
    fn consume_statement_boundary(&mut self) -> Result<(), CompilerError> {
        if self.match_tag(TokenTag::Newline) {
            self.skip_newlines();
            return Ok(());
        }

        if self.check(TokenTag::Eof) {
            return Ok(());
        }

        Err(CompilerError::syntax(
            MessageId::SyntaxStatementsNeedNewline,
            vec![],
            self.peek().span,
        ))
    }

    /// 判断当前位置是否落在本阶段尚未开放的关键字上。
    fn is_unsupported_keyword(&self) -> bool {
        matches!(
            self.peek().kind.tag(),
            TokenTag::Const
                | TokenTag::Func
                | TokenTag::If
                | TokenTag::Elif
                | TokenTag::Else
                | TokenTag::While
                | TokenTag::For
                | TokenTag::In
                | TokenTag::Return
                | TokenTag::Break
                | TokenTag::Continue
                | TokenTag::Indent
                | TokenTag::Dedent
        )
    }

    /// 根据关键字词法单元返回其原始关键字文本。
    fn keyword_name(&self, kind: &TokenKind) -> &'static str {
        match kind {
            TokenKind::Const => "const",
            TokenKind::Func => "func",
            TokenKind::If => "if",
            TokenKind::Elif => "elif",
            TokenKind::Else => "else",
            TokenKind::While => "while",
            TokenKind::For => "for",
            TokenKind::In => "in",
            TokenKind::Return => "return",
            TokenKind::Break => "break",
            TokenKind::Continue => "continue",
            TokenKind::Indent => "indent",
            TokenKind::Dedent => "dedent",
            _ => "unknown",
        }
    }

    /// 尝试匹配一个二元运算符，并转换为 AST 中的运算符枚举。
    fn match_binary_operator(&mut self, tags: &[TokenTag]) -> Option<BinaryOp> {
        for tag in tags {
            if self.match_tag(*tag) {
                return Some(match tag {
                    TokenTag::Plus => BinaryOp::Add,
                    TokenTag::Minus => BinaryOp::Subtract,
                    TokenTag::Star => BinaryOp::Multiply,
                    TokenTag::Slash => BinaryOp::Divide,
                    TokenTag::EqualEqual => BinaryOp::Equal,
                    TokenTag::BangEqual => BinaryOp::NotEqual,
                    TokenTag::Less => BinaryOp::Less,
                    TokenTag::LessEqual => BinaryOp::LessEqual,
                    TokenTag::Greater => BinaryOp::Greater,
                    TokenTag::GreaterEqual => BinaryOp::GreaterEqual,
                    _ => unreachable!(),
                });
            }
        }
        None
    }

    /// 尝试消费一个指定类别的词法单元并返回是否成功。
    fn match_tag(&mut self, tag: TokenTag) -> bool {
        if self.check(tag) {
            self.advance();
            return true;
        }
        false
    }

    /// 校验当前词法单元是否属于指定类别。
    fn check(&self, tag: TokenTag) -> bool {
        self.peek().kind.tag() == tag
    }

    /// 跳过连续出现的换行分隔符。
    fn skip_newlines(&mut self) {
        while self.check(TokenTag::Newline) {
            self.advance();
        }
    }

    /// 消费一个指定类别的词法单元，失败时生成对应语法错误。
    fn consume(&mut self, tag: TokenTag, message_id: MessageId) -> Result<Token, CompilerError> {
        if self.check(tag) {
            return Ok(self.advance().clone());
        }
        Err(CompilerError::syntax(message_id, vec![], self.peek().span))
    }

    /// 消费一个标识符，并返回其文本与位置。
    fn consume_identifier(&mut self) -> Result<(String, Span), CompilerError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Identifier(name) => Ok((name, token.span)),
            _ => Err(CompilerError::syntax(
                MessageId::SyntaxExpectedIdentifier,
                vec![],
                token.span,
            )),
        }
    }

    /// 返回当前词法单元并向前推进一个位置。
    fn advance(&mut self) -> &Token {
        if !self.check(TokenTag::Eof) {
            self.current += 1;
        }
        self.previous()
    }

    /// 返回当前尚未消费的词法单元。
    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }

    /// 返回最近一次成功消费的词法单元。
    fn previous(&self) -> &Token {
        &self.tokens[self.current.saturating_sub(1)]
    }
}

#[cfg(test)]
mod tests {
    use super::Parser;
    use crate::ast::{BinaryOp, Expr, Stmt, UnaryOp};
    use crate::lexer::Lexer;
    use crate::settings::{CompilerSettings, UiLanguage};
    use crate::source::SourceFile;

    /// 先做词法分析再做语法分析，减少测试样板代码。
    fn parse(text: &str) -> crate::ast::Program {
        let settings = CompilerSettings::new(UiLanguage::ZhCn);
        let source = SourceFile::new("<test>", text.to_string());
        let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
        Parser::new(tokens, &settings).parse_program().unwrap()
    }

    /// 校验变量声明能够被解析为独立语句节点。
    #[test]
    fn should_parse_variable_declaration() {
        let program = parse("var answer:int = 42\n");
        assert!(matches!(program.statements[0], Stmt::VarDecl { .. }));
    }

    /// 校验逻辑运算和一元运算的优先级关系符合预期。
    #[test]
    fn should_parse_expression_precedence() {
        let program = parse("print(1 < 2 or not false)\n");
        let Stmt::ExprStmt { expr, .. } = &program.statements[0] else {
            panic!("首条语句不是表达式语句");
        };
        let Expr::Call { arguments, .. } = expr else {
            panic!("表达式不是函数调用");
        };
        let Expr::Binary {
            op, left, right, ..
        } = &arguments[0]
        else {
            panic!("参数不是逻辑二元表达式");
        };
        assert_eq!(*op, BinaryOp::Or);
        assert!(matches!(
            **left,
            Expr::Binary {
                op: BinaryOp::Less,
                ..
            }
        ));
        assert!(matches!(
            **right,
            Expr::Unary {
                op: UnaryOp::Not,
                ..
            }
        ));
    }

    /// 校验未支持关键字会在语法阶段直接报错。
    #[test]
    fn should_reject_unsupported_keyword() {
        let settings = CompilerSettings::new(UiLanguage::ZhCn);
        let source = SourceFile::new("<test>", "if true:\n    print(1)\n".to_string());
        let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
        let error = Parser::new(tokens, &settings).parse_program().unwrap_err();
        assert_eq!(
            error.message_id,
            crate::diagnostic::MessageId::SyntaxUnsupportedKeyword
        );
    }
}
