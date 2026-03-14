use std::collections::HashMap;

use crate::ast::{AssignOp, BinaryOp, BindingKind, Expr, Literal, Program, Stmt, TypeName, UnaryOp};
use crate::diagnostic::CompilerError;
use crate::settings::CompilerSettings;
use crate::source::Span;

/// 表示语义分析阶段内部使用的值类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueType {
    Int,
    Float,
    Bool,
    String,
    Unit,
}

impl ValueType {
    /// 把显式类型标注转换为语义阶段的值类型枚举。
    pub fn from_annotation(annotation: TypeName) -> Self {
        match annotation {
            TypeName::Int => Self::Int,
            TypeName::Float => Self::Float,
            TypeName::Bool => Self::Bool,
            TypeName::String => Self::String,
        }
    }

    /// 返回类型在诊断信息中使用的稳定名称。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Unit => "unit",
        }
    }
}

/// 保存用户函数的参数与返回类型信息，供调用校验与代码生成复用。
#[derive(Clone, Debug)]
pub struct FunctionSignature {
    pub parameter_types: Vec<ValueType>,
    pub return_type: ValueType,
    pub span: Span,
}

/// 保存当前作用域中单个名称的类型与可变性元数据。
#[derive(Clone, Copy, Debug)]
struct SymbolInfo {
    value_type: ValueType,
    is_mutable: bool,
}

impl SymbolInfo {
    /// 按给定值类型和绑定种类创建新的符号信息。
    fn new(value_type: ValueType, binding_kind: BindingKind) -> Self {
        Self {
            value_type,
            is_mutable: binding_kind.is_mutable(),
        }
    }
}

