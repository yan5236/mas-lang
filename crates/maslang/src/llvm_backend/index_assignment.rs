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
        let (container, place) = self.compile_index_target(target)?;
        let compiled = self.compile_expr(index)?;
        let key = self.expect_value(compiled)?;
        let element_type = match &container.value_type {
            ValueType::List(t) | ValueType::Array(t, _) | ValueType::Map(_, t) => t.as_ref(),
            _ => unreachable!("semantic container check"),
        };
        let assigned = self.compile_expected(value, element_type)?;
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
                let pointer = match place {
                    Some(pointer) => pointer,
                    None => self.resolve_array_pointer(target, container)?,
                };
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
    /// 求值容器目标并保留数组字段及嵌套数组的原始存储，避免写入临时副本；接收者只求值一次并保留 GC 根。
    fn compile_index_target(
        &mut self,
        target: &Expr,
    ) -> Result<(TypedValue<'ctx>, Option<PointerValue<'ctx>>), CompilerError> {
        if let Expr::Grouped { expression, .. } = target {
            return self.compile_index_target(expression);
        }
        if let Expr::Index {
            target: outer,
            index,
            span,
        } = target
        {
            let (container, place) = self.compile_index_target(outer)?;
            let index_value = self.compile_expected(
                index,
                &match &container.value_type {
                    ValueType::Map(key, _) => *key.clone(),
                    _ => ValueType::Int,
                },
            )?;
            let (value, place) = match container.value_type.clone() {
                ValueType::Array(element, size) => {
                    let pointer = match place {
                        Some(p) => p,
                        None => self.resolve_array_pointer(outer, container)?,
                    };
                    let index = index_value.value.into_int_value();
                    let value = self.build_array_get(pointer, &element, size, index, *span)?;
                    let slot = unsafe {
                        self.map_builder(self.builder.build_in_bounds_gep(
                            self.array_type_for(&element, size)?,
                            pointer,
                            &[self.context.i64_type().const_zero(), index],
                            "array.nested.slot",
                        ))?
                    };
                    (value, Some(slot))
                }
                ValueType::List(element) => (
                    self.build_list_get(
                        container.value.into_pointer_value(),
                        &element,
                        index_value.value.into_int_value(),
                        *span,
                    )?,
                    None,
                ),
                ValueType::Map(key, value) => (
                    self.build_map_get(
                        container.value.into_pointer_value(),
                        &key,
                        &value,
                        index_value,
                        *span,
                    )?,
                    None,
                ),
                _ => unreachable!("semantic index check"),
            };
            self.protect_gc_value(&value)?;
            return Ok((value, place));
        }
        let member = match target {
            Expr::FieldAccess { target, name, span } => {
                Some((target.as_ref(), name.as_str(), *span))
            }
            Expr::PropertyAccess {
                target,
                property,
                span,
            } => Some((target.as_ref(), property.display_name(), *span)),
            _ => None,
        };
        if let Some((receiver, name, span)) = member {
            let compiled = self.compile_expr(receiver)?;
            let receiver = self.expect_value(compiled)?;
            let (value, place) = if matches!(receiver.value_type, ValueType::Struct(_)) {
                let (pointer, ty) = self.field_pointer(&receiver, name, span)?;
                let value = self.map_builder(self.builder.build_load(
                    self.basic_type_for(&ty)?,
                    pointer,
                    "container.field",
                ))?;
                (
                    TypedValue {
                        value_type: ty,
                        value,
                    },
                    Some(pointer),
                )
            } else if matches!(receiver.value_type, ValueType::Nullable(_)) && name == "value" {
                let pointer = receiver.value.into_pointer_value();
                (self.unwrap_nullable(receiver, span)?, Some(pointer))
            } else {
                return Err(CompilerError::build_message(
                    "该属性不能作为容器赋值目标",
                    span,
                ));
            };
            self.protect_gc_value(&value)?;
            return Ok((value, place));
        }
        let compiled = self.compile_expr(target)?;
        Ok((self.expect_value(compiled)?, None))
    }
}
