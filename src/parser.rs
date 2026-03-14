use crate::ast::{
    AssignOp, BinaryOp, BindingKind, ConditionalBranch, Expr, Literal, Parameter, Program, Stmt,
    TypeName, UnaryOp,
};
use crate::diagnostic::CompilerError;
use crate::settings::CompilerSettings;
use crate::source::Span;
use crate::token::{Token, TokenKind, TokenTag};

/// 根据词法单元序列构建第二阶段使用的抽象语法树。
pub struct Parser<'a> {
    tokens: Vec<Token>,
    current: usize,
    settings: &'a CompilerSettings,
}

impl<'a> Parser<'a> {
    /// 使用完整的词法单元列表和设置创建新的语法分析器。
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
        if self.match_tag(TokenTag::Func) {
            return self.parse_function_declaration();
        }
        if self.match_tag(TokenTag::Var) {
            return self.parse_binding_declaration(BindingKind::Var);
        }
        if self.match_tag(TokenTag::Const) {
            return self.parse_binding_declaration(BindingKind::Const);
        }
        if self.match_tag(TokenTag::If) {
            return self.parse_if_statement();
        }
        if self.match_tag(TokenTag::While) {
            return self.parse_while_statement();
        }
        if self.match_tag(TokenTag::For) {
            return self.parse_for_statement();
        }
        if self.match_tag(TokenTag::Break) {
            return Ok(Stmt::Break {
                span: self.previous().span,
            });
        }
        if self.match_tag(TokenTag::Continue) {
            return Ok(Stmt::Continue {
                span: self.previous().span,
            });
        }
        if self.match_tag(TokenTag::Return) {
            return self.parse_return_statement();
        }
        if self.is_assignment_start() {
            return self.parse_assignment();
        }

