use std::collections::HashMap;

use inkwell::AddressSpace;
use inkwell::OptimizationLevel;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FloatValue, FunctionValue, GlobalValue, IntValue,
    PointerValue, StructValue,
};
use inkwell::{FloatPredicate, IntPredicate};

use crate::ast::{
    AssignOp, BinaryOp, ConditionalBranch, Expr, Literal, Program, Stmt, TypeName, UnaryOp,
};
use crate::diagnostic::{CompilerError, MessageId};
use crate::semantic::ValueType;
use crate::settings::CompilerSettings;
use crate::source::{SourceFile, Span};

/// 保存 LLVM 后端生成的目标文件字节。
pub struct GeneratedObject {
    pub object_bytes: Vec<u8>,
}

/// 保存单个变量的类型、指针位置和可变性信息。
#[derive(Clone, Copy)]
struct VariableInfo<'ctx> {
    value_type: ValueType,
    pointer: PointerValue<'ctx>,
    is_mutable: bool,
}

/// 保存表达式在 LLVM 中的值和静态类型。
#[derive(Clone, Copy)]
struct TypedValue<'ctx> {
    value_type: ValueType,
    value: BasicValueEnum<'ctx>,
}

/// 表示表达式代码生成结果是否为可参与后续计算的值。
enum CodegenValue<'ctx> {
    Value(TypedValue<'ctx>),
    Unit,
}

/// 保存用户函数的 LLVM 符号、返回类型和参数名。
#[derive(Clone)]
struct UserFunctionInfo<'ctx> {
    function: FunctionValue<'ctx>,
    return_type: ValueType,
}

/// 保存运行时 ABI 中需要调用的外部函数。
struct RuntimeFunctions<'ctx> {
    print_int: FunctionValue<'ctx>,
    print_float: FunctionValue<'ctx>,
    print_bool: FunctionValue<'ctx>,
    print_string: FunctionValue<'ctx>,
    string_concat: FunctionValue<'ctx>,
    string_equals: FunctionValue<'ctx>,
}

/// 保存循环内 `break` 与 `continue` 的目标基本块。
#[derive(Clone, Copy)]
struct LoopContext<'ctx> {
    break_block: inkwell::basic_block::BasicBlock<'ctx>,
    continue_block: inkwell::basic_block::BasicBlock<'ctx>,
}

