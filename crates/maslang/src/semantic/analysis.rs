use crate::ast::{BindingKind, Program, Stmt};
use crate::diagnostic::CompilerError;

use super::{FunctionSignature, SemanticAnalyzer, SymbolInfo, ValueType};

impl<'a> SemanticAnalyzer<'a> {
    /// 校验整棵程序语法树中的声明、控制流与类型规则。
    pub fn analyze(&mut self, program: &Program) -> Result<(), CompilerError> {
        let _ = self.settings;
        self.collect_structs(program)?;
        self.collect_function_signatures(program)?;

        for statement in &program.statements {
            if !matches!(
                statement,
                Stmt::StructDecl { .. }
                    | Stmt::FunctionDecl { .. }
                    | Stmt::ExternFunctionDecl { .. }
            ) {
                self.analyze_statement(statement)?;
            }
        }

        self.validate_main_signature()?;

        for statement in &program.statements {
            if let Stmt::FunctionDecl { .. } = statement {
                self.analyze_function(statement)?;
            }
        }

        Ok(())
    }

    /// 预先收集所有顶层函数签名，支持前向调用和递归调用。
    fn collect_function_signatures(&mut self, program: &Program) -> Result<(), CompilerError> {
        for statement in &program.statements {
            let (name, parameters, return_type, span, is_extern) = match statement {
                Stmt::FunctionDecl {
                    name,
                    parameters,
                    return_type,
                    span,
                    ..
                } => (name, parameters, return_type, span, false),
                Stmt::ExternFunctionDecl {
                    name,
                    parameters,
                    return_type,
                    span,
                    ..
                } => (name, parameters, return_type, span, true),
                _ => continue,
            };

            if matches!(
                name.as_str(),
                "print"
                    | "len"
                    | "int"
                    | "float"
                    | "string"
                    | "bool"
                    | "try_int"
                    | "try_float"
                    | "try_bool"
            ) {
                return Err(CompilerError::semantic_message(
                    format!("用户函数不能覆盖内建函数 `{name}`"),
                    *span,
                ));
            }

            if self.functions.contains_key(name) {
                return Err(CompilerError::semantic_message(
                    format!("函数 `{name}` 重复定义"),
                    *span,
                ));
            }

            let mut parameter_types = Vec::with_capacity(parameters.len());
            for parameter in parameters {
                parameter_types
                    .push(self.value_type_from_annotation(&parameter.type_name, parameter.span)?);
            }
            let return_type = if let Some(annotation) = return_type {
                self.value_type_from_annotation(annotation, *span)?
            } else {
                ValueType::Unit
            };

            self.functions.insert(
                name.clone(),
                FunctionSignature {
                    parameter_types,
                    return_type,
                    is_extern,
                    span: *span,
                },
            );
        }

        Ok(())
    }

    /// 校验 `main` 函数若存在则必须满足无参入口约束。
    fn validate_main_signature(&self) -> Result<(), CompilerError> {
        let Some(signature) = self.functions.get("main") else {
            return Ok(());
        };

        if !signature.parameter_types.is_empty() {
            return Err(CompilerError::semantic_message(
                "`main` 函数不能声明参数",
                signature.span,
            ));
        }

        Ok(())
    }

    /// 分析单个函数体，并检查其返回路径是否满足声明约束。
    fn analyze_function(&mut self, statement: &Stmt) -> Result<(), CompilerError> {
        let Stmt::FunctionDecl {
            name,
            parameters,
            return_type,
            body,
            span,
            ..
        } = statement
        else {
            unreachable!("只有函数定义语句会进入该分支");
        };

        self.push_frame();
        for parameter in parameters {
            if self.current_scope().contains_key(&parameter.name) {
                self.pop_frame();
                return Err(CompilerError::semantic_message(
                    format!("函数 `{name}` 的参数 `{}` 重复声明", parameter.name),
                    parameter.span,
                ));
            }
            let parameter_type =
                self.value_type_from_annotation(&parameter.type_name, parameter.span)?;
            self.current_scope_mut().insert(
                parameter.name.clone(),
                SymbolInfo::new(parameter_type, BindingKind::Var),
            );
        }

        let previous_return_type = self.current_return_type.clone();
        self.current_return_type = Some(if let Some(annotation) = return_type {
            self.value_type_from_annotation(annotation, *span)?
        } else {
            ValueType::Unit
        });

        let mut result = Ok(());
        for nested in body {
            if let Err(error) = self.analyze_statement(nested) {
                result = Err(error);
                break;
            }
        }

        if result.is_ok()
            && self.current_return_type != Some(ValueType::Unit)
            && !self.block_guarantees_return(body)
        {
            result = Err(CompilerError::semantic_message(
                format!("函数 `{name}` 声明了返回值，但并非所有路径都会返回"),
                *span,
            ));
        }

        self.current_return_type = previous_return_type;
        self.pop_frame();
        result
    }
}
