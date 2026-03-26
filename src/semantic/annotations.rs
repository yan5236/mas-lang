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
            TypeName::Int => Ok(ValueType::Int),
            TypeName::Float => Ok(ValueType::Float),
            TypeName::Bool => Ok(ValueType::Bool),
            TypeName::String => Ok(ValueType::String),
            TypeName::List(element) => match element.as_ref() {
                TypeName::Int => Ok(ValueType::List(Box::new(ValueType::Int))),
                TypeName::Float => Ok(ValueType::List(Box::new(ValueType::Float))),
                TypeName::Bool => Ok(ValueType::List(Box::new(ValueType::Bool))),
                TypeName::String => Ok(ValueType::List(Box::new(ValueType::String))),
                TypeName::List(_) | TypeName::Map(_, _) | TypeName::Array(_, _) => Err(
                    CompilerError::semantic_message("当前阶段列表元素只支持基础类型", span),
                ),
            },
            TypeName::Map(key, value) => {
                let key_type = self.value_type_from_annotation(key, span)?;
                let value_type = self.value_type_from_annotation(value, span)?;
                self.ensure_map_key_type(&key_type, span)?;
                self.ensure_scalar_type(&value_type, "当前阶段字典值只支持基础类型", span)?;
                Ok(ValueType::Map(Box::new(key_type), Box::new(value_type)))
            }
            TypeName::Array(element, size) => match element.as_ref() {
                TypeName::Int => Ok(ValueType::Array(Box::new(ValueType::Int), *size)),
                TypeName::Float => Ok(ValueType::Array(Box::new(ValueType::Float), *size)),
                TypeName::Bool => Ok(ValueType::Array(Box::new(ValueType::Bool), *size)),
                TypeName::String => Ok(ValueType::Array(Box::new(ValueType::String), *size)),
                TypeName::List(_) | TypeName::Map(_, _) | TypeName::Array(_, _) => Err(
                    CompilerError::semantic_message("当前阶段数组元素只支持基础类型", span),
                ),
            },
        }
    }

    /// 校验给定类型是否属于当前阶段允许作为字典键的基础类型。
    pub(super) fn ensure_map_key_type(
        &self,
        value_type: &ValueType,
        span: Span,
    ) -> Result<(), CompilerError> {
        self.ensure_scalar_type(value_type, "当前阶段字典键只支持基础类型", span)
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