/// 使用作用域栈和函数符号表对 AST 进行静态语义检查。
pub struct SemanticAnalyzer<'a> {
    settings: &'a CompilerSettings,
    scopes: Vec<HashMap<String, SymbolInfo>>,
    frame_starts: Vec<usize>,
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
            functions: HashMap::new(),
            current_return_type: None,
            loop_depth: 0,
        }
    }

    /// 校验整棵程序语法树中的声明、控制流与类型规则。
    pub fn analyze(&mut self, program: &Program) -> Result<(), CompilerError> {
        let _ = self.settings;
        self.collect_function_signatures(program)?;

        for statement in &program.statements {
            if !matches!(statement, Stmt::FunctionDecl { .. }) {
                self.analyze_statement(statement)?;
            }
        }

        self.validate_main_signature()?;

        for statement in &program.statements {
            if let Stmt::FunctionDecl { .. } = statement {
                self.analyze_function(statement)?;
            }
        }

        Ok(())
    }

    /// 预先收集所有顶层函数签名，支持前向调用和递归调用。
    fn collect_function_signatures(&mut self, program: &Program) -> Result<(), CompilerError> {
        for statement in &program.statements {
            let Stmt::FunctionDecl {
                name,
                parameters,
                return_type,
                span,
                ..
            } = statement
            else {
                continue;
            };

            if name == "print" {
                return Err(CompilerError::semantic_message(
                    "用户函数不能覆盖内建函数 `print`",
                    *span,
                ));
            }

            if self.functions.contains_key(name) {
                return Err(CompilerError::semantic_message(
                    format!("函数 `{name}` 重复定义"),
                    *span,
                ));
            }

            let parameter_types = parameters
                .iter()
                .map(|parameter| ValueType::from_annotation(parameter.type_name))
                .collect();
            let return_type = return_type
                .map(ValueType::from_annotation)
                .unwrap_or(ValueType::Unit);

            self.functions.insert(
                name.clone(),
                FunctionSignature {
                    parameter_types,
                    return_type,
                    span: *span,
                },
            );
        }

        Ok(())
    }

    /// 校验 `main` 函数若存在则必须满足无参入口约束。
    fn validate_main_signature(&self) -> Result<(), CompilerError> {
        let Some(signature) = self.functions.get("main") else {
            return Ok(());
        };

        if !signature.parameter_types.is_empty() {
            return Err(CompilerError::semantic_message(
                "`main` 函数不能声明参数",
                signature.span,
            ));
        }

        Ok(())
    }

    /// 分析单个函数体，并检查其返回路径是否满足声明约束。
    fn analyze_function(&mut self, statement: &Stmt) -> Result<(), CompilerError> {
        let Stmt::FunctionDecl {
            name,
            parameters,
            return_type,
            body,
            span,
        } = statement
        else {
            unreachable!("只有函数定义语句会进入该分支");
        };

        self.push_frame();
        for parameter in parameters {
            if self.current_scope().contains_key(&parameter.name) {
                self.pop_frame();
                return Err(CompilerError::semantic_message(
                    format!("函数 `{name}` 的参数 `{}` 重复声明", parameter.name),
                    parameter.span,
                ));
            }
            self.current_scope_mut().insert(
                parameter.name.clone(),
                SymbolInfo::new(
                    ValueType::from_annotation(parameter.type_name),
                    BindingKind::Var,
                ),
            );
        }

        let previous_return_type = self.current_return_type;
        self.current_return_type = Some(
            return_type
                .map(ValueType::from_annotation)
                .unwrap_or(ValueType::Unit),
        );

        let mut result = Ok(());
        for nested in body {
            if let Err(error) = self.analyze_statement(nested) {
                result = Err(error);
                break;
            }
        }

        if result.is_ok()
            && self.current_return_type != Some(ValueType::Unit)
            && !self.block_guarantees_return(body)
        {
            result = Err(CompilerError::semantic_message(
                format!("函数 `{name}` 声明了返回值，但并非所有路径都会返回"),
                *span,
            ));
        }

        self.current_return_type = previous_return_type;
        self.pop_frame();
        result
    }

    /// 分析单条语句，并在需要时更新作用域中的变量或上下文状态。
    fn analyze_statement(&mut self, statement: &Stmt) -> Result<(), CompilerError> {
        match statement {
            Stmt::FunctionDecl { span, .. } => Err(CompilerError::semantic_message(
                "函数定义只能出现在顶层",
                *span,
            )),
            Stmt::VarDecl {
                binding_kind,
                name,
                annotation,
                initializer,
                span,
            } => {
                let value_type = self.infer_expression_type(initializer)?;
                if self.current_scope().contains_key(name) {
                    return Err(CompilerError::semantic_message(
                        format!("{} `{name}` 在当前作用域中重复声明", binding_kind.keyword()),
                        *span,
                    ));
                }

                if let Some(annotation) = annotation {
                    let expected = ValueType::from_annotation(*annotation);
                    if expected != value_type {
                        return Err(CompilerError::semantic_message(
                            format!(
                                "{} `{name}` 的类型与初始化表达式不匹配",
                                binding_kind.keyword()
                            ),
                            *span,
                        ));
                    }
                }

                self.current_scope_mut()
                    .insert(name.clone(), SymbolInfo::new(value_type, *binding_kind));
                Ok(())
            }
            Stmt::Assign {
                name,
                op,
                value,
                span,
            } => {
                let symbol = self.lookup(name).ok_or_else(|| {
                    CompilerError::semantic_message(format!("未声明变量 `{name}`"), *span)
                })?;
                if !symbol.is_mutable {
                    return Err(CompilerError::semantic_message(
                        format!("常量 `{name}` 不能重新赋值"),
                        *span,
                    ));
                }
                let expected = symbol.value_type;
                let actual = self.infer_expression_type(value)?;
                let result = self.infer_assignment_type(expected, actual, *op, *span)?;
                if expected != result {
                    return Err(CompilerError::semantic_message(
                        format!("变量 `{name}` 的赋值类型不匹配"),
                        *span,
                    ));
                }
                Ok(())
            }
            Stmt::If {
                branches,
                else_branch,
                ..
            } => {
                for branch in branches {
                    let condition_type = self.infer_expression_type(&branch.condition)?;
                    if condition_type != ValueType::Bool {
                        return Err(CompilerError::semantic_message(
                            "条件表达式必须是布尔值",
                            branch.condition.span(),
                        ));
                    }
                    self.analyze_block(&branch.body)?;
                }

                if let Some(else_branch) = else_branch {
                    self.analyze_block(else_branch)?;
                }

                Ok(())
            }
            Stmt::While {
                condition, body, span, ..
            } => {
                if self.infer_expression_type(condition)? != ValueType::Bool {
                    return Err(CompilerError::semantic_message("条件表达式必须是布尔值", *span));
                }
                self.loop_depth += 1;
                let result = self.analyze_block(body);
                self.loop_depth -= 1;
                result
            }
            Stmt::ForRange {
                name,
                start,
                end,
                body,
                span,
            } => {
                if self.infer_expression_type(start)? != ValueType::Int
                    || self.infer_expression_type(end)? != ValueType::Int
                {
                    return Err(CompilerError::semantic_message(
                        "for 范围的起点和终点必须是整数",
                        *span,
                    ));
                }

                self.loop_depth += 1;
                self.push_scope();
                self.current_scope_mut().insert(
                    name.clone(),
                    SymbolInfo::new(ValueType::Int, BindingKind::Var),
                );
                let result = self.analyze_statements(body);
                self.pop_scope();
                self.loop_depth -= 1;
                result
            }
            Stmt::Break { span } | Stmt::Continue { span } => {
                if self.loop_depth == 0 {
                    return Err(CompilerError::semantic_message(
                        "break 和 continue 只能出现在循环体中",
                        *span,
                    ));
                }
                Ok(())
            }
            Stmt::Return { value, span } => {
                let Some(expected_return_type) = self.current_return_type else {
                    return Err(CompilerError::semantic_message(
                        "return 只能出现在函数体中",
                        *span,
                    ));
                };

                match (expected_return_type, value) {
                    (ValueType::Unit, None) => Ok(()),
                    (ValueType::Unit, Some(_)) => Err(CompilerError::semantic_message(
                        "当前函数没有返回值，return 后不能跟表达式",
                        *span,
                    )),
                    (_, None) => Err(CompilerError::semantic_message(
                        "当前函数声明了返回值，return 后必须跟表达式",
                        *span,
                    )),
                    (expected, Some(expr)) => {
                        let actual = self.infer_expression_type(expr)?;
                        if expected != actual {
                            return Err(CompilerError::semantic_message("返回值类型不匹配", *span));
                        }
                        Ok(())
                    }
                }
            }
            Stmt::ExprStmt { expr, .. } => {
                self.infer_expression_type(expr)?;
                Ok(())
            }
        }
    }

    /// 分析一组语句，并保持块级作用域的生命周期正确结束。
    fn analyze_block(&mut self, statements: &[Stmt]) -> Result<(), CompilerError> {
        self.push_scope();
        let result = self.analyze_statements(statements);
        self.pop_scope();
        result
    }

    /// 顺序分析一个语句数组，直到全部通过或遇到第一条错误语句。
    fn analyze_statements(&mut self, statements: &[Stmt]) -> Result<(), CompilerError> {
        for statement in statements {
            self.analyze_statement(statement)?;
        }
        Ok(())
    }

    /// 推导表达式类型，并在遇到不合法组合时抛出语义错误。
    fn infer_expression_type(&mut self, expression: &Expr) -> Result<ValueType, CompilerError> {
        match expression {
            Expr::Literal { value, .. } => Ok(match value {
                Literal::Int(_) => ValueType::Int,
                Literal::Float(_) => ValueType::Float,
                Literal::Bool(_) => ValueType::Bool,
                Literal::String(_) => ValueType::String,
            }),
            Expr::Identifier { name, span } => self
                .lookup(name)
                .map(|symbol| symbol.value_type)
                .ok_or_else(|| CompilerError::semantic_message(format!("未声明变量 `{name}`"), *span)),
            Expr::Grouped { expression, .. } => self.infer_expression_type(expression),
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
        }
    }

    /// 校验赋值语句或复合赋值语句的结果类型是否合法。
    fn infer_assignment_type(
        &self,
        left: ValueType,
        right: ValueType,
        op: AssignOp,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match op {
            AssignOp::Assign => Ok(right),
            AssignOp::AddAssign => self.infer_binary_value_type(BinaryOp::Add, left, right, span),
            AssignOp::SubtractAssign => {
                self.infer_binary_value_type(BinaryOp::Subtract, left, right, span)
            }
            AssignOp::MultiplyAssign => {
                self.infer_binary_value_type(BinaryOp::Multiply, left, right, span)
            }
            AssignOp::DivideAssign => {
                self.infer_binary_value_type(BinaryOp::Divide, left, right, span)
            }
            AssignOp::ModAssign => self.infer_binary_value_type(BinaryOp::Modulo, left, right, span),
        }
    }

    /// 校验二元表达式两侧类型是否允许参与当前运算。
    fn infer_binary_type(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let left_type = self.infer_expression_type(left)?;
        let right_type = self.infer_expression_type(right)?;
        self.infer_binary_value_type(op, left_type, right_type, span)
    }

    /// 在已知左右值类型的前提下，推导二元表达式的结果类型。
    fn infer_binary_value_type(
        &self,
        op: BinaryOp,
        left: ValueType,
        right: ValueType,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match op {
            BinaryOp::And | BinaryOp::Or => {
                if left == ValueType::Bool && right == ValueType::Bool {
                    Ok(ValueType::Bool)
                } else {
                    Err(CompilerError::semantic_message("逻辑运算只支持布尔值", span))
                }
            }
            BinaryOp::Add => match (left, right) {
                (ValueType::Int, ValueType::Int) => Ok(ValueType::Int),
                (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                (ValueType::String, ValueType::String) => Ok(ValueType::String),
                _ => Err(CompilerError::semantic_message("当前类型不支持加法运算", span)),
            },
            BinaryOp::Subtract | BinaryOp::Multiply => match (left, right) {
                (ValueType::Int, ValueType::Int) => Ok(ValueType::Int),
                (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                _ => Err(CompilerError::semantic_message("当前类型不支持数值运算", span)),
            },
            BinaryOp::Divide => match (left, right) {
                (ValueType::Int, ValueType::Int)
                | (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                _ => Err(CompilerError::semantic_message("当前类型不支持除法运算", span)),
            },
            BinaryOp::Modulo | BinaryOp::IntDivide => {
                if left == ValueType::Int && right == ValueType::Int {
                    Ok(ValueType::Int)
                } else {
                    Err(CompilerError::semantic_message("取模和整数除法只支持整数", span))
                }
            }
            BinaryOp::Equal | BinaryOp::NotEqual => {
                if left == right && left != ValueType::Unit {
                    Ok(ValueType::Bool)
                } else {
                    Err(CompilerError::semantic_message(
                        "相等比较要求两侧类型一致且不能为空类型",
                        span,
                    ))
                }
            }
            BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual => match (left, right) {
                (ValueType::Int, ValueType::Int)
                | (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Bool),
                _ => Err(CompilerError::semantic_message("大小比较只支持数值类型", span)),
            },
        }
    }

    /// 校验一元表达式的操作数类型是否合法，并返回结果类型。
    fn infer_unary_type(
        &mut self,
        op: UnaryOp,
        operand: &Expr,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let operand_type = self.infer_expression_type(operand)?;

        match (op, operand_type) {
            (UnaryOp::Negate, ValueType::Int) => Ok(ValueType::Int),
            (UnaryOp::Negate, ValueType::Float) => Ok(ValueType::Float),
            (UnaryOp::Not, ValueType::Bool) => Ok(ValueType::Bool),
            _ => Err(CompilerError::semantic_message(
                "当前一元表达式的操作数类型不支持该运算",
                span,
            )),
        }
    }

    /// 校验内建函数与用户函数调用是否合法，并返回其结果类型。
    fn infer_call_type(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let Expr::Identifier { name, .. } = callee else {
            return Err(CompilerError::semantic_message("当前阶段只支持调用具名函数", span));
        };

        if name == "print" {
            if arguments.len() != 1 {
                return Err(CompilerError::semantic_message(
                    "内建函数 `print` 需要且只接受一个参数",
                    span,
                ));
            }
            self.infer_expression_type(&arguments[0])?;
            return Ok(ValueType::Unit);
        }

        let signature = self.functions.get(name).cloned().ok_or_else(|| {
            CompilerError::semantic_message(format!("未声明函数 `{name}`"), callee.span())
        })?;

        if signature.parameter_types.len() != arguments.len() {
            return Err(CompilerError::semantic_message(
                format!(
                    "函数 `{name}` 期望 {} 个参数，但实际传入 {} 个",
                    signature.parameter_types.len(),
                    arguments.len()
                ),
                span,
            ));
        }

        for (index, (expected, argument)) in signature
            .parameter_types
            .iter()
            .zip(arguments.iter())
            .enumerate()
        {
            let actual = self.infer_expression_type(argument)?;
            if *expected != actual {
                return Err(CompilerError::semantic_message(
                    format!("函数 `{name}` 的第 {} 个参数类型不匹配", index + 1),
                    argument.span(),
                ));
            }
        }

        Ok(signature.return_type)
    }

    /// 判断一个语句块是否保证所有控制流路径都会返回。
    fn block_guarantees_return(&self, statements: &[Stmt]) -> bool {
        statements
            .iter()
            .any(|statement| self.statement_guarantees_return(statement))
    }

    /// 判断单条语句是否必然终止于 `return`。
    fn statement_guarantees_return(&self, statement: &Stmt) -> bool {
        match statement {
            Stmt::Return { .. } => true,
            Stmt::If {
                branches,
                else_branch,
                ..
            } => {
                !branches.is_empty()
                    && else_branch.is_some()
                    && branches
                        .iter()
                        .all(|branch| self.block_guarantees_return(&branch.body))
                    && else_branch
                        .as_ref()
                        .is_some_and(|body| self.block_guarantees_return(body))
            }
            _ => false,
        }
    }

    /// 在当前函数帧中压入一个新的局部变量作用域。
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// 弹出当前最内层的局部作用域。
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// 为函数分析创建新的栈帧边界，并初始化函数局部作用域。
    fn push_frame(&mut self) {
        self.frame_starts.push(self.scopes.len());
        self.push_scope();
    }

    /// 销毁当前函数分析帧及其所有局部作用域。
    fn pop_frame(&mut self) {
        let frame_start = self
            .frame_starts
            .pop()
            .expect("函数帧栈至少包含顶层帧");
        while self.scopes.len() > frame_start {
            self.scopes.pop();
        }
    }

    /// 在当前帧和全局作用域中按可见性顺序查找变量类型。
    fn lookup(&self, name: &str) -> Option<SymbolInfo> {
        let frame_start = *self.frame_starts.last().expect("帧栈至少包含顶层帧");
        for scope in self.scopes[frame_start..].iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(*symbol);
            }
        }

        if frame_start > 0 {
            return self.scopes[0].get(name).copied();
        }

        None
    }

    /// 返回当前最内层作用域的不可变引用。
    fn current_scope(&self) -> &HashMap<String, SymbolInfo> {
        self.scopes.last().expect("作用域栈至少包含全局作用域")
    }

    /// 返回当前最内层作用域的可变引用。
    fn current_scope_mut(&mut self) -> &mut HashMap<String, SymbolInfo> {
        self.scopes
            .last_mut()
            .expect("作用域栈至少包含全局作用域")
    }
}

