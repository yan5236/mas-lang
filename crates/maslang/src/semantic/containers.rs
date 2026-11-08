use crate::ast::{Expr, MapEntry, MethodName, TypeName};
use crate::diagnostic::CompilerError;
use crate::source::Span;

use super::{SemanticAnalyzer, ValueType};

impl<'a> SemanticAnalyzer<'a> {
    /// 推导列表字面量的元素类型，并校验元素必须同构。
    pub(super) fn infer_list_literal_type(
        &mut self,
        elements: &[Expr],
        hint: Option<&ValueType>,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        if elements.is_empty() {
            return match hint {
                Some(ValueType::List(element)) => {
                    Ok(ValueType::List(Box::new((**element).clone())))
                }
                _ => Err(CompilerError::semantic_message(
                    "空列表字面量必须放在显式 `list<T>` 类型上下文中",
                    span,
                )),
            };
        }

        let hinted_element = match hint {
            Some(ValueType::List(element)) => Some(element.as_ref()),
            _ => None,
        };
        let first_type = self.infer_expression_type_with_hint(&elements[0], hinted_element)?;
        self.ensure_list_element_type(&first_type, span)?;

        for element in &elements[1..] {
            let element_type = self.infer_expression_type_with_hint(element, Some(&first_type))?;
            if element_type != first_type {
                return Err(CompilerError::semantic_message(
                    "列表字面量中的元素类型必须保持一致",
                    element.span(),
                ));
            }
        }

        Ok(ValueType::List(Box::new(first_type)))
    }

    /// 推导字典字面量的键值类型，并校验每个条目的键和值都保持同构。
    pub(super) fn infer_map_literal_type(
        &mut self,
        entries: &[MapEntry],
        hint: Option<&ValueType>,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        if entries.is_empty() {
            return match hint {
                Some(ValueType::Map(key, value)) => Ok(ValueType::Map(
                    Box::new((**key).clone()),
                    Box::new((**value).clone()),
                )),
                _ => Err(CompilerError::semantic_message(
                    "空字典字面量必须放在显式 `map<K,V>` 类型上下文中",
                    span,
                )),
            };
        }

        let (hinted_key, hinted_value) = match hint {
            Some(ValueType::Map(key, value)) => (Some(key.as_ref()), Some(value.as_ref())),
            _ => (None, None),
        };
        let first_key = self.infer_expression_type_with_hint(&entries[0].key, hinted_key)?;
        let first_value = self.infer_expression_type_with_hint(&entries[0].value, hinted_value)?;
        self.ensure_map_key_type(&first_key, entries[0].key.span())?;
        self.ensure_list_element_type(&first_value, entries[0].value.span())?;

        for entry in &entries[1..] {
            let key_type = self.infer_expression_type_with_hint(&entry.key, Some(&first_key))?;
            if key_type != first_key {
                return Err(CompilerError::semantic_message(
                    "字典字面量中的键类型必须保持一致",
                    entry.key.span(),
                ));
            }
            let value_type =
                self.infer_expression_type_with_hint(&entry.value, Some(&first_value))?;
            if value_type != first_value {
                return Err(CompilerError::semantic_message(
                    "字典字面量中的值类型必须保持一致",
                    entry.value.span(),
                ));
            }
        }

        Ok(ValueType::Map(Box::new(first_key), Box::new(first_value)))
    }

    /// 推导固定长度数组初始化表达式的类型，并校验元素数量和元素类型。
    pub(super) fn infer_array_initializer_type(
        &mut self,
        element_type: &TypeName,
        size: usize,
        elements: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let expected_element = self.value_type_from_annotation(element_type, span)?;
        if elements.len() != size {
            return Err(CompilerError::semantic_message(
                format!(
                    "数组初始化需要 {size} 个元素，但实际提供了 {} 个",
                    elements.len()
                ),
                span,
            ));
        }

        for element in elements {
            let actual = self.infer_expression_type_with_hint(element, Some(&expected_element))?;
            if actual != expected_element {
                return Err(CompilerError::semantic_message(
                    "数组初始化中的元素类型必须与声明类型一致",
                    element.span(),
                ));
            }
        }

        Ok(ValueType::Array(Box::new(expected_element), size))
    }

