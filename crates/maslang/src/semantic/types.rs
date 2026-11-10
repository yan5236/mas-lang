use crate::ast::BindingKind;
use crate::source::Span;

/// 保存枚举的名义身份与源码显示名。
///
/// 身份取自模块装载器改写后的内部名，保证不同模块的同名枚举互不兼容；
/// 显示名保留用户书写形式，避免诊断或语言服务暴露 `__ms_` 前缀。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumType {
    pub internal: String,
    pub display: String,
}

impl EnumType {
    /// 使用内部身份名和源码显示名创建枚举类型。
    pub fn new(internal: String, display: String) -> Self {
        Self { internal, display }
    }
}

/// 表示语义分析阶段内部使用的值类型。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueType {
    Struct(String),
    Enum(EnumType),
    Nullable(Box<ValueType>),
    Null,
    Int,
    Float,
    Bool,
    String,
    /// 不可变字节对象；长度、索引和切片语义独立于字符串，内容可为任意字节。
    Bytes,
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
            Self::Struct(name) => name.clone(),
            Self::Enum(info) => info.display.clone(),
            Self::Nullable(inner) => format!("{}?", inner.display_name()),
            Self::Null => "null".to_string(),
            Self::Int => "int".to_string(),
            Self::Float => "float".to_string(),
            Self::Bool => "bool".to_string(),
            Self::String => "string".to_string(),
            Self::Bytes => "bytes".to_string(),
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

    /// 返回枚举类型携带的名义身份；其他类型返回 `None`。
    pub fn enum_identity(&self) -> Option<&EnumType> {
        match self {
            Self::Enum(info) => Some(info),
            _ => None,
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
