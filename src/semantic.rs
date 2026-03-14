use std::collections::HashMap;

use crate::ast::{BinaryOp, Expr, Literal, Program, Stmt, TypeName, UnaryOp};
use crate::diagnostic::{CompilerError, MessageId};
use crate::settings::CompilerSettings;
use crate::source::Span;

/// 表示语义分析阶段内部使用的值类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueType {
    Int,
    Float,
    Bool,
    String,
    Unit,
}

impl ValueType {
    /// 把显式类型标注转换为语义阶段的值类型枚举。
    pub fn from_annotation(annotation: TypeName) -> Self {
        match annotation {
            TypeName::Int => Self::Int,
            TypeName::Float => Self::Float,
            TypeName::Bool => Self::Bool,
            TypeName::String => Self::String,
        }
    }

    /// 返回类型在诊断信息中使用的稳定名称。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Unit => "unit",
        }
    }
}

/// 使用符号表对 AST 进行静态语义检查。
pub struct SemanticAnalyzer<'a> {
    settings: &'a CompilerSettings,
    symbols: HashMap<String, ValueType>,
}

impl<'a> SemanticAnalyzer<'a> {
    /// 创建一个新的语义分析器实例。
    pub fn new(settings: &'a CompilerSettings) -> Self {
        Self {
            settings,
            symbols: HashMap::new(),
        }
    }

    /// 逐条分析程序中的语句，并检查名称和类型规则。
    pub fn analyze(&mut self, program: &Program) -> Result<(), CompilerError> {
        let _ = self.settings;
        for statement in &program.statements {
            self.analyze_statement(statement)?;
        }
        Ok(())
    }

    /// 分析单条语句，并在需要时更新当前符号表。
    fn analyze_statement(&mut self, statement: &Stmt) -> Result<(), CompilerError> {
        match statement {
            Stmt::VarDecl {
                name,
                annotation,
                initializer,
                span,
            } => {
                if self.symbols.contains_key(name) {
                    return Err(CompilerError::semantic(
                        MessageId::SemanticDuplicateVariable,
                        vec![name.clone()],
                        *span,
                    ));
                }

                let inferred = self.infer_expression_type(initializer)?;
                if let Some(annotation) = annotation {
                    let expected = ValueType::from_annotation(*annotation);
                    if expected != inferred {
                        return Err(self.type_mismatch_error(expected, inferred, *span));
                    }
                }

                self.symbols.insert(name.clone(), inferred);
                Ok(())
            }
            Stmt::ExprStmt { expr, .. } => {
                self.infer_expression_type(expr)?;
                Ok(())
            }
        }
    }

    /// 推导表达式类型，并在遇到不合法组合时抛出语义错误。
    fn infer_expression_type(&mut self, expression: &Expr) -> Result<ValueType, CompilerError> {
        match expression {
            Expr::Literal { value, .. } => Ok(match value {
                Literal::Int(_) => ValueType::Int,
                Literal::Float(_) => ValueType::Float,
                Literal::Bool(_) => ValueType::Bool,
                Literal::String(_) => ValueType::String,
            }),
            Expr::Identifier { name, span } => self.symbols.get(name).copied().ok_or_else(|| {
                CompilerError::semantic(
                    MessageId::SemanticUnknownVariable,
                    vec![name.clone()],
                    *span,
                )
            }),
            Expr::Grouped { expression, .. } => self.infer_expression_type(expression),
            Expr::Unary { op, operand, span } => {
                let operand_type = self.infer_expression_type(operand)?;
                match (op, operand_type) {
                    (UnaryOp::Negate, ValueType::Int) => Ok(ValueType::Int),
                    (UnaryOp::Negate, ValueType::Float) => Ok(ValueType::Float),
                    (UnaryOp::Not, ValueType::Bool) => Ok(ValueType::Bool),
                    _ => Err(CompilerError::semantic(
                        MessageId::SemanticInvalidUnaryOperand,
                        vec![],
                        *span,
                    )),
                }
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                let left_type = self.infer_expression_type(left)?;
                let right_type = self.infer_expression_type(right)?;
                self.infer_binary_type(*op, left_type, right_type, *span)
            }
            Expr::Call {
                callee,
                arguments,
                span,
            } => self.infer_call_type(callee, arguments, *span),
        }
    }

