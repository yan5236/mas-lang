use inkwell::{FloatPredicate, IntPredicate};

use super::*;
use crate::ast::{BinaryOp, Expr};

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译二元表达式，并根据静态类型规则选择对应的 LLVM 指令。
    pub(super) fn compile_binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.compile_short_circuit(op, left, right, span);
        }

        let compiled_left = self.compile_expr(left)?;
        let left_value = self.expect_value(compiled_left)?;
        let compiled_right = self.compile_expr(right)?;
        let right_value = self.expect_value(compiled_right)?;
        let result = self.compile_binary_values(op, left_value, right_value, span)?;
        Ok(CodegenValue::Value(result))
    }

    /// 在已知左右操作数的前提下编译二元表达式值。
    pub(super) fn compile_binary_values(
        &mut self,
        op: BinaryOp,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        match op {
            BinaryOp::Add => self.compile_add(left, right, span),
            BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => {
                self.compile_numeric_binary(left, right, op, span)
            }
            BinaryOp::Modulo | BinaryOp::IntDivide => {
                self.compile_integer_binary(left, right, op, span)
            }
            BinaryOp::Equal | BinaryOp::NotEqual => self.compile_equality(left, right, op, span),
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
                self.compile_order_compare(left, right, op, span)
            }
            BinaryOp::And | BinaryOp::Or => unreachable!("短路逻辑在外层单独处理"),
        }
    }

    /// 编译 `and/or` 的短路求值逻辑。
    pub(super) fn compile_short_circuit(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let function = self.current_function.expect("当前必须处于函数编译上下文");
        let compiled_left = self.compile_expr(left)?;
        let left_value = self.expect_value(compiled_left)?;
        if left_value.value_type != ValueType::Bool {
            return Err(CompilerError::build_message("短路逻辑只支持布尔值", span));
        }

        let slot = self.allocate_local(&ValueType::Bool, "logic.tmp")?;
        self.map_builder(self.builder.build_store(slot, left_value.value))?;

        let rhs_block = self.context.append_basic_block(function, "logic.rhs");
        let merge_block = self.context.append_basic_block(function, "logic.merge");
        match op {
            BinaryOp::And => self.map_builder(self.builder.build_conditional_branch(
                left_value.value.into_int_value(),
                rhs_block,
                merge_block,
            ))?,
            BinaryOp::Or => self.map_builder(self.builder.build_conditional_branch(
                left_value.value.into_int_value(),
                merge_block,
                rhs_block,
            ))?,
            _ => unreachable!(),
        };

        self.builder.position_at_end(rhs_block);
        let compiled_right = self.compile_expr(right)?;
        let right_value = self.expect_value(compiled_right)?;
        if right_value.value_type != ValueType::Bool {
            return Err(CompilerError::build_message("短路逻辑只支持布尔值", span));
        }
        self.map_builder(self.builder.build_store(slot, right_value.value))?;
        self.map_builder(self.builder.build_unconditional_branch(merge_block))?;

        self.builder.position_at_end(merge_block);
        let result = self.map_builder(self.builder.build_load(
            self.context.bool_type(),
            slot,
            "logic.result",
        ))?;
        Ok(CodegenValue::Value(TypedValue {
            value_type: ValueType::Bool,
            value: result,
        }))
    }

    /// 编译加法表达式，额外处理字符串拼接分支。
    pub(super) fn compile_add(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if left.value_type == ValueType::String && right.value_type == ValueType::String {
            let left_string = left.value.into_struct_value();
            let right_string = right.value.into_struct_value();
            let left_ptr = self
                .extract_string_part(left_string, 0, "left.ptr")?
                .into_pointer_value();
            let left_len = self
                .extract_string_part(left_string, 1, "left.len")?
                .into_int_value();
            let right_ptr = self
                .extract_string_part(right_string, 0, "right.ptr")?
                .into_pointer_value();
            let right_len = self
                .extract_string_part(right_string, 1, "right.len")?
                .into_int_value();
            let out_len_ptr = self.allocate_local(&ValueType::Int, "concat.out_len")?;
            let call = self.map_builder(self.builder.build_call(
                self.runtime.string_concat,
                &[
                    left_ptr.into(),
                    left_len.into(),
                    right_ptr.into(),
                    right_len.into(),
                    out_len_ptr.into(),
                ],
                "concat.string",
            ))?;
            let ptr_value = call
                .try_as_basic_value()
                .unwrap_basic()
                .into_pointer_value();
            let len_value = self
                .map_builder(self.builder.build_load(
                    self.context.i64_type(),
                    out_len_ptr,
                    "concat.len",
                ))?
                .into_int_value();
            return Ok(TypedValue {
                value_type: ValueType::String,
                value: self.build_runtime_string(ptr_value, len_value)?.into(),
            });
        }

        self.compile_numeric_binary(left, right, BinaryOp::Add, span)
    }

    /// 编译数值二元运算；整数加减乘检测溢出，混合运算保持浮点提升语义。
    pub(super) fn compile_numeric_binary(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        match op {
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply => {
                if left.value_type == ValueType::Int && right.value_type == ValueType::Int {
                    let value = self
                        .checked_int_arithmetic(
                            op,
                            left.value.into_int_value(),
                            right.value.into_int_value(),
                            span,
                        )?
                        .into();
                    return Ok(TypedValue {
                        value_type: ValueType::Int,
                        value,
                    });
                }
            }
            BinaryOp::Divide => {}
            _ => unreachable!("只有数值二元运算会进入该分支"),
        }

        let (left_float, right_float) = self.coerce_to_float_pair(left, right, span)?;
        let value = match op {
            BinaryOp::Add => self
                .map_builder(
                    self.builder
                        .build_float_add(left_float, right_float, "add.float"),
                )?
                .into(),
            BinaryOp::Subtract => self
                .map_builder(
                    self.builder
                        .build_float_sub(left_float, right_float, "sub.float"),
                )?
                .into(),
            BinaryOp::Multiply => self
                .map_builder(
                    self.builder
                        .build_float_mul(left_float, right_float, "mul.float"),
                )?
                .into(),
            BinaryOp::Divide => self
                .map_builder(
                    self.builder
                        .build_float_div(left_float, right_float, "div.float"),
                )?
                .into(),
            _ => unreachable!(),
        };
        Ok(TypedValue {
            value_type: ValueType::Float,
            value,
        })
    }

    /// 编译整数取余和除法，先检查零除数与 MIN/-1，再生成对应 LLVM 指令。
    pub(super) fn compile_integer_binary(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if left.value_type != ValueType::Int || right.value_type != ValueType::Int {
            return Err(CompilerError::build_message(
                "取模和整数除法只支持整数",
                span,
            ));
        }
        let left = left.value.into_int_value();
        let right = right.value.into_int_value();
        self.check_int_division(left, right, span)?;
        let value = match op {
            BinaryOp::Modulo => {
                self.map_builder(self.builder.build_int_signed_rem(left, right, "mod.int"))?
            }
            BinaryOp::IntDivide => {
                self.map_builder(self.builder.build_int_signed_div(left, right, "intdiv.int"))?
            }
            _ => unreachable!(),
        };
        Ok(TypedValue {
            value_type: ValueType::Int,
            value: value.into(),
        })
    }

    /// 编译相等比较；浮点不等包含无序情形，使 NaN != 任意浮点值恒为真。
    pub(super) fn compile_equality(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let value = match (&left.value_type, &right.value_type) {
            (ValueType::Int, ValueType::Int) | (ValueType::Bool, ValueType::Bool) => self
                .map_builder(self.builder.build_int_compare(
                    if op == BinaryOp::Equal {
                        IntPredicate::EQ
                    } else {
                        IntPredicate::NE
                    },
                    left.value.into_int_value(),
                    right.value.into_int_value(),
                    "cmp.int",
                ))?
                .into(),
            (ValueType::String, ValueType::String) => {
                let left = left.value.into_struct_value();
                let right = right.value.into_struct_value();
                let call = self.map_builder(self.builder.build_call(
                    self.runtime.string_equals,
                    &[
                        self.extract_string_part(left, 0, "eq.left.ptr")?.into(),
                        self.extract_string_part(left, 1, "eq.left.len")?.into(),
                        self.extract_string_part(right, 0, "eq.right.ptr")?.into(),
                        self.extract_string_part(right, 1, "eq.right.len")?.into(),
                    ],
                    "cmp.string",
                ))?;
                let result = call.try_as_basic_value().unwrap_basic().into_int_value();
                self.map_builder(self.builder.build_int_compare(
                    if op == BinaryOp::Equal {
                        IntPredicate::NE
                    } else {
                        IntPredicate::EQ
                    },
                    result,
                    self.context.i8_type().const_zero(),
                    "cmp.string.bool",
                ))?
                .into()
            }
            (ValueType::Float, ValueType::Float)
            | (ValueType::Float, ValueType::Int)
            | (ValueType::Int, ValueType::Float) => {
                let (left_float, right_float) = self.coerce_to_float_pair(left, right, span)?;
                self.map_builder(self.builder.build_float_compare(
                    if op == BinaryOp::Equal {
                        FloatPredicate::OEQ
                    } else {
                        FloatPredicate::UNE
                    },
                    left_float,
                    right_float,
                    "cmp.float",
                ))?
                .into()
            }
            _ => {
                return Err(CompilerError::build_message(
                    "当前相等比较无法生成目标代码",
                    span,
                ));
            }
        };

        Ok(TypedValue {
            value_type: ValueType::Bool,
            value,
        })
    }

    /// 编译大小比较表达式。
    pub(super) fn compile_order_compare(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        op: BinaryOp,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let value = if left.value_type == ValueType::Int && right.value_type == ValueType::Int {
            self.map_builder(self.builder.build_int_compare(
                match op {
                    BinaryOp::Less => IntPredicate::SLT,
                    BinaryOp::LessEqual => IntPredicate::SLE,
                    BinaryOp::Greater => IntPredicate::SGT,
                    BinaryOp::GreaterEqual => IntPredicate::SGE,
                    _ => unreachable!(),
                },
                left.value.into_int_value(),
                right.value.into_int_value(),
                "cmp.order.int",
            ))?
            .into()
        } else {
            let (left_float, right_float) = self.coerce_to_float_pair(left, right, span)?;
            self.map_builder(self.builder.build_float_compare(
                match op {
                    BinaryOp::Less => FloatPredicate::OLT,
                    BinaryOp::LessEqual => FloatPredicate::OLE,
                    BinaryOp::Greater => FloatPredicate::OGT,
                    BinaryOp::GreaterEqual => FloatPredicate::OGE,
                    _ => unreachable!(),
                },
                left_float,
                right_float,
                "cmp.order.float",
            ))?
            .into()
        };

        Ok(TypedValue {
            value_type: ValueType::Bool,
            value,
        })
    }
}
