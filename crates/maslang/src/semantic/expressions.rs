use crate::ast::{Expr, Literal, PropertyName};
use crate::diagnostic::CompilerError;
use crate::source::Span;

use super::{SemanticAnalyzer, ValueType};

impl<'a> SemanticAnalyzer<'a> {
    /// 推导表达式类型，并在遇到不合法组合时抛出语义错误。
    pub(super) fn infer_expression_type(
        &mut self,
        expression: &Expr,
    ) -> Result<ValueType, CompilerError> {
        self.infer_expression_type_with_hint(expression, None)
    }

    /// 在可用时利用期望类型提示表达式类型推导，主要用于空列表字面量。
    pub(super) fn infer_expression_type_with_hint(
        &mut self,
        expression: &Expr,
        hint: Option<&ValueType>,
    ) -> Result<ValueType, CompilerError> {
        if let Some(ValueType::Nullable(inner)) = hint {
            let actual = self.infer_expression_type_with_hint(expression, Some(inner))?;
            if actual == ValueType::Null
                || actual == **inner
                || actual == ValueType::Nullable(inner.clone())
            {
                return Ok(ValueType::Nullable(inner.clone()));
            }
            return Ok(actual);
        }
        match expression {
            Expr::FieldAccess { target, name, span } => {
                self.infer_field_or_member_type(target, name, *span)
            }
            Expr::Literal { value, .. } => Ok(match value {
                Literal::Null => ValueType::Null,
                Literal::Int(_) => ValueType::Int,
                Literal::Float(_) => ValueType::Float,
                Literal::Bool(_) => ValueType::Bool,
                Literal::String(_) => ValueType::String,
            }),
            Expr::Identifier { name, span } => self
                .lookup(name)
                .map(|symbol| symbol.value_type.clone())
                .ok_or_else(|| {
                    CompilerError::semantic_message(format!("未声明变量 `{name}`"), *span)
                }),
            Expr::Grouped { expression, .. } => {
                self.infer_expression_type_with_hint(expression, hint)
            }
            Expr::Unary { op, operand, span } => self.infer_unary_type(*op, operand, *span),
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => self.infer_binary_type(*op, left, right, *span),
            Expr::Call {
                callee,
                arguments,
                span,
            } => self.infer_call_type(callee, arguments, *span),
            Expr::ListLiteral { elements, span } => {
                self.infer_list_literal_type(elements, hint, *span)
            }
            Expr::MapLiteral { entries, span } => self.infer_map_literal_type(entries, hint, *span),
            Expr::ArrayInitializer {
                element_type,
                size,
                elements,
                span,
            } => self.infer_array_initializer_type(element_type, *size, elements, *span),
            Expr::Index {
                target,
                index,
                span,
            } => self.infer_index_type(target, index, *span),
            Expr::Slice {
                target,
                start,
                end,
                span,
            } => self.infer_slice_type(target, start.as_deref(), end.as_deref(), *span),
            Expr::MethodCall {
                target,
                method,
                arguments,
                span,
            } => self.infer_method_type(target, *method, arguments, *span),
            Expr::PropertyAccess {
                target,
                property,
                span,
            } => self.infer_property_access_type(target, *property, *span),
        }
    }

    /// 推导安全转换结果上的 `ok/value` 属性访问类型。
    fn infer_property_access_type(
        &mut self,
        target: &Expr,
        property: PropertyName,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let target_type = self.infer_expression_type(target)?;
        if let ValueType::Nullable(inner) = &target_type {
            if property == PropertyName::Value {
                return Ok(*inner.clone());
            }
        }
        if matches!(target_type, ValueType::Struct(_)) {
            return self.infer_field_type(target, property.display_name(), span);
        }
        let ValueType::SafeConvertResult(value_type) = target_type else {
            return Err(CompilerError::semantic_message(
                "当前表达式不支持属性访问",
                span,
            ));
        };

        Ok(match property {
            PropertyName::Ok => ValueType::Bool,
            PropertyName::Value => (*value_type).clone(),
        })
    }
}
