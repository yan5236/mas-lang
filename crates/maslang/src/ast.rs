use crate::source::Span;

/// 表示整个源码文件解析后的顶层程序节点。
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

/// 表示函数参数的名称、类型和源码区间。
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub type_name: TypeName,
    pub span: Span,
}

/// 表示 `import` 的模块来源、可选的本地命名空间别名及原路径区间。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportPath {
    pub segments: Vec<String>,
    pub alias: Option<String>,
    pub span: Span,
}

impl ImportPath {
    /// 返回当前模块路径是否显式使用了 `local.` 前缀。
    pub fn is_local(&self) -> bool {
        self.segments
            .first()
            .is_some_and(|segment| segment == "local")
    }

    /// 返回去除 `local.` 前缀后的真实模块路径片段。
    pub fn normalized_segments(&self) -> &[String] {
        if self.is_local() {
            &self.segments[1..]
        } else {
            &self.segments
        }
    }
}

/// 表示具名导入中的单个导入项，支持可选别名。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportItem {
    pub name: String,
    pub alias: Option<String>,
    pub span: Span,
}

/// 表示 `import` 支持的导入形式：具名导入或全部导入。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportKind {
    Named(Vec<ImportItem>),
    All,
}

/// 表示绑定声明的可变性，用于区分 `var` 与 `const`。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Var,
    Const,
}

impl BindingKind {
    /// 返回当前绑定是否允许在声明后再次被赋值。
    pub fn is_mutable(self) -> bool {
        matches!(self, Self::Var)
    }

    /// 返回当前绑定在错误消息中使用的关键字文本。
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Var => "变量",
            Self::Const => "常量",
        }
    }
}

