use std::collections::HashMap;

use inkwell::AddressSpace;
use inkwell::OptimizationLevel;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicTypeEnum, StructType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FloatValue, FunctionValue, IntValue, PointerValue,
    StructValue,
};
use inkwell::{FloatPredicate, IntPredicate};

use crate::ast::{BinaryOp, Expr, Literal, Program, Stmt, UnaryOp};
use crate::diagnostic::{CompilerError, MessageId};
use crate::semantic::ValueType;
use crate::settings::CompilerSettings;
use crate::source::{SourceFile, Span};

/// 保存 LLVM 后端生成的目标文件字节。
pub struct GeneratedObject {
    pub object_bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
struct VariableInfo<'ctx> {
    value_type: ValueType,
    pointer: PointerValue<'ctx>,
}

#[derive(Clone, Copy)]
struct TypedValue<'ctx> {
    value_type: ValueType,
    value: BasicValueEnum<'ctx>,
}

enum CodegenValue<'ctx> {
    Value(TypedValue<'ctx>),
    Unit,
}

struct RuntimeFunctions<'ctx> {
    print_int: FunctionValue<'ctx>,
    print_float: FunctionValue<'ctx>,
    print_bool: FunctionValue<'ctx>,
    print_string: FunctionValue<'ctx>,
    string_concat: FunctionValue<'ctx>,
}

