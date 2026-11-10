use crate::ast::TypeName;
use crate::diagnostic::CompilerError;
use crate::source::Span;

use super::{SemanticAnalyzer, ValueType};

impl<'a> SemanticAnalyzer<'a> {
    /// 把语法树中的类型标注转换为语义阶段使用的值类型。
    pub(super) fn value_type_from_annotation(
        &self,
        annotation: &TypeName,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match annotation {
            TypeName::Named(name) => {
                if self.structs.contains_key(name) {
                    Ok(ValueType::Struct(name.clone()))
                } else if let Some(value_type) = self.enum_value_type(name) {
                    Ok(value_type)
                } else {
                    Err(CompilerError::semantic_message(
                        format!("未知类型 `{name}`，请先定义或导入结构体或枚举"),
                        span,
                    ))
                }
            }
            TypeName::Nullable(inner) => Ok(ValueType::Nullable(Box::new(
                self.value_type_from_annotation(inner, span)?,
            ))),
            TypeName::Int => Ok(ValueType::Int),
            TypeName::Float => Ok(ValueType::Float),
            TypeName::Bool => Ok(ValueType::Bool),
            TypeName::String => Ok(ValueType::String),
            TypeName::Bytes => Ok(ValueType::Bytes),
            TypeName::List(element) => {
                let element = self.value_type_from_annotation(element, span)?;
                self.ensure_list_element_type(&element, span)?;
                Ok(ValueType::List(Box::new(element)))
            }
            TypeName::Map(key, value) => {
                let key_type = self.value_type_from_annotation(key, span)?;
                let value_type = self.value_type_from_annotation(value, span)?;
                self.ensure_map_key_type(&key_type, span)?;
                self.ensure_map_value_type(&value_type, span)?;
                Ok(ValueType::Map(Box::new(key_type), Box::new(value_type)))
            }
            TypeName::Array(element, size) => {
                let element = self.value_type_from_annotation(element, span)?;

                Ok(ValueType::Array(Box::new(element), *size))
            }
        }
    }

    /// 列表与字典值接受基础值及托管引用；内联数组仍需使用固定数组布局，不能直接放入动态槽位。
    /// 枚举不在首版容器元素范围内，单独给出明确诊断，避免退化成泛化消息。
    pub(super) fn ensure_list_element_type(
        &self,
        value_type: &ValueType,
        span: Span,
    ) -> Result<(), CompilerError> {
        if value_type.enum_identity().is_some() {
            return Err(CompilerError::semantic_message(
                "首版容器元素暂不支持枚举；请使用枚举字段、可空枚举或 int 序号",
                span,
            ));
        }
        if matches!(
            value_type,
            ValueType::Struct(_)
                | ValueType::Nullable(_)
                | ValueType::List(_)
                | ValueType::Map(_, _)
                | ValueType::Bytes
        ) {
            return Ok(());
        }
        self.ensure_scalar_type(
            value_type,
            "动态容器元素支持基础值、结构体、可空值、列表或字典；定长数组请改用列表",
            span,
        )
    }

    /// 校验字典值类型：与列表元素同源，但字节对象首版不支持放进字典值。
    /// 字节的相等与读写语义独立于字符串键值路径，避免为它引入额外的容器布局约定。
    pub(super) fn ensure_map_value_type(
        &self,
        value_type: &ValueType,
        span: Span,
    ) -> Result<(), CompilerError> {
        if *value_type == ValueType::Bytes {
            return Err(CompilerError::semantic_message(
                "首版不支持把 `bytes` 作为字典值；请改写为结构体字段或 `list<bytes>`",
                span,
            ));
        }
        self.ensure_list_element_type(value_type, span)
    }

    /// 校验字典键为基础值或结构体身份引用；可空键和容器键尚无稳定键约定。
    pub(super) fn ensure_map_key_type(
        &self,
        value_type: &ValueType,
        span: Span,
    ) -> Result<(), CompilerError> {
        if value_type.enum_identity().is_some() {
            return Err(CompilerError::semantic_message(
                "首版字典键暂不支持枚举；请改用 int 序号或结构体引用",
                span,
            ));
        }
        if matches!(value_type, ValueType::Struct(_)) {
            return Ok(());
        }
        self.ensure_scalar_type(value_type, "字典键支持基础类型或结构体引用", span)
    }

    /// 校验给定类型是否属于当前阶段允许进入容器的基础类型。
    pub(super) fn ensure_scalar_type(
        &self,
        value_type: &ValueType,
        message: &str,
        span: Span,
    ) -> Result<(), CompilerError> {
        match value_type {
            ValueType::Int | ValueType::Float | ValueType::Bool | ValueType::String => Ok(()),
            _ => Err(CompilerError::semantic_message(message, span)),
        }
    }
}
