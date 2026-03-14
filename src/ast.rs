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
    FunctionDecl {
        name: String,
        parameters: Vec<Parameter>,
        return_type: Option<TypeName>,
        body: Vec<Stmt>,
        span: Span,
    },
    VarDecl {
        binding_kind: BindingKind,
        name: String,
        annotation: Option<TypeName>,
        initializer: Expr,
        span: Span,
    },
    Assign {
        name: String,
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

impl Stmt {
    /// 返回语句自身在源码中的覆盖区间。
    pub fn span(&self) -> Span {
        match self {
            Self::FunctionDecl { span, .. }
            | Self::VarDecl { span, .. }
            | Self::Assign { span, .. }
            | Self::If { span, .. }
            | Self::While { span, .. }
            | Self::ForRange { span, .. }
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
    Grouped {
        expression: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    /// 返回表达式自身在源码中的覆盖区间。
    pub fn span(&self) -> Span {
        match self {
            Self::Literal { span, .. }
            | Self::Identifier { span, .. }
            | Self::Binary { span, .. }
            | Self::Unary { span, .. }
            | Self::Call { span, .. }
            | Self::Grouped { span, .. } => *span,
        }
    }
}

/// 表示当前阶段允许出现的字面量值。
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeName {
    Int,
    Float,
    Bool,
    String,
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