/// 管理单次代码生成过程中的 LLVM 上下文、模块、符号表和运行时声明。
struct CodeGenerator<'ctx, 'src> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    source: &'src SourceFile,
    runtime: RuntimeFunctions<'ctx>,
    string_type: StructType<'ctx>,
    string_index: usize,
    global_index: usize,
    globals: HashMap<String, VariableInfo<'ctx>>,
    functions: HashMap<String, UserFunctionInfo<'ctx>>,
    current_function: Option<FunctionValue<'ctx>>,
    entry_function: Option<FunctionValue<'ctx>>,
    current_return_type: ValueType,
    scopes: Vec<HashMap<String, VariableInfo<'ctx>>>,
    loop_stack: Vec<LoopContext<'ctx>>,
}

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 初始化模块、目标三元组、运行时函数声明和第二阶段代码生成状态。
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
        let runtime = Self::declare_runtime(context, &module);

        Ok(Self {
            context,
            module,
            builder,
            source,
            runtime,
            string_type,
            string_index: 0,
            global_index: 0,
            globals: HashMap::new(),
            functions: HashMap::new(),
            current_function: None,
            entry_function: None,
            current_return_type: ValueType::Unit,
            scopes: Vec::new(),
            loop_stack: Vec::new(),
        })
    }

    /// 把整个程序编译为入口函数和所有用户函数的 LLVM IR。
    fn compile_program(&mut self, program: &Program) -> Result<(), CompilerError> {
        self.declare_user_functions(program)?;
        self.compile_entry(program)?;

        for statement in &program.statements {
            if let Stmt::FunctionDecl {
                name,
                parameters,
                return_type,
                body,
                span,
            } = statement
            {
                self.compile_user_function(name, parameters, *return_type, body, *span)?;
            }
        }

        Ok(())
    }

    /// 为所有用户函数预先声明 LLVM 符号，支持前向调用和递归调用。
    fn declare_user_functions(&mut self, program: &Program) -> Result<(), CompilerError> {
        for statement in &program.statements {
            let Stmt::FunctionDecl {
                name,
                parameters,
                return_type,
                ..
            } = statement
            else {
                continue;
            };

            let parameter_types = parameters
                .iter()
                .map(|parameter| {
                    self.basic_metadata_type_for(value_type_from_annotation(parameter.type_name))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let function_type = match return_type.map(value_type_from_annotation) {
                Some(value_type) => self
                    .basic_type_for(value_type)?
                    .fn_type(&parameter_types, false),
                None => self.context.void_type().fn_type(&parameter_types, false),
            };
            let function = self
                .module
                .add_function(&format!("ms_user_{name}"), function_type, None);
            self.functions.insert(
                name.clone(),
                UserFunctionInfo {
                    function,
                    return_type: return_type
                        .map(value_type_from_annotation)
                        .unwrap_or(ValueType::Unit),
                },
            );
        }

        Ok(())
    }

    /// 编译顶层入口函数，并在末尾自动调用无参 `main()`。
    fn compile_entry(&mut self, program: &Program) -> Result<(), CompilerError> {
        let entry_function = self
            .module
            .add_function("mascript_entry", self.context.void_type().fn_type(&[], false), None);
        let entry_block = self.context.append_basic_block(entry_function, "entry");
        self.builder.position_at_end(entry_block);

        self.current_function = Some(entry_function);
        self.entry_function = Some(entry_function);
        self.current_return_type = ValueType::Unit;
        self.scopes = vec![HashMap::new()];
        self.loop_stack.clear();

        let terminated = self.compile_statements(&program.statements)?;
        if !terminated {
            self.compile_optional_main_call()?;
            self.map_builder(self.builder.build_return(None))?;
        }

        Ok(())
    }

    /// 编译单个用户函数，并把参数写入局部变量作用域。
    fn compile_user_function(
        &mut self,
        name: &str,
        parameters: &[crate::ast::Parameter],
        return_type: Option<TypeName>,
        body: &[Stmt],
        span: Span,
    ) -> Result<(), CompilerError> {
        let info = self.functions.get(name).cloned().ok_or_else(|| {
            CompilerError::build_message(format!("代码生成阶段找不到函数 `{name}`"), span)
        })?;
        let entry_block = self.context.append_basic_block(info.function, "entry");
        self.builder.position_at_end(entry_block);

        self.current_function = Some(info.function);
        self.current_return_type = return_type
            .map(value_type_from_annotation)
            .unwrap_or(ValueType::Unit);
        self.scopes = vec![HashMap::new()];
        self.loop_stack.clear();

        for (index, parameter) in parameters.iter().enumerate() {
            let value = info.function.get_nth_param(index as u32).ok_or_else(|| {
                CompilerError::build_message(
                    format!("函数 `{name}` 缺少第 {} 个参数值", index + 1),
                    parameter.span,
                )
            })?;
            let value_type = value_type_from_annotation(parameter.type_name);
            let pointer = self.allocate_local(value_type, &parameter.name)?;
            self.map_builder(self.builder.build_store(pointer, value))?;
            self.current_scope_mut().insert(
                parameter.name.clone(),
                VariableInfo {
                    value_type,
                    pointer,
                    is_mutable: true,
                },
            );
        }

        let terminated = self.compile_statements(body)?;
        if !terminated {
            match self.current_return_type {
                ValueType::Unit => {
                    self.map_builder(self.builder.build_return(None))?;
                }
                _ => {
                    return Err(CompilerError::build_message(
                        format!("函数 `{name}` 缺少返回值"),
                        span,
                    ));
                }
            }
        }

        Ok(())
    }

    /// 在入口函数末尾自动插入对无参 `main()` 的调用。
    fn compile_optional_main_call(&mut self) -> Result<(), CompilerError> {
        let Some(main) = self.functions.get("main").cloned() else {
            return Ok(());
        };
        let call = self
            .map_builder(self.builder.build_call(main.function, &[], "call.main"))?;
        if main.return_type != ValueType::Unit {
            let _ = call.try_as_basic_value().unwrap_basic();
        }
        Ok(())
    }

    /// 预声明最小运行时 ABI，供 `print`、字符串拼接和字符串比较直接调用。
    fn declare_runtime(context: &'ctx Context, module: &Module<'ctx>) -> RuntimeFunctions<'ctx> {
        let void_type = context.void_type();
        let i64_type = context.i64_type();
        let i8_type = context.i8_type();
        let f64_type = context.f64_type();
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
            string_equals: module.add_function(
                "string_equals",
                i8_type.fn_type(
                    &[
                        ptr_type.into(),
                        i64_type.into(),
                        ptr_type.into(),
                        i64_type.into(),
                    ],
                    false,
                ),
                None,
            ),
        }
    }

    /// 顺序编译一个语句数组，并返回当前控制流是否已经终止。
    fn compile_statements(&mut self, statements: &[Stmt]) -> Result<bool, CompilerError> {
        let mut terminated = false;
        for statement in statements {
            if terminated {
                break;
            }
            terminated = self.compile_statement(statement)?;
        }
        Ok(terminated)
    }

    /// 编译单条语句，并返回当前基本块是否已经结束。
    fn compile_statement(&mut self, statement: &Stmt) -> Result<bool, CompilerError> {
        match statement {
            Stmt::FunctionDecl { .. } => Ok(false),
            Stmt::VarDecl {
                binding_kind,
                name,
                annotation,
                initializer,
                span,
            } => {
                let compiled = self.compile_expr(initializer)?;
                let value = self.expect_value(compiled)?;
                let expected = annotation
                    .as_ref()
                    .map(|annotation| value_type_from_annotation(*annotation))
                    .unwrap_or(value.value_type);
                if expected != value.value_type {
                    return Err(CompilerError::build_message(
                        format!("变量 `{name}` 的初始化值无法按 `{}` 落地", expected.display_name()),
                        *span,
                    ));
                }

                if self.is_top_level_scope() {
                    let variable =
                        self.declare_global(name, expected, binding_kind.is_mutable())?;
                    self.store_variable(variable, value.value)?;
                } else {
                    let pointer = self.allocate_local(expected, name)?;
                    self.map_builder(self.builder.build_store(pointer, value.value))?;
                    self.current_scope_mut().insert(
                        name.clone(),
                        VariableInfo {
                            value_type: expected,
                            pointer,
                            is_mutable: binding_kind.is_mutable(),
                        },
                    );
                }

                Ok(false)
            }
            Stmt::Assign {
                name,
                op,
                value,
                span,
            } => {
                let variable = self.lookup_variable(name).ok_or_else(|| {
                    CompilerError::build_message(format!("代码生成阶段找不到变量 `{name}`"), *span)
                })?;
                if !variable.is_mutable {
                    return Err(CompilerError::build_message(
                        format!("常量 `{name}` 不能重新赋值"),
                        *span,
                    ));
                }

                let assigned = match op {
                    AssignOp::Assign => {
                        let compiled = self.compile_expr(value)?;
                        self.expect_value(compiled)?
                    }
                    _ => {
                        let current = self.load_variable(variable)?;
                        let compiled = self.compile_expr(value)?;
                        let right = self.expect_value(compiled)?;
                        let binary_op = match op {
                            AssignOp::AddAssign => BinaryOp::Add,
                            AssignOp::SubtractAssign => BinaryOp::Subtract,
                            AssignOp::MultiplyAssign => BinaryOp::Multiply,
                            AssignOp::DivideAssign => BinaryOp::Divide,
                            AssignOp::ModAssign => BinaryOp::Modulo,
                            AssignOp::Assign => unreachable!(),
                        };
                        self.compile_binary_values(binary_op, current, right, *span)?
                    }
                };

                if assigned.value_type != variable.value_type {
                    return Err(CompilerError::build_message("赋值类型无法写入目标变量", *span));
                }
                self.store_variable(variable, assigned.value)?;
                Ok(false)
            }
            Stmt::If {
                branches,
                else_branch,
                ..
            } => self.compile_if(branches, else_branch.as_ref()),
            Stmt::While {
                condition, body, ..
            } => self.compile_while(condition, body),
            Stmt::ForRange {
                name,
                start,
                end,
                body,
                span,
            } => self.compile_for_range(name, start, end, body, *span),
            Stmt::Break { span } => {
                let loop_context = self.loop_stack.last().ok_or_else(|| {
                    CompilerError::build_message("代码生成阶段遇到了非法的 break", *span)
                })?;
                self.map_builder(
                    self.builder
                        .build_unconditional_branch(loop_context.break_block),
                )?;
                Ok(true)
            }
            Stmt::Continue { span } => {
                let loop_context = self.loop_stack.last().ok_or_else(|| {
                    CompilerError::build_message("代码生成阶段遇到了非法的 continue", *span)
                })?;
                self.map_builder(
                    self.builder
                        .build_unconditional_branch(loop_context.continue_block),
                )?;
                Ok(true)
            }
            Stmt::Return { value, span } => {
                self.compile_return(value.as_ref(), *span)?;
                Ok(true)
            }
            Stmt::ExprStmt { expr, .. } => {
                self.compile_expr(expr)?;
                Ok(false)
            }
        }
    }

    /// 编译 `return` 语句，并对 `unit`/非 `unit` 返回值执行一致性检查。
    fn compile_return(&mut self, value: Option<&Expr>, span: Span) -> Result<(), CompilerError> {
        match (self.current_return_type, value) {
            (ValueType::Unit, None) => self.map_builder(self.builder.build_return(None)).map(|_| ()),
            (ValueType::Unit, Some(_)) => Err(CompilerError::build_message(
                "当前函数没有返回值，return 后不能跟表达式",
                span,
            )),
            (_, None) => Err(CompilerError::build_message(
                "当前函数声明了返回值，return 后必须跟表达式",
                span,
            )),
            (expected, Some(expr)) => {
                let compiled = self.compile_expr(expr)?;
                let value = self.expect_value(compiled)?;
                if value.value_type != expected {
                    return Err(CompilerError::build_message("返回值类型不匹配", span));
                }
                self.map_builder(self.builder.build_return(Some(&value.value)))?;
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
    fn compile_identifier(
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

    /// 编译 `if / elif / else` 控制流，并维护最终汇合块。
    fn compile_if(
        &mut self,
        branches: &[ConditionalBranch],
        else_branch: Option<&Vec<Stmt>>,
    ) -> Result<bool, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let after_block = self.context.append_basic_block(function, "if.after");
        let mut next_test_block = None;
        let mut has_fallthrough = else_branch.is_none();

        for (index, branch) in branches.iter().enumerate() {
            if let Some(block) = next_test_block.take() {
                self.builder.position_at_end(block);
            }

            let compiled = self.compile_expr(&branch.condition)?;
            let condition = self.expect_value(compiled)?;
            let then_block = self.context.append_basic_block(function, "if.then");
            let false_block = if index + 1 < branches.len() || else_branch.is_some() {
                let block = self.context.append_basic_block(function, "if.next");
                next_test_block = Some(block);
                block
            } else {
                after_block
            };

            self.map_builder(self.builder.build_conditional_branch(
                condition.value.into_int_value(),
                then_block,
                false_block,
            ))?;
            self.builder.position_at_end(then_block);
            self.push_scope();
            let terminated = self.compile_statements(&branch.body)?;
            self.pop_scope();
            if !terminated && !self.current_block_has_terminator() {
                self.map_builder(self.builder.build_unconditional_branch(after_block))?;
                has_fallthrough = true;
            }
        }

        if let Some(else_branch) = else_branch {
            let else_block = next_test_block.expect("存在 else 时必须提前创建 else 块");
            self.builder.position_at_end(else_block);
            self.push_scope();
            let terminated = self.compile_statements(else_branch)?;
            self.pop_scope();
            if !terminated && !self.current_block_has_terminator() {
                self.map_builder(self.builder.build_unconditional_branch(after_block))?;
                has_fallthrough = true;
            }
        }

        if has_fallthrough {
            self.builder.position_at_end(after_block);
            Ok(false)
        } else {
            Ok(true)
        }
    }

    /// 编译 `while` 循环，并设置 `break/continue` 的目标块。
    fn compile_while(&mut self, condition: &Expr, body: &[Stmt]) -> Result<bool, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let header_block = self.context.append_basic_block(function, "while.header");
        let body_block = self.context.append_basic_block(function, "while.body");
        let exit_block = self.context.append_basic_block(function, "while.exit");

        self.map_builder(self.builder.build_unconditional_branch(header_block))?;
        self.builder.position_at_end(header_block);
        let compiled = self.compile_expr(condition)?;
        let condition = self.expect_value(compiled)?;
        self.map_builder(self.builder.build_conditional_branch(
            condition.value.into_int_value(),
            body_block,
            exit_block,
        ))?;

        self.builder.position_at_end(body_block);
        self.loop_stack.push(LoopContext {
            break_block: exit_block,
            continue_block: header_block,
        });
        self.push_scope();
        let terminated = self.compile_statements(body)?;
        self.pop_scope();
        self.loop_stack.pop();
        if !terminated && !self.current_block_has_terminator() {
            self.map_builder(self.builder.build_unconditional_branch(header_block))?;
        }

        self.builder.position_at_end(exit_block);
        Ok(false)
    }

    /// 编译整数半开区间 `for` 循环。
    fn compile_for_range(
        &mut self,
        name: &str,
        start: &Expr,
        end: &Expr,
        body: &[Stmt],
        span: Span,
    ) -> Result<bool, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let compiled_start = self.compile_expr(start)?;
        let start_value = self.expect_value(compiled_start)?;
        let compiled_end = self.compile_expr(end)?;
        let end_value = self.expect_value(compiled_end)?;
        if start_value.value_type != ValueType::Int || end_value.value_type != ValueType::Int {
            return Err(CompilerError::build_message("for 范围只支持整数边界", span));
        }

        self.push_scope();
        let iter_pointer = self.allocate_local(ValueType::Int, &format!("{name}.iter"))?;
        let end_pointer = self.allocate_local(ValueType::Int, &format!("{name}.end"))?;
        self.map_builder(self.builder.build_store(iter_pointer, start_value.value))?;
        self.map_builder(self.builder.build_store(end_pointer, end_value.value))?;
        self.current_scope_mut().insert(
            name.to_string(),
            VariableInfo {
                value_type: ValueType::Int,
                pointer: iter_pointer,
                is_mutable: true,
            },
        );

        let header_block = self.context.append_basic_block(function, "for.header");
        let body_block = self.context.append_basic_block(function, "for.body");
        let continue_block = self.context.append_basic_block(function, "for.continue");
        let exit_block = self.context.append_basic_block(function, "for.exit");

        self.map_builder(self.builder.build_unconditional_branch(header_block))?;
        self.builder.position_at_end(header_block);
        let current = self
            .map_builder(self.builder.build_load(self.context.i64_type(), iter_pointer, "for.current"))?
            .into_int_value();
        let end_loaded = self
            .map_builder(self.builder.build_load(self.context.i64_type(), end_pointer, "for.end"))?
            .into_int_value();
        let condition = self.map_builder(self.builder.build_int_compare(
            IntPredicate::SLT,
            current,
            end_loaded,
            "for.cond",
        ))?;
        self.map_builder(self.builder.build_conditional_branch(
            condition,
            body_block,
            exit_block,
        ))?;

        self.builder.position_at_end(body_block);
        self.loop_stack.push(LoopContext {
            break_block: exit_block,
            continue_block,
        });
        let terminated = self.compile_statements(body)?;
        self.loop_stack.pop();
        if !terminated && !self.current_block_has_terminator() {
            self.map_builder(self.builder.build_unconditional_branch(continue_block))?;
        }

        self.builder.position_at_end(continue_block);
        let iter_now = self
            .map_builder(self.builder.build_load(self.context.i64_type(), iter_pointer, "for.next.load"))?
            .into_int_value();
        let next = self.map_builder(self.builder.build_int_add(
            iter_now,
            self.context.i64_type().const_int(1, false),
            "for.next",
        ))?;
        self.map_builder(self.builder.build_store(iter_pointer, next))?;
        self.map_builder(self.builder.build_unconditional_branch(header_block))?;

        self.builder.position_at_end(exit_block);
        self.pop_scope();
        Ok(false)
    }

    /// 编译二元表达式，并根据静态类型规则选择对应的 LLVM 指令。
    fn compile_binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.compile_short_circuit(op, left, right, span);
        }

        let compiled_left = self.compile_expr(left)?;
        let left_value = self.expect_value(compiled_left)?;
        let compiled_right = self.compile_expr(right)?;
        let right_value = self.expect_value(compiled_right)?;
        let result = self.compile_binary_values(op, left_value, right_value, span)?;
        Ok(CodegenValue::Value(result))
    }

    /// 在已知左右操作数的前提下编译二元表达式值。
    fn compile_binary_values(
        &mut self,
        op: BinaryOp,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        match op {
            BinaryOp::Add => self.compile_add(left, right, span),
            BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => {
                self.compile_numeric_binary(left, right, op, span)
            }
            BinaryOp::Modulo | BinaryOp::IntDivide => {
                self.compile_integer_binary(left, right, op, span)
            }
            BinaryOp::Equal | BinaryOp::NotEqual => self.compile_equality(left, right, op, span),
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                self.compile_order_compare(left, right, op, span)
            }
            BinaryOp::And | BinaryOp::Or => unreachable!("短路逻辑在外层单独处理"),
        }
    }

    /// 编译 `and/or` 的短路求值逻辑。
    fn compile_short_circuit(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let compiled_left = self.compile_expr(left)?;
        let left_value = self.expect_value(compiled_left)?;
        if left_value.value_type != ValueType::Bool {
            return Err(CompilerError::build_message("短路逻辑只支持布尔值", span));
        }

        let slot = self.allocate_local(ValueType::Bool, "logic.tmp")?;
        self.map_builder(self.builder.build_store(slot, left_value.value))?;

        let rhs_block = self.context.append_basic_block(function, "logic.rhs");
        let merge_block = self.context.append_basic_block(function, "logic.merge");
        match op {
            BinaryOp::And => self.map_builder(self.builder.build_conditional_branch(
                left_value.value.into_int_value(),
                rhs_block,
                merge_block,
            ))?,
            BinaryOp::Or => self.map_builder(self.builder.build_conditional_branch(
                left_value.value.into_int_value(),
                merge_block,
                rhs_block,
            ))?,
            _ => unreachable!(),
        };

        self.builder.position_at_end(rhs_block);
        let compiled_right = self.compile_expr(right)?;
        let right_value = self.expect_value(compiled_right)?;
        if right_value.value_type != ValueType::Bool {
            return Err(CompilerError::build_message("短路逻辑只支持布尔值", span));
        }
        self.map_builder(self.builder.build_store(slot, right_value.value))?;
        self.map_builder(self.builder.build_unconditional_branch(merge_block))?;

        self.builder.position_at_end(merge_block);
        let result = self
            .map_builder(self.builder.build_load(self.context.bool_type(), slot, "logic.result"))?;
        Ok(CodegenValue::Value(TypedValue {
            value_type: ValueType::Bool,
            value: result,
        }))
    }

    /// 编译加法表达式，额外处理字符串拼接分支。
    fn compile_add(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if left.value_type == ValueType::String && right.value_type == ValueType::String {
            let left_string = left.value.into_struct_value();
            let right_string = right.value.into_struct_value();
            let left_ptr = self
                .extract_string_part(left_string, 0, "left.ptr")?
                .into_pointer_value();
            let left_len = self
                .extract_string_part(left_string, 1, "left.len")?
                .into_int_value();
            let right_ptr = self
                .extract_string_part(right_string, 0, "right.ptr")?
                .into_pointer_value();
            let right_len = self
                .extract_string_part(right_string, 1, "right.len")?
                .into_int_value();
            let out_len_ptr = self.allocate_local(ValueType::Int, "concat.out_len")?;
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

        self.compile_numeric_binary(left, right, BinaryOp::Add, span)
    }

    /// 编译数值二元运算，并在必要时把整数提升为浮点数。
    fn compile_numeric_binary(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        match op {
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply => {
                if left.value_type == ValueType::Int && right.value_type == ValueType::Int {
                    let value = match op {
                        BinaryOp::Add => self.map_builder(self.builder.build_int_add(
                            left.value.into_int_value(),
                            right.value.into_int_value(),
                            "add.int",
                        ))?.into(),
                        BinaryOp::Subtract => self.map_builder(self.builder.build_int_sub(
                            left.value.into_int_value(),
                            right.value.into_int_value(),
                            "sub.int",
                        ))?.into(),
                        BinaryOp::Multiply => self.map_builder(self.builder.build_int_mul(
                            left.value.into_int_value(),
                            right.value.into_int_value(),
                            "mul.int",
                        ))?.into(),
                        _ => unreachable!(),
                    };
                    return Ok(TypedValue {
                        value_type: ValueType::Int,
                        value,
                    });
                }
            }
            BinaryOp::Divide => {}
            _ => unreachable!("只有数值二元运算会进入该分支"),
        }

        let (left_float, right_float) = self.coerce_to_float_pair(left, right, span)?;
        let value = match op {
            BinaryOp::Add => self
                .map_builder(self.builder.build_float_add(left_float, right_float, "add.float"))?
                .into(),
            BinaryOp::Subtract => self
                .map_builder(self.builder.build_float_sub(left_float, right_float, "sub.float"))?
                .into(),
            BinaryOp::Multiply => self
                .map_builder(self.builder.build_float_mul(left_float, right_float, "mul.float"))?
                .into(),
            BinaryOp::Divide => self
                .map_builder(self.builder.build_float_div(left_float, right_float, "div.float"))?
                .into(),
            _ => unreachable!(),
        };
        Ok(TypedValue {
            value_type: ValueType::Float,
            value,
        })
    }

    /// 编译只接受整数的取模与整数除法表达式。
    fn compile_integer_binary(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if left.value_type != ValueType::Int || right.value_type != ValueType::Int {
            return Err(CompilerError::build_message("取模和整数除法只支持整数", span));
        }
        let left = left.value.into_int_value();
        let right = right.value.into_int_value();
        let value = match op {
            BinaryOp::Modulo => {
                self.map_builder(self.builder.build_int_signed_rem(left, right, "mod.int"))?
            }
            BinaryOp::IntDivide => {
                self.map_builder(self.builder.build_int_signed_div(left, right, "intdiv.int"))?
            }
            _ => unreachable!(),
        };
        Ok(TypedValue {
            value_type: ValueType::Int,
            value: value.into(),
        })
    }

    /// 编译相等比较表达式。
    fn compile_equality(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let value = match (left.value_type, right.value_type) {
            (ValueType::Int, ValueType::Int) | (ValueType::Bool, ValueType::Bool) => self
                .map_builder(self.builder.build_int_compare(
                    if op == BinaryOp::Equal {
                        IntPredicate::EQ
                    } else {
                        IntPredicate::NE
                    },
                    left.value.into_int_value(),
                    right.value.into_int_value(),
                    "cmp.int",
                ))?
                .into(),
            (ValueType::String, ValueType::String) => {
                let left = left.value.into_struct_value();
                let right = right.value.into_struct_value();
                let call = self.map_builder(self.builder.build_call(
                    self.runtime.string_equals,
                    &[
                        self.extract_string_part(left, 0, "eq.left.ptr")?.into(),
                        self.extract_string_part(left, 1, "eq.left.len")?.into(),
                        self.extract_string_part(right, 0, "eq.right.ptr")?.into(),
                        self.extract_string_part(right, 1, "eq.right.len")?.into(),
                    ],
                    "cmp.string",
                ))?;
                let result = call
                    .try_as_basic_value()
                    .unwrap_basic()
                    .into_int_value();
                self.map_builder(self.builder.build_int_compare(
                    if op == BinaryOp::Equal {
                        IntPredicate::NE
                    } else {
                        IntPredicate::EQ
                    },
                    result,
                    self.context.i8_type().const_zero(),
                    "cmp.string.bool",
                ))?
                .into()
            }
            _ => {
                let (left_float, right_float) = self.coerce_to_float_pair(left, right, span)?;
                self.map_builder(self.builder.build_float_compare(
                    if op == BinaryOp::Equal {
                        FloatPredicate::OEQ
                    } else {
                        FloatPredicate::ONE
                    },
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
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let value = if left.value_type == ValueType::Int && right.value_type == ValueType::Int {
            self.map_builder(self.builder.build_int_compare(
                match op {
                    BinaryOp::Less => IntPredicate::SLT,
                    BinaryOp::LessEqual => IntPredicate::SLE,
                    BinaryOp::Greater => IntPredicate::SGT,
                    BinaryOp::GreaterEqual => IntPredicate::SGE,
                    _ => unreachable!(),
                },
                left.value.into_int_value(),
                right.value.into_int_value(),
                "cmp.order.int",
            ))?
            .into()
        } else {
            let (left_float, right_float) = self.coerce_to_float_pair(left, right, span)?;
            self.map_builder(self.builder.build_float_compare(
                match op {
                    BinaryOp::Less => FloatPredicate::OLT,
                    BinaryOp::LessEqual => FloatPredicate::OLE,
                    BinaryOp::Greater => FloatPredicate::OGT,
                    BinaryOp::GreaterEqual => FloatPredicate::OGE,
                    _ => unreachable!(),
                },
                left_float,
                right_float,
                "cmp.order.float",
            ))?
            .into()
        };

        Ok(TypedValue {
            value_type: ValueType::Bool,
            value,
        })
    }

    /// 编译内建函数与用户函数调用。
    fn compile_call(
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
            let value = call
                .try_as_basic_value()
                .unwrap_basic();
            Ok(CodegenValue::Value(TypedValue {
                value_type: info.return_type,
                value,
            }))
        }
    }

    /// 把两个数值表达式统一提升为浮点值。
    fn coerce_to_float_pair(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        span: Span,
    ) -> Result<(FloatValue<'ctx>, FloatValue<'ctx>), CompilerError> {
        Ok((self.to_float(left, span)?, self.to_float(right, span)?))
    }

    /// 把单个整数或浮点值转换为浮点值。
    fn to_float(
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

    /// 为新的全局变量创建可寻址的 LLVM 全局对象。
    fn declare_global(
        &mut self,
        name: &str,
        value_type: ValueType,
        is_mutable: bool,
    ) -> Result<VariableInfo<'ctx>, CompilerError> {
        if let Some(variable) = self.globals.get(name).copied() {
            return Ok(variable);
        }

        let symbol = format!("__ms_global_{}_{}", name, self.global_index);
        self.global_index += 1;
        let global = self.module.add_global(self.basic_type_for(value_type)?, None, &symbol);
        self.initialize_global(&global, value_type);
        let variable = VariableInfo {
            value_type,
            pointer: global.as_pointer_value(),
            is_mutable,
        };
        self.globals.insert(name.to_string(), variable);
        Ok(variable)
    }

    /// 为全局变量写入零值初始化器，确保在入口执行前内存布局稳定。
    fn initialize_global(&self, global: &GlobalValue<'ctx>, value_type: ValueType) {
        match value_type {
            ValueType::Int => global.set_initializer(&self.context.i64_type().const_zero()),
            ValueType::Float => global.set_initializer(&self.context.f64_type().const_zero()),
            ValueType::Bool => global.set_initializer(&self.context.bool_type().const_zero()),
            ValueType::String => global.set_initializer(&self.string_type.const_zero()),
            ValueType::Unit => {}
        }
    }

    /// 为局部变量创建栈内存槽位。
    fn allocate_local(
        &self,
        value_type: ValueType,
        name: &str,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        self.map_builder(
            self.builder
                .build_alloca(self.basic_type_for(value_type)?, name),
        )
    }

    /// 从变量指针中加载值。
    fn load_variable(&self, variable: VariableInfo<'ctx>) -> Result<TypedValue<'ctx>, CompilerError> {
        let value = self.map_builder(self.builder.build_load(
            self.basic_type_for(variable.value_type)?,
            variable.pointer,
            "load.var",
        ))?;
        Ok(TypedValue {
            value_type: variable.value_type,
            value,
        })
    }

    /// 把 LLVM 值写回变量指针。
    fn store_variable(
        &self,
        variable: VariableInfo<'ctx>,
        value: BasicValueEnum<'ctx>,
    ) -> Result<(), CompilerError> {
        self.map_builder(self.builder.build_store(variable.pointer, value))?;
        Ok(())
    }

    /// 返回指定语义类型在 LLVM 中对应的基础类型对象。
    fn basic_type_for(&self, value_type: ValueType) -> Result<BasicTypeEnum<'ctx>, CompilerError> {
        Ok(match value_type {
            ValueType::Int => self.context.i64_type().into(),
            ValueType::Float => self.context.f64_type().into(),
            ValueType::Bool => self.context.bool_type().into(),
            ValueType::String => self.string_type.into(),
            ValueType::Unit => {
                return Err(CompilerError::build_message(
                    "unit 不应该被映射为可存储的基础类型",
                    Span::new(0, self.source.text().len()),
                ))
            }
        })
    }

    /// 把基础类型转换为函数签名所需的元数据类型。
    fn basic_metadata_type_for(
        &self,
        value_type: ValueType,
    ) -> Result<BasicMetadataTypeEnum<'ctx>, CompilerError> {
        Ok(self.basic_type_for(value_type)?.into())
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
            CodegenValue::Unit => Err(CompilerError::build_message(
                "表达式返回了不可参与计算的 unit 值",
                Span::new(0, self.source.text().len()),
            )),
        }
    }

    /// 判断当前正在生成的语句是否位于顶层入口作用域。
    fn is_top_level_scope(&self) -> bool {
        self.current_function.is_some()
            && self.current_function == self.entry_function
            && self.scopes.len() == 1
    }

    /// 在局部作用域链和全局变量表中查找变量。
    fn lookup_variable(&self, name: &str) -> Option<VariableInfo<'ctx>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
            .or_else(|| self.globals.get(name).copied())
    }

    /// 压入一个新的块级作用域。
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// 弹出当前块级作用域。
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// 返回当前最内层作用域的可变引用。
    fn current_scope_mut(&mut self) -> &mut HashMap<String, VariableInfo<'ctx>> {
        self.scopes
            .last_mut()
            .expect("作用域栈至少包含一个当前作用域")
    }

    /// 判断当前插入块是否已经包含终结指令。
    fn current_block_has_terminator(&self) -> bool {
        self.builder
            .get_insert_block()
            .and_then(|block| block.get_terminator())
            .is_some()
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
}

