use std::collections::HashMap;

use crate::settings::CompilerSettings;

mod analysis;
mod annotations;
mod containers;
mod enums;
mod expressions;
mod operations;
mod records;
mod scope;
mod statements;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use self::containers::ARRAY_TRY_SET_STORAGE_HINT;
pub use self::types::{EnumType, FunctionSignature, ValueType};

use self::enums::EnumInfo;
use self::types::SymbolInfo;

/// 使用作用域栈和函数符号表对 AST 进行静态语义检查。
pub struct SemanticAnalyzer<'a> {
    settings: &'a CompilerSettings,
    scopes: Vec<HashMap<String, SymbolInfo>>,
    frame_starts: Vec<usize>,
    structs: HashMap<String, Vec<crate::ast::Parameter>>,
    /// 按内部身份名登记枚举声明；身份名由模块装载器重写，因此不同模块互不影响。
    enums: HashMap<String, EnumInfo>,
    functions: HashMap<String, FunctionSignature>,
    current_return_type: Option<ValueType>,
    loop_depth: usize,
}

impl<'a> SemanticAnalyzer<'a> {
    /// 创建一个新的语义分析器，并初始化全局作用域。
    pub fn new(settings: &'a CompilerSettings) -> Self {
        Self {
            settings,
            scopes: vec![HashMap::new()],
            frame_starts: vec![0],
            structs: HashMap::new(),
            enums: HashMap::new(),
            functions: HashMap::new(),
            current_return_type: None,
            loop_depth: 0,
        }
    }
}
