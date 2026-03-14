use crate::diagnostic::{CompilerError, MessageId};
use crate::settings::CompilerSettings;
use crate::source::{SourceFile, Span};
use crate::token::{Token, TokenKind};

/// 按顺序把源码文本切分成缩进敏感的词法单元序列。
pub struct Lexer<'a> {
    source: &'a SourceFile,
    position: usize,
    settings: &'a CompilerSettings,
}

impl<'a> Lexer<'a> {
    /// 使用给定源码和设置创建新的词法分析器实例。
    pub fn new(source: &'a SourceFile, settings: &'a CompilerSettings) -> Self {
        Self {
            source,
            position: 0,
            settings,
        }
    }

    /// 完整扫描源码文本，并在末尾补上 `EOF` 词法单元。
    pub fn tokenize(mut self) -> Result<Vec<Token>, CompilerError> {
        let _ = self.settings;
        let mut tokens = Vec::new();
        let mut indent_stack = vec![0usize];
        let text = self.source.text();
        let mut line_start = 0usize;

        while line_start < text.len() {
            let line_end = text[line_start..]
                .find('\n')
                .map(|offset| line_start + offset)
                .unwrap_or(text.len());
            let next_line_start = if line_end < text.len() {
                line_end + 1
            } else {
                line_end
            };

            let (indent_width, content_start, is_blank) =
                self.inspect_line(line_start, line_end)?;
            if !is_blank {
                self.emit_indentation(
                    line_start,
                    content_start,
                    indent_width,
                    &mut indent_stack,
                    &mut tokens,
                )?;
                self.position = content_start;
                self.lex_line_content(line_end, &mut tokens)?;
            }

            tokens.push(Token::new(
                TokenKind::Newline,
                Span::new(line_end, next_line_start),
            ));
            line_start = next_line_start;
        }

        while indent_stack.len() > 1 {
            indent_stack.pop();
            tokens.push(Token::new(
                TokenKind::Dedent,
                Span::new(text.len(), text.len()),
            ));
        }

        tokens.push(Token::new(
            TokenKind::Eof,
            Span::new(text.len(), text.len()),
        ));
        Ok(tokens)
    }

    /// 检查一行的前导空白，并判断该行是否为空行或注释行。
    fn inspect_line(
        &self,
        line_start: usize,
        line_end: usize,
    ) -> Result<(usize, usize, bool), CompilerError> {
        let bytes = self.source.text().as_bytes();
        let mut cursor = line_start;
        let mut indent_width = 0usize;

        while cursor < line_end {
            match bytes[cursor] {
                b' ' => {
                    indent_width += 1;
                    cursor += 1;
                }
                b'\t' => {
                    return Err(CompilerError::lexical(
                        MessageId::LexTabIndent,
                        vec![],
                        Span::new(cursor, cursor + 1),
                    ));
                }
                _ => break,
            }
        }

        let remainder = &self.source.text()[cursor..line_end];
        let is_blank = remainder.trim().is_empty() || remainder.trim_start().starts_with("//");
        Ok((indent_width, cursor, is_blank))
    }

    /// 按严格的 4 空格规则生成 `INDENT` 与 `DEDENT` 词法单元。
    fn emit_indentation(
        &self,
        line_start: usize,
        content_start: usize,
        indent_width: usize,
        indent_stack: &mut Vec<usize>,
        tokens: &mut Vec<Token>,
    ) -> Result<(), CompilerError> {
        if indent_width % 4 != 0 {
            return Err(CompilerError::lexical(
                MessageId::LexIndentMultipleOfFour,
                vec![],
                Span::new(line_start, content_start),
            ));
        }

        let current = *indent_stack.last().expect("缩进栈必须至少保留根级缩进");
        if indent_width > current {
            if indent_width != current + 4 {
                return Err(CompilerError::lexical(
                    MessageId::LexIndentJumpTooLarge,
                    vec![],
                    Span::new(line_start, content_start),
                ));
            }
            indent_stack.push(indent_width);
            tokens.push(Token::new(
                TokenKind::Indent,
                Span::new(line_start, content_start),
            ));
            return Ok(());
        }

        while indent_width < *indent_stack.last().expect("缩进栈必须至少保留根级缩进")
        {
            indent_stack.pop();
            tokens.push(Token::new(
                TokenKind::Dedent,
                Span::new(line_start, content_start),
            ));
        }

        if indent_width != *indent_stack.last().expect("缩进栈必须至少保留根级缩进") {
            return Err(CompilerError::lexical(
                MessageId::LexIndentMismatch,
                vec![],
                Span::new(line_start, content_start),
            ));
        }

        Ok(())
    }

