use inkwell::values::{BasicMetadataValueEnum, FloatValue, FunctionValue};

use super::*;
use crate::ast::{Expr, Literal, UnaryOp};

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译表达式，并把 LLVM 值与静态类型一起返回给调用方。
    pub(super) fn compile_expr(
        &mut self,
        expression: &Expr,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        match expression {
            Expr::Literal { value, .. } => Ok(CodegenValue::Value(self.compile_literal(value)?)),
            Expr::Identifier { name, span } => self
                .compile_identifier(name, *span)
                .map(CodegenValue::Value),
            Expr::Grouped { expression, .. } => self.compile_expr(expression),
            Expr::Unary { op, operand, span } => self.compile_unary(*op, operand, *span),
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => self.compile_binary(*op, left, right, *span),
            Expr::Call {
                callee,
                arguments,
                span,
            } => self.compile_call(callee, arguments, *span),
        }
    }

    /// 把字面量转换为 LLVM 常量值。
    pub(super) fn compile_literal(
        &mut self,
        literal: &Literal,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        Ok(match literal {
            Literal::Int(value) => TypedValue {
                value_type: ValueType::Int,
                value: self.context.i64_type().const_int(*value as u64, true).into(),
            },
            Literal::Float(value) => TypedValue {
                value_type: ValueType::Float,
                value: self.context.f64_type().const_float(*value).into(),
            },
            Literal::Bool(value) => TypedValue {
                value_type: ValueType::Bool,
                value: self
                    .context
                    .bool_type()
                    .const_int(u64::from(*value), false)
                    .into(),
            },
            Literal::String(value) => TypedValue {
                value_type: ValueType::String,
                value: self.build_string_literal(value)?.into(),
            },
        })
    }

    /// 从变量指针中加载一个已声明变量的当前值。
    pub(super) fn compile_identifier(
        &self,
        name: &str,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let variable = self.lookup_variable(name).ok_or_else(|| {
            CompilerError::build_message(format!("代码生成阶段找不到变量 `{name}`"), span)
        })?;
        self.load_variable(variable)
    }

    /// 编译一元表达式，并生成对应的 LLVM 指令。
    pub(super) fn compile_unary(
        &mut self,
        op: UnaryOp,
        operand: &Expr,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let compiled = self.compile_expr(operand)?;
        let value = self.expect_value(compiled)?;
        let result = match (op, value.value_type) {
            (UnaryOp::Negate, ValueType::Int) => TypedValue {
                value_type: ValueType::Int,
                value: self
                    .map_builder(self.builder.build_int_neg(value.value.into_int_value(), "neg.int"))?
                    .into(),
            },
            (UnaryOp::Negate, ValueType::Float) => TypedValue {
                value_type: ValueType::Float,
                value: self
                    .map_builder(self.builder.build_float_neg(value.value.into_float_value(), "neg.float"))?
                    .into(),
            },
            (UnaryOp::Not, ValueType::Bool) => TypedValue {
                value_type: ValueType::Bool,
                value: self
                    .map_builder(self.builder.build_not(value.value.into_int_value(), "not.bool"))?
                    .into(),
            },
            _ => {
                return Err(CompilerError::build_message(
                    "当前一元表达式无法生成目标代码",
                    span,
                ))
            }
        };
        Ok(CodegenValue::Value(result))
    }

    /// 编译内建函数与用户函数调用。
    pub(super) fn compile_call(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let Expr::Identifier { name, .. } = callee else {
            return Err(CompilerError::build_message("当前阶段只支持具名函数调用", span));
        };

        if name == "print" {
            let compiled = self.compile_expr(&arguments[0])?;
            let argument = self.expect_value(compiled)?;
            let (function, args): (FunctionValue<'ctx>, Vec<BasicMetadataValueEnum<'ctx>>) =
                match argument.value_type {
                    ValueType::Int => (self.runtime.print_int, vec![argument.value.into()]),
                    ValueType::Float => (self.runtime.print_float, vec![argument.value.into()]),
                    ValueType::Bool => {
                        let bool_value = self.map_builder(self.builder.build_int_z_extend(
                            argument.value.into_int_value(),
                            self.context.i8_type(),
                            "bool.cast",
                        ))?;
                        (self.runtime.print_bool, vec![bool_value.into()])
                    }
                    ValueType::String => {
                        let string_value = argument.value.into_struct_value();
                        let ptr = self.extract_string_part(string_value, 0, "print.ptr")?;
                        let len = self.extract_string_part(string_value, 1, "print.len")?;
                        (self.runtime.print_string, vec![ptr.into(), len.into()])
                    }
                    ValueType::Unit => {
                        return Err(CompilerError::build_message("print 不能打印 unit 值", span));
                    }
                };

            self.map_builder(self.builder.build_call(function, &args, "call.print"))?;
            return Ok(CodegenValue::Unit);
        }

        let info = self.functions.get(name).cloned().ok_or_else(|| {
            CompilerError::build_message(format!("未声明函数 `{name}`"), callee.span())
        })?;
        let mut args = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let compiled = self.compile_expr(argument)?;
            args.push(self.expect_value(compiled)?.value.into());
        }
        let call = self.map_builder(self.builder.build_call(info.function, &args, "call.user"))?;
        if info.return_type == ValueType::Unit {
            Ok(CodegenValue::Unit)
        } else {
            let value = call.try_as_basic_value().unwrap_basic();
            Ok(CodegenValue::Value(TypedValue {
                value_type: info.return_type,
                value,
            }))
        }
    }

    /// 把两个数值表达式统一提升为浮点值。
    pub(super) fn coerce_to_float_pair(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        span: Span,
    ) -> Result<(FloatValue<'ctx>, FloatValue<'ctx>), CompilerError> {
        Ok((self.to_float(left, span)?, self.to_float(right, span)?))
    }

    /// 把单个整数或浮点值转换为浮点值。
    pub(super) fn to_float(
        &mut self,
        value: TypedValue<'ctx>,
        span: Span,
    ) -> Result<FloatValue<'ctx>, CompilerError> {
        match value.value_type {
            ValueType::Float => Ok(value.value.into_float_value()),
            ValueType::Int => Ok(self.map_builder(self.builder.build_signed_int_to_float(
                value.value.into_int_value(),
                self.context.f64_type(),
                "int.to.float",
            ))?),
            _ => Err(CompilerError::build_message("当前值无法提升为浮点数", span)),
        }
    }
}
