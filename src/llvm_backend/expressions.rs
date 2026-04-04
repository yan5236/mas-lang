use inkwell::values::{BasicMetadataValueEnum, FloatValue, FunctionValue, PointerValue};

use super::*;
use crate::ast::{Expr, Literal, MethodName, PropertyName, TypeName, UnaryOp};

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
            Expr::ListLiteral { elements, span } => self.compile_list_literal(elements, *span),
            Expr::MapLiteral { entries, span } => self.compile_map_literal(entries, *span),
            Expr::ArrayInitializer {
                element_type,
                size,
                elements,
                span,
            } => self.compile_array_initializer(element_type, *size, elements, *span),
            Expr::Index {
                target,
                index,
                span,
            } => self.compile_index(target, index, *span),
            Expr::Slice {
                target,
                start,
                end,
                span,
            } => self.compile_slice(target, start.as_deref(), end.as_deref(), *span),
            Expr::MethodCall {
                target,
                method,
                arguments,
                span,
            } => self.compile_method_call(target, *method, arguments, *span),
            Expr::PropertyAccess {
                target,
                property,
                span,
            } => self.compile_property_access(target, *property, *span),
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
                value: self
                    .context
                    .i64_type()
                    .const_int(*value as u64, true)
                    .into(),
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
        let result = match (op, value.value_type.clone()) {
            (UnaryOp::Negate, ValueType::Int) => TypedValue {
                value_type: ValueType::Int,
                value: self
                    .map_builder(
                        self.builder
                            .build_int_neg(value.value.into_int_value(), "neg.int"),
                    )?
                    .into(),
            },
            (UnaryOp::Negate, ValueType::Float) => TypedValue {
                value_type: ValueType::Float,
                value: self
                    .map_builder(
                        self.builder
                            .build_float_neg(value.value.into_float_value(), "neg.float"),
                    )?
                    .into(),
            },
            (UnaryOp::Not, ValueType::Bool) => TypedValue {
                value_type: ValueType::Bool,
                value: self
                    .map_builder(
                        self.builder
                            .build_not(value.value.into_int_value(), "not.bool"),
                    )?
                    .into(),
            },
            _ => {
                return Err(CompilerError::build_message(
                    "当前一元表达式无法生成目标代码",
                    span,
                ));
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
            return Err(CompilerError::build_message(
                "当前阶段只支持具名函数调用",
                span,
            ));
        };

        if name == "print" {
            let compiled = self.compile_expr(&arguments[0])?;
            let argument = self.expect_value(compiled)?;
            let (function, args): (FunctionValue<'ctx>, Vec<BasicMetadataValueEnum<'ctx>>) =
                match argument.value_type.clone() {
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
                    ValueType::Unit
                    | ValueType::SafeConvertResult(_)
                    | ValueType::List(_)
                    | ValueType::Map(_, _)
                    | ValueType::Array(_, _) => {
                        return Err(CompilerError::build_message(
                            "print 不能打印当前值类型",
                            span,
                        ));
                    }
                };

            self.map_builder(self.builder.build_call(function, &args, "call.print"))?;
            return Ok(CodegenValue::Unit);
        }

        if name == "len" {
            let compiled = self.compile_expr(&arguments[0])?;
            let argument = self.expect_value(compiled)?;
            let (function, args, call_name): (
                FunctionValue<'ctx>,
                Vec<BasicMetadataValueEnum<'ctx>>,
                &str,
            ) = match argument.value_type {
                ValueType::List(_) => {
                    let (list_value, _) = self.expect_list_value(argument, span)?;
                    (
                        self.runtime.list_len,
                        vec![list_value.into()],
                        "call.list_len",
                    )
                }
                ValueType::Map(_, _) => {
                    let (map_value, _, _) = self.expect_map_value(argument, span)?;
                    (self.runtime.map_len, vec![map_value.into()], "call.map_len")
                }
                _ => {
                    return Err(CompilerError::build_message(
                        "len 只支持列表或字典类型",
                        span,
                    ));
                }
            };
            let call = self.map_builder(self.builder.build_call(function, &args, call_name))?;
            return Ok(CodegenValue::Value(TypedValue {
                value_type: ValueType::Int,
                value: call.try_as_basic_value().unwrap_basic(),
            }));
        }

        if let Some(value) = self.compile_builtin_convert_call(name, arguments, span)? {
            return Ok(CodegenValue::Value(value));
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

    /// 编译安全转换结果上的 `ok/value` 属性访问，并从聚合结构中提取目标字段。
    fn compile_property_access(
        &mut self,
        target: &Expr,
        property: PropertyName,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let compiled = self.compile_expr(target)?;
        let target_value = self.expect_value(compiled)?;
        let ValueType::SafeConvertResult(value_type) = &target_value.value_type else {
            return Err(CompilerError::build_message(
                "当前表达式不支持属性访问",
                span,
            ));
        };

        let aggregate = target_value.value.into_struct_value();
        let extracted = match property {
            PropertyName::Ok => TypedValue {
                value_type: ValueType::Bool,
                value: self
                    .extract_string_part(aggregate, 0, "safe_convert.ok")?
                    .into_int_value()
                    .into(),
            },
            PropertyName::Value => TypedValue {
                value_type: value_type.as_ref().clone(),
                value: self.extract_string_part(aggregate, 1, "safe_convert.value")?,
            },
        };
        Ok(CodegenValue::Value(extracted))
    }

    /// 编译基础类型转换内建函数，并按源类型选择直接 LLVM 指令或运行时 ABI。
    fn compile_builtin_convert_call(
        &mut self,
        name: &str,
        arguments: &[Expr],
        span: Span,
    ) -> Result<Option<TypedValue<'ctx>>, CompilerError> {
        let converted = match name {
            "int" => Some(self.compile_int_conversion(arguments, span)?),
            "float" => Some(self.compile_float_conversion(arguments, span)?),
            "string" => Some(self.compile_string_conversion(arguments, span)?),
            "bool" => Some(self.compile_bool_conversion(arguments, span)?),
            "try_int" => Some(self.compile_try_int_conversion(arguments, span)?),
            "try_float" => Some(self.compile_try_float_conversion(arguments, span)?),
            "try_bool" => Some(self.compile_try_bool_conversion(arguments, span)?),
            _ => None,
        };
        Ok(converted)
    }

    /// 把基础类型值转换为 `int`，其中字符串分支通过运行时解析十进制文本。
    fn compile_int_conversion(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let argument = self.compile_scalar_convert_argument("int", arguments, span)?;
        match argument.value_type {
            ValueType::Int => Ok(argument),
            ValueType::Float => Ok(TypedValue {
                value_type: ValueType::Int,
                value: self
                    .map_builder(self.builder.build_float_to_signed_int(
                        argument.value.into_float_value(),
                        self.context.i64_type(),
                        "float.to.int",
                    ))?
                    .into(),
            }),
            ValueType::Bool => Ok(TypedValue {
                value_type: ValueType::Int,
                value: self
                    .map_builder(self.builder.build_int_z_extend(
                        argument.value.into_int_value(),
                        self.context.i64_type(),
                        "bool.to.int",
                    ))?
                    .into(),
            }),
            ValueType::String => {
                let string_value = argument.value.into_struct_value();
                let call = self.map_builder(
                    self.builder.build_call(
                        self.runtime.string_to_int,
                        &[
                            self.extract_string_part(string_value, 0, "int.arg.ptr")?
                                .into(),
                            self.extract_string_part(string_value, 1, "int.arg.len")?
                                .into(),
                        ],
                        "call.string_to_int",
                    ),
                )?;
                Ok(TypedValue {
                    value_type: ValueType::Int,
                    value: call.try_as_basic_value().unwrap_basic(),
                })
            }
            _ => Err(CompilerError::build_message(
                "当前阶段 `int` 只支持基础类型转换",
                span,
            )),
        }
    }

    /// 把基础类型值转换为 `float`，其中字符串分支通过运行时解析浮点文本。
    fn compile_float_conversion(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let argument = self.compile_scalar_convert_argument("float", arguments, span)?;
        match argument.value_type {
            ValueType::Float => Ok(argument),
            ValueType::Int => Ok(TypedValue {
                value_type: ValueType::Float,
                value: self
                    .map_builder(self.builder.build_signed_int_to_float(
                        argument.value.into_int_value(),
                        self.context.f64_type(),
                        "int.to.float",
                    ))?
                    .into(),
            }),
            ValueType::Bool => {
                let integer = self.map_builder(self.builder.build_int_z_extend(
                    argument.value.into_int_value(),
                    self.context.i64_type(),
                    "bool.to.int",
                ))?;
                Ok(TypedValue {
                    value_type: ValueType::Float,
                    value: self
                        .map_builder(self.builder.build_signed_int_to_float(
                            integer,
                            self.context.f64_type(),
                            "bool.to.float",
                        ))?
                        .into(),
                })
            }
            ValueType::String => {
                let string_value = argument.value.into_struct_value();
                let call = self.map_builder(
                    self.builder.build_call(
                        self.runtime.string_to_float,
                        &[
                            self.extract_string_part(string_value, 0, "float.arg.ptr")?
                                .into(),
                            self.extract_string_part(string_value, 1, "float.arg.len")?
                                .into(),
                        ],
                        "call.string_to_float",
                    ),
                )?;
                Ok(TypedValue {
                    value_type: ValueType::Float,
                    value: call.try_as_basic_value().unwrap_basic(),
                })
            }
            _ => Err(CompilerError::build_message(
                "当前阶段 `float` 只支持基础类型转换",
                span,
            )),
        }
    }

    /// 把基础类型值转换为 `string`，数值和布尔值通过运行时格式化为 UTF-8 文本。
    fn compile_string_conversion(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let argument = self.compile_scalar_convert_argument("string", arguments, span)?;
        match argument.value_type {
            ValueType::String => Ok(argument),
            ValueType::Int => self.build_scalar_string_call(
                self.runtime.int_to_string,
                &[argument.value.into()],
                "call.int_to_string",
                "int.string.out_len",
                "load.int.string.len",
            ),
            ValueType::Float => self.build_scalar_string_call(
                self.runtime.float_to_string,
                &[argument.value.into()],
                "call.float_to_string",
                "float.string.out_len",
                "load.float.string.len",
            ),
            ValueType::Bool => {
                let bool_value = self.map_builder(self.builder.build_int_z_extend(
                    argument.value.into_int_value(),
                    self.context.i8_type(),
                    "bool.to.i8",
                ))?;
                self.build_scalar_string_call(
                    self.runtime.bool_to_string,
                    &[bool_value.into()],
                    "call.bool_to_string",
                    "bool.string.out_len",
                    "load.bool.string.len",
                )
            }
            _ => Err(CompilerError::build_message(
                "当前阶段 `string` 只支持基础类型转换",
                span,
            )),
        }
    }

    /// 把基础类型值转换为 `bool`，采用零值/空字符串判假、其余判真的规则。
    fn compile_bool_conversion(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let argument = self.compile_scalar_convert_argument("bool", arguments, span)?;
        let value = match argument.value_type {
            ValueType::Bool => argument.value.into_int_value(),
            ValueType::Int => self.map_builder(self.builder.build_int_compare(
                inkwell::IntPredicate::NE,
                argument.value.into_int_value(),
                self.context.i64_type().const_zero(),
                "int.to.bool",
            ))?,
            ValueType::Float => self.map_builder(self.builder.build_float_compare(
                inkwell::FloatPredicate::UNE,
                argument.value.into_float_value(),
                self.context.f64_type().const_zero(),
                "float.to.bool",
            ))?,
            ValueType::String => {
                let string_value = argument.value.into_struct_value();
                let length = self
                    .extract_string_part(string_value, 1, "bool.arg.len")?
                    .into_int_value();
                self.map_builder(self.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    length,
                    self.context.i64_type().const_zero(),
                    "string.to.bool",
                ))?
            }
            _ => {
                return Err(CompilerError::build_message(
                    "当前阶段 `bool` 只支持基础类型转换",
                    span,
                ));
            }
        };
        Ok(TypedValue {
            value_type: ValueType::Bool,
            value: value.into(),
        })
    }

    /// 编译 `try_int`，字符串分支返回失败标记而不是直接触发运行时错误。
    fn compile_try_int_conversion(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let argument = self.compile_scalar_convert_argument("try_int", arguments, span)?;
        match argument.value_type {
            ValueType::Int => self.build_safe_convert_success(ValueType::Int, argument.value),
            ValueType::Float => self.build_safe_convert_success(
                ValueType::Int,
                self.map_builder(self.builder.build_float_to_signed_int(
                    argument.value.into_float_value(),
                    self.context.i64_type(),
                    "try.float.to.int",
                ))?
                .into(),
            ),
            ValueType::Bool => self.build_safe_convert_success(
                ValueType::Int,
                self.map_builder(self.builder.build_int_z_extend(
                    argument.value.into_int_value(),
                    self.context.i64_type(),
                    "try.bool.to.int",
                ))?
                .into(),
            ),
            ValueType::String => self.build_try_string_int_result(argument.value.into_struct_value()),
            _ => Err(CompilerError::build_message(
                "当前阶段 `try_int` 只支持基础类型转换",
                span,
            )),
        }
    }

    /// 编译 `try_float`，字符串分支返回失败标记而不是直接触发运行时错误。
    fn compile_try_float_conversion(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let argument = self.compile_scalar_convert_argument("try_float", arguments, span)?;
        match argument.value_type {
            ValueType::Float => self.build_safe_convert_success(ValueType::Float, argument.value),
            ValueType::Int => self.build_safe_convert_success(
                ValueType::Float,
                self.map_builder(self.builder.build_signed_int_to_float(
                    argument.value.into_int_value(),
                    self.context.f64_type(),
                    "try.int.to.float",
                ))?
                .into(),
            ),
            ValueType::Bool => {
                let integer = self.map_builder(self.builder.build_int_z_extend(
                    argument.value.into_int_value(),
                    self.context.i64_type(),
                    "try.bool.to.int",
                ))?;
                self.build_safe_convert_success(
                    ValueType::Float,
                    self.map_builder(self.builder.build_signed_int_to_float(
                        integer,
                        self.context.f64_type(),
                        "try.bool.to.float",
                    ))?
                    .into(),
                )
            }
            ValueType::String => {
                self.build_try_string_float_result(argument.value.into_struct_value())
            }
            _ => Err(CompilerError::build_message(
                "当前阶段 `try_float` 只支持基础类型转换",
                span,
            )),
        }
    }

    /// 编译 `try_bool`，字符串分支仅接受精确的 `true/false` 文本。
    fn compile_try_bool_conversion(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let argument = self.compile_scalar_convert_argument("try_bool", arguments, span)?;
        match argument.value_type {
            ValueType::Bool => self.build_safe_convert_success(ValueType::Bool, argument.value),
            ValueType::Int => self.build_safe_convert_success(
                ValueType::Bool,
                self.map_builder(self.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    argument.value.into_int_value(),
                    self.context.i64_type().const_zero(),
                    "try.int.to.bool",
                ))?
                .into(),
            ),
            ValueType::Float => self.build_safe_convert_success(
                ValueType::Bool,
                self.map_builder(self.builder.build_float_compare(
                    inkwell::FloatPredicate::UNE,
                    argument.value.into_float_value(),
                    self.context.f64_type().const_zero(),
                    "try.float.to.bool",
                ))?
                .into(),
            ),
            ValueType::String => self.build_try_string_bool_result(argument.value.into_struct_value()),
            _ => Err(CompilerError::build_message(
                "当前阶段 `try_bool` 只支持基础类型转换",
                span,
            )),
        }
    }

    /// 校验类型转换内建函数的参数数量，并把唯一参数编译为后续转换逻辑可复用的基础类型值。
    fn compile_scalar_convert_argument(
        &mut self,
        name: &str,
        arguments: &[Expr],
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if arguments.len() != 1 {
            return Err(CompilerError::build_message(
                format!("内建函数 `{name}` 需要且只接受一个参数"),
                span,
            ));
        }
        let compiled = self.compile_expr(&arguments[0])?;
        let argument = self.expect_value(compiled)?;
        if matches!(
            argument.value_type,
            ValueType::Int | ValueType::Float | ValueType::Bool | ValueType::String
        ) {
            Ok(argument)
        } else {
            Err(CompilerError::build_message(
                format!("当前阶段 `{name}` 只支持基础类型转换"),
                arguments[0].span(),
            ))
        }
    }

    /// 调用“标量转字符串”运行时函数，并把 `ptr + out_len` 结果重建为内部字符串结构。
    fn build_scalar_string_call(
        &mut self,
        function: FunctionValue<'ctx>,
        arguments: &[BasicMetadataValueEnum<'ctx>],
        call_name: &str,
        out_len_name: &str,
        load_name: &str,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let out_len_ptr = self.allocate_local(&ValueType::Int, out_len_name)?;
        self.map_builder(
            self.builder
                .build_store(out_len_ptr, self.context.i64_type().const_zero()),
        )?;

        let mut call_args = arguments.to_vec();
        call_args.push(out_len_ptr.into());
        let call = self.map_builder(self.builder.build_call(function, &call_args, call_name))?;
        let pointer = call
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();
        let length = self
            .map_builder(
                self.builder
                    .build_load(self.context.i64_type(), out_len_ptr, load_name),
            )?
            .into_int_value();
        Ok(TypedValue {
            value_type: ValueType::String,
            value: self.build_runtime_string(pointer, length)?.into(),
        })
    }

    /// 把一个“必定成功”的基础类型转换结果组装成 `{ ok, value }` 结构值。
    fn build_safe_convert_success(
        &self,
        value_type: ValueType,
        value: BasicValueEnum<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        self.build_safe_convert_result(
            value_type,
            self.context.bool_type().const_int(1, false),
            value,
        )
    }

    /// 把成功标记与目标值组装成安全转换结果，供 `try_*` 内建函数统一复用。
    fn build_safe_convert_result(
        &self,
        value_type: ValueType,
        ok: inkwell::values::IntValue<'ctx>,
        value: BasicValueEnum<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let result_type = self.safe_convert_result_type(&value_type)?;
        let with_ok = self
            .map_builder(self.builder.build_insert_value(result_type.get_undef(), ok, 0, "safe.ok"))?
            .into_struct_value();
        let with_value = self
            .map_builder(self.builder.build_insert_value(with_ok, value, 1, "safe.value"))?
            .into_struct_value();
        Ok(TypedValue {
            value_type: ValueType::SafeConvertResult(Box::new(value_type)),
            value: with_value.into(),
        })
    }

    /// 调用运行时的安全整数解析函数，并返回带成功标记的结果结构。
    fn build_try_string_int_result(
        &mut self,
        value: inkwell::values::StructValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let out_ptr = self.allocate_local(&ValueType::Int, "try.int.out")?;
        self.map_builder(
            self.builder
                .build_store(out_ptr, self.context.i64_type().const_zero()),
        )?;
        let ok = self.build_try_string_parse_ok(
            self.runtime.try_string_to_int,
            value,
            out_ptr,
            "call.try_string_to_int",
        )?;
        let parsed = self
            .map_builder(self.builder.build_load(self.context.i64_type(), out_ptr, "load.try.int"))?;
        self.build_safe_convert_result(ValueType::Int, ok, parsed)
    }

    /// 调用运行时的安全浮点解析函数，并返回带成功标记的结果结构。
    fn build_try_string_float_result(
        &mut self,
        value: inkwell::values::StructValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let out_ptr = self.allocate_local(&ValueType::Float, "try.float.out")?;
        self.map_builder(
            self.builder
                .build_store(out_ptr, self.context.f64_type().const_zero()),
        )?;
        let ok = self.build_try_string_parse_ok(
            self.runtime.try_string_to_float,
            value,
            out_ptr,
            "call.try_string_to_float",
        )?;
        let parsed = self
            .map_builder(self.builder.build_load(self.context.f64_type(), out_ptr, "load.try.float"))?;
        self.build_safe_convert_result(ValueType::Float, ok, parsed)
    }

    /// 调用运行时的安全布尔解析函数，并把 `u8` 结果规范化成内部 `i1` 布尔值。
    fn build_try_string_bool_result(
        &mut self,
        value: inkwell::values::StructValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let out_ptr = self.map_builder(
            self.builder
                .build_alloca(self.context.i8_type(), "try.bool.out.raw"),
        )?;
        self.map_builder(
            self.builder
                .build_store(out_ptr, self.context.i8_type().const_zero()),
        )?;
        let ok = self.build_try_string_parse_ok(
            self.runtime.try_string_to_bool,
            value,
            out_ptr,
            "call.try_string_to_bool",
        )?;
        let raw = self
            .map_builder(self.builder.build_load(self.context.i8_type(), out_ptr, "load.try.bool.raw"))?
            .into_int_value();
        let parsed = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            raw,
            self.context.i8_type().const_zero(),
            "try.bool.value",
        ))?;
        self.build_safe_convert_result(ValueType::Bool, ok, parsed.into())
    }

    /// 统一构造安全字符串解析调用，并把运行时返回的 `u8` 成功标记转换为内部 `i1`。
    fn build_try_string_parse_ok(
        &mut self,
        function: FunctionValue<'ctx>,
        value: inkwell::values::StructValue<'ctx>,
        out_ptr: PointerValue<'ctx>,
        call_name: &str,
    ) -> Result<inkwell::values::IntValue<'ctx>, CompilerError> {
        let call = self.map_builder(self.builder.build_call(
            function,
            &[
                self.extract_string_part(value, 0, "try.string.ptr")?.into(),
                self.extract_string_part(value, 1, "try.string.len")?.into(),
                out_ptr.into(),
            ],
            call_name,
        ))?;
        let ok_raw = call.try_as_basic_value().unwrap_basic().into_int_value();
        self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            ok_raw,
            self.context.i8_type().const_zero(),
            "try.parse.ok",
        ))
    }

    /// 编译列表字面量，并逐项调用运行时追加元素。
    pub(super) fn compile_list_literal(
        &mut self,
        elements: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let element_type = if let Some(first) = elements.first() {
            let compiled_first = self.compile_expr(first)?;
            let first_value = self.expect_value(compiled_first)?;
            self.expect_list_element_type(first_value.value_type, span)?
        } else {
            return Err(CompilerError::build_message(
                "代码生成阶段无法直接编译缺少类型信息的空列表",
                span,
            ));
        };

        let list_value = self.build_empty_list(&element_type)?;
        for element in elements {
            let compiled = self.compile_expr(element)?;
            let value = self.expect_value(compiled)?;
            self.build_list_append(list_value, &element_type, value, span)?;
        }

        Ok(CodegenValue::Value(TypedValue {
            value_type: ValueType::List(Box::new(element_type)),
            value: list_value.into(),
        }))
    }

    /// 编译字典字面量，并逐项调用运行时写入键值对。
    pub(super) fn compile_map_literal(
        &mut self,
        entries: &[crate::ast::MapEntry],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let Some(first_entry) = entries.first() else {
            return Err(CompilerError::build_message(
                "代码生成阶段无法直接编译缺少类型信息的空字典",
                span,
            ));
        };

        let first_key_compiled = self.compile_expr(&first_entry.key)?;
        let first_key = self.expect_value(first_key_compiled)?;
        let first_value_compiled = self.compile_expr(&first_entry.value)?;
        let first_value = self.expect_value(first_value_compiled)?;
        let key_type =
            self.expect_list_element_type(first_key.value_type.clone(), first_entry.key.span())?;
        let value_type = self
            .expect_list_element_type(first_value.value_type.clone(), first_entry.value.span())?;
        let map_value = self.build_empty_map(&key_type, &value_type)?;
        self.build_map_set(
            map_value,
            &key_type,
            &value_type,
            first_key,
            first_value,
            span,
        )?;

        for entry in &entries[1..] {
            let key_compiled = self.compile_expr(&entry.key)?;
            let key = self.expect_value(key_compiled)?;
            let value_compiled = self.compile_expr(&entry.value)?;
            let value = self.expect_value(value_compiled)?;
            if key.value_type != key_type {
                return Err(CompilerError::build_message(
                    "字典字面量中的键类型必须保持一致",
                    entry.key.span(),
                ));
            }
            if value.value_type != value_type {
                return Err(CompilerError::build_message(
                    "字典字面量中的值类型必须保持一致",
                    entry.value.span(),
                ));
            }
            self.build_map_set(map_value, &key_type, &value_type, key, value, entry.span)?;
        }

        Ok(CodegenValue::Value(TypedValue {
            value_type: ValueType::Map(Box::new(key_type), Box::new(value_type)),
            value: map_value.into(),
        }))
    }

    /// 编译固定长度数组初始化表达式，并把结果组装为 LLVM 原生数组值。
    pub(super) fn compile_array_initializer(
        &mut self,
        element_type: &TypeName,
        size: usize,
        elements: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let element_value_type = value_type_from_annotation(element_type);
        if !matches!(
            element_value_type,
            ValueType::Int | ValueType::Float | ValueType::Bool | ValueType::String
        ) {
            return Err(CompilerError::build_message(
                "当前阶段数组元素只支持基础类型",
                span,
            ));
        }
        if elements.len() != size {
            return Err(CompilerError::build_message(
                format!(
                    "数组初始化需要 {size} 个元素，但实际提供了 {} 个",
                    elements.len()
                ),
                span,
            ));
        }

        let array_value_type = ValueType::Array(Box::new(element_value_type.clone()), size);
        let array_pointer = self.allocate_local(&array_value_type, "array.init")?;
        for (index, element) in elements.iter().enumerate() {
            let compiled = self.compile_expr(element)?;
            let value = self.expect_value(compiled)?;
            if value.value_type != element_value_type {
                return Err(CompilerError::build_message(
                    "数组初始化中的元素类型必须与声明类型一致",
                    element.span(),
                ));
            }
            self.build_array_store(
                array_pointer,
                &element_value_type,
                size,
                self.context.i64_type().const_int(index as u64, false),
                value.value,
            )?;
        }

        let array_value = self.map_builder(self.builder.build_load(
            self.array_type_for(&element_value_type, size)?,
            array_pointer,
            "load.array.init",
        ))?;
        Ok(CodegenValue::Value(TypedValue {
            value_type: array_value_type,
            value: array_value,
        }))
    }

    /// 编译列表、数组或字典索引访问。
    pub(super) fn compile_index(
        &mut self,
        target: &Expr,
        index: &Expr,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let compiled_target = self.compile_expr(target)?;
        let target_value = self.expect_value(compiled_target)?;
        match target_value.value_type.clone() {
            ValueType::List(element_type) => {
                let compiled_index = self.compile_expr(index)?;
                let index_value = self.expect_value(compiled_index)?;
                if index_value.value_type != ValueType::Int {
                    return Err(CompilerError::build_message("索引必须是整数", span));
                }
                let (list_value, _) = self.expect_list_value(target_value, span)?;
                Ok(CodegenValue::Value(self.build_list_get(
                    list_value,
                    &element_type,
                    index_value.value.into_int_value(),
                    span,
                )?))
            }
            ValueType::Array(element_type, size) => {
                let compiled_index = self.compile_expr(index)?;
                let index_value = self.expect_value(compiled_index)?;
                if index_value.value_type != ValueType::Int {
                    return Err(CompilerError::build_message("索引必须是整数", span));
                }
                let array_pointer = self.resolve_array_pointer(target, target_value)?;
                Ok(CodegenValue::Value(self.build_array_get(
                    array_pointer,
                    element_type.as_ref(),
                    size,
                    index_value.value.into_int_value(),
                )?))
            }
            ValueType::Map(key_type, value_type) => {
                let compiled_index = self.compile_expr(index)?;
                let index_value = self.expect_value(compiled_index)?;
                if index_value.value_type != *key_type {
                    return Err(CompilerError::build_message("字典下标的键类型不匹配", span));
                }
                let (map_value, _, _) = self.expect_map_value(target_value, span)?;
                Ok(CodegenValue::Value(self.build_map_get(
                    map_value,
                    key_type.as_ref(),
                    value_type.as_ref(),
                    index_value,
                    span,
                )?))
            }
            _ => Err(CompilerError::build_message(
                "索引访问当前只支持列表、数组或字典类型",
                span,
            )),
        }
    }

    /// 编译列表切片表达式。
    pub(super) fn compile_slice(
        &mut self,
        target: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let compiled_target = self.compile_expr(target)?;
        let target_value = self.expect_value(compiled_target)?;
        let (list_value, element_type) = self.expect_list_value(target_value, span)?;
        let start_value = if let Some(start) = start {
            let compiled_start = self.compile_expr(start)?;
            self.expect_value(compiled_start)?.value.into_int_value()
        } else {
            self.context.i64_type().const_zero()
        };
        let end_value = if let Some(end) = end {
            let compiled_end = self.compile_expr(end)?;
            self.expect_value(compiled_end)?.value.into_int_value()
        } else {
            let len_call = self.map_builder(self.builder.build_call(
                self.runtime.list_len,
                &[list_value.into()],
                "slice.len",
            ))?;
            len_call
                .try_as_basic_value()
                .unwrap_basic()
                .into_int_value()
        };
        let call = self.map_builder(self.builder.build_call(
            self.runtime.list_slice,
            &[list_value.into(), start_value.into(), end_value.into()],
            "call.list_slice",
        ))?;
        Ok(CodegenValue::Value(TypedValue {
            value_type: ValueType::List(Box::new(element_type)),
            value: call.try_as_basic_value().unwrap_basic(),
        }))
    }

    /// 编译对象方法调用，并根据目标类型分派到字符串、列表或字典实现。
    pub(super) fn compile_method_call(
        &mut self,
        target: &Expr,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let compiled_target = self.compile_expr(target)?;
        let target_value = self.expect_value(compiled_target)?;
        match target_value.value_type.clone() {
            ValueType::String => {
                self.compile_string_method_call(target_value, method, arguments, span)
            }
            ValueType::List(_) => {
                self.compile_list_method_call(target_value, method, arguments, span)
            }
            ValueType::Map(_, _) => {
                self.compile_map_method_call(target_value, method, arguments, span)
            }
            _ if method.supports_string_target() => Err(CompilerError::build_message(
                format!(
                    "字符串方法 `{}` 只能用于 `string` 类型",
                    method.display_name()
                ),
                span,
            )),
            _ => Err(CompilerError::build_message(
                "当前方法只能用于字符串、列表或字典类型",
                span,
            )),
        }
    }

    /// 编译字符串方法调用，并复用现有字符串 ABI 进行参数拆包和结果重建。
    pub(super) fn compile_string_method_call(
        &mut self,
        target_value: TypedValue<'ctx>,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let string_value = self.expect_string_value(target_value, span, "当前表达式不是字符串类型")?;
        let mut target_args = self.build_string_runtime_arguments(string_value)?;

        match method {
            MethodName::Trim => Ok(CodegenValue::Value(
                self.build_string_method_result_from_call(
                    self.runtime.string_trim,
                    &target_args,
                    "call.string_trim",
                )?,
            )),
            MethodName::Contains => {
                let argument = self.compile_string_method_argument(&arguments[0])?;
                target_args.extend(self.build_string_runtime_arguments(argument)?);
                Ok(CodegenValue::Value(self.build_bool_result_from_call(
                    self.runtime.string_contains,
                    &target_args,
                    "call.string_contains",
                    "string.contains.bool",
                )?))
            }
            MethodName::StartsWith => {
                let argument = self.compile_string_method_argument(&arguments[0])?;
                target_args.extend(self.build_string_runtime_arguments(argument)?);
                Ok(CodegenValue::Value(self.build_bool_result_from_call(
                    self.runtime.string_starts_with,
                    &target_args,
                    "call.string_starts_with",
                    "string.starts_with.bool",
                )?))
            }
            MethodName::EndsWith => {
                let argument = self.compile_string_method_argument(&arguments[0])?;
                target_args.extend(self.build_string_runtime_arguments(argument)?);
                Ok(CodegenValue::Value(self.build_bool_result_from_call(
                    self.runtime.string_ends_with,
                    &target_args,
                    "call.string_ends_with",
                    "string.ends_with.bool",
                )?))
            }
            MethodName::Replace => {
                let from_value = self.compile_string_method_argument(&arguments[0])?;
                let to_value = self.compile_string_method_argument(&arguments[1])?;
                target_args.extend(self.build_string_runtime_arguments(from_value)?);
                target_args.extend(self.build_string_runtime_arguments(to_value)?);
                Ok(CodegenValue::Value(
                    self.build_string_method_result_from_call(
                        self.runtime.string_replace,
                        &target_args,
                        "call.string_replace",
                    )?,
                ))
            }
            MethodName::Append
            | MethodName::Remove
            | MethodName::Pop
            | MethodName::Sort
            | MethodName::Reverse
            | MethodName::Index
            | MethodName::Keys
            | MethodName::Values => Err(CompilerError::build_message(
                format!("字符串类型不支持方法 `{}`", method.display_name()),
                span,
            )),
        }
    }

    /// 编译列表方法调用。
    pub(super) fn compile_list_method_call(
        &mut self,
        target_value: TypedValue<'ctx>,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let (list_value, element_type) = self.expect_list_value(target_value, span)?;

        match method {
            MethodName::Append => {
                let compiled_argument = self.compile_expr(&arguments[0])?;
                let argument = self.expect_value(compiled_argument)?;
                self.build_list_append(list_value, &element_type, argument, span)?;
                Ok(CodegenValue::Unit)
            }
            MethodName::Remove => {
                let compiled_argument = self.compile_expr(&arguments[0])?;
                let argument = self.expect_value(compiled_argument)?;
                let function = self.list_runtime_function("list_remove", &element_type)?;
                let mut args = vec![list_value.into()];
                args.extend(self.build_list_value_arguments(argument, span)?);
                self.map_builder(self.builder.build_call(function, &args, "call.list_remove"))?;
                Ok(CodegenValue::Unit)
            }
            MethodName::Pop => {
                let function = if arguments.is_empty() {
                    self.list_runtime_function("list_pop", &element_type)?
                } else {
                    self.list_runtime_function("list_pop_at", &element_type)?
                };
                let mut args = vec![list_value.into()];
                if let Some(index) = arguments.first() {
                    let compiled_index = self.compile_expr(index)?;
                    let index_value = self.expect_value(compiled_index)?;
                    args.push(index_value.value.into());
                }
                Ok(CodegenValue::Value(self.build_list_result_from_call(
                    function,
                    &args,
                    &element_type,
                    "call.list_pop",
                )?))
            }
            MethodName::Sort => {
                let function = self.list_runtime_function("list_sort", &element_type)?;
                self.map_builder(self.builder.build_call(
                    function,
                    &[list_value.into()],
                    "call.list_sort",
                ))?;
                Ok(CodegenValue::Unit)
            }
            MethodName::Reverse => {
                self.map_builder(self.builder.build_call(
                    self.runtime.list_reverse,
                    &[list_value.into()],
                    "call.list_reverse",
                ))?;
                Ok(CodegenValue::Unit)
            }
            MethodName::Index => {
                let compiled_argument = self.compile_expr(&arguments[0])?;
                let argument = self.expect_value(compiled_argument)?;
                let function = self.list_runtime_function("list_index", &element_type)?;
                let mut args = vec![list_value.into()];
                args.extend(self.build_list_value_arguments(argument, span)?);
                let call =
                    self.map_builder(self.builder.build_call(function, &args, "call.list_index"))?;
                Ok(CodegenValue::Value(TypedValue {
                    value_type: ValueType::Int,
                    value: call.try_as_basic_value().unwrap_basic(),
                }))
            }
            MethodName::Contains => {
                let compiled_argument = self.compile_expr(&arguments[0])?;
                let argument = self.expect_value(compiled_argument)?;
                let function = self.list_runtime_function("list_contains", &element_type)?;
                let mut args = vec![list_value.into()];
                args.extend(self.build_list_value_arguments(argument, span)?);
                let call = self.map_builder(self.builder.build_call(
                    function,
                    &args,
                    "call.list_contains",
                ))?;
                let raw = call.try_as_basic_value().unwrap_basic().into_int_value();
                let value = self.map_builder(self.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    raw,
                    self.context.i8_type().const_zero(),
                    "contains.bool",
                ))?;
                Ok(CodegenValue::Value(TypedValue {
                    value_type: ValueType::Bool,
                    value: value.into(),
                }))
            }
            MethodName::Keys
            | MethodName::Values
            | MethodName::Trim
            | MethodName::StartsWith
            | MethodName::EndsWith
            | MethodName::Replace => Err(CompilerError::build_message(
                format!("列表类型不支持方法 `{}`", method.display_name()),
                span,
            )),
        }
    }

    /// 编译字典方法调用。
    pub(super) fn compile_map_method_call(
        &mut self,
        target_value: TypedValue<'ctx>,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let (map_value, key_type, value_type) = self.expect_map_value(target_value, span)?;
        match method {
            MethodName::Remove => {
                let key_compiled = self.compile_expr(&arguments[0])?;
                let key = self.expect_value(key_compiled)?;
                let function = self.map_runtime_function("map_remove", &key_type, None)?;
                let mut args = vec![map_value.into()];
                args.extend(self.build_list_value_arguments(key, span)?);
                args.extend(self.build_map_error_site_arguments(span)?);
                self.map_builder(self.builder.build_call(function, &args, "call.map_remove"))?;
                Ok(CodegenValue::Unit)
            }
            MethodName::Contains => {
                let key_compiled = self.compile_expr(&arguments[0])?;
                let key = self.expect_value(key_compiled)?;
                let function = self.map_runtime_function("map_contains", &key_type, None)?;
                let mut args = vec![map_value.into()];
                args.extend(self.build_list_value_arguments(key, span)?);
                let call = self.map_builder(self.builder.build_call(
                    function,
                    &args,
                    "call.map_contains",
                ))?;
                let raw = call.try_as_basic_value().unwrap_basic().into_int_value();
                let value = self.map_builder(self.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    raw,
                    self.context.i8_type().const_zero(),
                    "map.contains.bool",
                ))?;
                Ok(CodegenValue::Value(TypedValue {
                    value_type: ValueType::Bool,
                    value: value.into(),
                }))
            }
            MethodName::Keys => Ok(CodegenValue::Value(TypedValue {
                value_type: ValueType::List(Box::new(key_type)),
                value: self
                    .map_builder(self.builder.build_call(
                        self.runtime.map_keys,
                        &[map_value.into()],
                        "call.map_keys",
                    ))?
                    .try_as_basic_value()
                    .unwrap_basic(),
            })),
            MethodName::Values => Ok(CodegenValue::Value(TypedValue {
                value_type: ValueType::List(Box::new(value_type)),
                value: self
                    .map_builder(self.builder.build_call(
                        self.runtime.map_values,
                        &[map_value.into()],
                        "call.map_values",
                    ))?
                    .try_as_basic_value()
                    .unwrap_basic(),
            })),
            MethodName::Append
            | MethodName::Pop
            | MethodName::Sort
            | MethodName::Reverse
            | MethodName::Index
            | MethodName::Trim
            | MethodName::StartsWith
            | MethodName::EndsWith
            | MethodName::Replace => Err(CompilerError::build_message(
                format!("字典类型不支持方法 `{}`", method.display_name()),
                span,
            )),
        }
    }

    /// 创建一个带有元素类型标记的空列表对象。
    pub(super) fn build_empty_list(
        &mut self,
        element_type: &ValueType,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        let tag = self.list_type_tag(element_type)?;
        let call = self.map_builder(self.builder.build_call(
            self.runtime.list_create,
            &[self.context.i8_type().const_int(tag as u64, false).into()],
            "call.list_create",
        ))?;
        Ok(call
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value())
    }

    /// 向列表对象追加一个元素。
    pub(super) fn build_list_append(
        &mut self,
        list_value: PointerValue<'ctx>,
        element_type: &ValueType,
        value: TypedValue<'ctx>,
        span: Span,
    ) -> Result<(), CompilerError> {
        let function = self.list_runtime_function("list_append", element_type)?;
        let mut args = vec![list_value.into()];
        args.extend(self.build_list_value_arguments(value, span)?);
        self.map_builder(self.builder.build_call(function, &args, "call.list_append"))?;
        Ok(())
    }

    /// 通过运行时调用读取列表指定下标上的元素。
    pub(super) fn build_list_get(
        &mut self,
        list_value: PointerValue<'ctx>,
        element_type: &ValueType,
        index_value: inkwell::values::IntValue<'ctx>,
        _span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let function = self.list_runtime_function("list_get", element_type)?;
        self.build_list_result_from_call(
            function,
            &[list_value.into(), index_value.into()],
            element_type,
            "call.list_get",
        )
    }

    /// 创建一个带有键类型和值类型标记的空字典对象。
    pub(super) fn build_empty_map(
        &mut self,
        key_type: &ValueType,
        value_type: &ValueType,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        let key_tag = self.list_type_tag(key_type)?;
        let value_tag = self.list_type_tag(value_type)?;
        let call = self.map_builder(
            self.builder.build_call(
                self.runtime.map_create,
                &[
                    self.context
                        .i8_type()
                        .const_int(key_tag as u64, false)
                        .into(),
                    self.context
                        .i8_type()
                        .const_int(value_tag as u64, false)
                        .into(),
                ],
                "call.map_create",
            ),
        )?;
        Ok(call
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value())
    }

    /// 向字典对象写入或更新一个键值对。
    pub(super) fn build_map_set(
        &mut self,
        map_value: PointerValue<'ctx>,
        key_type: &ValueType,
        value_type: &ValueType,
        key: TypedValue<'ctx>,
        value: TypedValue<'ctx>,
        span: Span,
    ) -> Result<(), CompilerError> {
        let function = self.map_runtime_function("map_set", key_type, Some(value_type))?;
        let mut args = vec![map_value.into()];
        args.extend(self.build_list_value_arguments(key, span)?);
        args.extend(self.build_list_value_arguments(value, span)?);
        self.map_builder(self.builder.build_call(function, &args, "call.map_set"))?;
        Ok(())
    }

    /// 通过运行时调用读取字典中指定键对应的值。
    pub(super) fn build_map_get(
        &mut self,
        map_value: PointerValue<'ctx>,
        key_type: &ValueType,
        value_type: &ValueType,
        key: TypedValue<'ctx>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let function = self.map_runtime_function("map_get", key_type, Some(value_type))?;
        let mut args = vec![map_value.into()];
        args.extend(self.build_list_value_arguments(key, span)?);
        args.extend(self.build_map_error_site_arguments(span)?);
        self.build_map_result_from_call(function, &args, value_type, "call.map_get")
    }

    /// 根据元素类型把值拆成运行时函数需要的参数列表。
    pub(super) fn build_list_value_arguments(
        &mut self,
        value: TypedValue<'ctx>,
        span: Span,
    ) -> Result<Vec<BasicMetadataValueEnum<'ctx>>, CompilerError> {
        match value.value_type {
            ValueType::Int | ValueType::Float => Ok(vec![value.value.into()]),
            ValueType::Bool => {
                let bool_value = self.map_builder(self.builder.build_int_z_extend(
                    value.value.into_int_value(),
                    self.context.i8_type(),
                    "list.bool.cast",
                ))?;
                Ok(vec![bool_value.into()])
            }
            ValueType::String => {
                let string_value = value.value.into_struct_value();
                let ptr = self.extract_string_part(string_value, 0, "list.string.ptr")?;
                let len = self.extract_string_part(string_value, 1, "list.string.len")?;
                Ok(vec![ptr.into(), len.into()])
            }
            _ => Err(CompilerError::build_message(
                "当前值不能作为列表元素写入运行时",
                span,
            )),
        }
    }

    /// 把列表运行时调用结果恢复为后端内部的带类型值。
    pub(super) fn build_list_result_from_call(
        &mut self,
        function: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
        element_type: &ValueType,
        call_name: &str,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if matches!(element_type, ValueType::String) {
            return self.build_string_list_result_from_call(function, args, call_name);
        }

        let call = self.map_builder(self.builder.build_call(function, args, call_name))?;
        let raw = call.try_as_basic_value().unwrap_basic();
        match element_type {
            ValueType::Bool => {
                let value = self.map_builder(self.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    raw.into_int_value(),
                    self.context.i8_type().const_zero(),
                    "list.bool.result",
                ))?;
                Ok(TypedValue {
                    value_type: ValueType::Bool,
                    value: value.into(),
                })
            }
            _ => Ok(TypedValue {
                value_type: element_type.clone(),
                value: raw,
            }),
        }
    }

    /// 把字典运行时调用结果恢复为后端内部的带类型值。
    pub(super) fn build_map_result_from_call(
        &mut self,
        function: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
        value_type: &ValueType,
        call_name: &str,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if matches!(value_type, ValueType::String) {
            return self.build_string_map_result_from_call(function, args, call_name);
        }

        let call = self.map_builder(self.builder.build_call(function, args, call_name))?;
        let raw = call.try_as_basic_value().unwrap_basic();
        match value_type {
            ValueType::Bool => {
                let value = self.map_builder(self.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    raw.into_int_value(),
                    self.context.i8_type().const_zero(),
                    "map.bool.result",
                ))?;
                Ok(TypedValue {
                    value_type: ValueType::Bool,
                    value: value.into(),
                })
            }
            _ => Ok(TypedValue {
                value_type: value_type.clone(),
                value: raw,
            }),
        }
    }

    /// 把字符串运行时调用结果恢复为后端内部的带类型字符串值。
    pub(super) fn build_string_method_result_from_call(
        &mut self,
        function: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
        call_name: &str,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let out_len_ptr = self.allocate_local(&ValueType::Int, "string.method.out_len")?;
        self.map_builder(
            self.builder
                .build_store(out_len_ptr, self.context.i64_type().const_zero()),
        )?;

        let mut call_args = args.to_vec();
        call_args.push(out_len_ptr.into());
        let call = self.map_builder(self.builder.build_call(function, &call_args, call_name))?;
        let pointer = call
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();
        let length = self
            .map_builder(self.builder.build_load(
                self.context.i64_type(),
                out_len_ptr,
                "load.string.method.len",
            ))?
            .into_int_value();

        Ok(TypedValue {
            value_type: ValueType::String,
            value: self.build_runtime_string(pointer, length)?.into(),
        })
    }

    /// 处理字符串列表读取与弹出调用，使用额外长度输出参数重建内部字符串值。
    pub(super) fn build_string_list_result_from_call(
        &mut self,
        function: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
        call_name: &str,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let out_len_ptr = self.allocate_local(&ValueType::Int, "list.string.out_len")?;
        self.map_builder(
            self.builder
                .build_store(out_len_ptr, self.context.i64_type().const_zero()),
        )?;

        let mut call_args = args.to_vec();
        call_args.push(out_len_ptr.into());
        let call = self.map_builder(self.builder.build_call(function, &call_args, call_name))?;
        let pointer = call
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();
        let length = self
            .map_builder(self.builder.build_load(
                self.context.i64_type(),
                out_len_ptr,
                "load.list.string.len",
            ))?
            .into_int_value();

        Ok(TypedValue {
            value_type: ValueType::String,
            value: self.build_runtime_string(pointer, length)?.into(),
        })
    }

    /// 把字符串结构值拆成运行时函数统一使用的 `ptr + len` 参数列表。
    pub(super) fn build_string_runtime_arguments(
        &self,
        value: StructValue<'ctx>,
    ) -> Result<Vec<BasicMetadataValueEnum<'ctx>>, CompilerError> {
        let ptr = self.extract_string_part(value, 0, "string.runtime.ptr")?;
        let len = self.extract_string_part(value, 1, "string.runtime.len")?;
        Ok(vec![ptr.into(), len.into()])
    }

    /// 编译字符串方法参数，并确保参与调用的参数本身也是字符串值。
    pub(super) fn compile_string_method_argument(
        &mut self,
        argument: &Expr,
    ) -> Result<StructValue<'ctx>, CompilerError> {
        let compiled_argument = self.compile_expr(argument)?;
        let argument_value = self.expect_value(compiled_argument)?;
        self.expect_string_value(
            argument_value,
            argument.span(),
            "字符串方法参数必须是字符串类型",
        )
    }

    /// 把运行时返回的 `i8` 布尔值恢复为后端内部统一使用的布尔类型。
    pub(super) fn build_bool_result_from_call(
        &mut self,
        function: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
        call_name: &str,
        compare_name: &str,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let call = self.map_builder(self.builder.build_call(function, args, call_name))?;
        let raw = call.try_as_basic_value().unwrap_basic().into_int_value();
        let value = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            raw,
            self.context.i8_type().const_zero(),
            compare_name,
        ))?;
        Ok(TypedValue {
            value_type: ValueType::Bool,
            value: value.into(),
        })
    }

    /// 处理字符串字典读取调用，使用额外长度输出参数重建内部字符串值。
    pub(super) fn build_string_map_result_from_call(
        &mut self,
        function: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
        call_name: &str,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let out_len_ptr = self.allocate_local(&ValueType::Int, "map.string.out_len")?;
        self.map_builder(
            self.builder
                .build_store(out_len_ptr, self.context.i64_type().const_zero()),
        )?;

        // 字典字符串读取的运行时签名要求把 out_len 放在“键参数”之后、“错误定位参数”之前。
        // 这里不能像列表那样直接追加到末尾，否则会把路径字符串错当成 out_len 传给 LLVM。
        let error_site_arg_count = 6;
        let insert_index = args
            .len()
            .checked_sub(error_site_arg_count)
            .ok_or_else(|| {
                CompilerError::build_message("字典字符串读取参数数量不合法", Span::new(0, 0))
            })?;
        let mut call_args = args.to_vec();
        call_args.insert(insert_index, out_len_ptr.into());
        let call = self.map_builder(self.builder.build_call(function, &call_args, call_name))?;
        let pointer = call
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();
        let length = self
            .map_builder(self.builder.build_load(
                self.context.i64_type(),
                out_len_ptr,
                "load.map.string.len",
            ))?
            .into_int_value();

        Ok(TypedValue {
            value_type: ValueType::String,
            value: self.build_runtime_string(pointer, length)?.into(),
        })
    }

    /// 校验表达式结果是否为列表值，并拆出句柄和元素类型。
    pub(super) fn expect_list_value(
        &self,
        value: TypedValue<'ctx>,
        span: Span,
    ) -> Result<(PointerValue<'ctx>, ValueType), CompilerError> {
        let ValueType::List(element_type) = value.value_type else {
            return Err(CompilerError::build_message("当前表达式不是列表类型", span));
        };
        Ok((value.value.into_pointer_value(), (*element_type).clone()))
    }

    /// 校验表达式结果是否为字典值，并拆出句柄、键类型和值类型。
    pub(super) fn expect_map_value(
        &self,
        value: TypedValue<'ctx>,
        span: Span,
    ) -> Result<(PointerValue<'ctx>, ValueType, ValueType), CompilerError> {
        let ValueType::Map(key_type, value_type) = value.value_type else {
            return Err(CompilerError::build_message("当前表达式不是字典类型", span));
        };
        Ok((
            value.value.into_pointer_value(),
            (*key_type).clone(),
            (*value_type).clone(),
        ))
    }

    /// 校验表达式结果是否为字符串值，并返回内部复用的 `{ ptr, len }` 结构。
    pub(super) fn expect_string_value(
        &self,
        value: TypedValue<'ctx>,
        span: Span,
        message: &str,
    ) -> Result<StructValue<'ctx>, CompilerError> {
        if value.value_type != ValueType::String {
            return Err(CompilerError::build_message(message, span));
        }
        Ok(value.value.into_struct_value())
    }

    /// 校验一个值类型是否能作为当前阶段的列表元素类型。
    pub(super) fn expect_list_element_type(
        &self,
        value_type: ValueType,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match value_type {
            ValueType::Int | ValueType::Float | ValueType::Bool | ValueType::String => {
                Ok(value_type)
            }
            _ => Err(CompilerError::build_message(
                "当前阶段列表元素只支持基础类型",
                span,
            )),
        }
    }

    /// 把数组表达式解析为可索引的内存地址，必要时先落到临时栈槽。
    pub(super) fn resolve_array_pointer(
        &mut self,
        target: &Expr,
        value: TypedValue<'ctx>,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        if let Expr::Identifier { name, .. } = target {
            if let Some(variable) = self.lookup_variable(name) {
                if variable.value_type == value.value_type {
                    return Ok(variable.pointer);
                }
            }
        }

        let pointer = self.allocate_local(&value.value_type, "array.index.temp")?;
        self.map_builder(self.builder.build_store(pointer, value.value))?;
        Ok(pointer)
    }

    /// 把单个数组元素写入指定下标位置。
    pub(super) fn build_array_store(
        &mut self,
        array_pointer: PointerValue<'ctx>,
        element_type: &ValueType,
        size: usize,
        index_value: inkwell::values::IntValue<'ctx>,
        value: inkwell::values::BasicValueEnum<'ctx>,
    ) -> Result<(), CompilerError> {
        let zero = self.context.i64_type().const_zero();
        let element_pointer = unsafe {
            self.map_builder(self.builder.build_in_bounds_gep(
                self.array_type_for(element_type, size)?,
                array_pointer,
                &[zero, index_value],
                "array.element.ptr",
            ))?
        };
        self.map_builder(self.builder.build_store(element_pointer, value))?;
        Ok(())
    }

    /// 从固定长度数组指定下标读取单个元素。
    pub(super) fn build_array_get(
        &mut self,
        array_pointer: PointerValue<'ctx>,
        element_type: &ValueType,
        size: usize,
        index_value: inkwell::values::IntValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let zero = self.context.i64_type().const_zero();
        let element_pointer = unsafe {
            self.map_builder(self.builder.build_in_bounds_gep(
                self.array_type_for(element_type, size)?,
                array_pointer,
                &[zero, index_value],
                "array.index.ptr",
            ))?
        };
        let value = self.map_builder(self.builder.build_load(
            self.basic_type_for(element_type)?,
            element_pointer,
            "load.array.item",
        ))?;
        Ok(TypedValue {
            value_type: element_type.clone(),
            value,
        })
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