        self.parse_expression_statement()
    }

    /// 解析 `func name(args) [-> type] :` 形式的函数定义。
    fn parse_function_declaration(&mut self) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        let (name, _) = self.consume_identifier("函数定义缺少函数名")?;
        self.consume(TokenTag::LeftParen, "函数定义缺少左括号 `(`")?;
        let mut parameters = Vec::new();

        if !self.check(TokenTag::RightParen) {
            loop {
                let (param_name, param_span) = self.consume_identifier("函数参数缺少参数名")?;
                self.consume(TokenTag::Colon, "函数参数缺少类型分隔符 `:`")?;
                let type_name = self.parse_type_name()?;
                parameters.push(Parameter {
                    name: param_name,
                    type_name,
                    span: param_span,
                });
                if !self.match_tag(TokenTag::Comma) {
                    break;
                }
            }
        }

        self.consume(TokenTag::RightParen, "函数定义缺少右括号 `)`")?;
        let return_type = if self.match_tag(TokenTag::Arrow) {
            Some(self.parse_type_name()?)
        } else {
            None
        };
        let (body, body_span) = self.parse_suite("函数定义缺少函数体")?;

        Ok(Stmt::FunctionDecl {
            name,
            parameters,
            return_type,
            body,
            span: Span::merge(start, body_span),
        })
    }

    /// 解析 `var/const name[:type] = expr` 形式的绑定声明语句。
    fn parse_binding_declaration(
        &mut self,
        binding_kind: BindingKind,
    ) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        let declaration_name = binding_kind.keyword();
        let missing_name = format!("{declaration_name}声明缺少名称");
        let (name, _) = self.consume_identifier(&missing_name)?;
        let annotation = if self.match_tag(TokenTag::Colon) {
            Some(self.parse_type_name()?)
        } else {
            None
        };

        self.consume(TokenTag::Equal, "声明缺少 `=` 初始化符号")?;
        let initializer = self.parse_expression()?;

        let span = Span::merge(start, initializer.span());
        Ok(Stmt::VarDecl {
            binding_kind,
            name,
            annotation,
            initializer,
            span,
        })
    }

    /// 解析 `name = expr` 或复合赋值形式的赋值语句。
    fn parse_assignment(&mut self) -> Result<Stmt, CompilerError> {
        let (name, start) = self.consume_identifier("赋值语句缺少变量名")?;
        let op = self.parse_assign_operator()?;
        let value = self.parse_expression()?;

        let span = Span::merge(start, value.span());
        Ok(Stmt::Assign {
            name,
            op,
            value,
            span,
        })
    }

    /// 解析 `if / elif / else` 组成的条件分支语句。
    fn parse_if_statement(&mut self) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        let mut branches = Vec::new();
        let first_condition = self.parse_expression()?;
        let (first_body, first_span) = self.parse_suite("if 语句缺少代码块")?;
        branches.push(ConditionalBranch {
            condition: first_condition.clone(),
            body: first_body,
            span: Span::merge(first_condition.span(), first_span),
        });

        while self.match_tag(TokenTag::Elif) {
            let condition = self.parse_expression()?;
            let (body, body_span) = self.parse_suite("elif 语句缺少代码块")?;
            branches.push(ConditionalBranch {
                span: Span::merge(condition.span(), body_span),
                condition,
                body,
            });
        }

        let else_span = if self.match_tag(TokenTag::Else) {
            let (body, body_span) = self.parse_suite("else 语句缺少代码块")?;
            let else_branch = Some(body);
            return Ok(Stmt::If {
                branches,
                else_branch,
                span: Span::merge(start, body_span),
            });
        } else {
            first_span
        };

        Ok(Stmt::If {
            branches,
            else_branch: None,
            span: Span::merge(start, else_span),
        })
    }

    /// 解析 `while condition :` 形式的循环语句。
    fn parse_while_statement(&mut self) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        let condition = self.parse_expression()?;
        let (body, body_span) = self.parse_suite("while 语句缺少代码块")?;

        Ok(Stmt::While {
            condition,
            body,
            span: Span::merge(start, body_span),
        })
    }

    /// 解析 `for i in start..end :` 形式的范围循环语句。
    fn parse_for_statement(&mut self) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        let (name, _) = self.consume_identifier("for 循环缺少循环变量名")?;
        self.consume(TokenTag::In, "for 循环缺少 `in` 关键字")?;
        let range_start = self.parse_expression()?;
        self.consume(TokenTag::DotDot, "for 循环范围缺少 `..`")?;
        let range_end = self.parse_expression()?;
        let (body, body_span) = self.parse_suite("for 循环缺少代码块")?;

        Ok(Stmt::ForRange {
            name,
            start: range_start,
            end: range_end,
            body,
            span: Span::merge(start, body_span),
        })
    }

    /// 解析 `return` 或 `return expr` 形式的返回语句。
    fn parse_return_statement(&mut self) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        if self.is_statement_end() {
            return Ok(Stmt::Return {
                value: None,
                span: start,
            });
        }

        let value = self.parse_expression()?;
        Ok(Stmt::Return {
            value: Some(value.clone()),
            span: Span::merge(start, value.span()),
        })
    }

    /// 解析普通表达式语句，例如函数调用。
    fn parse_expression_statement(&mut self) -> Result<Stmt, CompilerError> {
        let expr = self.parse_expression()?;
        Ok(Stmt::ExprStmt {
            span: expr.span(),
            expr,
        })
    }

    /// 解析冒号加缩进构成的代码块。
    fn parse_suite(&mut self, message: &str) -> Result<(Vec<Stmt>, Span), CompilerError> {
        self.consume(TokenTag::Colon, "代码块缺少冒号 `:`")?;
        self.consume(TokenTag::Newline, "冒号后必须换行开始代码块")?;
        self.consume(TokenTag::Indent, message)?;

        let block_start = self.previous().span;
        let mut statements = Vec::new();
        self.skip_newlines();

        while !self.check(TokenTag::Dedent) {
            if self.check(TokenTag::Eof) {
                return Err(CompilerError::syntax_message(
                    "代码块缺少正确的缩进结束",
                    block_start,
                ));
            }
            statements.push(self.parse_statement()?);
            self.consume_statement_boundary()?;
        }

        if statements.is_empty() {
            return Err(CompilerError::syntax_message("代码块不能为空", block_start));
        }

        let dedent = self.consume(TokenTag::Dedent, "代码块缺少结束缩进")?;
        Ok((statements, Span::merge(block_start, dedent.span)))
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

    /// 解析等值比较表达式。
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

    /// 解析乘除、取模和整数除法表达式。
    fn parse_multiplicative(&mut self) -> Result<Expr, CompilerError> {
        let mut expr = self.parse_unary()?;

        while let Some(op) = self.match_binary_operator(&[
            TokenTag::Star,
            TokenTag::Slash,
            TokenTag::Percent,
            TokenTag::IntDivide,
        ]) {
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

    /// 解析一元负号和逻辑非。
    fn parse_unary(&mut self) -> Result<Expr, CompilerError> {
        if self.match_tag(TokenTag::Minus) {
            let operator = self.previous().span;
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Negate,
                operand: Box::new(operand.clone()),
                span: Span::merge(operator, operand.span()),
            });
        }

        if self.match_tag(TokenTag::Not) {
            let operator = self.previous().span;
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand.clone()),
                span: Span::merge(operator, operand.span()),
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

            let close = self.consume(TokenTag::RightParen, "函数调用缺少右括号 `)`")?;
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
                let close = self.consume(TokenTag::RightParen, "分组表达式缺少右括号 `)`")?;
                Ok(Expr::Grouped {
                    expression: Box::new(expression),
                    span: Span::merge(token.span, close.span),
                })
            }
            _ => Err(CompilerError::syntax_message("这里需要一个表达式", token.span)),
        }
    }

    /// 解析显式类型标注中的内建类型名称。
    fn parse_type_name(&mut self) -> Result<TypeName, CompilerError> {
        let (name, span) = self.consume_identifier("类型标注缺少类型名")?;
        TypeName::parse(&name)
            .ok_or_else(|| CompilerError::syntax_message(format!("未知的内建类型 `{name}`"), span))
    }

    /// 解析赋值运算符，并转换为语法树内部的赋值类型枚举。
    fn parse_assign_operator(&mut self) -> Result<AssignOp, CompilerError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Equal => Ok(AssignOp::Assign),
            TokenKind::PlusEqual => Ok(AssignOp::AddAssign),
            TokenKind::MinusEqual => Ok(AssignOp::SubtractAssign),
            TokenKind::StarEqual => Ok(AssignOp::MultiplyAssign),
            TokenKind::SlashEqual => Ok(AssignOp::DivideAssign),
            TokenKind::PercentEqual => Ok(AssignOp::ModAssign),
            _ => Err(CompilerError::syntax_message("这里需要一个赋值运算符", token.span)),
        }
    }

    /// 判断当前位置是否处于一条语句的自然结束位置。
    fn is_statement_end(&self) -> bool {
        matches!(
            self.peek().kind.tag(),
            TokenTag::Newline | TokenTag::Dedent | TokenTag::Eof
        )
    }

    /// 校验并消费语句边界，允许换行、结束缩进或文件结尾。
    fn consume_statement_boundary(&mut self) -> Result<(), CompilerError> {
        if self.match_tag(TokenTag::Newline) {
            self.skip_newlines();
            return Ok(());
        }

        if self.previous().kind.tag() == TokenTag::Dedent {
            return Ok(());
        }

        if self.check(TokenTag::Dedent) || self.check(TokenTag::Eof) {
            return Ok(());
        }

        Err(CompilerError::syntax_message(
            "语句之间需要使用换行分隔",
            self.peek().span,
        ))
    }

    /// 判断当前位置是否符合赋值语句的开头模式。
    fn is_assignment_start(&self) -> bool {
        self.check(TokenTag::Identifier)
            && matches!(
                self.peek_next().kind.tag(),
                TokenTag::Equal
                    | TokenTag::PlusEqual
                    | TokenTag::MinusEqual
                    | TokenTag::StarEqual
                    | TokenTag::SlashEqual
                    | TokenTag::PercentEqual
            )
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
                    TokenTag::Percent => BinaryOp::Modulo,
                    TokenTag::IntDivide => BinaryOp::IntDivide,
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

    /// 消费一个指定类别的词法单元，失败时生成中文语法错误。
    fn consume(&mut self, tag: TokenTag, message: &str) -> Result<Token, CompilerError> {
        if self.check(tag) {
            return Ok(self.advance().clone());
        }
        Err(CompilerError::syntax_message(message, self.peek().span))
    }

    /// 消费一个标识符，并返回其文本与位置。
    fn consume_identifier(&mut self, message: &str) -> Result<(String, Span), CompilerError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Identifier(name) => Ok((name, token.span)),
            _ => Err(CompilerError::syntax_message(message, token.span)),
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

    /// 返回下一个词法单元，若已到结尾则复用 EOF。
    fn peek_next(&self) -> &Token {
        self.tokens
            .get(self.current + 1)
            .unwrap_or_else(|| self.tokens.last().expect("词法单元列表不能为空"))
    }

    /// 返回最近一次成功消费的词法单元。
    fn previous(&self) -> &Token {
        &self.tokens[self.current.saturating_sub(1)]
    }
}

