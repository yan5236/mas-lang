use super::*;
use crate::ast::{Expr, MethodName};

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译列表安全方法（`try_get`/`try_set`/`try_pop`/`try_index`/`try_remove`/`try_slice`）。
    /// 失败必须返回 `ok=false` 或 `false`，不得触发运行时错误，也不得改动原列表。
    pub(super) fn compile_list_safe_method(
        &mut self,
        _target_value: TypedValue<'ctx>,
        _method: MethodName,
        _arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        Err(CompilerError::build_message("列表安全方法尚未实现", span))
    }

    /// 编译固定数组安全方法（`try_get`/`try_set`）；越界返回失败状态且不访问元素地址。
    pub(super) fn compile_array_safe_method(
        &mut self,
        _target: &Expr,
        _target_value: TypedValue<'ctx>,
        _method: MethodName,
        _arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        Err(CompilerError::build_message("数组安全方法尚未实现", span))
    }
}