    /// 扫描单行内部的词法单元，直到换行符或注释起始位置。
    fn lex_line_content(
        &mut self,
        line_end: usize,
        tokens: &mut Vec<Token>,
    ) -> Result<(), CompilerError> {
        while self.position < line_end {
            let character = self
                .peek_char(line_end)
                .expect("当前位置必须位于当前行内部");
            match character {
                ' ' | '\r' => {
                    self.advance_char(line_end);
                }
                '\t' => {
                    return Err(CompilerError::lexical(
                        MessageId::LexTabInCode,
                        vec![],
                        Span::new(self.position, self.position + 1),
                    ));
                }
                '/' if self.peek_next_char(line_end) == Some('/') => break,
                '"' => tokens.push(self.lex_string(line_end)?),
                '0'..='9' => tokens.push(self.lex_number(line_end)?),
                _ if is_identifier_start(character) => tokens.push(self.lex_identifier(line_end)),
                _ => tokens.push(self.lex_operator_or_punctuation(line_end)?),
            }
        }

        Ok(())
    }

    /// 读取当前位置字符而不推进游标，限定读取范围不跨越当前行。
    fn peek_char(&self, line_end: usize) -> Option<char> {
        if self.position >= line_end {
            return None;
        }
        self.source.text()[self.position..line_end].chars().next()
    }

    /// 读取当前位置之后的下一个字符，限定读取范围不跨越当前行。
    fn peek_next_char(&self, line_end: usize) -> Option<char> {
        let mut chars = self.source.text()[self.position..line_end].chars();
        chars.next()?;
        chars.next()
    }

    /// 消费当前位置字符并推进到下一个字节位置。
    fn advance_char(&mut self, line_end: usize) -> Option<char> {
        let character = self.peek_char(line_end)?;
        self.position += character.len_utf8();
        Some(character)
    }

    /// 解析字符串字面量，并处理最基础的转义序列。
    fn lex_string(&mut self, line_end: usize) -> Result<Token, CompilerError> {
        let start = self.position;
        self.advance_char(line_end);
        let mut value = String::new();

        while let Some(character) = self.peek_char(line_end) {
            match character {
                '"' => {
                    self.advance_char(line_end);
                    return Ok(Token::new(
                        TokenKind::StringLiteral(value),
                        Span::new(start, self.position),
                    ));
                }
                '\\' => {
                    self.advance_char(line_end);
                    let escaped = self.advance_char(line_end).ok_or_else(|| {
                        CompilerError::lexical(
                            MessageId::LexUnterminatedString,
                            vec![],
                            Span::new(start, self.position),
                        )
                    })?;
                    let mapped = match escaped {
                        '"' => '"',
                        '\\' => '\\',
                        'n' => '\n',
                        't' => '\t',
                        _ => {
                            return Err(CompilerError::lexical(
                                MessageId::LexInvalidEscape,
                                vec![escaped.to_string()],
                                Span::new(self.position.saturating_sub(1), self.position),
                            ));
                        }
                    };
                    value.push(mapped);
                }
                _ => {
                    value.push(character);
                    self.advance_char(line_end);
                }
            }
        }

        Err(CompilerError::lexical(
            MessageId::LexUnterminatedString,
            vec![],
            Span::new(start, line_end),
        ))
    }

