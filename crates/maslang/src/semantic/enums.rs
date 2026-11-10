use std::collections::HashSet;

use crate::ast::{Expr, Program, Stmt};
use crate::diagnostic::CompilerError;
use crate::source::Span;

use super::{EnumType, SemanticAnalyzer, ValueType};

/// 保存单个枚举声明的源码显示名和按书写顺序排列的成员名。
///
/// 成员下标即内部 lowering 的整数序号，属于实现细节，不写入公开语义。
pub(super) struct EnumInfo {
    pub(super) display_name: String,
    pub(super) members: Vec<String>,
}

impl EnumInfo {
    /// 返回成员在声明中的序号；成员不存在时返回 `None`。
    fn member_index(&self, name: &str) -> Option<usize> {
        self.members.iter().position(|member| member == name)
    }
}

impl SemanticAnalyzer<'_> {
    /// 收集全部顶层枚举声明：拒绝内建名冲突、重复声明、空枚举和重复成员。
    ///
    /// 枚举身份使用模块重写后的名称，因此不同模块的同名枚举天然互不兼容。
    pub(super) fn collect_enums(&mut self, program: &Program) -> Result<(), CompilerError> {
        for statement in &program.statements {
            let Stmt::EnumDecl {
                name,
                source_name,
                members,
                span,
                ..
            } = statement
            else {
                continue;
            };

            if is_builtin_name(name)
                || self.enums.contains_key(name)
                || self.structs.contains_key(name)
            {
                return Err(CompilerError::semantic_message(
                    format!("枚举名称 `{source_name}` 重复或与内建名称冲突"),
                    *span,
                ));
            }

            let mut seen = HashSet::new();
            let mut member_names = Vec::with_capacity(members.len());
            for member in members {
                if !seen.insert(member.name.as_str()) {
                    return Err(CompilerError::semantic_message(
                        format!("枚举 `{source_name}` 的成员 `{}` 重复定义", member.name),
                        member.span,
                    ));
                }
                member_names.push(member.name.clone());
            }
            if member_names.is_empty() {
                return Err(CompilerError::semantic_message(
                    format!("枚举 `{source_name}` 至少需要一个成员"),
                    *span,
                ));
            }

            self.enums.insert(
                name.clone(),
                EnumInfo {
                    display_name: source_name.clone(),
                    members: member_names,
                },
            );
        }

        for statement in &program.statements {
            let (name, span) = match statement {
                Stmt::FunctionDecl { name, span, .. }
                | Stmt::ExternFunctionDecl { name, span, .. } => (name, *span),
                _ => continue,
            };
            if self.enums.contains_key(name) {
                return Err(CompilerError::semantic_message(
                    "函数名不能与枚举名称相同",
                    span,
                ));
            }
        }

        Ok(())
    }

    /// 返回指定内部身份名对应的枚举值类型；未登记时返回 `None`。
    pub(super) fn enum_value_type(&self, internal: &str) -> Option<ValueType> {
        let info = self.enums.get(internal)?;
        Some(ValueType::Enum(EnumType::new(
            internal.to_string(),
            info.display_name.clone(),
        )))
    }

    /// 解析 `Enum.Member` 形式的成员引用，或回退到普通结构体字段访问。
    ///
    /// 接收者标识符命中枚举声明时按成员名返回该枚举类型；成员不存在、接收者
    /// 既非枚举也非变量时返回带源码位置的诊断。
    pub(super) fn infer_field_or_member_type(
        &mut self,
        target: &Expr,
        name: &str,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        if let Expr::Identifier {
            name: target_name, ..
        } = target
        {
            if let Some((display, member_exists)) = self
                .enums
                .get(target_name)
                .map(|info| (info.display_name.clone(), info.member_index(name).is_some()))
            {
                if !member_exists {
                    return Err(CompilerError::semantic_message(
                        format!("枚举 `{display}` 没有成员 `{name}`"),
                        span,
                    ));
                }
                return Ok(self
                    .enum_value_type(target_name)
                    .expect("刚确认存在的枚举声明必然可构造成值类型"));
            }
            if self.lookup(target_name).is_none() && !self.structs.contains_key(target_name) {
                return Err(CompilerError::semantic_message(
                    format!("`{target_name}` 不是已声明的枚举，不能引用成员 `{name}`"),
                    span,
                ));
            }
        }
        self.infer_field_type(target, name, span)
    }
}

/// 判断名称是否与语言内建类型、内建函数或入口函数冲突。
fn is_builtin_name(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "float"
            | "bool"
            | "string"
            | "bytes"
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
            | "bytes_from_list"
            | "encode_utf8"
            | "decode_utf8"
            | "main"
    )
}