/// 管理单次代码生成过程中的 LLVM 上下文、模块、符号表和运行时声明。
struct CodeGenerator<'ctx, 'src> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    source: &'src SourceFile,
    variables: HashMap<String, VariableInfo<'ctx>>,
    runtime: RuntimeFunctions<'ctx>,
    string_type: StructType<'ctx>,
    string_index: usize,
}

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 初始化模块、目标三元组、运行时函数声明和入口函数骨架。
    fn new(context: &'ctx Context, source: &'src SourceFile) -> Result<Self, CompilerError> {
        Target::initialize_native(&InitializationConfig::default()).map_err(|error| {
            CompilerError::build(
                MessageId::BuildCreateTargetFailed,
                vec![error],
                Span::new(0, source.text().len()),
            )
        })?;

        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|error| {
            CompilerError::build(
                MessageId::BuildCreateTargetFailed,
                vec![error.to_string()],
                Span::new(0, source.text().len()),
            )
        })?;

        let target_machine = target
            .create_target_machine(
                &triple,
                TargetMachine::get_host_cpu_name().to_string().as_str(),
                TargetMachine::get_host_cpu_features().to_string().as_str(),
                OptimizationLevel::Default,
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| {
                CompilerError::build(
                    MessageId::BuildCreateTargetFailed,
                    vec!["无法为当前宿主平台创建目标机器".to_string()],
                    Span::new(0, source.text().len()),
                )
            })?;

        let module = context.create_module("mascript");
        module.set_triple(&triple);
        module.set_data_layout(&target_machine.get_target_data().get_data_layout());

        let builder = context.create_builder();
        let string_type = context.struct_type(
            &[
                context.ptr_type(AddressSpace::default()).into(),
                context.i64_type().into(),
            ],
            false,
        );
        let runtime = Self::declare_runtime(context, &module, string_type);

        Ok(Self {
            context,
            module,
            builder,
            source,
            variables: HashMap::new(),
            runtime,
            string_type,
            string_index: 0,
        })
    }

    /// 把整个程序编译为入口函数体，并补上最终 `ret void`。
    fn compile_program(&mut self, program: &Program) -> Result<(), CompilerError> {
        let fn_type = self.context.void_type().fn_type(&[], false);
        let entry_fn = self.module.add_function("mascript_entry", fn_type, None);
        let entry_block = self.context.append_basic_block(entry_fn, "entry");
        self.builder.position_at_end(entry_block);

        for statement in &program.statements {
            self.compile_statement(statement)?;
        }

        self.map_builder(self.builder.build_return(None))?;
        Ok(())
    }

    /// 把构建完成的模块校验后输出为 COFF 目标文件字节。
    fn finish_object(self) -> Result<GeneratedObject, CompilerError> {
        self.module.verify().map_err(|error| {
            CompilerError::build(
                MessageId::BuildVerifyModuleFailed,
                vec![error.to_string()],
                Span::new(0, self.source.text().len()),
            )
        })?;

        let triple = self.module.get_triple();
        let target = Target::from_triple(&triple).map_err(|error| {
            CompilerError::build(
                MessageId::BuildCreateTargetFailed,
                vec![error.to_string()],
                Span::new(0, self.source.text().len()),
            )
        })?;
        let target_machine = target
            .create_target_machine(
                &triple,
                TargetMachine::get_host_cpu_name().to_string().as_str(),
                TargetMachine::get_host_cpu_features().to_string().as_str(),
                OptimizationLevel::Default,
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| {
                CompilerError::build(
                    MessageId::BuildCreateTargetFailed,
                    vec!["无法为当前宿主平台创建目标机器".to_string()],
                    Span::new(0, self.source.text().len()),
                )
            })?;
        let buffer = target_machine
            .write_to_memory_buffer(&self.module, FileType::Object)
            .map_err(|error| {
                CompilerError::build(
                    MessageId::BuildWriteIrFailed,
                    vec![error.to_string()],
                    Span::new(0, self.source.text().len()),
                )
            })?;

        Ok(GeneratedObject {
            object_bytes: buffer.as_slice().to_vec(),
        })
    }

    /// 返回当前模块的 LLVM IR 字符串，供测试校验运行时符号绑定。
    #[cfg(test)]
    fn finish_ir(self) -> Result<String, CompilerError> {
        self.module.verify().map_err(|error| {
            CompilerError::build(
                MessageId::BuildVerifyModuleFailed,
                vec![error.to_string()],
                Span::new(0, self.source.text().len()),
            )
        })?;
        Ok(self.module.print_to_string().to_string())
    }

    /// 预声明最小运行时 ABI，供后续 `print` 和字符串拼接直接调用。
    fn declare_runtime(
        context: &'ctx Context,
        module: &Module<'ctx>,
        _string_type: StructType<'ctx>,
    ) -> RuntimeFunctions<'ctx> {
        let void_type = context.void_type();
        let i64_type = context.i64_type();
        let f64_type = context.f64_type();
        let i8_type = context.i8_type();
        let ptr_type = context.ptr_type(AddressSpace::default());

        RuntimeFunctions {
            print_int: module.add_function(
                "print_int",
                void_type.fn_type(&[i64_type.into()], false),
                None,
            ),
            print_float: module.add_function(
                "print_float",
                void_type.fn_type(&[f64_type.into()], false),
                None,
            ),
            print_bool: module.add_function(
                "print_bool",
                void_type.fn_type(&[i8_type.into()], false),
                None,
            ),
            print_string: module.add_function(
                "print_string",
                void_type.fn_type(&[ptr_type.into(), i64_type.into()], false),
                None,
            ),
            string_concat: module.add_function(
                "string_concat",
                ptr_type.fn_type(
                    &[
                        ptr_type.into(),
                        i64_type.into(),
                        ptr_type.into(),
                        i64_type.into(),
                        ptr_type.into(),
                    ],
                    false,
                ),
                None,
            ),
        }
    }

    /// 编译单条语句，并在需要时写入局部变量栈槽。
    fn compile_statement(&mut self, statement: &Stmt) -> Result<(), CompilerError> {
        match statement {
            Stmt::VarDecl {
                name,
                annotation: _,
                initializer,
                ..
            } => {
                let compiled = self.compile_expr(initializer)?;
                let value = self.expect_value(compiled)?;
                let pointer = self.map_builder(
                    self.builder
                        .build_alloca(self.basic_type_for(value.value_type), name),
                )?;
                self.map_builder(self.builder.build_store(pointer, value.value))?;
                self.variables.insert(
                    name.clone(),
                    VariableInfo {
                        value_type: value.value_type,
                        pointer,
                    },
                );
                Ok(())
            }
            Stmt::ExprStmt { expr, .. } => {
                self.compile_expr(expr)?;
                Ok(())
            }
        }
    }

    /// 编译表达式，并把 LLVM 值与静态类型一起返回给调用方。
    fn compile_expr(&mut self, expression: &Expr) -> Result<CodegenValue<'ctx>, CompilerError> {
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
    fn compile_literal(&mut self, literal: &Literal) -> Result<TypedValue<'ctx>, CompilerError> {
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

    /// 从变量栈槽中加载一个已声明变量的当前值。
    fn compile_identifier(
        &self,
        name: &str,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let variable = self.variables.get(name).ok_or_else(|| {
            CompilerError::build(
                MessageId::BuildWriteIrFailed,
                vec![format!("语义阶段遗漏了变量 `{name}` 的检查")],
                span,
            )
        })?;
        let value = self.map_builder(self.builder.build_load(
            self.basic_type_for(variable.value_type),
            variable.pointer,
            &format!("{name}.load"),
        ))?;
        Ok(TypedValue {
            value_type: variable.value_type,
            value,
        })
    }

    /// 编译一元表达式，并生成对应的 LLVM 指令。
    fn compile_unary(
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
                return Err(CompilerError::build(
                    MessageId::BuildWriteIrFailed,
                    vec![format!("无法为当前一元表达式生成代码: {span:?}")],
                    span,
                ));
            }
        };
        Ok(CodegenValue::Value(result))
    }

    /// 编译二元表达式，并根据静态类型规则选择对应的 LLVM 指令。
    fn compile_binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let left_compiled = self.compile_expr(left)?;
        let left_value = self.expect_value(left_compiled)?;
        let right_compiled = self.compile_expr(right)?;
        let right_value = self.expect_value(right_compiled)?;

        let result = match op {
            BinaryOp::Add => self.compile_add(left_value, right_value)?,
            BinaryOp::Subtract => self.compile_numeric_binary(left_value, right_value, op)?,
            BinaryOp::Multiply => self.compile_numeric_binary(left_value, right_value, op)?,
            BinaryOp::Divide => self.compile_numeric_binary(left_value, right_value, op)?,
            BinaryOp::And => TypedValue {
                value_type: ValueType::Bool,
                value: self
                    .map_builder(self.builder.build_and(
                        left_value.value.into_int_value(),
                        right_value.value.into_int_value(),
                        "and.bool",
                    ))?
                    .into(),
            },
            BinaryOp::Or => TypedValue {
                value_type: ValueType::Bool,
                value: self
                    .map_builder(self.builder.build_or(
                        left_value.value.into_int_value(),
                        right_value.value.into_int_value(),
                        "or.bool",
                    ))?
                    .into(),
            },
            BinaryOp::Equal | BinaryOp::NotEqual => {
                self.compile_equality(left_value, right_value, op)?
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                self.compile_order_compare(left_value, right_value, op)?
            }
        };

        if result.value_type == ValueType::Unit {
            return Err(CompilerError::build(
                MessageId::BuildWriteIrFailed,
                vec![format!("表达式生成了非法的 unit 值: {span:?}")],
                span,
            ));
        }

        Ok(CodegenValue::Value(result))
    }

    /// 编译加法表达式，额外处理字符串拼接分支。
    fn compile_add(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if left.value_type == ValueType::String && right.value_type == ValueType::String {
            let left_string = left.value.into_struct_value();
            let right_string = right.value.into_struct_value();
            let left_ptr = self.extract_string_part(left_string, 0, "left.ptr")?;
            let left_len = self.extract_string_part(left_string, 1, "left.len")?;
            let right_ptr = self.extract_string_part(right_string, 0, "right.ptr")?;
            let right_len = self.extract_string_part(right_string, 1, "right.len")?;
            let out_len_ptr = self.map_builder(
                self.builder
                    .build_alloca(self.context.i64_type(), "concat.out_len"),
            )?;
            let call = self.map_builder(self.builder.build_call(
                self.runtime.string_concat,
                &[
                    left_ptr.into(),
                    left_len.into(),
                    right_ptr.into(),
                    right_len.into(),
                    out_len_ptr.into(),
                ],
                "concat.string",
            ))?;
            let ptr_value = call
                .try_as_basic_value()
                .unwrap_basic()
                .into_pointer_value();
            let len_value = self
                .map_builder(self.builder.build_load(
                    self.context.i64_type(),
                    out_len_ptr,
                    "concat.len",
                ))?
                .into_int_value();
            return Ok(TypedValue {
                value_type: ValueType::String,
                value: self.build_runtime_string(ptr_value, len_value)?.into(),
            });
        }

        self.compile_numeric_binary(left, right, BinaryOp::Add)
    }

    /// 编译数值二元运算，并按需要把整数提升为浮点数。
    fn compile_numeric_binary(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        match self.coerce_numeric_pair(left, right)? {
            NumericPair::Int(left_int, right_int) => {
                let value = match op {
                    BinaryOp::Add => self
                        .map_builder(self.builder.build_int_add(left_int, right_int, "add.int"))?
                        .into(),
                    BinaryOp::Subtract => self
                        .map_builder(self.builder.build_int_sub(left_int, right_int, "sub.int"))?
                        .into(),
                    BinaryOp::Multiply => self
                        .map_builder(self.builder.build_int_mul(left_int, right_int, "mul.int"))?
                        .into(),
                    _ => unreachable!(),
                };
                Ok(TypedValue {
                    value_type: ValueType::Int,
                    value,
                })
            }
            NumericPair::Float(left_float, right_float) => {
                let value = match op {
                    BinaryOp::Add => self
                        .map_builder(self.builder.build_float_add(
                            left_float,
                            right_float,
                            "add.float",
                        ))?
                        .into(),
                    BinaryOp::Subtract => self
                        .map_builder(self.builder.build_float_sub(
                            left_float,
                            right_float,
                            "sub.float",
                        ))?
                        .into(),
                    BinaryOp::Multiply => self
                        .map_builder(self.builder.build_float_mul(
                            left_float,
                            right_float,
                            "mul.float",
                        ))?
                        .into(),
                    BinaryOp::Divide => self
                        .map_builder(self.builder.build_float_div(
                            left_float,
                            right_float,
                            "div.float",
                        ))?
                        .into(),
                    _ => unreachable!(),
                };
                let value_type = if op == BinaryOp::Divide {
                    ValueType::Float
                } else {
                    ValueType::Float
                };
                Ok(TypedValue { value_type, value })
            }
        }
    }

    /// 编译相等比较表达式。
    fn compile_equality(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let value = match (left.value_type, right.value_type) {
            (ValueType::Int, ValueType::Int) | (ValueType::Bool, ValueType::Bool) => {
                let predicate = if op == BinaryOp::Equal {
                    IntPredicate::EQ
                } else {
                    IntPredicate::NE
                };
                self.map_builder(self.builder.build_int_compare(
                    predicate,
                    left.value.into_int_value(),
                    right.value.into_int_value(),
                    "cmp.int",
                ))?
                .into()
            }
            _ => {
                let (left_float, right_float) = self.coerce_to_float_pair(left, right)?;
                let predicate = if op == BinaryOp::Equal {
                    FloatPredicate::OEQ
                } else {
                    FloatPredicate::ONE
                };
                self.map_builder(self.builder.build_float_compare(
                    predicate,
                    left_float,
                    right_float,
                    "cmp.float",
                ))?
                .into()
            }
        };

        Ok(TypedValue {
            value_type: ValueType::Bool,
            value,
        })
    }

    /// 编译大小比较表达式。
    fn compile_order_compare(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let (left_float, right_float) = self.coerce_to_float_pair(left, right)?;
        let predicate = match op {
            BinaryOp::Less => FloatPredicate::OLT,
            BinaryOp::LessEqual => FloatPredicate::OLE,
            BinaryOp::Greater => FloatPredicate::OGT,
            BinaryOp::GreaterEqual => FloatPredicate::OGE,
            _ => unreachable!(),
        };
        let value = self
            .map_builder(self.builder.build_float_compare(
                predicate,
                left_float,
                right_float,
                "cmp.order",
            ))?
            .into();
        Ok(TypedValue {
            value_type: ValueType::Bool,
            value,
        })
    }

    /// 编译函数调用表达式，本阶段仅支持 `print(...)`。
    fn compile_call(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let Expr::Identifier { name, .. } = callee else {
            return Err(CompilerError::build(
                MessageId::BuildWriteIrFailed,
                vec!["当前阶段只支持具名函数调用".to_string()],
                span,
            ));
        };

        if name != "print" {
            return Err(CompilerError::build(
                MessageId::BuildWriteIrFailed,
                vec![format!("未知函数 `{name}`")],
                span,
            ));
        }

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
                    return Err(CompilerError::build(
                        MessageId::BuildWriteIrFailed,
                        vec!["print 不能打印 unit 值".to_string()],
                        span,
                    ));
                }
            };

        self.map_builder(self.builder.build_call(function, &args, "call.print"))?;
        Ok(CodegenValue::Unit)
    }

    /// 把两个数值表达式统一为整数对或浮点对，供算术运算复用。
    fn coerce_numeric_pair(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
    ) -> Result<NumericPair<'ctx>, CompilerError> {
        if left.value_type == ValueType::Int && right.value_type == ValueType::Int {
            return Ok(NumericPair::Int(
                left.value.into_int_value(),
                right.value.into_int_value(),
            ));
        }

        let (left_float, right_float) = self.coerce_to_float_pair(left, right)?;
        Ok(NumericPair::Float(left_float, right_float))
    }

    /// 把两个数值表达式统一提升为浮点值。
    fn coerce_to_float_pair(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
    ) -> Result<(FloatValue<'ctx>, FloatValue<'ctx>), CompilerError> {
        Ok((self.to_float(left)?, self.to_float(right)?))
    }

    /// 把单个整数或浮点值转换为浮点值。
    fn to_float(&mut self, value: TypedValue<'ctx>) -> Result<FloatValue<'ctx>, CompilerError> {
        match value.value_type {
            ValueType::Float => Ok(value.value.into_float_value()),
            ValueType::Int => Ok(self.map_builder(self.builder.build_signed_int_to_float(
                value.value.into_int_value(),
                self.context.f64_type(),
                "int.to.float",
            ))?),
            _ => Err(CompilerError::build(
                MessageId::BuildWriteIrFailed,
                vec!["当前值无法提升为浮点数".to_string()],
                Span::new(0, self.source.text().len()),
            )),
        }
    }

    /// 把字符串字面量构造成 `{ ptr, len }` 结构值。
    fn build_string_literal(&mut self, value: &str) -> Result<StructValue<'ctx>, CompilerError> {
        let global = self.map_builder(
            self.builder
                .build_global_string_ptr(value, &format!("string.{}", self.string_index)),
        )?;
        self.string_index += 1;
        self.build_runtime_string(
            global.as_pointer_value(),
            self.context.i64_type().const_int(value.len() as u64, false),
        )
    }

    /// 使用运行期值构造内部字符串结构。
    fn build_runtime_string(
        &self,
        pointer: PointerValue<'ctx>,
        length: IntValue<'ctx>,
    ) -> Result<StructValue<'ctx>, CompilerError> {
        let initial = self.string_type.get_undef();
        let with_ptr = self
            .map_builder(
                self.builder
                    .build_insert_value(initial, pointer, 0, "string.ptr"),
            )?
            .into_struct_value();
        let with_len = self
            .map_builder(
                self.builder
                    .build_insert_value(with_ptr, length, 1, "string.len"),
            )?
            .into_struct_value();
        Ok(with_len)
    }

    /// 从内部字符串结构值中提取指定字段，避免重复写 `extractvalue` 指令。
    fn extract_string_part(
        &self,
        value: StructValue<'ctx>,
        index: u32,
        name: &str,
    ) -> Result<BasicValueEnum<'ctx>, CompilerError> {
        self.map_builder(self.builder.build_extract_value(value, index, name))
    }

    /// 返回指定语义类型在 LLVM 中对应的基础类型对象。
    fn basic_type_for(&self, value_type: ValueType) -> BasicTypeEnum<'ctx> {
        match value_type {
            ValueType::Int => self.context.i64_type().into(),
            ValueType::Float => self.context.f64_type().into(),
            ValueType::Bool => self.context.bool_type().into(),
            ValueType::String => self.string_type.into(),
            ValueType::Unit => panic!("unit 不应该被映射为可存储的基础类型"),
        }
    }

    /// 统一包装所有 Inkwell 构建器错误，转换成内部构建诊断。
    fn map_builder<T>(&self, value: Result<T, BuilderError>) -> Result<T, CompilerError> {
        value.map_err(|error| {
            CompilerError::build(
                MessageId::BuildWriteIrFailed,
                vec![error.to_string()],
                Span::new(0, self.source.text().len()),
            )
        })
    }

    /// 确保表达式编译结果不是 `unit`，否则返回内部构建错误。
    fn expect_value(&self, value: CodegenValue<'ctx>) -> Result<TypedValue<'ctx>, CompilerError> {
        match value {
            CodegenValue::Value(value) => Ok(value),
            CodegenValue::Unit => Err(CompilerError::build(
                MessageId::BuildWriteIrFailed,
                vec!["表达式返回了不可参与计算的 unit 值".to_string()],
                Span::new(0, self.source.text().len()),
            )),
        }
    }
}

