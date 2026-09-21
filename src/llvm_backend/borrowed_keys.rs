use super::string_builder_analysis::reads;
use super::*;
use crate::ast::{AssignTarget, Expr};

/// 仅允许候选名作为直接查询键出现；普通参数、返回字符串、容器值及别名一律拒绝。
fn safe_expr(expr: &Expr, name: &str) -> bool {
    if !reads(expr, name) {
        return true;
    }
    let key = |expr: &Expr| matches!(expr, Expr::Identifier { name: found, .. } if found == name);
    match expr {
        Expr::Index { target, index, .. } => !reads(target, name) && key(index),
        Expr::MethodCall {
            target,
            method,
            arguments,
            ..
        } => {
            !reads(target, name)
                && *method == crate::ast::MethodName::Contains
                && arguments.len() == 1
                && key(&arguments[0])
        }
        Expr::Binary { left, right, .. } => safe_expr(left, name) && safe_expr(right, name),
        Expr::Unary { operand, .. } => safe_expr(operand, name),
        Expr::Grouped { expression, .. } => safe_expr(expression, name),
        _ => false,
    }
}

/// 验证声明之后的整个词法块只借用键；拒绝重赋值、遮蔽、嵌套循环及未建模的语句。
fn key_only(body: &[Stmt], name: &str) -> bool {
    body.iter().all(|stmt| match stmt {
        Stmt::VarDecl { name: found, initializer, .. } => found != name && initializer.as_ref().is_none_or(|x| safe_expr(x, name)),
        Stmt::Assign { target, value, .. } => safe_expr(value, name) && match target {
            AssignTarget::Identifier { name: found, .. } => found != name,
            AssignTarget::Index { target, index, .. } => !reads(target, name) &&
                (!reads(index, name) || matches!(index.as_ref(), Expr::Identifier { name: found, .. } if found == name)),
        },
        Stmt::ExprStmt { expr, .. } => safe_expr(expr, name),
        Stmt::Return { value, .. } => value.as_ref().is_none_or(|x| safe_expr(x, name)),
        Stmt::If { branches, else_branch, .. } => branches.iter().all(|b| safe_expr(&b.condition, name) && key_only(&b.body, name))
            && else_branch.as_ref().is_none_or(|b| key_only(b, name)),
        Stmt::Break { .. } | Stmt::Continue { .. } => true,
        _ => false,
    })
}

/// 找出仅作查询/赋值键的局部 string(...) 声明；具体参数类型由正常转换入口检查，不改变求值次数。
pub(super) fn candidate(statement: &Stmt, rest: &[Stmt]) -> Option<Span> {
    if let Stmt::VarDecl {
        name,
        initializer:
            Some(Expr::Call {
                callee,
                arguments,
                span,
            }),
        ..
    } = statement
    {
        if matches!(callee.as_ref(), Expr::Identifier { name, .. } if name == "string")
            && arguments.len() == 1
            && key_only(rest, name)
        {
            return Some(*span);
        }
    }
    None
}

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 在函数入口分配独立 20 字节缓冲，每次执行重新格式化 i64；结果仅在已证明的当前块内借用。
    pub(super) fn build_borrowed_int_key(
        &mut self,
        argument: TypedValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let function = self
            .module
            .get_function("int_to_string_into")
            .unwrap_or_else(|| {
                self.module.add_function(
                    "int_to_string_into",
                    self.context
                        .i64_type()
                        .fn_type(&[self.context.i64_type().into(), ptr_type.into()], false),
                    None,
                )
            });
        let buffer =
            self.allocate_entry_slot(self.context.i8_type().array_type(20).into(), "key.bytes")?;
        let call = self.map_builder(self.builder.build_call(
            function,
            &[argument.value.into(), buffer.into()],
            "key.length",
        ))?;
        let value = self
            .build_runtime_string(
                buffer,
                call.try_as_basic_value().unwrap_basic().into_int_value(),
            )?
            .into();
        self.borrowed_values.insert(value);
        Ok(TypedValue {
            value_type: ValueType::String,
            value,
        })
    }

    /// 借用键改用同签名写入包装；命中直接更新，缺失才复制托管键，避免独立拥有化造成第二次查找。
    pub(super) fn borrowed_key_setter(
        &self,
        function: FunctionValue<'ctx>,
        key: &TypedValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        if !self.borrowed_values.contains(&key.value) {
            return function;
        }
        let name = format!("{}_borrowed", function.get_name().to_string_lossy());
        self.module
            .get_function(&name)
            .unwrap_or_else(|| self.module.add_function(&name, function.get_type(), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer::Lexer, parser::Parser};

    /// 用真实解析器检查允许的键位置及逃逸/遮蔽/嵌套循环的拒绝边界，避免优化依赖基准名称。
    #[test]
    fn recognizes_only_non_escaping_key_uses() {
        let settings = CompilerSettings::default_settings();
        for (tail, eligible) in [
            ("m[key] = m[key] + 1\n", true),
            ("if m.contains(key):\n    m[key] = 1\n", true),
            ("return key\n", false),
            ("var alias = key\n", false),
            ("capture(key)\n", false),
            ("values.append(key)\n", false),
            ("m[key] = key\n", false),
            ("key = string(2)\n", false),
            ("if true:\n    var key = string(2)\n", false),
            ("for i in 0..2:\n    m[key] = 1\n", false),
        ] {
            let source = SourceFile::new("<borrowed-key>", format!("var key = string(1)\n{tail}"));
            let tokens = Lexer::new(&source, &settings).tokenize().unwrap();
            let program = Parser::new(tokens, &settings).parse_program().unwrap();
            assert_eq!(
                candidate(&program.statements[0], &program.statements[1..]).is_some(),
                eligible,
                "{tail}"
            );
        }
    }
}
