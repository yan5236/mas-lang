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

    /// 使用 LLVM 有符号溢出 intrinsic 同时得到 i64 结果和溢出位；失败仍在结果使用前报告源码位置。
    /// intrinsic 的回绕结果是已定义值，仅在无溢出分支使用，不引入 nsw 假设或扩大语言整数范围。
    pub(super) fn checked_int_arithmetic(
        &mut self,
        op: BinaryOp,
        left: IntValue<'ctx>,
        right: IntValue<'ctx>,
        span: Span,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let name = match op {
            BinaryOp::Add => "llvm.sadd.with.overflow",
            BinaryOp::Subtract => "llvm.ssub.with.overflow",
            BinaryOp::Multiply => "llvm.smul.with.overflow",
            _ => unreachable!("only add/subtract/multiply"),
        };
        let function = inkwell::intrinsics::Intrinsic::find(name)
            .expect("LLVM signed overflow intrinsic")
            .get_declaration(&self.module, &[self.context.i64_type().into()])
            .expect("i64 overflow intrinsic declaration");
        let call = self.map_builder(self.builder.build_call(
            function,
            &[left.into(), right.into()],
            "checked.arithmetic",
        ))?;
        let result = call.try_as_basic_value().unwrap_basic().into_struct_value();
        let value = self
            .map_builder(self.builder.build_extract_value(result, 0, "checked.value"))?
            .into_int_value();
        let overflow = self
            .map_builder(
                self.builder
                    .build_extract_value(result, 1, "checked.overflow"),
            )?
            .into_int_value();
        let valid = self.map_builder(self.builder.build_not(overflow, "checked.valid"))?;
        self.emit_safety_check(valid, 2, span)?;
        Ok(value)
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

/// 精确识别后端生成的三个纯 i64 溢出 intrinsic，供 GC 分析复用；不放行其他 LLVM 或原生调用。
pub(super) fn is_checked_arithmetic_intrinsic(name: &[u8]) -> bool {
    matches!(
        name,
        b"llvm.sadd.with.overflow.i64"
            | b"llvm.ssub.with.overflow.i64"
            | b"llvm.smul.with.overflow.i64"
    )
}
