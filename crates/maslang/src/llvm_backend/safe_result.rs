use inkwell::values::{BasicValueEnum, IntValue, PointerValue};

use super::*;
use crate::ast::{Expr, MethodName};

/// 编译安全入口共用的结果构造：统一的 `{ ok, value }` 槽位、失败占位值、
/// 受控分支与 GC 根登记。列表、数组和字典都复用这里的实现，避免各自拼接聚合值。
impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 判断失败结果的占位载荷能否被直接读取。
    /// 标量、null/可空引用和字符串（复用有效空串）可以；结构体、列表、字典以及
    /// 含托管元素或字符串元素的数组没有安全零值，`.value` 访问处会先检查 `.ok` 并给出语言诊断。
    pub(super) fn failure_placeholder_supported(value_type: &ValueType) -> bool {
        match value_type {
            ValueType::Int
            | ValueType::Float
            | ValueType::Bool
            | ValueType::String
            | ValueType::Bytes
            | ValueType::Enum(_)
            | ValueType::Null
            | ValueType::Nullable(_)
            | ValueType::Unit => true,
            ValueType::Array(element, _) => Self::zero_placeholder_safe(element),
            ValueType::Struct(_)
            | ValueType::List(_)
            | ValueType::Map(_, _)
            | ValueType::SafeConvertResult(_) => false,
        }
    }

    /// 判断全零占位是否仍可安全读取；字符串的零值是空指针，不能当作有效空串，
    /// 因此字符串元素不出现在这里，只有标量、null 引用和同构标量数组可以。
    fn zero_placeholder_safe(value_type: &ValueType) -> bool {
        match value_type {
            ValueType::Int
            | ValueType::Float
            | ValueType::Bool
            | ValueType::Enum(_)
            | ValueType::Null
            | ValueType::Nullable(_)
            | ValueType::Unit => true,
            ValueType::Array(element, _) => Self::zero_placeholder_safe(element),
            ValueType::String
            | ValueType::Bytes
            | ValueType::Struct(_)
            | ValueType::List(_)
            | ValueType::Map(_, _)
            | ValueType::SafeConvertResult(_) => false,
        }
    }

    /// 构造失败结果的无害占位载荷：字符串使用有效空串，字节使用合法空字节，其余类型使用零值。
    /// 只有 `failure_placeholder_supported` 为真的载荷会被直接读取，因此零值不会
    /// 以无效引用或空指针的形式进入运行时入口。
    pub(super) fn build_safe_failure_placeholder(
        &mut self,
        payload: &ValueType,
    ) -> Result<BasicValueEnum<'ctx>, CompilerError> {
        if matches!(payload, ValueType::String) {
            return Ok(self.build_string_literal("", false)?.into());
        }
        if matches!(payload, ValueType::Bytes) {
            return Ok(self.build_empty_bytes()?.into());
        }
        Ok(self.basic_type_for(payload)?.const_zero())
    }

    /// 在合法分支内生成一次元素访问，并把结果写入带 GC 根的结果槽。
    /// 失败分支只写占位值，不接触容器地址、不删除元素，因此失败后容器状态完全不变。
    pub(super) fn build_guarded_result<F>(
        &mut self,
        valid: IntValue<'ctx>,
        payload: &ValueType,
        prefix: &str,
        build_value: F,
    ) -> Result<TypedValue<'ctx>, CompilerError>
    where
        F: FnOnce(&mut Self) -> Result<BasicValueEnum<'ctx>, CompilerError>,
    {
        let slot = self.allocate_safe_result_slot(payload)?;
        let (ok_block, failed_block, done_block) = self.append_safe_blocks(prefix)?;
        self.map_builder(
            self.builder
                .build_conditional_branch(valid, ok_block, failed_block),
        )?;

        self.builder.position_at_end(failed_block);
        let placeholder = self.build_safe_failure_placeholder(payload)?;
        self.store_safe_result(
            slot,
            payload,
            self.context.bool_type().const_zero(),
            placeholder,
        )?;
        self.map_builder(self.builder.build_unconditional_branch(done_block))?;

        self.builder.position_at_end(ok_block);
        let value = build_value(self)?;
        self.store_safe_result(
            slot,
            payload,
            self.context.bool_type().const_int(1, false),
            value,
        )?;
        self.map_builder(self.builder.build_unconditional_branch(done_block))?;

        self.builder.position_at_end(done_block);
        self.load_safe_result(slot, payload)
    }

    /// 在合法分支内生成一次会报错的修改操作，返回与条件一致的布尔结果（`true` 表示已生效）。
    /// 失败分支不执行任何容器调用，因此长度、容量、元素与 GC 引用关系保持原样。
    pub(super) fn build_guarded_action<F>(
        &mut self,
        valid: IntValue<'ctx>,
        prefix: &str,
        action: F,
    ) -> Result<TypedValue<'ctx>, CompilerError>
    where
        F: FnOnce(&mut Self) -> Result<(), CompilerError>,
    {
        let function = self.current_function.expect("function context");
        let action_block = self
            .context
            .append_basic_block(function, &format!("{prefix}.act"));
        let done_block = self
            .context
            .append_basic_block(function, &format!("{prefix}.done"));
        self.map_builder(
            self.builder
                .build_conditional_branch(valid, action_block, done_block),
        )?;

        self.builder.position_at_end(action_block);
        action(self)?;
        self.map_builder(self.builder.build_unconditional_branch(done_block))?;

        self.builder.position_at_end(done_block);
        Ok(TypedValue {
            value_type: ValueType::Bool,
            value: valid.into(),
        })
    }

    /// 追加安全结果的合法、失败与汇合基本块。
    fn append_safe_blocks(
        &self,
        prefix: &str,
    ) -> Result<
        (
            inkwell::basic_block::BasicBlock<'ctx>,
            inkwell::basic_block::BasicBlock<'ctx>,
            inkwell::basic_block::BasicBlock<'ctx>,
        ),
        CompilerError,
    > {
        let function = self.current_function.expect("function context");
        Ok((
            self.context
                .append_basic_block(function, &format!("{prefix}.ok")),
            self.context
                .append_basic_block(function, &format!("{prefix}.failed")),
            self.context
                .append_basic_block(function, &format!("{prefix}.done")),
        ))
    }

    /// 分配安全结果槽；槽按载荷类型登记 GC 根，托管载荷在语句边界之后仍可被追踪。
    fn allocate_safe_result_slot(
        &self,
        payload: &ValueType,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        self.allocate_local(
            &ValueType::SafeConvertResult(Box::new(payload.clone())),
            "safe.result",
        )
    }

    /// 用统一的 `{ ok, value }` 结构写入结果槽，不手工拼接聚合值。
    fn store_safe_result(
        &self,
        slot: PointerValue<'ctx>,
        payload: &ValueType,
        ok: IntValue<'ctx>,
        value: BasicValueEnum<'ctx>,
    ) -> Result<(), CompilerError> {
        let assembled = self.build_safe_convert_result(payload.clone(), ok, value)?;
        self.map_builder(self.builder.build_store(slot, assembled.value))?;
        Ok(())
    }

    /// 从结果槽读回安全结果，返回带 `try_result<T>` 类型标记的表达式值。
    fn load_safe_result(
        &self,
        slot: PointerValue<'ctx>,
        payload: &ValueType,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let result_type = ValueType::SafeConvertResult(Box::new(payload.clone()));
        let value = self.map_builder(self.builder.build_load(
            self.basic_type_for(&result_type)?,
            slot,
            "load.safe.result",
        ))?;
        Ok(TypedValue {
            value_type: result_type,
            value,
        })
    }

    /// 编译安全方法的下标实参；语义阶段已校验为整数，这里保留显式检查避免绕过前端的调用。
    pub(super) fn compile_safe_index_argument(
        &mut self,
        argument: &Expr,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let compiled = self.compile_expr(argument)?;
        let value = self.expect_value(compiled)?;
        if value.value_type != ValueType::Int {
            return Err(CompilerError::build_message(
                "安全方法的下标参数必须是整数",
                argument.span(),
            ));
        }
        Ok(value.value.into_int_value())
    }

    /// 校验安全方法的实参数量；语义阶段已检查，重复校验用于保护直接调用代码生成入口的场景。
    pub(super) fn expect_safe_argument_count(
        &self,
        arguments: &[Expr],
        expected: usize,
        method: MethodName,
        span: Span,
    ) -> Result<(), CompilerError> {
        if arguments.len() == expected {
            return Ok(());
        }
        Err(CompilerError::build_message(
            format!(
                "安全方法 `{}` 需要 {expected} 个参数，但实际传入 {}",
                method.display_name(),
                arguments.len()
            ),
            span,
        ))
    }
}
