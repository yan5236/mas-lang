use inkwell::IntPredicate;
use inkwell::values::IntValue;

use super::*;
use crate::ast::Expr;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译受检整数内建（`try_add`/`try_sub`/`try_mul`/`try_div`/`try_mod`/`try_neg`）。
    /// 名称不属于该组时返回 `None`；失败返回 `ok=false`，危险指令只能在合法分支执行。
    pub(super) fn compile_checked_int_intrinsic(
        &mut self,
        name: &str,
        arguments: &[Expr],
        span: Span,
    ) -> Result<Option<TypedValue<'ctx>>, CompilerError> {
        let arity = match name {
            "try_add" | "try_sub" | "try_mul" | "try_div" | "try_mod" => 2,
            "try_neg" => 1,
            _ => return Ok(None),
        };
        // 语义阶段已经校验过实参数量和类型，这里重复校验以保护绕过前端的代码生成入口。
        if arguments.len() != arity {
            return Err(CompilerError::build_message(
                format!(
                    "内建函数 `{name}` 需要 {arity} 个整数参数，但实际传入 {}",
                    arguments.len()
                ),
                span,
            ));
        }
        let mut operands = Vec::with_capacity(arity);
        for argument in arguments {
            let compiled = self.compile_expr(argument)?;
            let value = self.expect_value(compiled)?;
            if value.value_type != ValueType::Int {
                return Err(CompilerError::build_message(
                    format!("内建函数 `{name}` 的参数必须是整数"),
                    argument.span(),
                ));
            }
            operands.push(value.value.into_int_value());
        }

        let (ok, value) = match name {
            "try_add" => self.build_checked_overflow_intrinsic(
                "llvm.sadd.with.overflow",
                operands[0],
                operands[1],
            )?,
            "try_sub" => self.build_checked_overflow_intrinsic(
                "llvm.ssub.with.overflow",
                operands[0],
                operands[1],
            )?,
            "try_mul" => self.build_checked_overflow_intrinsic(
                "llvm.smul.with.overflow",
                operands[0],
                operands[1],
            )?,
            "try_div" => self.build_checked_int_division(operands[0], operands[1], true)?,
            "try_mod" => self.build_checked_int_division(operands[0], operands[1], false)?,
            _ => {
                // 取负等价于 `0 - a`，`ssub.with.overflow` 只在 `a == MIN` 时报告溢出。
                let zero = self.context.i64_type().const_zero();
                self.build_checked_overflow_intrinsic("llvm.ssub.with.overflow", zero, operands[0])?
            }
        };
        let result = self.build_safe_convert_result(ValueType::Int, ok, value.into())?;
        Ok(Some(result))
    }

    /// 调用 i64 有符号溢出 intrinsic，同时取回绕结果与合法标记；失败时不生成任何错误分支。
    fn build_checked_overflow_intrinsic(
        &mut self,
        intrinsic: &str,
        left: IntValue<'ctx>,
        right: IntValue<'ctx>,
    ) -> Result<(IntValue<'ctx>, IntValue<'ctx>), CompilerError> {
        let function = inkwell::intrinsics::Intrinsic::find(intrinsic)
            .expect("LLVM signed overflow intrinsic")
            .get_declaration(&self.module, &[self.context.i64_type().into()])
            .expect("i64 overflow intrinsic declaration");
        let call = self.map_builder(self.builder.build_call(
            function,
            &[left.into(), right.into()],
            "checked.int.call",
        ))?;
        let result = call.try_as_basic_value().unwrap_basic().into_struct_value();
        let wrapped = self
            .map_builder(
                self.builder
                    .build_extract_value(result, 0, "checked.int.value"),
            )?
            .into_int_value();
        let overflow = self
            .map_builder(
                self.builder
                    .build_extract_value(result, 1, "checked.int.overflow"),
            )?
            .into_int_value();
        let ok = self.map_builder(self.builder.build_not(overflow, "checked.int.ok"))?;
        let value = self.build_zero_placeholder(ok, wrapped)?;
        Ok((ok, value))
    }

    /// 生成受检整除/取余：零除数与 `MIN / -1` 走失败分支，`sdiv`/`srem` 只在合法分支执行。
    /// 两条分支在汇合块用 phi 选择 `ok` 与负载，失败时负载为 0，不使用可能触发 SIGFPE 的推测执行。
    fn build_checked_int_division(
        &mut self,
        left: IntValue<'ctx>,
        right: IntValue<'ctx>,
        is_division: bool,
    ) -> Result<(IntValue<'ctx>, IntValue<'ctx>), CompilerError> {
        let function = self.current_function.expect("function context");
        let int = self.context.i64_type();
        let failed_block = self
            .context
            .append_basic_block(function, "checked.div.failed");
        let safe_block = self
            .context
            .append_basic_block(function, "checked.div.safe");
        let merge_block = self
            .context
            .append_basic_block(function, "checked.div.merge");

        let zero = int.const_zero();
        let nonzero = self.map_builder(self.builder.build_int_compare(
            IntPredicate::NE,
            right,
            zero,
            "checked.div.nonzero",
        ))?;
        let minimum = self.map_builder(self.builder.build_int_compare(
            IntPredicate::EQ,
            left,
            int.const_int(i64::MIN as u64, true),
            "checked.div.minimum",
        ))?;
        let negative_one = self.map_builder(self.builder.build_int_compare(
            IntPredicate::EQ,
            right,
            int.const_int(-1i64 as u64, true),
            "checked.div.negative_one",
        ))?;
        let overflow = self.map_builder(self.builder.build_and(
            minimum,
            negative_one,
            "checked.div.overflow",
        ))?;
        let no_overflow =
            self.map_builder(self.builder.build_not(overflow, "checked.div.no_overflow"))?;
        let usable = self.map_builder(self.builder.build_and(
            nonzero,
            no_overflow,
            "checked.div.usable",
        ))?;
        self.map_builder(
            self.builder
                .build_conditional_branch(usable, safe_block, failed_block),
        )?;

        self.builder.position_at_end(failed_block);
        self.map_builder(self.builder.build_unconditional_branch(merge_block))?;

        self.builder.position_at_end(safe_block);
        let computed = if is_division {
            self.map_builder(
                self.builder
                    .build_int_signed_div(left, right, "checked.div.value"),
            )?
        } else {
            self.map_builder(
                self.builder
                    .build_int_signed_rem(left, right, "checked.div.value"),
            )?
        };
        self.map_builder(self.builder.build_unconditional_branch(merge_block))?;

        self.builder.position_at_end(merge_block);
        let ok_false = self.context.bool_type().const_zero();
        let ok_true = self.context.bool_type().const_int(1, false);
        let ok = self.map_builder(
            self.builder
                .build_phi(self.context.bool_type(), "checked.div.ok"),
        )?;
        ok.add_incoming(&[(&ok_false, failed_block), (&ok_true, safe_block)]);
        let placeholder = int.const_zero();
        let value = self.map_builder(self.builder.build_phi(int, "checked.div.value"))?;
        value.add_incoming(&[(&placeholder, failed_block), (&computed, safe_block)]);
        Ok((
            ok.as_basic_value().into_int_value(),
            value.as_basic_value().into_int_value(),
        ))
    }

    /// 失败时把负载替换为占位 0：回绕结果或非法运算结果只在 `ok=true` 分支对外可见。
    fn build_zero_placeholder(
        &self,
        ok: IntValue<'ctx>,
        value: IntValue<'ctx>,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let zero = self.context.i64_type().const_zero();
        Ok(self
            .map_builder(
                self.builder
                    .build_select(ok, value, zero, "checked.int.placeholder"),
            )?
            .into_int_value())
    }
}
