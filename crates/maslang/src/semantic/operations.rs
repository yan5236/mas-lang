use crate::ast::{AssignOp, BinaryOp, Expr, UnaryOp};
use crate::diagnostic::CompilerError;
use crate::source::Span;

use super::{SemanticAnalyzer, ValueType};

impl<'a> SemanticAnalyzer<'a> {
    /// 校验赋值语句或复合赋值语句的结果类型是否合法。
    pub(super) fn infer_assignment_type(
        &self,
        left: &ValueType,
        right: &ValueType,
        op: AssignOp,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match op {
            AssignOp::Assign => Ok(right.clone()),
            AssignOp::AddAssign => self.infer_binary_value_type(BinaryOp::Add, left, right, span),
            AssignOp::SubtractAssign => {
                self.infer_binary_value_type(BinaryOp::Subtract, left, right, span)
            }
            AssignOp::MultiplyAssign => {
                self.infer_binary_value_type(BinaryOp::Multiply, left, right, span)
            }
            AssignOp::DivideAssign => {
                self.infer_binary_value_type(BinaryOp::Divide, left, right, span)
            }
            AssignOp::ModAssign => {
                self.infer_binary_value_type(BinaryOp::Modulo, left, right, span)
            }
        }
    }

    /// 校验二元表达式两侧类型是否允许参与当前运算。
    pub(super) fn infer_binary_type(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let left_type = self.infer_expression_type(left)?;
        let right_type = self.infer_expression_type(right)?;
        self.infer_binary_value_type(op, &left_type, &right_type, span)
    }

