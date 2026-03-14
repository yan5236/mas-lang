use crate::source::Span;

/// 表示整个源码文件在语法分析完成后的顶层程序节点。
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

/// 表示当前阶段允许出现的所有语句节点。
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    VarDecl {
        name: String,
        annotation: Option<TypeName>,
        initializer: Expr,
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
            Self::VarDecl { span, .. } | Self::ExprStmt { span, .. } => *span,
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

/// 表示当前阶段支持的二元运算符。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
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
    /// 根据源码中的类型标识符文本解析出对应的内建类型。
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