    /// 解析整数或浮点数字面量，并避免把 `0..5` 误识别为浮点数。
    fn lex_number(&mut self, line_end: usize) -> Result<Token, CompilerError> {
        let start = self.position;
        let mut seen_dot = false;

        while let Some(character) = self.peek_char(line_end) {
            if character.is_ascii_digit() {
                self.advance_char(line_end);
                continue;
            }

            if character == '.'
                && !seen_dot
                && self
                    .peek_next_char(line_end)
                    .is_some_and(|next| next.is_ascii_digit())
                && !self.source.text()[self.position..line_end].starts_with("..")
            {
                seen_dot = true;
                self.advance_char(line_end);
                continue;
            }

            break;
        }

        let text = &self.source.text()[start..self.position];
        let kind = if seen_dot {
            TokenKind::FloatLiteral(text.parse().map_err(|_| {
                CompilerError::lexical(
                    MessageId::LexInvalidFloat,
                    vec![],
                    Span::new(start, self.position),
                )
            })?)
        } else {
            TokenKind::IntLiteral(text.parse().map_err(|_| {
                CompilerError::lexical(
                    MessageId::LexInvalidInt,
                    vec![],
                    Span::new(start, self.position),
                )
            })?)
        };

        Ok(Token::new(kind, Span::new(start, self.position)))
    }

    /// 解析关键字、布尔字面量或普通标识符。
    fn lex_identifier(&mut self, line_end: usize) -> Token {
        let start = self.position;

        while let Some(character) = self.peek_char(line_end) {
            if is_identifier_continue(character) {
                self.advance_char(line_end);
            } else {
                break;
            }
        }

        let text = &self.source.text()[start..self.position];
        let kind = match text {
            "var" => TokenKind::Var,
            "const" => TokenKind::Const,
            "func" => TokenKind::Func,
            "if" => TokenKind::If,
            "elif" => TokenKind::Elif,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "return" => TokenKind::Return,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "true" => TokenKind::BoolLiteral(true),
            "false" => TokenKind::BoolLiteral(false),
            _ => TokenKind::Identifier(text.to_string()),
        };

        Token::new(kind, Span::new(start, self.position))
    }

    /// 解析多字符运算符、单字符运算符和分隔符。
    fn lex_operator_or_punctuation(&mut self, line_end: usize) -> Result<Token, CompilerError> {
        let start = self.position;
        let rest = &self.source.text()[self.position..line_end];

        for (pattern, kind) in [
            ("->", TokenKind::Arrow),
            ("%/%", TokenKind::IntDivide),
            ("+=", TokenKind::PlusEqual),
            ("-=", TokenKind::MinusEqual),
            ("*=", TokenKind::StarEqual),
            ("/=", TokenKind::SlashEqual),
            ("%=", TokenKind::PercentEqual),
            ("..", TokenKind::DotDot),
            ("==", TokenKind::EqualEqual),
            ("!=", TokenKind::BangEqual),
            ("<=", TokenKind::LessEqual),
            (">=", TokenKind::GreaterEqual),
        ] {
            if rest.starts_with(pattern) {
                self.position += pattern.len();
                return Ok(Token::new(kind, Span::new(start, self.position)));
            }
        }

        let character = self.advance_char(line_end).ok_or_else(|| {
            CompilerError::lexical(
                MessageId::LexInvalidCharacter,
                vec!["".to_string()],
                Span::new(start, start),
            )
        })?;
        let kind = match character {
            ':' => TokenKind::Colon,
            '=' => TokenKind::Equal,
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            '<' => TokenKind::Less,
            '>' => TokenKind::Greater,
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            ',' => TokenKind::Comma,
            _ => {
                return Err(CompilerError::lexical(
                    MessageId::LexInvalidCharacter,
                    vec![character.to_string()],
                    Span::new(start, self.position),
                ));
            }
        };
        Ok(Token::new(kind, Span::new(start, self.position)))
    }
}