#[cfg(test)]
mod tests {
    use super::SemanticAnalyzer;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::settings::{CompilerSettings, UiLanguage};
    use crate::source::SourceFile;

    /// 执行一次完整的语义分析，用于校验成功或失败场景。
    fn analyze(text: &str) -> Result<(), String> {
        let settings = CompilerSettings::new(UiLanguage::ZhCn);
        let source = SourceFile::new("<test>", text.to_string());
        let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
        let program = Parser::new(tokens, &settings).parse_program().unwrap();
        SemanticAnalyzer::new(&settings)
            .analyze(&program)
            .map_err(|error| error.render(settings.language, Some(&source)))
    }

    /// 校验条件表达式必须是布尔值。
    #[test]
    fn should_reject_non_boolean_if_condition() {
        let error = analyze("if 1:\n    print(1)\n").unwrap_err();
        assert!(error.contains("条件表达式必须是布尔值"));
    }

    /// 校验 `break` 只能出现在循环体内部。
    #[test]
    fn should_reject_break_outside_loop() {
        let error = analyze("break\n").unwrap_err();
        assert!(error.contains("只能出现在循环体中"));
    }

    /// 校验函数返回值路径不完整时会被语义分析拦截。
    #[test]
    fn should_reject_missing_return_path() {
        let error = analyze("func add(a:int, b:int) -> int:\n    if true:\n        return a + b\n")
            .unwrap_err();
        assert!(error.contains("并非所有路径都会返回"));
    }

    /// 校验 `/` 在纯整数参与时也会被推导为浮点结果。
    #[test]
    fn should_reject_integer_variable_receiving_true_division_result() {
        let error = analyze("var result:int = 6 / 2\n").unwrap_err();
        assert!(error.contains("类型与初始化表达式不匹配"));
    }

    /// 校验 `%` 和 `%/%` 只接受整数运算数。
    #[test]
    fn should_reject_non_integer_modulo() {
        let error = analyze("print(5.0 % 2)\n").unwrap_err();
        assert!(error.contains("只支持整数"));
    }

    /// 校验常量一旦声明后就不能再次赋值。
    #[test]
    fn should_reject_const_reassignment() {
        let error = analyze("const answer = 42\nanswer = 1\n").unwrap_err();
        assert!(error.contains("常量 `answer` 不能重新赋值"));
    }
}