/// 表示条件分支中的单个 `if` 或 `elif` 分支。
#[derive(Clone, Debug, PartialEq)]
pub struct ConditionalBranch {
    pub condition: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// 表示当前阶段支持的所有语句节点。
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    StructDecl {
        is_public: bool,
        name: String,
        fields: Vec<Parameter>,
        span: Span,
    },
    FunctionDecl {
        is_public: bool,
        name: String,
        parameters: Vec<Parameter>,
        return_type: Option<TypeName>,
        body: Vec<Stmt>,
        span: Span,
    },
    ExternFunctionDecl {
        is_public: bool,
        name: String,
        parameters: Vec<Parameter>,
        return_type: Option<TypeName>,
        span: Span,
    },
    Import {
        path: ImportPath,
        kind: ImportKind,
        span: Span,
    },
    ExportImport {
        path: ImportPath,
        kind: ImportKind,
        span: Span,
    },
    VarDecl {
        is_public: bool,
        binding_kind: BindingKind,
        name: String,
        annotation: Option<TypeName>,
        initializer: Option<Expr>,
        span: Span,
    },
    Assign {
        target: AssignTarget,
        op: AssignOp,
        value: Expr,
        span: Span,
    },
    If {
        branches: Vec<ConditionalBranch>,
        else_branch: Option<Vec<Stmt>>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    ForRange {
        name: String,
        start: Expr,
        end: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    ForEach {
        key_name: String,
        value_name: Option<String>,
        iterable: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    ExprStmt {
        expr: Expr,
        span: Span,
    },
}

/// 表示赋值语句左侧当前阶段允许出现的目标形式。
#[derive(Clone, Debug, PartialEq)]
pub enum AssignTarget {
    Field {
        target: Box<Expr>,
        name: String,
        span: Span,
    },
    Identifier {
        name: String,
        span: Span,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
}

impl AssignTarget {
    /// 返回赋值目标自身在源码中的覆盖区间。
    pub fn span(&self) -> Span {
        match self {
            Self::Field { span, .. } | Self::Identifier { span, .. } | Self::Index { span, .. } => {
                *span
            }
        }
    }
}

impl Stmt {
    /// 返回语句自身在源码中的覆盖区间。
    pub fn span(&self) -> Span {
        match self {
            Self::StructDecl { span, .. }
            | Self::FunctionDecl { span, .. }
            | Self::ExternFunctionDecl { span, .. }
            | Self::Import { span, .. }
            | Self::ExportImport { span, .. }
            | Self::VarDecl { span, .. }
            | Self::Assign { span, .. }
            | Self::If { span, .. }
            | Self::While { span, .. }
            | Self::ForRange { span, .. }
            | Self::ForEach { span, .. }
            | Self::Break { span }
            | Self::Continue { span }
            | Self::Return { span, .. }
            | Self::ExprStmt { span, .. } => *span,
        }
    }
}

/// 表示当前阶段支持的所有表达式节点。
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    FieldAccess {
        target: Box<Expr>,
        name: String,
        span: Span,
    },
    Literal {
        value: Literal,
        span: Span,
    },
    Identifier {
        name: String,
        span: Span,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        arguments: Vec<Expr>,
        span: Span,
    },
    ListLiteral {
        elements: Vec<Expr>,
        span: Span,
    },
    MapLiteral {
        entries: Vec<MapEntry>,
        span: Span,
    },
    ArrayInitializer {
        element_type: TypeName,
        size: usize,
        elements: Vec<Expr>,
        span: Span,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Slice {
        target: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        span: Span,
    },
    MethodCall {
        target: Box<Expr>,
        method: MethodName,
        arguments: Vec<Expr>,
        span: Span,
    },
    PropertyAccess {
        target: Box<Expr>,
        property: PropertyName,
        span: Span,
    },
    Grouped {
        expression: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    /// 返回表达式自身在源码中的覆盖区间。
    pub fn span(&self) -> Span {
        match self {
            Self::FieldAccess { span, .. }
            | Self::Literal { span, .. }
            | Self::Identifier { span, .. }
            | Self::Binary { span, .. }
            | Self::Unary { span, .. }
            | Self::Call { span, .. }
            | Self::ListLiteral { span, .. }
            | Self::MapLiteral { span, .. }
            | Self::ArrayInitializer { span, .. }
            | Self::Index { span, .. }
            | Self::Slice { span, .. }
            | Self::MethodCall { span, .. }
            | Self::PropertyAccess { span, .. }
            | Self::Grouped { span, .. } => *span,
        }
    }
}

/// 表示字典字面量中的单个键值对。
#[derive(Clone, Debug, PartialEq)]
pub struct MapEntry {
    pub key: Expr,
    pub value: Expr,
    pub span: Span,
}

/// 表示字符串、列表与字典对象当前阶段支持的方法名称。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MethodName {
    Append,
    Remove,
    Pop,
    Sort,
    Reverse,
    Index,
    Contains,
    Keys,
    Values,
    Trim,
    StartsWith,
    EndsWith,
    Replace,
}

impl MethodName {
    /// 根据源码中的方法名文本解析出对应的方法枚举。
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "append" => Some(Self::Append),
            "remove" => Some(Self::Remove),
            "pop" => Some(Self::Pop),
            "sort" => Some(Self::Sort),
            "reverse" => Some(Self::Reverse),
            "index" => Some(Self::Index),
            "contains" => Some(Self::Contains),
            "keys" => Some(Self::Keys),
            "values" => Some(Self::Values),
            "trim" => Some(Self::Trim),
            "starts_with" => Some(Self::StartsWith),
            "ends_with" => Some(Self::EndsWith),
            "replace" => Some(Self::Replace),
            _ => None,
        }
    }

    /// 返回该方法是否允许以 `string` 作为调用目标。
    pub fn supports_string_target(self) -> bool {
        matches!(
            self,
            Self::Trim | Self::Contains | Self::StartsWith | Self::EndsWith | Self::Replace
        )
    }

    /// 返回方法在错误消息中使用的稳定文本。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Remove => "remove",
            Self::Pop => "pop",
            Self::Sort => "sort",
            Self::Reverse => "reverse",
            Self::Index => "index",
            Self::Contains => "contains",
            Self::Keys => "keys",
            Self::Values => "values",
            Self::Trim => "trim",
            Self::StartsWith => "starts_with",
            Self::EndsWith => "ends_with",
            Self::Replace => "replace",
        }
    }
}

/// 表示当前阶段允许访问的只读属性名称。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyName {
    Ok,
    Value,
}

impl PropertyName {
    /// 根据源码中的属性名文本解析出对应的属性枚举。
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "ok" => Some(Self::Ok),
            "value" => Some(Self::Value),
            _ => None,
        }
    }

    /// 返回属性在错误消息中使用的稳定文本。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Value => "value",
        }
    }
}

/// 表示当前阶段允许出现的字面量值。
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

/// 表示当前阶段支持的复合赋值运算符。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubtractAssign,
    MultiplyAssign,
    DivideAssign,
    ModAssign,
}

/// 表示当前阶段支持的二元运算符。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    IntDivide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

/// 表示当前阶段支持的一元运算符。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Negate,
    Not,
}

/// 表示显式类型标注中允许出现的内建类型。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeName {
    Named(String),
    Nullable(Box<TypeName>),
    Int,
    Float,
    Bool,
    String,
    List(Box<TypeName>),
    Map(Box<TypeName>, Box<TypeName>),
    Array(Box<TypeName>, usize),
}

impl TypeName {
    /// 根据源码中的标识符文本解析出对应的内建类型。
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "bool" => Some(Self::Bool),
            "string" => Some(Self::String),
            _ => None,
        }
    }
}
