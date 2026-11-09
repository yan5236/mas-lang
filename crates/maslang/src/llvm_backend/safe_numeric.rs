use super::*;
use crate::ast::Expr;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译受检整数内建（`try_add`/`try_sub`/`try_mul`/`try_div`/`try_mod`/`try_neg`）。
    /// 名称不属于该组时返回 `None`；失败返回 `ok=false`，危险指令只能在合法分支执行。
    pub(super) fn compile_checked_int_intrinsic(
        &mut self,
        name: &str,
        _arguments: &[Expr],
        span: Span,
    ) -> Result<Option<TypedValue<'ctx>>, CompilerError> {
        match name {
            "try_add" | "try_sub" | "try_mul" | "try_div" | "try_mod" | "try_neg" => Err(
                CompilerError::build_message("受检整数内建尚未实现", span),
            ),
            _ => Ok(None),
        }
    }
}