/// 判断字符是否可以作为标识符起始字符。
fn is_identifier_start(character: char) -> bool {
    character == '_' || character.is_alphabetic()
}

/// 判断字符是否可以作为标识符后续字符。
fn is_identifier_continue(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::Lexer;
    use crate::settings::CompilerSettings;
    use crate::token::{TokenKind, TokenTag};
    use crate::{settings::UiLanguage, source::SourceFile};

    /// 执行一次词法分析，并返回便于断言的词法类别列表。
    fn lex(text: &str) -> Vec<TokenKind> {
        let source = SourceFile::new("<test>", text.to_string());
        Lexer::new(&source, &CompilerSettings::new(UiLanguage::ZhCn))
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    /// 校验词法分析器能够正确识别基础字面量。
    #[test]
    fn should_tokenize_basic_literals() {
        let kinds = lex("var a = 1\nvar b = 1.5\nvar c = true\nvar d = \"文本\"\n");
        assert!(kinds.contains(&TokenKind::IntLiteral(1)));
        assert!(kinds.contains(&TokenKind::FloatLiteral(1.5)));
        assert!(kinds.contains(&TokenKind::BoolLiteral(true)));
        assert!(kinds.contains(&TokenKind::StringLiteral("文本".to_string())));
    }

    /// 校验第二阶段新增运算符能够被正确识别。
    #[test]
    fn should_tokenize_phase2_operators() {
        let kinds = lex("func add(a:int) -> int:\n    a += 1\n    return a %/% 2\nfor i in 0..5:\n    print(i % 2)\n");
        assert!(kinds.contains(&TokenKind::Arrow));
        assert!(kinds.contains(&TokenKind::PlusEqual));
        assert!(kinds.contains(&TokenKind::IntDivide));
        assert!(kinds.contains(&TokenKind::DotDot));
        assert!(kinds.contains(&TokenKind::Percent));
    }

    /// 校验 `0..5` 会被切成整数加区间运算符，而不是浮点数。
    #[test]
    fn should_tokenize_range_without_float_conflict() {
        let kinds = lex("for i in 0..5:\n    print(i)\n");
        assert!(kinds.windows(3).any(|window| {
            window == [
                TokenKind::IntLiteral(0),
                TokenKind::DotDot,
                TokenKind::IntLiteral(5),
            ]
        }));
    }

    /// 校验冒号块结构依然会生成缩进相关词法单元。
    #[test]
    fn should_tokenize_indent_and_dedent() {
        let kinds = lex("if true:\n    print(1)\nprint(2)\n");
        assert!(kinds.iter().any(|kind| kind.tag() == TokenTag::Indent));
        assert!(kinds.iter().any(|kind| kind.tag() == TokenTag::Dedent));
    }

    /// 校验非法字符会在词法分析阶段被立即拒绝。
    #[test]
    fn should_report_invalid_character() {
        let source = SourceFile::new("<test>", "@\n".to_string());
        let error = Lexer::new(&source, &CompilerSettings::new(UiLanguage::ZhCn))
            .tokenize()
            .unwrap_err();
        assert_eq!(
            error.message_id,
            crate::diagnostic::MessageId::LexInvalidCharacter
        );
    }

    /// 校验不合规缩进会触发专门的词法错误。
    #[test]
    fn should_report_invalid_indent() {
        let source = SourceFile::new("<test>", "if true:\n  print(1)\n".to_string());
        let error = Lexer::new(&source, &CompilerSettings::new(UiLanguage::ZhCn))
            .tokenize()
            .unwrap_err();
        assert_eq!(
            error.message_id,
            crate::diagnostic::MessageId::LexIndentMultipleOfFour
        );
    }
}
