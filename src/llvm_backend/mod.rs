use std::collections::HashMap;

use inkwell::AddressSpace;
use inkwell::OptimizationLevel;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::types::StructType;
use inkwell::values::{BasicValueEnum, FunctionValue, PointerValue, StructValue};

use crate::ast::{Program, Stmt, TypeName};
use crate::diagnostic::{CompilerError, MessageId};
use crate::semantic::ValueType;
use crate::settings::CompilerSettings;
use crate::source::{SourceFile, Span};

mod expressions;
mod functions;
mod gc;
mod global_state;
mod numeric_safety;
mod operations;
mod output;
mod runtime;
mod statements;
mod storage;

#[cfg(test)]
mod tests;

/// 保存 LLVM 后端生成的目标文件字节。
pub struct GeneratedObject {
    pub object_bytes: Vec<u8>,
}

/// 保存单个变量的类型、指针位置和可变性信息。
#[derive(Clone)]
struct VariableInfo<'ctx> {
    value_type: ValueType,
    pointer: PointerValue<'ctx>,
    is_mutable: bool,
}

/// 保存表达式在 LLVM 中的值和静态类型。
#[derive(Clone)]
struct TypedValue<'ctx> {
    value_type: ValueType,
    value: BasicValueEnum<'ctx>,
}

/// 表示表达式代码生成结果是否为可参与后续计算的值。
enum CodegenValue<'ctx> {
    Value(TypedValue<'ctx>),
    Unit,
}

/// 保存用户函数的 LLVM 符号与返回类型信息。
#[derive(Clone)]
struct FunctionInfo<'ctx> {
    function: FunctionValue<'ctx>,
    return_type: ValueType,
    abi: FunctionAbi,
}

/// 区分用户函数与原生函数声明的 ABI 处理路径。
#[derive(Clone, Copy, Eq, PartialEq)]
enum FunctionAbi {
    User,
    Extern,
}

/// 保存运行时 ABI 中需要调用的外部函数。
struct RuntimeFunctions<'ctx> {
    print_int: FunctionValue<'ctx>,
    print_float: FunctionValue<'ctx>,
    print_bool: FunctionValue<'ctx>,
    print_string: FunctionValue<'ctx>,
    string_to_int: FunctionValue<'ctx>,
    string_to_float: FunctionValue<'ctx>,
    try_string_to_int: FunctionValue<'ctx>,
    try_string_to_float: FunctionValue<'ctx>,
    try_string_to_bool: FunctionValue<'ctx>,
    int_to_string: FunctionValue<'ctx>,
    float_to_string: FunctionValue<'ctx>,
    bool_to_string: FunctionValue<'ctx>,
    string_concat: FunctionValue<'ctx>,
    string_equals: FunctionValue<'ctx>,
    string_trim: FunctionValue<'ctx>,
    string_contains: FunctionValue<'ctx>,
    string_starts_with: FunctionValue<'ctx>,
    string_ends_with: FunctionValue<'ctx>,
    string_replace: FunctionValue<'ctx>,
    list_create: FunctionValue<'ctx>,
    list_len: FunctionValue<'ctx>,
    list_slice: FunctionValue<'ctx>,
    list_reverse: FunctionValue<'ctx>,
    map_create: FunctionValue<'ctx>,
    map_len: FunctionValue<'ctx>,
    map_keys: FunctionValue<'ctx>,
    map_values: FunctionValue<'ctx>,
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
    source_path_value: Option<StructValue<'ctx>>,
    source_text_value: Option<StructValue<'ctx>>,
    globals: HashMap<String, VariableInfo<'ctx>>,
    global_initialized: HashMap<String, PointerValue<'ctx>>,
    functions: HashMap<String, FunctionInfo<'ctx>>,
    current_function: Option<FunctionValue<'ctx>>,
    entry_function: Option<FunctionValue<'ctx>>,
    current_return_type: ValueType,
    scopes: Vec<HashMap<String, VariableInfo<'ctx>>>,
    loop_stack: Vec<LoopContext<'ctx>>,
    gc_roots: std::cell::RefCell<Vec<(ValueType, PointerValue<'ctx>)>>,
    gc_temporaries: std::cell::RefCell<Vec<(ValueType, PointerValue<'ctx>)>>,
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

        let target_machine = create_target_machine(source, &TargetMachine::get_default_triple())?;
        let module = context.create_module("mascript");
        module.set_triple(&target_machine.get_triple());
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
            source_path_value: None,
            source_text_value: None,
            globals: HashMap::new(),
            global_initialized: HashMap::new(),
            functions: HashMap::new(),
            current_function: None,
            entry_function: None,
            current_return_type: ValueType::Unit,
            scopes: Vec::new(),
            loop_stack: Vec::new(),
            gc_roots: std::cell::RefCell::new(Vec::new()),
            gc_temporaries: std::cell::RefCell::new(Vec::new()),
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
                ..
            } = statement
            {
                self.compile_user_function(name, parameters, return_type.clone(), body, *span)?;
            }
        }

        Ok(())
    }
}

/// 复用宿主默认配置为指定三元组构造目标机器，避免初始化与输出阶段重复拼装。
fn create_target_machine(
    source: &SourceFile,
    triple: &TargetTriple,
) -> Result<TargetMachine, CompilerError> {
    let target = Target::from_triple(triple).map_err(|error| {
        CompilerError::build(
            MessageId::BuildCreateTargetFailed,
            vec![error.to_string()],
            Span::new(0, source.text().len()),
        )
    })?;

    target
        .create_target_machine(
            triple,
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
        })
}

/// 把语法树中的类型标注转换为代码生成阶段复用的静态类型。
fn value_type_from_annotation(annotation: &TypeName) -> ValueType {
    match annotation {
        TypeName::Int => ValueType::Int,
        TypeName::Float => ValueType::Float,
        TypeName::Bool => ValueType::Bool,
        TypeName::String => ValueType::String,
        TypeName::List(element) => ValueType::List(Box::new(value_type_from_annotation(element))),
        TypeName::Map(key, value) => ValueType::Map(
            Box::new(value_type_from_annotation(key)),
            Box::new(value_type_from_annotation(value)),
        ),
        TypeName::Array(element, size) => {
            ValueType::Array(Box::new(value_type_from_annotation(element)), *size)
        }
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