    /// 在已知左右值类型的前提下，推导二元表达式的结果类型。
    fn infer_binary_value_type(
        &self,
        op: BinaryOp,
        left: &ValueType,
        right: &ValueType,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual)
            && (matches!(left, ValueType::Nullable(_) | ValueType::Null)
                && *right == ValueType::Null
                || matches!(right, ValueType::Nullable(_) | ValueType::Null)
                    && *left == ValueType::Null)
        {
            return Ok(ValueType::Bool);
        }
        match op {
            BinaryOp::And | BinaryOp::Or => {
                if left == &ValueType::Bool && right == &ValueType::Bool {
                    Ok(ValueType::Bool)
                } else {
                    Err(CompilerError::semantic_message(
                        "逻辑运算只支持布尔值",
                        span,
                    ))
                }
            }
            BinaryOp::Add => match (left, right) {
                (ValueType::Int, ValueType::Int) => Ok(ValueType::Int),
                (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                (ValueType::String, ValueType::String) => Ok(ValueType::String),
                _ => Err(CompilerError::semantic_message(
                    "当前类型不支持加法运算",
                    span,
                )),
            },
            BinaryOp::Subtract | BinaryOp::Multiply => match (left, right) {
                (ValueType::Int, ValueType::Int) => Ok(ValueType::Int),
                (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                _ => Err(CompilerError::semantic_message(
                    "当前类型不支持数值运算",
                    span,
                )),
            },
            BinaryOp::Divide => match (left, right) {
                (ValueType::Int, ValueType::Int)
                | (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                _ => Err(CompilerError::semantic_message(
                    "当前类型不支持除法运算",
                    span,
                )),
            },
            BinaryOp::Modulo | BinaryOp::IntDivide => {
                if left == &ValueType::Int && right == &ValueType::Int {
                    Ok(ValueType::Int)
                } else {
                    Err(CompilerError::semantic_message(
                        "取模和整数除法只支持整数",
                        span,
                    ))
                }
            }
            BinaryOp::Equal | BinaryOp::NotEqual => {
                if left == right
                    && left != &ValueType::Unit
                    && !matches!(
                        left,
                        ValueType::Struct(_)
                            | ValueType::Nullable(_)
                            | ValueType::Null
                            | ValueType::SafeConvertResult(_)
                            | ValueType::List(_)
                            | ValueType::Map(_, _)
                            | ValueType::Array(_, _)
                    )
                {
                    Ok(ValueType::Bool)
                } else {
                    Err(CompilerError::semantic_message(
                        "相等比较要求两侧类型一致且不能为空类型、列表类型、字典类型或数组类型",
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
                    _ => Err(CompilerError::semantic_message(
                        "大小比较只支持数值类型",
                        span,
                    )),
                }
            }
        }
    }

    /// 校验一元表达式的操作数类型是否合法，并返回结果类型。
    pub(super) fn infer_unary_type(
        &mut self,
        op: UnaryOp,
        operand: &Expr,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let operand_type = self.infer_expression_type(operand)?;

        match (op, operand_type) {
            (UnaryOp::Negate, ValueType::Int) => Ok(ValueType::Int),
            (UnaryOp::Negate, ValueType::Float) => Ok(ValueType::Float),
            (UnaryOp::Not, ValueType::Bool) => Ok(ValueType::Bool),
            _ => Err(CompilerError::semantic_message(
                "当前一元表达式的操作数类型不支持该运算",
                span,
            )),
        }
    }

    /// 校验具名调用并推导返回类型；未解析为模块函数的点调用报告不支持的方法。
    pub(super) fn infer_call_type(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        if let Expr::FieldAccess { name, span, .. } = callee {
            return Err(CompilerError::semantic_message(
                format!("当前阶段不支持对象方法 `{name}`"),
                *span,
            ));
        }
        let Expr::Identifier { name, .. } = callee else {
            return Err(CompilerError::semantic_message(
                "当前阶段只支持调用具名函数",
                span,
            ));
        };

        if self.structs.contains_key(name) {
            return self.infer_constructor(name, arguments, span);
        }
        if name == "print" {
            if self.current_return_type.is_none() {
                return Err(CompilerError::semantic_message(
                    "`print` 只能在函数体内调用，请将输出放入 `main` 或其他函数",
                    span,
                ));
            }
            if arguments.len() != 1 {
                return Err(CompilerError::semantic_message(
                    "内建函数 `print` 需要且只接受一个参数",
                    span,
                ));
            }
            let argument_type = self.infer_expression_type(&arguments[0])?;
            if matches!(
                argument_type,
                ValueType::Unit
                    | ValueType::Struct(_)
                    | ValueType::Nullable(_)
                    | ValueType::Null
                    | ValueType::SafeConvertResult(_)
                    | ValueType::List(_)
                    | ValueType::Map(_, _)
                    | ValueType::Array(_, _)
            ) {
                return Err(CompilerError::semantic_message(
                    "当前阶段 `print` 只支持打印基础类型",
                    arguments[0].span(),
                ));
            }
            return Ok(ValueType::Unit);
        }

        if name == "len" {
            if arguments.len() != 1 {
                return Err(CompilerError::semantic_message(
                    "内建函数 `len` 需要且只接受一个参数",
                    span,
                ));
            }
            let argument_type = self.infer_expression_type(&arguments[0])?;
            if !matches!(
                argument_type,
                ValueType::String
                    | ValueType::Array(_, _)
                    | ValueType::List(_)
                    | ValueType::Map(_, _)
            ) {
                return Err(CompilerError::semantic_message(
                    "内建函数 `len` 只支持字符串、列表、字典或数组类型",
                    arguments[0].span(),
                ));
            }
            return Ok(ValueType::Int);
        }

        if let Some(value_type) = self.infer_builtin_convert_type(name, arguments, span)? {
            return Ok(value_type);
        }

        let signature = self.functions.get(name).cloned().ok_or_else(|| {
            CompilerError::semantic_message(format!("未声明函数 `{name}`"), callee.span())
        })?;

        if signature.parameter_types.len() != arguments.len() {
            return Err(CompilerError::semantic_message(
                format!(
                    "函数 `{name}` 期望 {} 个参数，但实际传入 {} 个",
                    signature.parameter_types.len(),
                    arguments.len()
                ),
                span,
            ));
        }

        for (index, (expected, argument)) in signature
            .parameter_types
            .iter()
            .zip(arguments.iter())
            .enumerate()
        {
            let actual = self.infer_expression_type_with_hint(argument, Some(expected))?;
            if *expected != actual {
                return Err(CompilerError::semantic_message(
                    format!("函数 `{name}` 的第 {} 个参数类型不匹配", index + 1),
                    argument.span(),
                ));
            }
        }

        Ok(signature.return_type)
    }

    /// 推导基础类型转换内建函数的返回类型，并阻止容器或空值参与当前阶段的转换。
    fn infer_builtin_convert_type(
        &mut self,
        name: &str,
        arguments: &[Expr],
        span: Span,
    ) -> Result<Option<ValueType>, CompilerError> {
        let target_type = match name {
            "int" => ValueType::Int,
            "float" => ValueType::Float,
            "string" => ValueType::String,
            "bool" => ValueType::Bool,
            "try_int" => ValueType::SafeConvertResult(Box::new(ValueType::Int)),
            "try_float" => ValueType::SafeConvertResult(Box::new(ValueType::Float)),
            "try_bool" => ValueType::SafeConvertResult(Box::new(ValueType::Bool)),
            _ => return Ok(None),
        };

        if arguments.len() != 1 {
            return Err(CompilerError::semantic_message(
                format!("内建函数 `{name}` 需要且只接受一个参数"),
                span,
            ));
        }

        let argument_type = self.infer_expression_type(&arguments[0])?;
        if matches!(
            argument_type,
            ValueType::Int | ValueType::Float | ValueType::Bool | ValueType::String
        ) {
            Ok(Some(target_type))
        } else {
            Err(CompilerError::semantic_message(
                format!("当前阶段 `{name}` 只支持基础类型转换"),
                arguments[0].span(),
            ))
        }
    }
}