    /// 推导列表、数组或字典索引表达式的返回值类型。
    pub(super) fn infer_index_type(
        &mut self,
        target: &Expr,
        index: &Expr,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let target_type = self.infer_expression_type(target)?;
        match target_type {
            ValueType::List(element) | ValueType::Array(element, _) => {
                let index_type = self.infer_expression_type(index)?;
                if index_type != ValueType::Int {
                    return Err(CompilerError::semantic_message(
                        "索引必须是整数",
                        index.span(),
                    ));
                }
                Ok((*element).clone())
            }
            ValueType::Map(key, value) => {
                let index_type = self.infer_expression_type_with_hint(index, Some(key.as_ref()))?;
                if index_type != *key {
                    return Err(CompilerError::semantic_message(
                        "字典下标的键类型不匹配",
                        index.span(),
                    ));
                }
                Ok((*value).clone())
            }
            _ => Err(CompilerError::semantic_message(
                "索引访问当前只支持列表、数组或字典类型",
                span,
            )),
        }
    }

    /// 推导列表切片表达式的返回值类型。
    pub(super) fn infer_slice_type(
        &mut self,
        target: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let target_type = self.infer_expression_type(target)?;
        if let Some(start) = start {
            if self.infer_expression_type(start)? != ValueType::Int {
                return Err(CompilerError::semantic_message(
                    "切片起始下标必须是整数",
                    start.span(),
                ));
            }
        }
        if let Some(end) = end {
            if self.infer_expression_type(end)? != ValueType::Int {
                return Err(CompilerError::semantic_message(
                    "切片结束下标必须是整数",
                    end.span(),
                ));
            }
        }
        match target_type {
            ValueType::List(element) => Ok(ValueType::List(element)),
            _ => Err(CompilerError::semantic_message(
                "切片当前只支持列表类型",
                span,
            )),
        }
    }

    /// 推导对象方法调用的返回值类型，并根据目标类型分派到字符串、列表或字典规则。
    pub(super) fn infer_method_type(
        &mut self,
        target: &Expr,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let target_type = self.infer_expression_type(target)?;
        match target_type.clone() {
            ValueType::String => self.infer_string_method_type(method, arguments, span),
            ValueType::List(element_type) => {
                self.infer_list_method_type(element_type, method, arguments, span)
            }
            ValueType::Map(key_type, value_type) => {
                self.infer_map_method_type(key_type, value_type, method, arguments, span)
            }
            _ if method.supports_string_target() => Err(CompilerError::semantic_message(
                format!(
                    "字符串方法 `{}` 只能用于 `string` 类型，当前目标类型为 `{}`",
                    method.display_name(),
                    target_type.display_name()
                ),
                span,
            )),
            _ => Err(CompilerError::semantic_message(
                "当前方法只能用于字符串、列表或字典类型",
                span,
            )),
        }
    }

    /// 推导字符串方法调用的返回值类型，并校验参数个数与参数类型规则。
    fn infer_string_method_type(
        &mut self,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match method {
            MethodName::Trim => {
                if !arguments.is_empty() {
                    return Err(CompilerError::semantic_message(
                        "字符串方法 `trim` 不接受参数",
                        span,
                    ));
                }
                Ok(ValueType::String)
            }
            MethodName::Contains | MethodName::StartsWith | MethodName::EndsWith => {
                if arguments.len() != 1 {
                    return Err(CompilerError::semantic_message(
                        format!(
                            "字符串方法 `{}` 需要且只接受一个 string 参数",
                            method.display_name()
                        ),
                        span,
                    ));
                }
                if self.infer_expression_type(&arguments[0])? != ValueType::String {
                    return Err(CompilerError::semantic_message(
                        format!(
                            "字符串方法 `{}` 的参数类型必须是 string",
                            method.display_name()
                        ),
                        arguments[0].span(),
                    ));
                }
                Ok(ValueType::Bool)
            }
            MethodName::Replace => {
                if arguments.len() != 2 {
                    return Err(CompilerError::semantic_message(
                        "字符串方法 `replace` 需要且只接受两个 string 参数",
                        span,
                    ));
                }
                for argument in arguments {
                    if self.infer_expression_type(argument)? != ValueType::String {
                        return Err(CompilerError::semantic_message(
                            "字符串方法 `replace` 的参数类型必须是 string",
                            argument.span(),
                        ));
                    }
                }
                Ok(ValueType::String)
            }
            MethodName::Append
            | MethodName::Remove
            | MethodName::Pop
            | MethodName::Sort
            | MethodName::Reverse
            | MethodName::Index
            | MethodName::Keys
            | MethodName::Values => Err(CompilerError::semantic_message(
                format!("字符串类型不支持方法 `{}`", method.display_name()),
                span,
            )),
        }
    }

