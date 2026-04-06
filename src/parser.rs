use crate::ast::{
    AssignOp, AssignTarget, BinaryOp, BindingKind, ConditionalBranch, Expr, Literal, MapEntry,
    ImportItem, ImportKind, ImportPath, MethodName, Parameter, Program, PropertyName, Stmt,
    TypeName, UnaryOp,
};
use crate::diagnostic::CompilerError;
use crate::lexer::Lexer;
use crate::settings::CompilerSettings;
use crate::source::{SourceFile, Span};
use crate::token::{Token, TokenKind, TokenTag};

/// 根据词法单元序列构建第三阶段列表能力所需的抽象语法树。
pub struct Parser<'a> {
    tokens: Vec<Token>,
    current: usize,
    settings: &'a CompilerSettings,
}

/// 表示模板字符串拆解后得到的一段普通文本或一段插值表达式。
enum TemplateSegment {
    Text {
        value: String,
        span: Span,
    },
    Expression {
        value: Expr,
    },
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
        if self.match_tag(TokenTag::Export) {
            self.consume(TokenTag::Import, "`export` 后必须跟 `import`")?;
            return self.parse_import_statement(true);
        }
        if self.match_tag(TokenTag::Import) {
            return self.parse_import_statement(false);
        }
        if self.match_tag(TokenTag::Pub) {
            return self.parse_public_statement();
        }
        if self.match_tag(TokenTag::Extern) {
            self.consume(TokenTag::Func, "`extern` 后必须跟 `func`")?;
            return self.parse_extern_function_declaration(false);
        }
        if self.match_tag(TokenTag::Func) {
            return self.parse_function_declaration(false);
        }
        if self.match_tag(TokenTag::Var) {
            return self.parse_binding_declaration(BindingKind::Var, false);
        }
        if self.match_tag(TokenTag::Const) {
            return self.parse_binding_declaration(BindingKind::Const, false);
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

    /// 解析 `pub func/var/const` 公开声明，其他语句会被拒绝。
    fn parse_public_statement(&mut self) -> Result<Stmt, CompilerError> {
        if self.match_tag(TokenTag::Func) {
            return self.parse_function_declaration(true);
        }
        if self.match_tag(TokenTag::Extern) {
            self.consume(TokenTag::Func, "`pub extern` 后必须跟 `func`")?;
            return self.parse_extern_function_declaration(true);
        }
        if self.match_tag(TokenTag::Var) {
            return self.parse_binding_declaration(BindingKind::Var, true);
        }
        if self.match_tag(TokenTag::Const) {
            return self.parse_binding_declaration(BindingKind::Const, true);
        }

        Err(CompilerError::syntax_message(
            "`pub` 只能用于顶层 `func`、`extern func`、`var` 或 `const` 声明",
            self.peek().span,
        ))
    }

    /// 解析 `func name(args) [-> type] :` 形式的函数定义。
    fn parse_function_declaration(&mut self, is_public: bool) -> Result<Stmt, CompilerError> {
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
            is_public,
            name,
            parameters,
            return_type,
            body,
            span: Span::merge(start, body_span),
        })
    }

    /// 解析 `extern func name(args) [-> type]` 形式的原生函数声明。
    fn parse_extern_function_declaration(
        &mut self,
        is_public: bool,
    ) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        let (name, _) = self.consume_identifier("extern 函数声明缺少函数名")?;
        self.consume(TokenTag::LeftParen, "extern 函数声明缺少左括号 `(`")?;
        let mut parameters = Vec::new();

        if !self.check(TokenTag::RightParen) {
            loop {
                let (param_name, param_span) =
                    self.consume_identifier("extern 函数参数缺少参数名")?;
                self.consume(TokenTag::Colon, "extern 函数参数缺少类型分隔符 `:`")?;
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

        self.consume(TokenTag::RightParen, "extern 函数声明缺少右括号 `)`")?;
        let return_type = if self.match_tag(TokenTag::Arrow) {
            Some(self.parse_type_name()?)
        } else {
            None
        };
        let span = Span::merge(start, self.previous().span);
        Ok(Stmt::ExternFunctionDecl {
            is_public,
            name,
            parameters,
            return_type,
            span,
        })
    }

    /// 解析 `var/const name[:type] = expr` 形式的绑定声明语句。
    fn parse_binding_declaration(
        &mut self,
        binding_kind: BindingKind,
        is_public: bool,
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
        let initializer = if self.match_tag(TokenTag::Equal) {
            Some(self.parse_expression()?)
        } else if binding_kind == BindingKind::Var
            && matches!(annotation.as_ref(), Some(TypeName::Array(_, _)))
            && self.is_statement_end()
        {
            None
        } else {
            return Err(CompilerError::syntax_message(
                "声明缺少 `=` 初始化符号",
                self.peek().span,
            ));
        };

        let span = initializer
            .as_ref()
            .map(|expr| Span::merge(start, expr.span()))
            .unwrap_or_else(|| Span::merge(start, self.previous().span));
        Ok(Stmt::VarDecl {
            is_public,
            binding_kind,
            name,
            annotation,
            initializer,
            span,
        })
    }

    /// 解析 `import` 或 `export import` 语句。
    fn parse_import_statement(&mut self, is_export: bool) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        let path = self.parse_import_path()?;
        let kind = if self.match_tag(TokenTag::Dot) {
            self.consume(TokenTag::Star, "全部导入缺少 `*`")?;
            ImportKind::All
        } else {
            self.consume(TokenTag::LeftBrace, "导入语句缺少左花括号 `{`")?;
            let mut items = Vec::new();
            if !self.check(TokenTag::RightBrace) {
                loop {
                    let (name, item_span) = self.consume_identifier("导入项缺少名称")?;
                    let alias = if self.match_tag(TokenTag::As) {
                        Some(self.consume_identifier("别名导入缺少别名")?.0)
                    } else {
                        None
                    };
                    items.push(ImportItem {
                        name,
                        alias,
                        span: item_span,
                    });
                    if !self.match_tag(TokenTag::Comma) {
                        break;
                    }
                }
            }
            self.consume(TokenTag::RightBrace, "导入语句缺少右花括号 `}`")?;
            ImportKind::Named(items)
        };
        let span = Span::merge(start, self.previous().span);

        if is_export {
            Ok(Stmt::ExportImport { path, kind, span })
        } else {
            Ok(Stmt::Import { path, kind, span })
        }
    }

    /// 解析 `foo.bar` 或 `local.foo.bar` 形式的模块路径。
    fn parse_import_path(&mut self) -> Result<ImportPath, CompilerError> {
        let (first_segment, start) = self.consume_identifier("导入路径缺少模块名")?;
        let mut segments = vec![first_segment];
        let mut end = start;

        while self.check(TokenTag::Dot)
            && self
                .peek_next()
                .kind
                .tag()
                .eq(&TokenTag::Identifier)
        {
            self.advance();
            let (segment, span) = self.consume_identifier("导入路径缺少模块片段")?;
            segments.push(segment);
            end = span;
        }

        Ok(ImportPath {
            segments,
            span: Span::merge(start, end),
        })
    }

    /// 解析 `name = expr` 或复合赋值形式的赋值语句。
    fn parse_assignment(&mut self) -> Result<Stmt, CompilerError> {
        let (name, start) = self.consume_identifier("赋值语句缺少变量名")?;
        if self.match_tag(TokenTag::LeftBracket) {
            let index = self.parse_expression()?;
            let close = self.consume(TokenTag::RightBracket, "下标赋值缺少右方括号 `]`")?;
            let op = self.parse_assign_operator()?;
            if op != AssignOp::Assign {
                return Err(CompilerError::syntax_message(
                    "当前阶段下标赋值只支持 `=`",
                    close.span,
                ));
            }
            let value = self.parse_expression()?;
            return Ok(Stmt::Assign {
                target: AssignTarget::Index {
                    target: Box::new(Expr::Identifier { name, span: start }),
                    index: Box::new(index),
                    span: Span::merge(start, close.span),
                },
                op,
                value: value.clone(),
                span: Span::merge(start, value.span()),
            });
        }

        let op = self.parse_assign_operator()?;
        let value = self.parse_expression()?;

        let span = Span::merge(start, value.span());
        Ok(Stmt::Assign {
            target: AssignTarget::Identifier { name, span: start },
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

    /// 解析范围循环或列表遍历循环。
    fn parse_for_statement(&mut self) -> Result<Stmt, CompilerError> {
        let start = self.previous().span;
        let (key_name, first_span) = self.consume_identifier("for 循环缺少循环变量名")?;
        let value_name = if self.match_tag(TokenTag::Comma) {
            Some(self.consume_identifier("for 字典遍历缺少值变量名")?.0)
        } else {
            None
        };
        self.consume(TokenTag::In, "for 循环缺少 `in` 关键字")?;
        let iterable_or_start = self.parse_expression()?;

        if self.match_tag(TokenTag::DotDot) {
            if value_name.is_some() {
                return Err(CompilerError::syntax_message(
                    "范围 for 不能同时声明键和值变量",
                    first_span,
                ));
            }
            let range_end = self.parse_expression()?;
            let (body, body_span) = self.parse_suite("for 循环缺少代码块")?;
            return Ok(Stmt::ForRange {
                name: key_name,
                start: iterable_or_start,
                end: range_end,
                body,
                span: Span::merge(start, body_span),
            });
        }

        let (body, body_span) = self.parse_suite("for 循环缺少代码块")?;
        Ok(Stmt::ForEach {
            key_name,
            value_name,
            iterable: iterable_or_start,
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

    /// 解析普通表达式语句，例如函数调用或列表方法调用。
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

        self.parse_postfix()
    }

    /// 解析函数调用、索引、切片和对象方法调用等后缀表达式。
    fn parse_postfix(&mut self) -> Result<Expr, CompilerError> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.match_tag(TokenTag::LeftParen) {
                expr = self.finish_call(expr)?;
                continue;
            }
            if self.match_tag(TokenTag::LeftBracket) {
                expr = self.finish_index_or_slice(expr)?;
                continue;
            }
            if self.match_tag(TokenTag::Dot) {
                expr = self.finish_member_access(expr)?;
                continue;
            }
            break;
        }

        Ok(expr)
    }

    /// 解析函数调用参数列表并生成调用表达式。
    fn finish_call(&mut self, callee: Expr) -> Result<Expr, CompilerError> {
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
        Ok(Expr::Call {
            callee: Box::new(callee),
            arguments,
            span: Span::merge(open_span, close.span),
        })
    }

    /// 解析索引访问或切片表达式。
    fn finish_index_or_slice(&mut self, target: Expr) -> Result<Expr, CompilerError> {
        let open_span = self.previous().span;
        if self.match_tag(TokenTag::Colon) {
            let end = if self.check(TokenTag::RightBracket) {
                None
            } else {
                Some(Box::new(self.parse_expression()?))
            };
            let close = self.consume(TokenTag::RightBracket, "切片表达式缺少右方括号 `]`")?;
            return Ok(Expr::Slice {
                target: Box::new(target),
                start: None,
                end,
                span: Span::merge(open_span, close.span),
            });
        }

        let first = self.parse_expression()?;
        if self.match_tag(TokenTag::Colon) {
            let end = if self.check(TokenTag::RightBracket) {
                None
            } else {
                Some(Box::new(self.parse_expression()?))
            };
            let close = self.consume(TokenTag::RightBracket, "切片表达式缺少右方括号 `]`")?;
            return Ok(Expr::Slice {
                target: Box::new(target),
                start: Some(Box::new(first)),
                end,
                span: Span::merge(open_span, close.span),
            });
        }

        let close = self.consume(TokenTag::RightBracket, "索引表达式缺少右方括号 `]`")?;
        Ok(Expr::Index {
            target: Box::new(target),
            index: Box::new(first),
            span: Span::merge(open_span, close.span),
        })
    }

    /// 解析方法调用或安全转换结果上的只读属性访问。
    fn finish_member_access(&mut self, target: Expr) -> Result<Expr, CompilerError> {
        let dot_span = self.previous().span;
        let (member_name, member_span) = self.consume_identifier("`.` 后缺少成员名")?;

        if self.match_tag(TokenTag::LeftParen) {
            let method = MethodName::parse(&member_name).ok_or_else(|| {
                CompilerError::syntax_message(
                    format!("当前阶段不支持对象方法 `{member_name}`"),
                    member_span,
                )
            })?;
            let mut arguments = Vec::new();

            if !self.check(TokenTag::RightParen) {
                loop {
                    arguments.push(self.parse_expression()?);
                    if !self.match_tag(TokenTag::Comma) {
                        break;
                    }
                }
            }

            let close = self.consume(TokenTag::RightParen, "方法调用缺少右括号 `)`")?;
            return Ok(Expr::MethodCall {
                target: Box::new(target),
                method,
                arguments,
                span: Span::merge(dot_span, close.span),
            });
        }

        let property = PropertyName::parse(&member_name).ok_or_else(|| {
            CompilerError::syntax_message(
                format!("当前阶段不支持属性 `{member_name}`"),
                member_span,
            )
        })?;
        Ok(Expr::PropertyAccess {
            target: Box::new(target),
            property,
            span: Span::merge(dot_span, member_span),
        })
    }

    /// 解析字面量、标识符、列表字面量和分组表达式等基础节点。
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
            TokenKind::StringLiteral(value) => self.parse_string_literal(value, token.span),
            TokenKind::Identifier(name) => {
                if name == "array" && self.check(TokenTag::Less) {
                    return self.parse_array_initializer(token.span);
                }
                Ok(Expr::Identifier {
                    name,
                    span: token.span,
                })
            }
            TokenKind::LeftParen => {
                let expression = self.parse_expression()?;
                let close = self.consume(TokenTag::RightParen, "分组表达式缺少右括号 `)`")?;
                Ok(Expr::Grouped {
                    expression: Box::new(expression),
                    span: Span::merge(token.span, close.span),
                })
            }
            TokenKind::LeftBracket => self.parse_list_literal(token.span),
            TokenKind::LeftBrace => self.parse_map_literal(token.span),
            _ => Err(CompilerError::syntax_message(
                "这里需要一个表达式",
                token.span,
            )),
        }
    }

    /// 解析普通字符串或模板字符串，并在需要时把插值降级为现有的字符串拼接表达式。
    fn parse_string_literal(&mut self, value: String, span: Span) -> Result<Expr, CompilerError> {
        let segments = self.parse_template_segments(&value, span)?;
        Ok(self.build_template_expression(segments, span))
    }

    /// 把字符串内容拆成普通文本片段与 `${...}` 插值片段，供后续统一转换为 AST。
    fn parse_template_segments(
        &self,
        value: &str,
        literal_span: Span,
    ) -> Result<Vec<TemplateSegment>, CompilerError> {
        let mut segments = Vec::new();
        let mut cursor = 0usize;

        while let Some(offset) = value[cursor..].find("${") {
            let interpolation_start = cursor + offset;
            if interpolation_start > cursor {
                segments.push(TemplateSegment::Text {
                    value: value[cursor..interpolation_start].to_string(),
                    span: self.offset_template_span(literal_span, cursor, interpolation_start),
                });
            }

            let expression_start = interpolation_start + 2;
            let expression_end = self.find_template_expression_end(value, expression_start, literal_span)?;
            let expression_source = &value[expression_start..expression_end];
            let trimmed = expression_source.trim();
            if trimmed.is_empty() {
                return Err(CompilerError::syntax_message(
                    "模板字符串中的 `${}` 不能为空表达式",
                    self.offset_template_span(literal_span, expression_start, expression_end),
                ));
            }

            let leading_trimmed = expression_source
                .find(trimmed)
                .expect("去除空白后的模板表达式必须仍然存在于原文本中");
            let trimmed_start = expression_start + leading_trimmed;
            let trimmed_end = trimmed_start + trimmed.len();
            let expression_span =
                self.offset_template_span(literal_span, trimmed_start, trimmed_end);
            let expression = self.parse_inline_template_expression(trimmed, expression_span)?;
            segments.push(TemplateSegment::Expression { value: expression });
            cursor = expression_end + 1;
        }

        if cursor < value.len() || segments.is_empty() {
            segments.push(TemplateSegment::Text {
                value: value[cursor..].to_string(),
                span: self.offset_template_span(literal_span, cursor, value.len()),
            });
        }

        Ok(segments)
    }

    /// 在模板字符串内部查找当前 `${` 对应的结束 `}`，并正确跳过字符串和嵌套括号。
    fn find_template_expression_end(
        &self,
        value: &str,
        start: usize,
        literal_span: Span,
    ) -> Result<usize, CompilerError> {
        let mut brace_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut paren_depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;

        for (offset, character) in value[start..].char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                    continue;
                }

                match character {
                    '\\' => escaped = true,
                    '"' => in_string = false,
                    _ => {}
                }
                continue;
            }

            match character {
                '"' => in_string = true,
                '(' => paren_depth += 1,
                ')' => paren_depth = paren_depth.saturating_sub(1),
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                '{' => brace_depth += 1,
                '}' => {
                    if brace_depth == 0 && bracket_depth == 0 && paren_depth == 0 {
                        return Ok(start + offset);
                    }
                    brace_depth = brace_depth.saturating_sub(1);
                }
                _ => {}
            }
        }

        Err(CompilerError::syntax_message(
            "模板字符串缺少结束 `}`",
            self.offset_template_span(literal_span, start.saturating_sub(2), value.len()),
        ))
    }

    /// 使用独立的词法和语法分析器解析 `${...}` 内部表达式，并把相对位置回写到原字符串跨度。
    fn parse_inline_template_expression(
        &self,
        expression_text: &str,
        expression_span: Span,
    ) -> Result<Expr, CompilerError> {
        let source = SourceFile::new("<template>", format!("{expression_text}\n"));
        let tokens = Lexer::new(&source, self.settings)
            .tokenize()
            .map_err(|error| self.offset_template_error(error, expression_span.start))?;
        let mut parser = Self::new(tokens, self.settings);
        let expression = parser
            .parse_expression()
            .map_err(|error| self.offset_template_error(error, expression_span.start))?;
        parser.skip_newlines();
        if !parser.check(TokenTag::Eof) {
            return Err(CompilerError::syntax_message(
                "模板字符串中的插值表达式格式无效",
                expression_span,
            ));
        }
        Ok(Self::offset_expression_spans(
            expression,
            expression_span.start,
        ))
    }

    /// 把拆好的模板片段重建为现有 AST 中的字符串字面量与 `+` 拼接表达式，复用旧链路。
    fn build_template_expression(&self, segments: Vec<TemplateSegment>, literal_span: Span) -> Expr {
        let has_interpolation = segments
            .iter()
            .any(|segment| matches!(segment, TemplateSegment::Expression { .. }));
        let mut parts = Vec::new();

        for segment in segments {
            match segment {
                TemplateSegment::Text { value, span } => {
                    if !value.is_empty() || !has_interpolation {
                        parts.push(Expr::Literal {
                            value: Literal::String(value),
                            span,
                        });
                    }
                }
                TemplateSegment::Expression { value } => {
                    parts.push(Self::build_template_string_convert_call(value));
                }
            }
        }

        let mut parts = parts.into_iter();
        let mut expression = parts.next().unwrap_or(Expr::Literal {
            value: Literal::String(String::new()),
            span: literal_span,
        });
        for next in parts {
            expression = Expr::Binary {
                span: Span::merge(expression.span(), next.span()),
                left: Box::new(expression),
                op: BinaryOp::Add,
                right: Box::new(next),
            };
        }
        expression
    }

    /// 把模板字符串中的单个插值表达式包装成 `string(expr)`，统一走已有的基础类型转换能力。
    fn build_template_string_convert_call(expression: Expr) -> Expr {
        let span = expression.span();
        Expr::Call {
            callee: Box::new(Expr::Identifier {
                name: "string".to_string(),
                span,
            }),
            arguments: vec![expression],
            span,
        }
    }

    /// 把模板字符串内部表达式的相对跨度平移到原始源码中的近似位置，便于后续阶段复用。
    fn offset_expression_spans(expression: Expr, offset: usize) -> Expr {
        match expression {
            Expr::Literal { value, span } => Expr::Literal {
                value,
                span: Self::shift_span(span, offset),
            },
            Expr::Identifier { name, span } => Expr::Identifier {
                name,
                span: Self::shift_span(span, offset),
            },
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => Expr::Binary {
                left: Box::new(Self::offset_expression_spans(*left, offset)),
                op,
                right: Box::new(Self::offset_expression_spans(*right, offset)),
                span: Self::shift_span(span, offset),
            },
            Expr::Unary { op, operand, span } => Expr::Unary {
                op,
                operand: Box::new(Self::offset_expression_spans(*operand, offset)),
                span: Self::shift_span(span, offset),
            },
            Expr::Call {
                callee,
                arguments,
                span,
            } => Expr::Call {
                callee: Box::new(Self::offset_expression_spans(*callee, offset)),
                arguments: arguments
                    .into_iter()
                    .map(|argument| Self::offset_expression_spans(argument, offset))
                    .collect(),
                span: Self::shift_span(span, offset),
            },
            Expr::ListLiteral { elements, span } => Expr::ListLiteral {
                elements: elements
                    .into_iter()
                    .map(|element| Self::offset_expression_spans(element, offset))
                    .collect(),
                span: Self::shift_span(span, offset),
            },
            Expr::MapLiteral { entries, span } => Expr::MapLiteral {
                entries: entries
                    .into_iter()
                    .map(|entry| MapEntry {
                        key: Self::offset_expression_spans(entry.key, offset),
                        value: Self::offset_expression_spans(entry.value, offset),
                        span: Self::shift_span(entry.span, offset),
                    })
                    .collect(),
                span: Self::shift_span(span, offset),
            },
            Expr::ArrayInitializer {
                element_type,
                size,
                elements,
                span,
            } => Expr::ArrayInitializer {
                element_type,
                size,
                elements: elements
                    .into_iter()
                    .map(|element| Self::offset_expression_spans(element, offset))
                    .collect(),
                span: Self::shift_span(span, offset),
            },
            Expr::Index {
                target,
                index,
                span,
            } => Expr::Index {
                target: Box::new(Self::offset_expression_spans(*target, offset)),
                index: Box::new(Self::offset_expression_spans(*index, offset)),
                span: Self::shift_span(span, offset),
            },
            Expr::Slice {
                target,
                start,
                end,
                span,
            } => Expr::Slice {
                target: Box::new(Self::offset_expression_spans(*target, offset)),
                start: start.map(|value| Box::new(Self::offset_expression_spans(*value, offset))),
                end: end.map(|value| Box::new(Self::offset_expression_spans(*value, offset))),
                span: Self::shift_span(span, offset),
            },
            Expr::MethodCall {
                target,
                method,
                arguments,
                span,
            } => Expr::MethodCall {
                target: Box::new(Self::offset_expression_spans(*target, offset)),
                method,
                arguments: arguments
                    .into_iter()
                    .map(|argument| Self::offset_expression_spans(argument, offset))
                    .collect(),
                span: Self::shift_span(span, offset),
            },
            Expr::PropertyAccess {
                target,
                property,
                span,
            } => Expr::PropertyAccess {
                target: Box::new(Self::offset_expression_spans(*target, offset)),
                property,
                span: Self::shift_span(span, offset),
            },
            Expr::Grouped { expression, span } => Expr::Grouped {
                expression: Box::new(Self::offset_expression_spans(*expression, offset)),
                span: Self::shift_span(span, offset),
            },
        }
    }

    /// 按模板字符串正文中的相对偏移，近似计算其在原始源码字符串字面量中的实际跨度。
    fn offset_template_span(&self, literal_span: Span, start: usize, end: usize) -> Span {
        Span::new(
            literal_span.start + 1 + start,
            literal_span.start + 1 + end,
        )
    }

    /// 把模板字符串子解析器返回的错误跨度平移到原始源码位置，避免后续诊断落回临时源码文件。
    fn offset_template_error(&self, error: CompilerError, offset: usize) -> CompilerError {
        CompilerError {
            span: Self::shift_span(error.span, offset),
            ..error
        }
    }

    /// 对单个源码跨度做统一平移，供模板字符串内部节点回写源码位置信息时复用。
    fn shift_span(span: Span, offset: usize) -> Span {
        Span::new(span.start + offset, span.end + offset)
    }

    /// 解析列表字面量。
    fn parse_list_literal(&mut self, start: Span) -> Result<Expr, CompilerError> {
        let mut elements = Vec::new();

        if !self.check(TokenTag::RightBracket) {
            loop {
                elements.push(self.parse_expression()?);
                if !self.match_tag(TokenTag::Comma) {
                    break;
                }
            }
        }

        let close = self.consume(TokenTag::RightBracket, "列表字面量缺少右方括号 `]`")?;
        Ok(Expr::ListLiteral {
            elements,
            span: Span::merge(start, close.span),
        })
    }

    /// 解析字典字面量。
    fn parse_map_literal(&mut self, start: Span) -> Result<Expr, CompilerError> {
        let mut entries = Vec::new();
        self.skip_literal_layout();

        if !self.check(TokenTag::RightBrace) {
            loop {
                let key = self.parse_expression()?;
                self.consume(TokenTag::Colon, "字典字面量中的键和值之间缺少冒号 `:`")?;
                let value = self.parse_expression()?;
                entries.push(MapEntry {
                    span: Span::merge(key.span(), value.span()),
                    key,
                    value,
                });
                if !self.match_tag(TokenTag::Comma) {
                    break;
                }
                self.skip_literal_layout();
                if self.check(TokenTag::RightBrace) {
                    break;
                }
            }
        }

        self.skip_literal_layout();
        let close = self.consume(TokenTag::RightBrace, "字典字面量缺少右花括号 `}`")?;
        Ok(Expr::MapLiteral {
            entries,
            span: Span::merge(start, close.span),
        })
    }

    /// 解析 `array<T,N>(...)` 形式的固定长度数组初始化表达式。
    fn parse_array_initializer(&mut self, start: Span) -> Result<Expr, CompilerError> {
        self.consume(TokenTag::Less, "`array` 初始化缺少左尖括号 `<`")?;
        let element_type = self.parse_type_name()?;
        self.consume(
            TokenTag::Comma,
            "`array` 初始化缺少元素类型与长度之间的逗号 `,`",
        )?;
        let size = self.parse_array_size()?;
        self.consume(TokenTag::Greater, "`array` 初始化缺少右尖括号 `>`")?;
        self.consume(TokenTag::LeftParen, "`array` 初始化缺少左括号 `(`")?;

        let mut elements = Vec::new();
        if !self.check(TokenTag::RightParen) {
            loop {
                elements.push(self.parse_expression()?);
                if !self.match_tag(TokenTag::Comma) {
                    break;
                }
            }
        }

        let close = self.consume(TokenTag::RightParen, "`array` 初始化缺少右括号 `)`")?;
        Ok(Expr::ArrayInitializer {
            element_type,
            size,
            elements,
            span: Span::merge(start, close.span),
        })
    }

    /// 解析显式类型标注中的内建类型名称、`list<T>` 或 `array<T,N>` 类型。
    fn parse_type_name(&mut self) -> Result<TypeName, CompilerError> {
        let (name, span) = self.consume_identifier("类型标注缺少类型名")?;
        match name.as_str() {
            "int" => Ok(TypeName::Int),
            "float" => Ok(TypeName::Float),
            "bool" => Ok(TypeName::Bool),
            "string" => Ok(TypeName::String),
            "list" => {
                self.consume(TokenTag::Less, "`list` 类型缺少左尖括号 `<`")?;
                let element_type = self.parse_type_name()?;
                self.consume(TokenTag::Greater, "`list` 类型缺少右尖括号 `>`")?;
                Ok(TypeName::List(Box::new(element_type)))
            }
            "map" => {
                self.consume(TokenTag::Less, "`map` 类型缺少左尖括号 `<`")?;
                let key_type = self.parse_type_name()?;
                self.consume(
                    TokenTag::Comma,
                    "`map` 类型缺少键类型与值类型之间的逗号 `,`",
                )?;
                let value_type = self.parse_type_name()?;
                self.consume(TokenTag::Greater, "`map` 类型缺少右尖括号 `>`")?;
                Ok(TypeName::Map(Box::new(key_type), Box::new(value_type)))
            }
            "array" => {
                self.consume(TokenTag::Less, "`array` 类型缺少左尖括号 `<`")?;
                let element_type = self.parse_type_name()?;
                self.consume(
                    TokenTag::Comma,
                    "`array` 类型缺少元素类型与长度之间的逗号 `,`",
                )?;
                let size = self.parse_array_size()?;
                self.consume(TokenTag::Greater, "`array` 类型缺少右尖括号 `>`")?;
                Ok(TypeName::Array(Box::new(element_type), size))
            }
            _ => Err(CompilerError::syntax_message(
                format!("未知的内建类型 `{name}`"),
                span,
            )),
        }
    }

    /// 解析数组类型和初始化中的固定长度常量。
    fn parse_array_size(&mut self) -> Result<usize, CompilerError> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::IntLiteral(value) if value > 0 => Ok(value as usize),
            TokenKind::IntLiteral(_) => Err(CompilerError::syntax_message(
                "数组长度必须是大于 0 的整数",
                token.span,
            )),
            _ => Err(CompilerError::syntax_message(
                "数组长度必须是整数常量",
                token.span,
            )),
        }
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
            _ => Err(CompilerError::syntax_message(
                "这里需要一个赋值运算符",
                token.span,
            )),
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
        self.is_identifier_assignment_start() || self.is_index_assignment_start()
    }

    /// 判断当前位置是否符合普通变量赋值语句的开头模式。
    fn is_identifier_assignment_start(&self) -> bool {
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

    /// 判断当前位置是否符合 `name[index] = value` 形式的下标赋值模式。
    fn is_index_assignment_start(&self) -> bool {
        if !(self.check(TokenTag::Identifier)
            && self.peek_next().kind.tag() == TokenTag::LeftBracket)
        {
            return false;
        }

        let mut depth = 0usize;
        for position in (self.current + 1)..self.tokens.len() {
            match self.tokens[position].kind.tag() {
                TokenTag::LeftBracket => depth += 1,
                TokenTag::RightBracket => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return self
                            .tokens
                            .get(position + 1)
                            .is_some_and(|token| token.kind.tag() == TokenTag::Equal);
                    }
                }
                TokenTag::Newline | TokenTag::Dedent | TokenTag::Eof if depth == 0 => return false,
                _ => {}
            }
        }

        false
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

    /// 跳过容器字面量内部仅用于排版的换行和缩进记号，避免多行写法被误判为语法错误。
    fn skip_literal_layout(&mut self) {
        while matches!(
            self.peek().kind.tag(),
            TokenTag::Newline | TokenTag::Indent | TokenTag::Dedent
        ) {
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
    use crate::ast::{
        AssignOp, AssignTarget, BinaryOp, BindingKind, Expr, ImportKind, MethodName,
        PropertyName, Stmt, TypeName, UnaryOp,
    };
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

    /// 先做词法分析和语法分析，并把失败结果渲染为稳定的中文诊断文本。
    fn parse_error(text: &str) -> String {
        let settings = CompilerSettings::new(UiLanguage::ZhCn);
        let source = SourceFile::new("<test>", text.to_string());
        let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
        Parser::new(tokens, &settings)
            .parse_program()
            .unwrap_err()
            .render(settings.language, Some(&source))
    }

    /// 校验函数定义能够被正确解析为顶层语句节点。
    #[test]
    fn should_parse_function_declaration() {
        let program = parse("func add(a:int, b:int) -> int:\n    return a + b\n");
        assert!(matches!(program.statements[0], Stmt::FunctionDecl { .. }));
    }

    /// 校验 `extern func` 声明能够被正确解析为独立的原生函数声明节点。
    #[test]
    fn should_parse_extern_function_declaration() {
        let program = parse("pub extern func mas_time_now_unix() -> int\n");
        assert!(matches!(program.statements[0], Stmt::ExternFunctionDecl { .. }));
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

    /// 校验具名导入与别名导入能够被正确解析为独立语句节点。
    #[test]
    fn should_parse_named_import_with_alias() {
        let program = parse("import utils.math { add as plus, sub }\n");
        let Stmt::Import { path, kind, .. } = &program.statements[0] else {
            panic!("首条语句不是导入语句");
        };
        assert_eq!(path.segments, vec!["utils", "math"]);
        let ImportKind::Named(items) = kind else {
            panic!("导入语句没有解析为具名导入");
        };
        assert_eq!(items[0].name, "add");
        assert_eq!(items[0].alias.as_deref(), Some("plus"));
        assert_eq!(items[1].name, "sub");
    }

    /// 校验 `export import` 与全部导入语法能够被正确识别。
    #[test]
    fn should_parse_export_import_all() {
        let program = parse("export import local.utils.*\n");
        let Stmt::ExportImport { path, kind, .. } = &program.statements[0] else {
            panic!("首条语句不是导出导入语句");
        };
        assert_eq!(path.segments, vec!["local", "utils"]);
        assert!(matches!(kind, ImportKind::All));
    }

    /// 校验 `if / elif / else` 链能够被解析为统一条件分支节点。
    #[test]
    fn should_parse_if_statement() {
        let program =
            parse("if true:\n    print(1)\nelif false:\n    print(2)\nelse:\n    print(3)\n");
        assert!(matches!(program.statements[0], Stmt::If { .. }));
    }

    /// 校验范围 `for` 循环能够解析起止表达式和循环体。
    #[test]
    fn should_parse_for_range_statement() {
        let program = parse("for i in 0..10:\n    print(i)\n");
        assert!(matches!(program.statements[0], Stmt::ForRange { .. }));
    }

    /// 校验列表遍历循环会被解析为独立语句节点。
    #[test]
    fn should_parse_for_each_statement() {
        let program = parse("for item in items:\n    print(item)\n");
        assert!(matches!(program.statements[0], Stmt::ForEach { .. }));
    }

    /// 校验字典遍历会保留键和值两个循环变量名。
    #[test]
    fn should_parse_map_for_each_statement() {
        let program = parse("for key, value in user:\n    print(key)\n");
        let Stmt::ForEach {
            key_name,
            value_name,
            ..
        } = &program.statements[0]
        else {
            panic!("首条语句不是 for-each 语句");
        };
        assert_eq!(key_name, "key");
        assert_eq!(value_name.as_deref(), Some("value"));
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

    /// 校验字典下标赋值会被解析为独立的下标赋值目标。
    #[test]
    fn should_parse_index_assignment() {
        let program = parse("user[\"name\"] = \"Mascript\"\n");
        let Stmt::Assign { target, op, .. } = &program.statements[0] else {
            panic!("首条语句不是赋值语句");
        };
        assert_eq!(*op, AssignOp::Assign);
        assert!(matches!(target, AssignTarget::Index { .. }));
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

    /// 校验列表字面量、索引、切片和方法调用能够被正确解析。
    #[test]
    fn should_parse_list_expressions() {
        let program = parse(
            "var items:list<int> = [1, 2, 3]\nprint(items[1])\nprint(items[1:3])\nitems.append(4)\n",
        );
        let Stmt::VarDecl {
            annotation,
            initializer,
            ..
        } = &program.statements[0]
        else {
            panic!("首条语句不是绑定声明");
        };
        assert!(matches!(annotation, Some(TypeName::List(_))));
        assert!(matches!(initializer, Some(Expr::ListLiteral { .. })));

        let Stmt::ExprStmt { expr, .. } = &program.statements[3] else {
            panic!("第四条语句不是表达式语句");
        };
        let Expr::MethodCall { method, .. } = expr else {
            panic!("第四条语句不是方法调用");
        };
        assert_eq!(*method, MethodName::Append);
    }

    /// 校验字典类型、字面量、索引和方法调用能够被正确解析。
    #[test]
    fn should_parse_map_expressions() {
        let program = parse(
            "var user:map<string,int> = {\"age\": 18}\nprint(user[\"age\"])\nuser.contains(\"age\")\n",
        );
        let Stmt::VarDecl {
            annotation,
            initializer,
            ..
        } = &program.statements[0]
        else {
            panic!("首条语句不是绑定声明");
        };
        assert!(matches!(annotation, Some(TypeName::Map(_, _))));
        assert!(matches!(initializer, Some(Expr::MapLiteral { .. })));

        let Stmt::ExprStmt { expr, .. } = &program.statements[2] else {
            panic!("第三条语句不是表达式语句");
        };
        let Expr::MethodCall { method, .. } = expr else {
            panic!("第三条语句不是方法调用");
        };
        assert_eq!(*method, MethodName::Contains);
    }

    /// 校验字符串实例方法会被统一解析为 `Expr::MethodCall` 节点。
    #[test]
    fn should_parse_string_method_expressions() {
        let program = parse(
            "var text = \"  你好，世界  \"\ntext.trim()\ntext.contains(\"好\")\ntext.starts_with(\" 你\")\ntext.ends_with(\"界  \")\ntext.replace(\"世界\", \"Mascript\")\n",
        );
        let expected_methods = [
            MethodName::Trim,
            MethodName::Contains,
            MethodName::StartsWith,
            MethodName::EndsWith,
            MethodName::Replace,
        ];

        for (statement, expected_method) in program.statements[1..].iter().zip(expected_methods.iter()) {
            let Stmt::ExprStmt { expr, .. } = statement else {
                panic!("字符串方法语句不是表达式语句");
            };
            let Expr::MethodCall { method, .. } = expr else {
                panic!("字符串方法语句没有解析成方法调用");
            };
            assert_eq!(method, expected_method);
        }
    }

    /// 校验未知的字符串对象方法仍会给出明确的未支持语法诊断。
    #[test]
    fn should_reject_unsupported_string_method_name() {
        let error = parse_error("var text = \"Mascript\"\ntext.lowercase()\n");
        assert!(error.contains("当前阶段不支持对象方法 `lowercase`"));
    }

    /// 校验安全转换结果的 `ok/value` 只读属性能够被正确解析。
    #[test]
    fn should_parse_safe_convert_property_access() {
        let program = parse("var result = try_int(\"123\")\nprint(result.ok)\nprint(result.value)\n");

        let Stmt::ExprStmt { expr, .. } = &program.statements[1] else {
            panic!("第二条语句不是表达式语句");
        };
        let Expr::Call { arguments, .. } = expr else {
            panic!("第二条语句不是函数调用");
        };
        let Expr::PropertyAccess { property, .. } = &arguments[0] else {
            panic!("第二条语句参数不是属性访问");
        };
        assert_eq!(*property, PropertyName::Ok);

        let Stmt::ExprStmt { expr, .. } = &program.statements[2] else {
            panic!("第三条语句不是表达式语句");
        };
        let Expr::Call { arguments, .. } = expr else {
            panic!("第三条语句不是函数调用");
        };
        let Expr::PropertyAccess { property, .. } = &arguments[0] else {
            panic!("第三条语句参数不是属性访问");
        };
        assert_eq!(*property, PropertyName::Value);
    }

    /// 校验模板字符串会被解析为复用现有字符串拼接与 `string(...)` 转换的表达式树。
    #[test]
    fn should_parse_template_string_as_concat_expression() {
        let program = parse("var name = \"Mascript\"\nprint(\"Hello ${name}: ${1 + 2}\")\n");

        let Stmt::ExprStmt { expr, .. } = &program.statements[1] else {
            panic!("第二条语句不是表达式语句");
        };
        let Expr::Call { arguments, .. } = expr else {
            panic!("第二条语句不是函数调用");
        };
        let Expr::Binary { op, right, .. } = &arguments[0] else {
            panic!("模板字符串没有被解析为拼接表达式");
        };
        assert_eq!(*op, BinaryOp::Add);

        let Expr::Call { callee, arguments, .. } = &**right else {
            panic!("模板字符串的最后一段没有被包装成 string(...) 调用");
        };
        let Expr::Identifier { name, .. } = &**callee else {
            panic!("模板字符串的插值转换不是具名函数调用");
        };
        assert_eq!(name, "string");
        assert!(matches!(
            arguments[0],
            Expr::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));

        assert!(matches!(
            arguments[0],
            Expr::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));
    }

    /// 校验多行字典字面量中的换行不会被误判为缺少表达式。
    #[test]
    fn should_parse_multiline_map_literal() {
        let program = parse(
            "func main():\n    var user:map<string,int> = {\n        \"age\": 18,\n        \"score\": 99\n    }\n",
        );
        let Stmt::FunctionDecl { body, .. } = &program.statements[0] else {
            panic!("首条语句不是函数声明");
        };
        let Stmt::VarDecl { initializer, .. } = &body[0] else {
            panic!("函数体首条语句不是绑定声明");
        };
        let Some(Expr::MapLiteral { entries, .. }) = initializer else {
            panic!("初始化值不是字典字面量");
        };
        assert_eq!(entries.len(), 2);
    }

    /// 校验数组类型、默认声明和初始化表达式能够被正确解析。
    #[test]
    fn should_parse_array_expressions() {
        let program = parse(
            "var fixed:array<int, 3>\nvar values = array<int, 3>(1, 2, 3)\nprint(values[1])\n",
        );
        let Stmt::VarDecl {
            annotation,
            initializer,
            ..
        } = &program.statements[0]
        else {
            panic!("首条语句不是绑定声明");
        };
        assert!(matches!(annotation, Some(TypeName::Array(_, 3))));
        assert!(initializer.is_none());

        let Stmt::VarDecl { initializer, .. } = &program.statements[1] else {
            panic!("第二条语句不是绑定声明");
        };
        assert!(matches!(
            initializer,
            Some(Expr::ArrayInitializer { size: 3, .. })
        ));
    }
}