enum NumericPair<'ctx> {
    Int(IntValue<'ctx>, IntValue<'ctx>),
    Float(FloatValue<'ctx>, FloatValue<'ctx>),
}

/// 使用 LLVM 后端把程序编译为目标文件字节。
pub fn compile_program(
    source: &SourceFile,
    _settings: &CompilerSettings,
    program: &Program,
) -> Result<GeneratedObject, CompilerError> {
    let context = Context::create();
    let mut generator = CodeGenerator::new(&context, source)?;
    generator.compile_program(program)?;
    generator.finish_object()
}

/// 使用 LLVM 后端把程序编译为 IR 文本，仅供测试校验使用。
#[cfg(test)]
fn compile_program_to_ir(source: &SourceFile, program: &Program) -> Result<String, CompilerError> {
    let context = Context::create();
    let mut generator = CodeGenerator::new(&context, source)?;
    generator.compile_program(program)?;
    generator.finish_ir()
}

#[cfg(test)]
mod tests {
    use super::{compile_program, compile_program_to_ir};
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::settings::{CompilerSettings, UiLanguage};
    use crate::source::SourceFile;

    /// 先做前端分析，再把程序交给 LLVM 后端生成测试产物。
    fn parse_program(text: &str) -> (SourceFile, crate::ast::Program) {
        let settings = CompilerSettings::new(UiLanguage::ZhCn);
        let source = SourceFile::new("<test>", text.to_string());
        let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
        let program = Parser::new(tokens, &settings).parse_program().unwrap();
        (source, program)
    }

    /// 校验 LLVM 后端能够生成非空目标文件。
    #[test]
    fn should_emit_object_bytes() {
        let settings = CompilerSettings::new(UiLanguage::ZhCn);
        let (source, program) = parse_program("print(1)\n");
        let object = compile_program(&source, &settings, &program).unwrap();
        assert!(!object.object_bytes.is_empty());
    }

    /// 校验 `print` 调用会绑定到正确的运行时符号。
    #[test]
    fn should_emit_print_runtime_symbol() {
        let (source, program) = parse_program("print(1)\n");
        let ir = compile_program_to_ir(&source, &program).unwrap();
        assert!(ir.contains("print_int"));
    }

    /// 校验字符串拼接路径会引用 `string_concat` 运行时符号。
    #[test]
    fn should_emit_string_concat_runtime_symbol() {
        let (source, program) = parse_program("print(\"a\" + \"b\")\n");
        let ir = compile_program_to_ir(&source, &program).unwrap();
        assert!(ir.contains("string_concat"));
    }
}
