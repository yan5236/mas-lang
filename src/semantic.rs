use std::collections::HashMap;

use crate::ast::{
    AssignOp, AssignTarget, BinaryOp, BindingKind, Expr, Literal, MethodName, Program, Stmt,
    TypeName, UnaryOp,
};
use crate::diagnostic::CompilerError;
use crate::settings::CompilerSettings;
use crate::source::Span;

/// 表示语义分析阶段内部使用的值类型。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueType {
    Int,
    Float,
    Bool,
    String,
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
    pub span: Span,
}

/// 保存当前作用域中单个名称的类型与可变性元数据。
#[derive(Clone, Debug)]
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

            if name == "print" || name == "len" {
                return Err(CompilerError::semantic_message(
                    format!("用户函数不能覆盖内建函数 `{name}`"),
                    *span,
                ));
            }

            if self.functions.contains_key(name) {
                return Err(CompilerError::semantic_message(
                    format!("函数 `{name}` 重复定义"),
                    *span,
                ));
            }

            let mut parameter_types = Vec::with_capacity(parameters.len());
            for parameter in parameters {
                parameter_types
                    .push(self.value_type_from_annotation(&parameter.type_name, parameter.span)?);
            }
            let return_type = if let Some(annotation) = return_type {
                self.value_type_from_annotation(annotation, *span)?
            } else {
                ValueType::Unit
            };

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
            let parameter_type =
                self.value_type_from_annotation(&parameter.type_name, parameter.span)?;
            self.current_scope_mut().insert(
                parameter.name.clone(),
                SymbolInfo::new(parameter_type, BindingKind::Var),
            );
        }

        let previous_return_type = self.current_return_type.clone();
        self.current_return_type = Some(if let Some(annotation) = return_type {
            self.value_type_from_annotation(annotation, *span)?
        } else {
            ValueType::Unit
        });

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
                let expected = if let Some(annotation) = annotation {
                    Some(self.value_type_from_annotation(annotation, *span)?)
                } else {
                    None
                };
                let value_type = if let Some(initializer) = initializer {
                    self.infer_expression_type_with_hint(initializer, expected.as_ref())?
                } else {
                    if *binding_kind == BindingKind::Const {
                        return Err(CompilerError::semantic_message(
                            "常量声明必须提供初始化表达式",
                            *span,
                        ));
                    }
                    match expected.clone() {
                        Some(ValueType::Array(_, _)) => {
                            expected.clone().expect("前一分支已经确认数组类型存在")
                        }
                        Some(_) => {
                            return Err(CompilerError::semantic_message(
                                "当前只有数组类型允许省略初始化表达式",
                                *span,
                            ));
                        }
                        None => {
                            return Err(CompilerError::semantic_message(
                                "省略初始化表达式时必须提供数组类型标注",
                                *span,
                            ));
                        }
                    }
                };
                if self.current_scope().contains_key(name) {
                    return Err(CompilerError::semantic_message(
                        format!("{} `{name}` 在当前作用域中重复声明", binding_kind.keyword()),
                        *span,
                    ));
                }

                if let Some(expected) = expected {
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
                target,
                op,
                value,
                span,
            } => match target {
                AssignTarget::Identifier { name, .. } => {
                    let symbol = self.lookup(name).ok_or_else(|| {
                        CompilerError::semantic_message(format!("未声明变量 `{name}`"), *span)
                    })?;
                    if !symbol.is_mutable {
                        return Err(CompilerError::semantic_message(
                            format!("常量 `{name}` 不能重新赋值"),
                            *span,
                        ));
                    }
                    let expected = symbol.value_type.clone();
                    let actual = self.infer_expression_type_with_hint(value, Some(&expected))?;
                    let result = self.infer_assignment_type(&expected, &actual, *op, *span)?;
                    if expected != result {
                        return Err(CompilerError::semantic_message(
                            format!("变量 `{name}` 的赋值类型不匹配"),
                            *span,
                        ));
                    }
                    Ok(())
                }
                AssignTarget::Index { target, index, .. } => {
                    if *op != AssignOp::Assign {
                        return Err(CompilerError::semantic_message(
                            "当前阶段下标赋值只支持 `=`",
                            *span,
                        ));
                    }
                    let target_type = self.infer_expression_type(target)?;
                    let ValueType::Map(key_type, value_type) = target_type else {
                        return Err(CompilerError::semantic_message(
                            "下标赋值当前只支持字典类型",
                            *span,
                        ));
                    };
                    let actual_key =
                        self.infer_expression_type_with_hint(index, Some(key_type.as_ref()))?;
                    if actual_key != *key_type {
                        return Err(CompilerError::semantic_message(
                            "字典键类型与下标表达式不匹配",
                            index.span(),
                        ));
                    }
                    let actual_value =
                        self.infer_expression_type_with_hint(value, Some(value_type.as_ref()))?;
                    if actual_value != *value_type {
                        return Err(CompilerError::semantic_message(
                            "字典值类型与赋值表达式不匹配",
                            value.span(),
                        ));
                    }
                    Ok(())
                }
            },
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
                condition,
                body,
                span,
                ..
            } => {
                if self.infer_expression_type(condition)? != ValueType::Bool {
                    return Err(CompilerError::semantic_message(
                        "条件表达式必须是布尔值",
                        *span,
                    ));
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
            Stmt::ForEach {
                key_name,
                value_name,
                iterable,
                body,
                span,
            } => {
                let iterable_type = self.infer_expression_type(iterable)?;
                self.loop_depth += 1;
                self.push_scope();
                match iterable_type {
                    ValueType::List(element_type) => {
                        if value_name.is_some() {
                            self.pop_scope();
                            self.loop_depth -= 1;
                            return Err(CompilerError::semantic_message(
                                "列表遍历只能声明一个循环变量",
                                *span,
                            ));
                        }
                        self.current_scope_mut().insert(
                            key_name.clone(),
                            SymbolInfo::new((*element_type).clone(), BindingKind::Var),
                        );
                    }
                    ValueType::Map(key_type, value_type) => {
                        let Some(value_name) = value_name else {
                            self.pop_scope();
                            self.loop_depth -= 1;
                            return Err(CompilerError::semantic_message(
                                "遍历字典时必须同时声明键和值变量",
                                *span,
                            ));
                        };
                        self.current_scope_mut().insert(
                            key_name.clone(),
                            SymbolInfo::new((*key_type).clone(), BindingKind::Var),
                        );
                        self.current_scope_mut().insert(
                            value_name.clone(),
                            SymbolInfo::new((*value_type).clone(), BindingKind::Var),
                        );
                    }
                    _ => {
                        self.pop_scope();
                        self.loop_depth -= 1;
                        return Err(CompilerError::semantic_message(
                            "for 遍历当前只支持列表或字典类型",
                            *span,
                        ));
                    }
                }
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
                let Some(expected_return_type) = self.current_return_type.clone() else {
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
                        let actual = self.infer_expression_type_with_hint(expr, Some(&expected))?;
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
        self.infer_expression_type_with_hint(expression, None)
    }

    /// 在可用时利用期望类型提示表达式类型推导，主要用于空列表字面量。
    fn infer_expression_type_with_hint(
        &mut self,
        expression: &Expr,
        hint: Option<&ValueType>,
    ) -> Result<ValueType, CompilerError> {
        match expression {
            Expr::Literal { value, .. } => Ok(match value {
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
        }
    }

    /// 把语法树中的类型标注转换为语义阶段使用的值类型。
    fn value_type_from_annotation(
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
    fn ensure_map_key_type(&self, value_type: &ValueType, span: Span) -> Result<(), CompilerError> {
        self.ensure_scalar_type(value_type, "当前阶段字典键只支持基础类型", span)
    }

    /// 校验给定类型是否属于当前阶段允许进入容器的基础类型。
    fn ensure_scalar_type(
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

    /// 推导列表字面量的元素类型，并校验元素必须同构。
    fn infer_list_literal_type(
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
        if matches!(first_type, ValueType::List(_) | ValueType::Array(_, _)) {
            return Err(CompilerError::semantic_message(
                "当前阶段列表元素只支持基础类型",
                span,
            ));
        }

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
    fn infer_map_literal_type(
        &mut self,
        entries: &[crate::ast::MapEntry],
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
        self.ensure_scalar_type(
            &first_value,
            "当前阶段字典值只支持基础类型",
            entries[0].value.span(),
        )?;

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
    fn infer_array_initializer_type(
        &mut self,
        element_type: &TypeName,
        size: usize,
        elements: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let expected_element = self.value_type_from_annotation(element_type, span)?;
        if !matches!(
            expected_element,
            ValueType::Int | ValueType::Float | ValueType::Bool | ValueType::String
        ) {
            return Err(CompilerError::semantic_message(
                "当前阶段数组元素只支持基础类型",
                span,
            ));
        }

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
    fn infer_index_type(
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
    fn infer_slice_type(
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

    /// 推导对象方法调用的返回值类型，并根据目标类型分派到列表或字典规则。
    fn infer_method_type(
        &mut self,
        target: &Expr,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        let target_type = self.infer_expression_type(target)?;
        match target_type {
            ValueType::List(element_type) => {
                self.infer_list_method_type(element_type, method, arguments, span)
            }
            ValueType::Map(key_type, value_type) => {
                self.infer_map_method_type(key_type, value_type, method, arguments, span)
            }
            _ => Err(CompilerError::semantic_message(
                "当前方法只能用于列表或字典类型",
                span,
            )),
        }
    }

    /// 推导列表方法调用的返回值类型，并校验方法参数规则。
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
                if !arguments.is_empty() {
                    return Err(CompilerError::semantic_message(
                        format!("列表方法 `{}` 不接受参数", method.display_name()),
                        span,
                    ));
                }
                Ok(ValueType::Unit)
            }
            MethodName::Keys | MethodName::Values => Err(CompilerError::semantic_message(
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
            | MethodName::Index => Err(CompilerError::semantic_message(
                format!("字典类型不支持方法 `{}`", method.display_name()),
                span,
            )),
        }
    }

    /// 校验赋值语句或复合赋值语句的结果类型是否合法。
    fn infer_assignment_type(
        &self,
        left: &ValueType,
        right: &ValueType,
        op: AssignOp,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match op {
            AssignOp::Assign => Ok(right.clone()),
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
            AssignOp::ModAssign => {
                self.infer_binary_value_type(BinaryOp::Modulo, left, right, span)
            }
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
        self.infer_binary_value_type(op, &left_type, &right_type, span)
    }

    /// 在已知左右值类型的前提下，推导二元表达式的结果类型。
    fn infer_binary_value_type(
        &self,
        op: BinaryOp,
        left: &ValueType,
        right: &ValueType,
        span: Span,
    ) -> Result<ValueType, CompilerError> {
        match op {
            BinaryOp::And | BinaryOp::Or => {
                if left == &ValueType::Bool && right == &ValueType::Bool {
                    Ok(ValueType::Bool)
                } else {
                    Err(CompilerError::semantic_message(
                        "逻辑运算只支持布尔值",
                        span,
                    ))
                }
            }
            BinaryOp::Add => match (left, right) {
                (ValueType::Int, ValueType::Int) => Ok(ValueType::Int),
                (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                (ValueType::String, ValueType::String) => Ok(ValueType::String),
                _ => Err(CompilerError::semantic_message(
                    "当前类型不支持加法运算",
                    span,
                )),
            },
            BinaryOp::Subtract | BinaryOp::Multiply => match (left, right) {
                (ValueType::Int, ValueType::Int) => Ok(ValueType::Int),
                (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                _ => Err(CompilerError::semantic_message(
                    "当前类型不支持数值运算",
                    span,
                )),
            },
            BinaryOp::Divide => match (left, right) {
                (ValueType::Int, ValueType::Int)
                | (ValueType::Float, ValueType::Float)
                | (ValueType::Float, ValueType::Int)
                | (ValueType::Int, ValueType::Float) => Ok(ValueType::Float),
                _ => Err(CompilerError::semantic_message(
                    "当前类型不支持除法运算",
                    span,
                )),
            },
            BinaryOp::Modulo | BinaryOp::IntDivide => {
                if left == &ValueType::Int && right == &ValueType::Int {
                    Ok(ValueType::Int)
                } else {
                    Err(CompilerError::semantic_message(
                        "取模和整数除法只支持整数",
                        span,
                    ))
                }
            }
            BinaryOp::Equal | BinaryOp::NotEqual => {
                if left == right
                    && left != &ValueType::Unit
                    && !matches!(
                        left,
                        ValueType::List(_) | ValueType::Map(_, _) | ValueType::Array(_, _)
                    )
                {
                    Ok(ValueType::Bool)
                } else {
                    Err(CompilerError::semantic_message(
                        "相等比较要求两侧类型一致且不能为空类型、列表类型、字典类型或数组类型",
                        span,
                    ))
                }
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                match (left, right) {
                    (ValueType::Int, ValueType::Int)
                    | (ValueType::Float, ValueType::Float)
                    | (ValueType::Float, ValueType::Int)
                    | (ValueType::Int, ValueType::Float) => Ok(ValueType::Bool),
                    _ => Err(CompilerError::semantic_message(
                        "大小比较只支持数值类型",
                        span,
                    )),
                }
            }
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
            return Err(CompilerError::semantic_message(
                "当前阶段只支持调用具名函数",
                span,
            ));
        };

        if name == "print" {
            if arguments.len() != 1 {
                return Err(CompilerError::semantic_message(
                    "内建函数 `print` 需要且只接受一个参数",
                    span,
                ));
            }
            let argument_type = self.infer_expression_type(&arguments[0])?;
            if matches!(
                argument_type,
                ValueType::Unit
                    | ValueType::List(_)
                    | ValueType::Map(_, _)
                    | ValueType::Array(_, _)
            ) {
                return Err(CompilerError::semantic_message(
                    "当前阶段 `print` 只支持打印基础类型",
                    arguments[0].span(),
                ));
            }
            return Ok(ValueType::Unit);
        }

        if name == "len" {
            if arguments.len() != 1 {
                return Err(CompilerError::semantic_message(
                    "内建函数 `len` 需要且只接受一个参数",
                    span,
                ));
            }
            let argument_type = self.infer_expression_type(&arguments[0])?;
            if !matches!(argument_type, ValueType::List(_) | ValueType::Map(_, _)) {
                return Err(CompilerError::semantic_message(
                    "当前阶段 `len` 只支持列表或字典类型",
                    arguments[0].span(),
                ));
            }
            return Ok(ValueType::Int);
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
            let actual = self.infer_expression_type_with_hint(argument, Some(expected))?;
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
        let frame_start = self.frame_starts.pop().expect("函数帧栈至少包含顶层帧");
        while self.scopes.len() > frame_start {
            self.scopes.pop();
        }
    }

    /// 在当前帧和全局作用域中按可见性顺序查找变量。
    fn lookup(&self, name: &str) -> Option<&SymbolInfo> {
        let frame_start = *self.frame_starts.last().expect("帧栈至少包含顶层帧");
        for scope in self.scopes[frame_start..].iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(symbol);
            }
        }

        if frame_start > 0 {
            return self.scopes[0].get(name);
        }

        None
    }

    /// 返回当前最内层作用域的不可变引用。
    fn current_scope(&self) -> &HashMap<String, SymbolInfo> {
        self.scopes.last().expect("作用域栈至少包含全局作用域")
    }

    /// 返回当前最内层作用域的可变引用。
    fn current_scope_mut(&mut self) -> &mut HashMap<String, SymbolInfo> {
        self.scopes.last_mut().expect("作用域栈至少包含全局作用域")
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

    /// 校验非同构列表字面量会在语义阶段被拒绝。
    #[test]
    fn should_reject_mixed_list_literal() {
        let error = analyze("var items = [1, true]\n").unwrap_err();
        assert!(error.contains("列表字面量中的元素类型必须保持一致"));
    }

    /// 校验空列表字面量必须带有显式类型上下文。
    #[test]
    fn should_reject_empty_list_without_annotation() {
        let error = analyze("var items = []\n").unwrap_err();
        assert!(error.contains("空列表字面量必须放在显式 `list<T>` 类型上下文中"));
    }

    /// 校验列表遍历会把循环变量绑定为元素类型。
    #[test]
    fn should_accept_for_each_on_list() {
        analyze("var items:list<int> = [1, 2]\nfor item in items:\n    print(item)\n").unwrap();
    }

    /// 校验字典遍历会把循环变量分别绑定为键类型和值类型。
    #[test]
    fn should_accept_for_each_on_map() {
        analyze(
            "var user:map<string,int> = {\"age\": 18}\nfor key, value in user:\n    print(key)\n    print(value)\n",
        )
        .unwrap();
    }

    /// 校验列表方法调用的参数类型需要和元素类型一致。
    #[test]
    fn should_reject_invalid_list_method_argument_type() {
        let error = analyze("var items:list<int> = [1]\nitems.append(true)\n").unwrap_err();
        assert!(error.contains("参数类型不匹配"));
    }

    /// 校验空字典字面量必须带有显式类型上下文。
    #[test]
    fn should_reject_empty_map_without_annotation() {
        let error = analyze("var user = {}\n").unwrap_err();
        assert!(error.contains("空字典字面量必须放在显式 `map<K,V>` 类型上下文中"));
    }

    /// 校验字典下标类型必须和声明的键类型一致。
    #[test]
    fn should_reject_invalid_map_index_type() {
        let error =
            analyze("var user:map<string,int> = {\"age\": 18}\nprint(user[1])\n").unwrap_err();
        assert!(error.contains("字典下标的键类型不匹配"));
    }

    /// 校验字典方法调用的参数类型需要和键类型一致。
    #[test]
    fn should_reject_invalid_map_method_argument_type() {
        let error =
            analyze("var user:map<string,int> = {\"age\": 18}\nuser.contains(1)\n").unwrap_err();
        assert!(error.contains("参数类型不匹配"));
    }

    /// 校验数组默认声明和元素访问能够通过语义分析。
    #[test]
    fn should_accept_array_declaration_and_index() {
        analyze("var fixed:array<int,3>\nvar values = array<int,3>(1, 2, 3)\nprint(values[1])\n")
            .unwrap();
    }

    /// 校验数组初始化的元素数量必须和固定长度完全一致。
    #[test]
    fn should_reject_array_initializer_length_mismatch() {
        let error = analyze("var values = array<int,3>(1, 2)\n").unwrap_err();
        assert!(error.contains("数组初始化需要 3 个元素"));
    }
}
