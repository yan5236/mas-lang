use inkwell::values::{BasicMetadataValueEnum, FloatValue, FunctionValue, PointerValue};

use super::*;
use crate::ast::{Expr, Literal, MethodName, TypeName, UnaryOp};

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

    /// 编译对象方法调用，并根据目标类型分派到列表或字典实现。
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
            ValueType::List(_) => {
                self.compile_list_method_call(target_value, method, arguments, span)
            }
            ValueType::Map(_, _) => {
                self.compile_map_method_call(target_value, method, arguments, span)
            }
            _ => Err(CompilerError::build_message(
                "当前方法只能用于列表或字典类型",
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
            MethodName::Keys | MethodName::Values => Err(CompilerError::build_message(
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
            | MethodName::Index => Err(CompilerError::build_message(
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
