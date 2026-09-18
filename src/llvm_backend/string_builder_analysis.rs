use crate::ast::{AssignOp, AssignTarget, Expr, Stmt};

/// 判断表达式是否引用候选变量，包括下标、参数及容器成员，避免观察未物化的中间字符串。
pub(super) fn reads(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Identifier { name: found, .. } => found == name,
        Expr::Literal { .. } => false,
        Expr::Binary { left, right, .. } => reads(left, name) || reads(right, name),
        Expr::Unary { operand, .. } => reads(operand, name),
        Expr::Grouped { expression, .. } => reads(expression, name),
        Expr::Call {
            callee, arguments, ..
        } => reads(callee, name) || arguments.iter().any(|x| reads(x, name)),
        Expr::MethodCall {
            target, arguments, ..
        } => reads(target, name) || arguments.iter().any(|x| reads(x, name)),
        Expr::ListLiteral { elements, .. } | Expr::ArrayInitializer { elements, .. } => {
            elements.iter().any(|x| reads(x, name))
        }
        Expr::MapLiteral { entries, .. } => entries
            .iter()
            .any(|x| reads(&x.key, name) || reads(&x.value, name)),
        Expr::Index { target, index, .. } => reads(target, name) || reads(index, name),
        Expr::Slice {
            target, start, end, ..
        } => {
            reads(target, name)
                || start.as_ref().is_some_and(|x| reads(x, name))
                || end.as_ref().is_some_and(|x| reads(x, name))
        }
        Expr::PropertyAccess { target, .. } => reads(target, name),
    }
}

/// 保守证明单层循环中只有 += 写候选变量且没有读取、遮蔽或提前返回；嵌套循环由自身独立分析。
pub(super) fn append_only(body: &[Stmt], name: &str) -> bool {
    body.iter().all(|stmt| match stmt {
        Stmt::Assign {
            target, op, value, ..
        } => {
            !reads(value, name)
                && match target {
                    AssignTarget::Identifier { name: found, .. } => {
                        found != name || *op == AssignOp::AddAssign
                    }
                    AssignTarget::Index { target, index, .. } => {
                        !reads(target, name) && !reads(index, name)
                    }
                }
        }
        Stmt::VarDecl {
            name: found,
            initializer,
            ..
        } => found != name && !initializer.as_ref().is_some_and(|x| reads(x, name)),
        Stmt::ExprStmt { expr, .. } => !reads(expr, name),
        Stmt::If {
            branches,
            else_branch,
            ..
        } => {
            branches
                .iter()
                .all(|x| !reads(&x.condition, name) && append_only(&x.body, name))
                && else_branch.as_ref().is_none_or(|x| append_only(x, name))
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => true,
        _ => false,
    })
}

/// 收集当前层及条件分支内实际追加的变量名，保持源码顺序且去重；不跨越嵌套循环。
pub(super) fn candidates(body: &[Stmt], names: &mut Vec<String>) {
    for stmt in body {
        match stmt {
            Stmt::Assign {
                target: AssignTarget::Identifier { name, .. },
                op: AssignOp::AddAssign,
                ..
            } => {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            Stmt::If {
                branches,
                else_branch,
                ..
            } => {
                for branch in branches {
                    candidates(&branch.body, names);
                }
                if let Some(body) = else_branch {
                    candidates(body, names);
                }
            }
            _ => {}
        }
    }
}
