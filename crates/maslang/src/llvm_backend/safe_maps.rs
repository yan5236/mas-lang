use super::*;
use crate::ast::{Expr, MethodName};

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译字典安全方法（`try_get`/`try_remove`）；缺键时返回 `ok=false` 或 `false`，
    /// 不触发运行时错误，也不改变字典内容。
    pub(super) fn compile_map_safe_method(
        &mut self,
        _target_value: TypedValue<'ctx>,
        _method: MethodName,
        _arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        Err(CompilerError::build_message("字典安全方法尚未实现", span))
    }
}
