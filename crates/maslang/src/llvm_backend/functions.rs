use std::collections::HashMap;

use inkwell::AddressSpace;
use inkwell::types::BasicType;

use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 为用户函数和原生函数声明预先分配 LLVM 符号，支持前向调用与递归调用。
    /// 用户函数只在当前整程序模块内部调用，标为 internal 允许跨函数常量传播；extern 与入口仍对外可见。
    pub(super) fn declare_user_functions(
        &mut self,
        program: &Program,
    ) -> Result<(), CompilerError> {
        for statement in &program.statements {
            let (name, parameters, return_type, abi) = match statement {
                Stmt::FunctionDecl {
                    name,
                    parameters,
                    return_type,
                    ..
                } => (name, parameters, return_type, FunctionAbi::User),
                Stmt::ExternFunctionDecl {
                    name,
                    parameters,
                    return_type,
                    ..
                } => (name, parameters, return_type, FunctionAbi::Extern),
                _ => continue,
            };

            let parameter_types = parameters
                .iter()
                .map(|parameter| {
                    let value_type = value_type_from_annotation(&parameter.type_name);
                    match abi {
                        FunctionAbi::User => self.basic_metadata_type_for(&value_type),
                        FunctionAbi::Extern => self.extern_metadata_type_for(&value_type),
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            let function_type = match return_type.as_ref().map(value_type_from_annotation) {
                Some(ValueType::String) if abi == FunctionAbi::Extern => {
                    let mut abi_parameters = parameter_types;
                    abi_parameters.push(self.context.ptr_type(AddressSpace::default()).into());
                    self.context
                        .ptr_type(AddressSpace::default())
                        .fn_type(&abi_parameters, false)
                }
                Some(value_type) => match abi {
                    FunctionAbi::User => self.basic_type_for(&value_type)?,
                    FunctionAbi::Extern => self.extern_basic_type_for(&value_type)?,
                }
                .fn_type(&parameter_types, false),
                None => self.context.void_type().fn_type(&parameter_types, false),
            };
            let symbol_name = match abi {
                FunctionAbi::User => format!("ms_user_{name}"),
                FunctionAbi::Extern => name.clone(),
            };
            let linkage = (abi == FunctionAbi::User).then_some(inkwell::module::Linkage::Internal);
            let function = self
                .module
                .add_function(&symbol_name, function_type, linkage);
            self.functions.insert(
                name.clone(),
                FunctionInfo {
                    parameter_types: parameters
                        .iter()
                        .map(|p| value_type_from_annotation(&p.type_name))
                        .collect(),
                    function,
                    return_type: return_type
                        .as_ref()
                        .map(value_type_from_annotation)
                        .unwrap_or(ValueType::Unit),
                    abi,
                },
            );
        }

        Ok(())
    }

    /// 编译顶层入口与自动 main 调用，并为全局和局部托管值安装 GC 根帧。
    pub(super) fn compile_entry(&mut self, program: &Program) -> Result<(), CompilerError> {
        let entry_function = self.module.add_function(
            "mascript_entry",
            self.context.void_type().fn_type(&[], false),
            None,
        );
        let entry_block = self.context.append_basic_block(entry_function, "entry");
        self.builder.position_at_end(entry_block);

        self.current_function = Some(entry_function);
        self.entry_function = Some(entry_function);
        self.current_return_type = ValueType::Unit;
        self.scopes = vec![HashMap::new()];
        self.loop_stack.clear();
        self.gc_roots.borrow_mut().clear();
        self.gc_handle_roots.borrow_mut().clear();
        self.string_builders.clear();
        self.borrowed_conversion = None;
        self.borrowed_slots.clear();
        self.borrowed_values.clear();
        self.gc_temporaries.borrow_mut().clear();

        let terminated = self.compile_statements(&program.statements)?;
        if !terminated {
            self.compile_optional_main_call()?;
            self.map_builder(self.builder.build_return(None))?;
        }

        self.finish_gc_frame()?;
        Ok(())
    }

    /// 编译用户函数并将参数写入根槽；所有返回路径在交还结果前移除当前 GC 帧。
    pub(super) fn compile_user_function(
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
            .as_ref()
            .map(value_type_from_annotation)
            .unwrap_or(ValueType::Unit);
        self.scopes = vec![HashMap::new()];
        self.loop_stack.clear();
        self.gc_roots.borrow_mut().clear();
        self.gc_handle_roots.borrow_mut().clear();
        self.string_builders.clear();
        self.borrowed_conversion = None;
        self.borrowed_slots.clear();
        self.borrowed_values.clear();
        self.gc_temporaries.borrow_mut().clear();

        for (index, parameter) in parameters.iter().enumerate() {
            let value = info.function.get_nth_param(index as u32).ok_or_else(|| {
                CompilerError::build_message(
                    format!("函数 `{name}` 缺少第 {} 个参数值", index + 1),
                    parameter.span,
                )
            })?;
            let value_type = value_type_from_annotation(&parameter.type_name);
            let pointer = self.allocate_local(&value_type, &parameter.name)?;
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
            match self.current_return_type.clone() {
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

        self.finish_gc_frame()?;
        Ok(())
    }

    /// 在入口函数末尾自动插入对无参 `main()` 的调用。
    pub(super) fn compile_optional_main_call(&mut self) -> Result<(), CompilerError> {
        let Some(main) = self.functions.get("main").cloned() else {
            return Ok(());
        };
        let call = self.map_builder(self.builder.build_call(main.function, &[], "call.main"))?;
        if main.return_type != ValueType::Unit {
            let _ = call.try_as_basic_value().unwrap_basic();
        }
        Ok(())
    }
}
