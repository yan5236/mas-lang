use std::collections::HashMap;

use inkwell::types::BasicType;

use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 为所有用户函数预先声明 LLVM 符号，支持前向调用和递归调用。
    pub(super) fn declare_user_functions(
        &mut self,
        program: &Program,
    ) -> Result<(), CompilerError> {
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
                    self.basic_metadata_type_for(&value_type_from_annotation(&parameter.type_name))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let function_type = match return_type.as_ref().map(value_type_from_annotation) {
                Some(value_type) => self
                    .basic_type_for(&value_type)?
                    .fn_type(&parameter_types, false),
                None => self.context.void_type().fn_type(&parameter_types, false),
            };
            let function =
                self.module
                    .add_function(&format!("ms_user_{name}"), function_type, None);
            self.functions.insert(
                name.clone(),
                UserFunctionInfo {
                    function,
                    return_type: return_type
                        .as_ref()
                        .map(value_type_from_annotation)
                        .unwrap_or(ValueType::Unit),
                },
            );
        }

        Ok(())
    }

    /// 编译顶层入口函数，并在末尾自动调用无参 `main()`。
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

        let terminated = self.compile_statements(&program.statements)?;
        if !terminated {
            self.compile_optional_main_call()?;
            self.map_builder(self.builder.build_return(None))?;
        }

        Ok(())
    }

    /// 编译单个用户函数，并把参数写入局部变量作用域。
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
