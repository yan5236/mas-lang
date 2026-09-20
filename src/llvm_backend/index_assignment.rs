use super::*;
use crate::ast::Expr;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 按目标、下标、右值的顺序各求值一次，再覆盖容器元素；数组写回原槽，越界时拒绝写入。
    /// 表达式入口保护 GC 临时值；列表在右值完成后检查当前长度，允许右值合法扩容并拒绝缩容后的越界。
    pub(super) fn compile_index_assignment(
        &mut self,
        target: &Expr,
        index: &Expr,
        value: &Expr,
        span: Span,
    ) -> Result<(), CompilerError> {
        let compiled = self.compile_expr(target)?;
        let container = self.expect_value(compiled)?;
        let compiled = self.compile_expr(index)?;
        let key = self.expect_value(compiled)?;
        let compiled = self.compile_expr(value)?;
        let assigned = self.expect_value(compiled)?;
        match container.value_type.clone() {
            ValueType::Map(key_type, value_type) => {
                let (map, _, _) = self.expect_map_value(container, span)?;
                self.build_map_set(map, &key_type, &value_type, key, assigned, span)
            }
            ValueType::List(element_type) => {
                let (list, _) = self.expect_list_value(container, span)?;
                let function = self.list_runtime_function("list_set", &element_type)?;
                let mut args = vec![list.into(), key.value.into()];
                args.extend(self.build_list_value_arguments(assigned, span)?);
                self.map_builder(self.builder.build_call(function, &args, "call.list_set"))?;
                Ok(())
            }
            ValueType::Array(element_type, size) => {
                let pointer = self.resolve_array_pointer(target, container)?;
                let index = key.value.into_int_value();
                let valid = self.map_builder(self.builder.build_int_compare(
                    inkwell::IntPredicate::ULT,
                    index,
                    self.context.i64_type().const_int(size as u64, false),
                    "array.in_range",
                ))?;
                self.emit_safety_check(valid, 3, span)?;
                self.build_array_store(pointer, &element_type, size, index, assigned.value)
            }
            _ => Err(CompilerError::build_message(
                "下标赋值只支持列表、数组或字典类型",
                span,
            )),
        }
    }
}
