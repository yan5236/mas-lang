use crate::ast::BindingKind;
use crate::source::Span;

/// 表示语义分析阶段内部使用的值类型。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueType {
    Int,
    Float,
    Bool,
    String,
    SafeConvertResult(Box<ValueType>),
    List(Box<ValueType>),
    Map(Box<ValueType>, Box<ValueType>),
    Array(Box<ValueType>, usize),
    Unit,
}

impl ValueType {
    /// 返回类型在诊断信息中使用的稳定名称。
    pub fn display_name(&self) -> String {
        match self {
            Self::Int => "int".to_string(),
            Self::Float => "float".to_string(),
            Self::Bool => "bool".to_string(),
            Self::String => "string".to_string(),
            Self::SafeConvertResult(value) => {
                format!("try_result<{}>", value.display_name())
            }
            Self::List(element) => format!("list<{}>", element.display_name()),
            Self::Map(key, value) => {
                format!("map<{},{}>", key.display_name(), value.display_name())
            }
            Self::Array(element, size) => format!("array<{},{}>", element.display_name(), size),
            Self::Unit => "unit".to_string(),
        }
    }
}

/// 保存用户函数的参数与返回类型信息，供调用校验与代码生成复用。
#[derive(Clone, Debug)]
pub struct FunctionSignature {
    pub parameter_types: Vec<ValueType>,
    pub return_type: ValueType,
    pub is_extern: bool,
    pub span: Span,
}

/// 保存当前作用域中单个名称的类型与可变性元数据。
#[derive(Clone, Debug)]
pub(crate) struct SymbolInfo {
    pub(crate) value_type: ValueType,
    pub(crate) is_mutable: bool,
}

impl SymbolInfo {
    /// 按给定值类型和绑定种类创建新的符号信息。
    pub(crate) fn new(value_type: ValueType, binding_kind: BindingKind) -> Self {
        Self {
            value_type,
            is_mutable: binding_kind.is_mutable(),
        }
    }
}