/// 把语法树中的类型标注转换为代码生成阶段复用的静态类型。
fn value_type_from_annotation(annotation: TypeName) -> ValueType {
    match annotation {
        TypeName::Int => ValueType::Int,
        TypeName::Float => ValueType::Float,
        TypeName::Bool => ValueType::Bool,
        TypeName::String => ValueType::String,
    }
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
    fn parse_program(text: &str) -> (SourceFile, crate::ast::Program, CompilerSettings) {
        let settings = CompilerSettings::new(UiLanguage::ZhCn);
        let source = SourceFile::new("<test>", text.to_string());
        let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
        let program = Parser::new(tokens, &settings).parse_program().unwrap();
        (source, program, settings)
    }

    /// 校验 LLVM 后端能够生成非空目标文件。
    #[test]
    fn should_emit_object_bytes() {
        let (source, program, settings) = parse_program("print(1)\n");
        let object = compile_program(&source, &settings, &program).unwrap();
        assert!(!object.object_bytes.is_empty());
    }

    /// 校验 `print` 调用会绑定到正确的运行时符号。
    #[test]
    fn should_emit_print_runtime_symbol() {
        let (source, program, _) = parse_program("print(1)\n");
        let ir = compile_program_to_ir(&source, &program).unwrap();
        assert!(ir.contains("print_int"));
    }

    /// 校验字符串拼接路径会引用 `string_concat` 运行时符号。
    #[test]
    fn should_emit_string_concat_runtime_symbol() {
        let (source, program, _) = parse_program("print(\"a\" + \"b\")\n");
        let ir = compile_program_to_ir(&source, &program).unwrap();
        assert!(ir.contains("string_concat"));
    }

    /// 校验字符串相等比较路径会引用 `string_equals` 运行时符号。
    #[test]
    fn should_emit_string_equals_runtime_symbol() {
        let (source, program, _) = parse_program("print(\"a\" == \"b\")\n");
        let ir = compile_program_to_ir(&source, &program).unwrap();
        assert!(ir.contains("string_equals"));
    }
}
