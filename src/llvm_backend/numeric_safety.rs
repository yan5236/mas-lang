use super::*;
use crate::ast::BinaryOp;
use inkwell::values::{FloatValue, IntValue};
use inkwell::{FloatPredicate, IntPredicate};

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 条件不成立时进入带源码定位的错误分支；调用后生成器定位到安全路径，危险指令只能放在其后。
    pub(super) fn emit_safety_check(
        &mut self,
        valid: IntValue<'ctx>,
        code: u64,
        span: Span,
    ) -> Result<(), CompilerError> {
        let function = self.current_function.expect("function context");
        let ready = self.context.append_basic_block(function, "safety.ready");
        let failed = self.context.append_basic_block(function, "safety.failed");
        self.map_builder(self.builder.build_conditional_branch(valid, ready, failed))?;
        self.builder.position_at_end(failed);
        let mut args = vec![self.context.i8_type().const_int(code, false).into()];
        args.extend(self.build_map_error_site_arguments(span)?);
        let error = self
            .module
            .get_function("mas_numeric_error")
            .unwrap_or_else(|| {
                let ptr = self.context.ptr_type(AddressSpace::default()).into();
                let int = self.context.i64_type().into();
                self.module.add_function(
                    "mas_numeric_error",
                    self.context.void_type().fn_type(
                        &[self.context.i8_type().into(), ptr, int, ptr, int, int, int],
                        false,
                    ),
                    None,
                )
            });
        self.map_builder(self.builder.build_call(error, &args, ""))?;
        self.map_builder(self.builder.build_unreachable())?;
        self.builder.position_at_end(ready);
        Ok(())
    }

    /// 在 i128 中计算两个 i64 的加减乘，确认结果可由 i64 表示后截断；避免先溢出再尝试检测。
    pub(super) fn checked_int_arithmetic(
        &mut self,
        op: BinaryOp,
        left: IntValue<'ctx>,
        right: IntValue<'ctx>,
        span: Span,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let wide = self.context.i128_type();
        let left = self.map_builder(self.builder.build_int_s_extend(left, wide, "checked.left"))?;
        let right = self.map_builder(self.builder.build_int_s_extend(
            right,
            wide,
            "checked.right",
        ))?;
        let value = match op {
            BinaryOp::Add => {
                self.map_builder(self.builder.build_int_add(left, right, "checked.add"))?
            }
            BinaryOp::Subtract => {
                self.map_builder(self.builder.build_int_sub(left, right, "checked.sub"))?
            }
            BinaryOp::Multiply => {
                self.map_builder(self.builder.build_int_mul(left, right, "checked.mul"))?
            }
            _ => unreachable!("only add/subtract/multiply"),
        };
        let lower = self.map_builder(self.builder.build_int_compare(
            IntPredicate::SGE,
            value,
            wide.const_int(i64::MIN as u64, true),
            "checked.lower",
        ))?;
        let upper = self.map_builder(self.builder.build_int_compare(
            IntPredicate::SLE,
            value,
            wide.const_int(i64::MAX as u64, false),
            "checked.upper",
        ))?;
        let valid = self.map_builder(self.builder.build_and(lower, upper, "checked.valid"))?;
        self.emit_safety_check(valid, 2, span)?;
        self.map_builder(self.builder.build_int_truncate(
            value,
            self.context.i64_type(),
            "checked.value",
        ))
    }

    /// 整数除法和取余执行前排除零除数及 MIN/-1，防止硬件异常与 LLVM 未定义值。
    pub(super) fn check_int_division(
        &mut self,
        left: IntValue<'ctx>,
        right: IntValue<'ctx>,
        span: Span,
    ) -> Result<(), CompilerError> {
        let int = self.context.i64_type();
        let nonzero = self.map_builder(self.builder.build_int_compare(
            IntPredicate::NE,
            right,
            int.const_zero(),
            "div.nonzero",
        ))?;
        self.emit_safety_check(nonzero, 1, span)?;
        let minimum = self.map_builder(self.builder.build_int_compare(
            IntPredicate::EQ,
            left,
            int.const_int(i64::MIN as u64, true),
            "div.minimum",
        ))?;
        let negative_one = self.map_builder(self.builder.build_int_compare(
            IntPredicate::EQ,
            right,
            int.const_int(-1i64 as u64, true),
            "div.negative_one",
        ))?;
        let overflow = self.map_builder(self.builder.build_and(
            minimum,
            negative_one,
            "div.overflow",
        ))?;
        let valid = self.map_builder(self.builder.build_not(overflow, "div.valid"))?;
        self.emit_safety_check(valid, 2, span)
    }

    /// 浮点转整数前检查有限范围；先将失败输入替换为零再转换，绝不向 fptosi 传入越界或 NaN。
    pub(super) fn checked_float_to_int(
        &self,
        value: FloatValue<'ctx>,
    ) -> Result<(IntValue<'ctx>, IntValue<'ctx>), CompilerError> {
        let float = self.context.f64_type();
        let lower = self.map_builder(self.builder.build_float_compare(
            FloatPredicate::OGE,
            value,
            float.const_float(-9223372036854775808.0),
            "convert.lower",
        ))?;
        let upper = self.map_builder(self.builder.build_float_compare(
            FloatPredicate::OLT,
            value,
            float.const_float(9223372036854775808.0),
            "convert.upper",
        ))?;
        let valid = self.map_builder(self.builder.build_and(lower, upper, "convert.valid"))?;
        let safe = self
            .map_builder(self.builder.build_select(
                valid,
                value,
                float.const_zero(),
                "convert.safe",
            ))?
            .into_float_value();
        let result = self.map_builder(self.builder.build_float_to_signed_int(
            safe,
            self.context.i64_type(),
            "convert.value",
        ))?;
        Ok((valid, result))
    }
}
