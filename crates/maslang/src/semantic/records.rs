use super::*;
use crate::ast::{Expr, Program, Stmt};
use crate::diagnostic::CompilerError;
use crate::source::Span;

impl SemanticAnalyzer<'_> {
    /// 预注册结构体以支持互相引用，再检查字段重复、类型和与函数名称的冲突。
    pub(super) fn collect_structs(&mut self, program: &Program) -> Result<(), CompilerError> {
        for statement in &program.statements {
            if let Stmt::StructDecl {
                name, fields, span, ..
            } = statement
            {
                if matches!(
                    name.as_str(),
                    "int"
                        | "float"
                        | "bool"
                        | "string"
                        | "list"
                        | "map"
                        | "array"
                        | "print"
                        | "try_print"
                        | "len"
                        | "try_int"
                        | "try_float"
                        | "try_bool"
                        | "try_add"
                        | "try_sub"
                        | "try_mul"
                        | "try_div"
                        | "try_mod"
                        | "try_neg"
                        | "main"
                ) || self.structs.insert(name.clone(), fields.clone()).is_some()
                {
                    return Err(CompilerError::semantic_message(
                        format!("结构体名称 `{name}` 重复或与内建名称冲突"),
                        *span,
                    ));
                }
            }
        }
        for statement in &program.statements {
            match statement {
                Stmt::StructDecl { fields, .. } => {
                    let mut names = std::collections::HashSet::new();
                    for field in fields {
                        if !names.insert(&field.name) {
                            return Err(CompilerError::semantic_message(
                                format!("字段 `{}` 重复定义", field.name),
                                field.span,
                            ));
                        }
                        self.value_type_from_annotation(&field.type_name, field.span)?;
                    }
                }
                Stmt::FunctionDecl { name, span, .. }
                | Stmt::ExternFunctionDecl { name, span, .. }
                    if self.structs.contains_key(name) =>
                {
                    return Err(CompilerError::semantic_message(
                        "函数名不能与结构体名称相同",
                        *span,
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// 检查构造函数的实参数量与每个字段的声明类型，返回具名结构体类型。
    pub(super) fn infer_constructor(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let fields = self.structs[name].clone();
        if fields.len() != args.len() {
            return Err(CompilerError::semantic_message(
                format!(
                    "结构体 `{name}` 需要 {} 个字段值，按声明顺序传入",
                    fields.len()
                ),
                span,
            ));
        }
        for (field, arg) in fields.iter().zip(args) {
            let expected = self.value_type_from_annotation(&field.type_name, field.span)?;
            if self.infer_expression_type_with_hint(arg, Some(&expected))? != expected {
                return Err(CompilerError::semantic_message(
                    format!("字段 `{}` 需要 `{}`", field.name, expected.display_name()),
                    arg.span(),
                ));
            }
        }
        Ok(ValueType::Struct(name.to_string()))
    }

    /// 查找结构体字段并返回其声明类型；可空对象必须先用 `.value` 显式取出。
    pub(super) fn infer_field_type(
        &mut self,
        target: &Expr,
        name: &str,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let target_type = self.infer_expression_type(target)?;
        let ValueType::Struct(record) = target_type else {
            return Err(CompilerError::semantic_message(
                "字段访问需要结构体；可空值请先判断 `!= null`，再用 `.value` 取值",
                span,
            ));
        };
        let field = self.structs[&record]
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| {
                CompilerError::semantic_message(
                    format!("结构体 `{record}` 没有字段 `{name}`"),
                    span,
                )
            })?;
        self.value_type_from_annotation(&field.type_name, span)
    }
}
