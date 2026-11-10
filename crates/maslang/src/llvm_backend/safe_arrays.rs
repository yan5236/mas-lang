use inkwell::values::{IntValue, PointerValue};

use super::*;
use crate::ast::{Expr, MethodName};
use crate::semantic::ARRAY_TRY_SET_STORAGE_HINT;

/// 固定数组的可恢复方法：数组是内联值，读取时先做范围判断；
/// 写入还要求目标有稳定存储地址，避免写入临时副本后仍报告成功。
impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译固定数组安全方法（`try_get`/`try_set`）；越界返回失败状态且不访问元素地址。
    pub(super) fn compile_array_safe_method(
        &mut self,
        target: &Expr,
        target_value: TypedValue<'ctx>,
        method: MethodName,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let ValueType::Array(element_type, size) = target_value.value_type.clone() else {
            return Err(CompilerError::build_message(
                "当前表达式不是固定数组类型",
                span,
            ));
        };
        match method {
            MethodName::TryGet => {
                self.expect_safe_argument_count(arguments, 1, MethodName::TryGet, span)?;
                let index = self.compile_safe_index_argument(&arguments[0])?;
                let pointer = self.resolve_array_pointer(target, target_value)?;
                let valid = self.build_array_in_range_check(index, size, "safe.array.get")?;
                let element = element_type.as_ref().clone();
                let result = self.build_guarded_result(
                    valid,
                    element_type.as_ref(),
                    "safe.array.get",
                    |this| {
                        let slot =
                            this.build_array_element_pointer(pointer, &element, size, index)?;
                        Ok(this.map_builder(this.builder.build_load(
                            this.basic_type_for(&element)?,
                            slot,
                            "load.array.try",
                        ))?)
                    },
                )?;
                Ok(CodegenValue::Value(result))
            }
            MethodName::TrySet => {
                self.expect_safe_argument_count(arguments, 2, MethodName::TrySet, span)?;
                // 先固定写入地址再求值实参，与下标赋值一致：右值里的重新赋值不会改变写入目标。
                let pointer = self.resolve_array_write_pointer(target, &target_value)?;
                let index = self.compile_safe_index_argument(&arguments[0])?;
                let assigned = self.compile_expected(&arguments[1], element_type.as_ref())?;
                let valid = self.build_array_in_range_check(index, size, "safe.array.set")?;
                let element = element_type.as_ref().clone();
                let result = self.build_guarded_action(valid, "safe.array.set", |this| {
                    let slot = this.build_array_element_pointer(pointer, &element, size, index)?;
                    this.map_builder(this.builder.build_store(slot, assigned.value))?;
                    Ok(())
                })?;
                Ok(CodegenValue::Value(result))
            }
            _ => Err(CompilerError::build_message(
                format!(
                    "固定数组安全方法分派收到不支持的方法 `{}`",
                    method.display_name()
                ),
                span,
            )),
        }
    }

    /// 判定固定数组下标可安全访问：`0 <= i < N`，`N` 是编译期常量。
    fn build_array_in_range_check(
        &mut self,
        index: IntValue<'ctx>,
        size: usize,
        prefix: &str,
    ) -> Result<IntValue<'ctx>, CompilerError> {
        let int = self.context.i64_type();
        let non_negative = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::SGE,
            index,
            int.const_zero(),
            &format!("{prefix}.non_negative"),
        ))?;
        let in_bounds = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::ULT,
            index,
            int.const_int(size as u64, false),
            &format!("{prefix}.in_bounds"),
        ))?;
        self.map_builder(self.builder.build_and(
            non_negative,
            in_bounds,
            &format!("{prefix}.valid"),
        ))
    }

    /// 计算数组元素地址；调用方必须先确认下标合法，本函数不再插入终止型安全检查。
    fn build_array_element_pointer(
        &mut self,
        array_pointer: PointerValue<'ctx>,
        element_type: &ValueType,
        size: usize,
        index: IntValue<'ctx>,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        let zero = self.context.i64_type().const_zero();
        unsafe {
            self.map_builder(self.builder.build_in_bounds_gep(
                self.array_type_for(element_type, size)?,
                array_pointer,
                &[zero, index],
                "array.try.ptr",
            ))
        }
    }

    /// 解析数组写入位置：变量直接复用真实栈槽或全局槽位，结构体字段链在接收者也是变量引用时
    /// 直接取字段地址；没有可写地址的临时数组值或数组元素在编译期拒绝，避免写入副本后仍报告成功。
    /// 调用方在编译实参之前调用本函数，重新求值只涉及变量和字段读取（不含调用与赋值），
    /// 因此不会重复执行带副作用的表达式，也不会改变写入目标。
    fn resolve_array_write_pointer(
        &mut self,
        target: &Expr,
        value: &TypedValue<'ctx>,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        match target {
            Expr::Grouped { expression, .. } => self.resolve_array_write_pointer(expression, value),
            Expr::Identifier { name, span } => match self.lookup_variable(name) {
                Some(variable) if variable.value_type == value.value_type => Ok(variable.pointer),
                _ => Err(Self::build_array_storage_error(*span)),
            },
            Expr::FieldAccess {
                target: receiver,
                name,
                span,
            } => {
                if !Self::is_place_chain(receiver) {
                    return Err(Self::build_array_storage_error(*span));
                }
                // 变量引用的重新加载没有副作用，字段地址直接指向原结构体对象。
                let compiled = self.compile_expr(receiver)?;
                let receiver_value = self.expect_value(compiled)?;
                let (pointer, field_type) = self.field_pointer(&receiver_value, name, *span)?;
                if field_type != value.value_type {
                    return Err(Self::build_array_storage_error(*span));
                }
                Ok(pointer)
            }
            _ => Err(Self::build_array_storage_error(target.span())),
        }
    }

    /// 判断表达式是否只由变量引用和结构体字段组成；这类表达式重新求值不会改变程序状态。
    fn is_place_chain(expression: &Expr) -> bool {
        match expression {
            Expr::Identifier { .. } => true,
            Expr::Grouped { expression, .. } => Self::is_place_chain(expression),
            Expr::FieldAccess { target, .. } => Self::is_place_chain(target),
            _ => false,
        }
    }

    /// 报告数组写入位置缺失：固定数组是内联值，只有变量和结构体字段里的数组才有可写地址。
    fn build_array_storage_error(span: Span) -> CompilerError {
        CompilerError::build_message(ARRAY_TRY_SET_STORAGE_HINT, span)
    }
}