    /// 推导列表方法返回类型并校验参数；结构体按引用查找，但没有默认排序规则。
    fn infer_list_method_type(
        &mut self,
        element_type: Box<ValueType>,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match method {
            MethodName::Append | MethodName::Remove | MethodName::Index | MethodName::Contains => {
                if arguments.len() != 1 {
                    return Err(CompilerError::semantic_message(
                        format!("列表方法 `{}` 需要且只接受一个参数", method.display_name()),
                        span,
                    ));
                }
                let argument_type = self
                    .infer_expression_type_with_hint(&arguments[0], Some(element_type.as_ref()))?;
                if argument_type != *element_type {
                    return Err(CompilerError::semantic_message(
                        format!("列表方法 `{}` 的参数类型不匹配", method.display_name()),
                        arguments[0].span(),
                    ));
                }
                Ok(match method {
                    MethodName::Append | MethodName::Remove => ValueType::Unit,
                    MethodName::Index => ValueType::Int,
                    MethodName::Contains => ValueType::Bool,
                    _ => unreachable!(),
                })
            }
            MethodName::Pop => match arguments.len() {
                0 => Ok((*element_type).clone()),
                1 => {
                    if self.infer_expression_type(&arguments[0])? != ValueType::Int {
                        return Err(CompilerError::semantic_message(
                            "列表方法 `pop` 的索引参数必须是整数",
                            arguments[0].span(),
                        ));
                    }
                    Ok((*element_type).clone())
                }
                _ => Err(CompilerError::semantic_message(
                    "列表方法 `pop` 最多接受一个参数",
                    span,
                )),
            },
            MethodName::Sort | MethodName::Reverse => {
                if method == MethodName::Sort
                    && !matches!(
                        *element_type,
                        ValueType::Int | ValueType::Float | ValueType::Bool | ValueType::String
                    )
                {
                    return Err(CompilerError::semantic_message(
                        "该列表不支持 sort()：元素没有默认排序规则",
                        span,
                    ));
                }
                if !arguments.is_empty() {
                    return Err(CompilerError::semantic_message(
                        format!("列表方法 `{}` 不接受参数", method.display_name()),
                        span,
                    ));
                }
                Ok(ValueType::Unit)
            }
            MethodName::Keys
            | MethodName::Values
            | MethodName::Trim
            | MethodName::StartsWith
            | MethodName::EndsWith
            | MethodName::Replace => Err(CompilerError::semantic_message(
                format!("列表类型不支持方法 `{}`", method.display_name()),
                span,
            )),
        }
    }

    /// 推导字典方法调用的返回值类型，并校验方法参数规则。
    fn infer_map_method_type(
        &mut self,
        key_type: Box<ValueType>,
        value_type: Box<ValueType>,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match method {
            MethodName::Remove | MethodName::Contains => {
                if arguments.len() != 1 {
                    return Err(CompilerError::semantic_message(
                        format!("字典方法 `{}` 需要且只接受一个参数", method.display_name()),
                        span,
                    ));
                }
                let argument_type =
                    self.infer_expression_type_with_hint(&arguments[0], Some(key_type.as_ref()))?;
                if argument_type != *key_type {
                    return Err(CompilerError::semantic_message(
                        format!("字典方法 `{}` 的参数类型不匹配", method.display_name()),
                        arguments[0].span(),
                    ));
                }
                Ok(match method {
                    MethodName::Remove => ValueType::Unit,
                    MethodName::Contains => ValueType::Bool,
                    _ => unreachable!(),
                })
            }
            MethodName::Keys => {
                if !arguments.is_empty() {
                    return Err(CompilerError::semantic_message(
                        "字典方法 `keys` 不接受参数",
                        span,
                    ));
                }
                Ok(ValueType::List(Box::new((*key_type).clone())))
            }
            MethodName::Values => {
                if !arguments.is_empty() {
                    return Err(CompilerError::semantic_message(
                        "字典方法 `values` 不接受参数",
                        span,
                    ));
                }
                Ok(ValueType::List(Box::new((*value_type).clone())))
            }
            MethodName::Append
            | MethodName::Pop
            | MethodName::Sort
            | MethodName::Reverse
            | MethodName::Index
            | MethodName::Trim
            | MethodName::StartsWith
            | MethodName::EndsWith
            | MethodName::Replace => Err(CompilerError::semantic_message(
                format!("字典类型不支持方法 `{}`", method.display_name()),
                span,
            )),
        }
    }
}