    /// 根据运算符和两侧类型推导二元表达式的结果类型。
    fn infer_binary_type(
        &self,
        op: BinaryOp,
        left: ValueType,
        right: ValueType,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match op {
            BinaryOp::Add => match (left, right) {
                (ValueType::Int, ValueType::Int) => Ok(ValueType::Int),
                (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                (ValueType::String, ValueType::String) => Ok(ValueType::String),
                _ => Err(CompilerError::semantic(
                    MessageId::SemanticInvalidBinaryOperand,
                    vec![],
                    span,
                )),
            },
            BinaryOp::Subtract | BinaryOp::Multiply => match (left, right) {
                (ValueType::Int, ValueType::Int) => Ok(ValueType::Int),
                (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                _ => Err(CompilerError::semantic(
                    MessageId::SemanticInvalidBinaryOperand,
                    vec![],
                    span,
                )),
            },
            BinaryOp::Divide => match (left, right) {
                (ValueType::Int, ValueType::Int)
                | (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                _ => Err(CompilerError::semantic(
                    MessageId::SemanticInvalidBinaryOperand,
                    vec![],
                    span,
                )),
            },
            BinaryOp::And | BinaryOp::Or => {
                if left == ValueType::Bool && right == ValueType::Bool {
                    Ok(ValueType::Bool)
                } else {
                    Err(CompilerError::semantic(
                        MessageId::SemanticInvalidBinaryOperand,
                        vec![],
                        span,
                    ))
                }
            }
            BinaryOp::Equal | BinaryOp::NotEqual => {
                if left == right && left != ValueType::Unit {
                    Ok(ValueType::Bool)
                } else {
                    Err(CompilerError::semantic(
                        MessageId::SemanticInvalidBinaryOperand,
                        vec![],
                        span,
                    ))
                }
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                match (left, right) {
                    (ValueType::Int, ValueType::Int)
                    | (ValueType::Float, ValueType::Float)
                    | (ValueType::Float, ValueType::Int)
                    | (ValueType::Int, ValueType::Float) => Ok(ValueType::Bool),
                    _ => Err(CompilerError::semantic(
                        MessageId::SemanticInvalidBinaryOperand,
                        vec![],
                        span,
                    )),
                }
            }
        }
    }

    /// 校验内建函数调用是否合法，并返回其结果类型。
    fn infer_call_type(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let Expr::Identifier { name, .. } = callee else {
            return Err(CompilerError::semantic(
                MessageId::SemanticOnlyNamedCall,
                vec![],
                span,
            ));
        };

        if name != "print" {
            return Err(CompilerError::semantic(
                MessageId::SemanticUnknownFunction,
                vec![name.clone()],
                callee.span(),
            ));
        }

        if arguments.len() != 1 {
            return Err(CompilerError::semantic(
                MessageId::SemanticPrintArity,
                vec![],
                span,
            ));
        }

        let value_type = self.infer_expression_type(&arguments[0])?;
        if value_type == ValueType::Unit {
            return Err(CompilerError::semantic(
                MessageId::SemanticInvalidBinaryOperand,
                vec![],
                arguments[0].span(),
            ));
        }

        Ok(ValueType::Unit)
    }

    /// 构造显式类型和推断类型不一致时使用的错误对象。
    fn type_mismatch_error(
        &self,
        expected: ValueType,
        actual: ValueType,
        span: Span,
    ) -> CompilerError {
        CompilerError::semantic(
            MessageId::SemanticTypeMismatch,
            vec![
                expected.display_name().to_string(),
                actual.display_name().to_string(),
            ],
            span,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::SemanticAnalyzer;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::settings::{CompilerSettings, UiLanguage};
    use crate::source::SourceFile;

    /// 执行一次完整语义分析，用于校验成功或失败场景。
    fn analyze(text: &str) -> Result<(), crate::diagnostic::CompilerError> {
        let settings = CompilerSettings::new(UiLanguage::ZhCn);
        let source = SourceFile::new("<test>", text.to_string());
        let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
        let program = Parser::new(tokens, &settings).parse_program().unwrap();
        SemanticAnalyzer::new(&settings).analyze(&program)
    }

    /// 校验重复声明变量会被语义分析拦截。
    #[test]
    fn should_reject_duplicate_variable() {
        let error = analyze("var x = 1\nvar x = 2\n").unwrap_err();
        assert_eq!(
            error.message_id,
            crate::diagnostic::MessageId::SemanticDuplicateVariable
        );
    }

    /// 校验访问未声明变量会返回明确的语义错误。
    #[test]
    fn should_reject_undeclared_variable() {
        let error = analyze("print(name)\n").unwrap_err();
        assert_eq!(
            error.message_id,
            crate::diagnostic::MessageId::SemanticUnknownVariable
        );
    }

    /// 校验显式类型与初始化表达式不匹配时会被阻止。
    #[test]
    fn should_reject_type_mismatch() {
        let error = analyze("var age:string = 18\n").unwrap_err();
        assert_eq!(
            error.message_id,
            crate::diagnostic::MessageId::SemanticTypeMismatch
        );
    }

    /// 校验内建打印函数必须只接收一个参数。
    #[test]
    fn should_reject_invalid_print_arity() {
        let error = analyze("print(1, 2)\n").unwrap_err();
        assert_eq!(
            error.message_id,
            crate::diagnostic::MessageId::SemanticPrintArity
        );
    }
}
