use std::collections::HashMap;

use crate::ast::Stmt;

use super::{SemanticAnalyzer, SymbolInfo};

impl<'a> SemanticAnalyzer<'a> {
    /// 判断一个语句块是否保证所有控制流路径都会返回。
    pub(super) fn block_guarantees_return(&self, statements: &[Stmt]) -> bool {
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
    pub(super) fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// 弹出当前最内层的局部作用域。
    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// 为函数分析创建新的栈帧边界，并初始化函数局部作用域。
    pub(super) fn push_frame(&mut self) {
        self.frame_starts.push(self.scopes.len());
        self.push_scope();
    }

    /// 销毁当前函数分析帧及其所有局部作用域。
    pub(super) fn pop_frame(&mut self) {
        let frame_start = self.frame_starts.pop().expect("函数帧栈至少包含顶层帧");
        while self.scopes.len() > frame_start {
            self.scopes.pop();
        }
    }

    /// 在当前帧和全局作用域中按可见性顺序查找变量。
    pub(super) fn lookup(&self, name: &str) -> Option<&SymbolInfo> {
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
    pub(super) fn current_scope(&self) -> &HashMap<String, SymbolInfo> {
        self.scopes.last().expect("作用域栈至少包含全局作用域")
    }

    /// 返回当前最内层作用域的可变引用。
    pub(super) fn current_scope_mut(&mut self) -> &mut HashMap<String, SymbolInfo> {
        self.scopes.last_mut().expect("作用域栈至少包含全局作用域")
    }
}