#[cfg(test)]
mod tests {
    use super::Parser;
    use crate::ast::{AssignOp, BinaryOp, BindingKind, Expr, Stmt, UnaryOp};
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

    /// 校验函数定义能够被正确解析为顶层语句节点。
    #[test]
    fn should_parse_function_declaration() {
        let program = parse("func add(a:int, b:int) -> int:\n    return a + b\n");
        assert!(matches!(program.statements[0], Stmt::FunctionDecl { .. }));
    }

    /// 校验 `const` 声明会带着不可变标记进入语法树。
    #[test]
    fn should_parse_const_declaration() {
        let program = parse("const answer:int = 42\n");
        let Stmt::VarDecl { binding_kind, .. } = &program.statements[0] else {
            panic!("首条语句不是绑定声明");
        };
        assert_eq!(*binding_kind, BindingKind::Const);
    }

    /// 校验 `if / elif / else` 链能够被解析为统一条件分支节点。
    #[test]
    fn should_parse_if_statement() {
        let program = parse(
            "if true:\n    print(1)\nelif false:\n    print(2)\nelse:\n    print(3)\n",
        );
        assert!(matches!(program.statements[0], Stmt::If { .. }));
    }

    /// 校验范围 `for` 循环能够解析起止表达式和循环体。
    #[test]
    fn should_parse_for_range_statement() {
        let program = parse("for i in 0..10:\n    print(i)\n");
        assert!(matches!(program.statements[0], Stmt::ForRange { .. }));
    }

    /// 校验复合赋值运算符能够被解析为独立赋值类型。
    #[test]
    fn should_parse_compound_assignment() {
        let program = parse("count += 1\n");
        let Stmt::Assign { op, .. } = &program.statements[0] else {
            panic!("首条语句不是赋值语句");
        };
        assert_eq!(*op, AssignOp::AddAssign);
    }

    /// 校验比较运算和逻辑运算的优先级关系符合预期。
    #[test]
    fn should_parse_comparison_and_logic_precedence() {
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
}
