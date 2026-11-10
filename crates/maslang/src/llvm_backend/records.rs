use super::*;
use crate::ast::{AssignOp, BinaryOp, Expr};
use inkwell::targets::TargetData;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 将字段声明转换为实际 LLVM 布局；结构体引用用指针保存，递归类型无需无限展开。
    fn record_layout(&self, name: &str) -> Result<StructType<'ctx>, CompilerError> {
        let fields = self.structs[name]
            .iter()
            .map(|f| self.basic_type_for(&self.value_type_from_annotation(&f.type_name)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.context.struct_type(&fields, false))
    }

    /// 按目标机器布局创建零初始化 GC 对象；偏移表为模块常量，扫描时不猜测标量内容。
    pub(super) fn allocate_record(
        &mut self,
        types: &[ValueType],
    ) -> Result<(PointerValue<'ctx>, StructType<'ctx>), CompilerError> {
        let fields = types
            .iter()
            .map(|t| self.basic_type_for(t))
            .collect::<Result<Vec<_>, _>>()?;
        let layout = self.context.struct_type(&fields, false);
        let data = TargetData::create(self.module.get_data_layout().as_str().to_str().unwrap());
        let mut offsets = Vec::new();
        for (i, ty) in types.iter().enumerate() {
            if let Some((kind, count)) = Self::gc_layout(ty) {
                let base = data.offset_of_element(&layout, i as u32).unwrap();
                let stride = if kind == 1 {
                    data.get_abi_size(&self.string_type)
                } else {
                    data.get_pointer_byte_size(None) as u64
                };
                for n in 0..count {
                    offsets.push(self.context.i64_type().const_int(base + n * stride, false));
                }
            }
        }
        let array = self.context.i64_type().const_array(&offsets);
        let table = self
            .module
            .add_global(array.get_type(), None, "record.trace");
        table.set_linkage(inkwell::module::Linkage::Private);
        table.set_constant(true);
        table.set_initializer(&array);
        let ptr = self.list_pointer_type();
        let int = self.context.i64_type();
        let alloc = self
            .module
            .get_function("mas_record_alloc")
            .unwrap_or_else(|| {
                self.module.add_function(
                    "mas_record_alloc",
                    ptr.fn_type(&[int.into(), ptr.into(), int.into()], false),
                    None,
                )
            });
        let call = self.map_builder(self.builder.build_call(
            alloc,
            &[
                int.const_int(data.get_abi_size(&layout), false).into(),
                table.as_pointer_value().into(),
                int.const_int(offsets.len() as u64, false).into(),
            ],
            "record.new",
        ))?;
        Ok((
            call.try_as_basic_value()
                .unwrap_basic()
                .into_pointer_value(),
            layout,
        ))
    }

    /// 编译构造实参并逐字段写入；每个临时对象保持 GC 根，允许后续实参分配或回收。
    pub(super) fn compile_constructor(
        &mut self,
        name: &str,
        args: &[Expr],
        _span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let fields = self.structs[name].clone();
        let types = fields
            .iter()
            .map(|f| self.value_type_from_annotation(&f.type_name))
            .collect::<Vec<_>>();
        let mut values = Vec::new();
        for (arg, ty) in args.iter().zip(&types) {
            values.push(self.compile_expected(arg, ty)?);
        }
        let (pointer, layout) = self.allocate_record(&types)?;
        for (i, value) in values.iter().enumerate() {
            let field = self.map_builder(self.builder.build_struct_gep(
                layout,
                pointer,
                i as u32,
                "field.init",
            ))?;
            self.map_builder(self.builder.build_store(field, value.value))?;
        }
        Ok(CodegenValue::Value(TypedValue {
            value_type: ValueType::Struct(name.to_string()),
            value: pointer.into(),
        }))
    }

    /// 把非空载荷包装成可空值：结构体引用直接复用指针，其他类型分配单字段托管记录。
    /// 记录会被 GC 追踪，因此字符串与字节字段的长度和指针在回收后仍然有效。
    pub(super) fn build_nullable_payload(
        &mut self,
        inner: &ValueType,
        value: BasicValueEnum<'ctx>,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        if matches!(inner, ValueType::Struct(_)) {
            return Ok(value.into_pointer_value());
        }
        let (pointer, layout) = self.allocate_record(&[inner.clone()])?;
        let field = self.map_builder(self.builder.build_struct_gep(
            layout,
            pointer,
            0,
            "nullable.init",
        ))?;
        self.map_builder(self.builder.build_store(field, value))?;
        Ok(pointer)
    }

    /// 按期望类型编译实参或返回值，处理空容器和 T 到 T? 的提升；不隐式解包可空值。
    pub(super) fn compile_expected(
        &mut self,
        expr: &Expr,
        expected: &ValueType,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let inner = if let ValueType::Nullable(inner) = expected {
            inner.as_ref()
        } else {
            expected
        };
        let value = match (expr, inner) {
            (Expr::ListLiteral { elements, span }, ValueType::List(element)) => {
                let compiled =
                    self.compile_list_literal_expected(elements, Some(element), *span)?;
                self.expect_value(compiled)?
            }
            (Expr::MapLiteral { entries, span }, ValueType::Map(key, value)) => {
                let compiled =
                    self.compile_map_literal_expected(entries, Some((key, value)), *span)?;
                self.expect_value(compiled)?
            }
            (Expr::Grouped { expression, .. }, _) => {
                return self.compile_expected(expression, expected);
            }
            _ => {
                let compiled = self.compile_expr(expr)?;
                self.expect_value(compiled)?
            }
        };
        let value = self.coerce_nullable(value, expected, expr.span())?;
        self.protect_gc_value(&value)?;
        Ok(value)
    }

    /// null 使用空指针；可空结构体复用对象引用，其他 T? 使用带精确追踪布局的托管包装。
    pub(super) fn coerce_nullable(
        &mut self,
        value: TypedValue<'ctx>,
        expected: &ValueType,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if &value.value_type == expected {
            return Ok(value);
        }
        if let ValueType::Nullable(inner) = expected {
            if value.value_type == ValueType::Null
                || (matches!(inner.as_ref(), ValueType::Struct(_))
                    && &value.value_type == inner.as_ref())
            {
                return Ok(TypedValue {
                    value_type: expected.clone(),
                    value: value.value,
                });
            }
            if &value.value_type == inner.as_ref() {
                let (pointer, layout) = self.allocate_record(&[*inner.clone()])?;
                let field = self.map_builder(self.builder.build_struct_gep(
                    layout,
                    pointer,
                    0,
                    "nullable.init",
                ))?;
                self.map_builder(self.builder.build_store(field, value.value))?;
                return Ok(TypedValue {
                    value_type: expected.clone(),
                    value: pointer.into(),
                });
            }
        }
        Err(CompilerError::build_message("表达式与目标类型不匹配", span))
    }

    /// 检查非空后提取值；空值进入带源码位置的错误分支，不生成未检查的解引用。
    pub(super) fn unwrap_nullable(
        &mut self,
        value: TypedValue<'ctx>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let ValueType::Nullable(inner) = value.value_type else {
            unreachable!()
        };
        let pointer = value.value.into_pointer_value();
        let valid =
            self.map_builder(self.builder.build_is_not_null(pointer, "nullable.present"))?;
        self.emit_safety_check(valid, 5, span)?;
        let value = if matches!(inner.as_ref(), ValueType::Struct(_)) {
            value.value
        } else {
            self.map_builder(self.builder.build_load(
                self.basic_type_for(&inner)?,
                pointer,
                "nullable.value",
            ))?
        };
        Ok(TypedValue {
            value_type: *inner,
            value,
        })
    }

    /// 根据静态结构体名称取得字段指针及字段类型，拒绝不存在的字段。
    pub(super) fn field_pointer(
        &mut self,
        target: &TypedValue<'ctx>,
        name: &str,
        span: Span,
    ) -> Result<(PointerValue<'ctx>, ValueType), CompilerError> {
        let ValueType::Struct(record) = &target.value_type else {
            return Err(CompilerError::build_message("字段目标不是结构体", span));
        };
        let (i, field) = self.structs[record]
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == name)
            .ok_or_else(|| CompilerError::build_message("未知字段", span))?;
        let ty = self.value_type_from_annotation(&field.type_name);
        let pointer = self.map_builder(self.builder.build_struct_gep(
            self.record_layout(record)?,
            target.value.into_pointer_value(),
            i as u32,
            "field.ptr",
        ))?;
        Ok((pointer, ty))
    }

    /// 加载已求值对象的字段，不重复执行目标表达式。
    pub(super) fn load_field(
        &mut self,
        target: TypedValue<'ctx>,
        name: &str,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let (pointer, ty) = self.field_pointer(&target, name, span)?;
        let value = self.map_builder(self.builder.build_load(
            self.basic_type_for(&ty)?,
            pointer,
            "field.value",
        ))?;
        Ok(TypedValue {
            value_type: ty,
            value,
        })
    }

    /// 编译任意深度字段读取，目标对象在可能分配的后续表达式中保持可达。
    /// `枚举名.成员` 在此直接降低为成员序号的 i64 常量，不经过普通字段访问。
    pub(super) fn compile_field_access(
        &mut self,
        target: &Expr,
        name: &str,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if let Expr::Identifier {
            name: enum_name, ..
        } = target
        {
            let resolved = self.enums.get(enum_name).map(|descriptor| {
                (
                    descriptor.member_index(name),
                    descriptor.display.clone(),
                    enum_name.clone(),
                )
            });
            if let Some((index, display, internal)) = resolved {
                let Some(index) = index else {
                    return Err(CompilerError::build_message(
                        format!("代码生成阶段找不到枚举 `{display}` 的成员 `{name}`"),
                        span,
                    ));
                };
                return Ok(CodegenValue::Value(TypedValue {
                    value_type: ValueType::Enum(EnumType::new(internal, display)),
                    value: self.context.i64_type().const_int(index as u64, false).into(),
                }));
            }
            // 标识符不是枚举时说明成员访问目标缺失，避免继续按普通字段读取生成代码。
            if self.lookup_variable(enum_name).is_none() {
                return Err(CompilerError::build_message(
                    format!("代码生成阶段找不到枚举 `{enum_name}`"),
                    span,
                ));
            }
        }
        let compiled = self.compile_expr(target)?;
        let target = self.expect_value(compiled)?;
        self.load_field(target, name, span).map(CodegenValue::Value)
    }

    /// 字段赋值先求值目标，再求值右侧；复合赋值复用既有数值安全规则，目标只执行一次。
    pub(super) fn compile_field_assignment(
        &mut self,
        target: &Expr,
        name: &str,
        op: AssignOp,
        expr: &Expr,
        span: Span,
    ) -> Result<(), CompilerError> {
        let compiled = self.compile_expr(target)?;
        let target = self.expect_value(compiled)?;
        let (pointer, ty) = self.field_pointer(&target, name, span)?;
        let value = if op == AssignOp::Assign {
            self.compile_expected(expr, &ty)?
        } else {
            let old = self.map_builder(self.builder.build_load(
                self.basic_type_for(&ty)?,
                pointer,
                "field.old",
            ))?;
            let old = TypedValue {
                value_type: ty.clone(),
                value: old,
            };
            self.protect_gc_value(&old)?;
            let compiled = self.compile_expr(expr)?;
            let right = self.expect_value(compiled)?;
            let op = match op {
                AssignOp::AddAssign => BinaryOp::Add,
                AssignOp::SubtractAssign => BinaryOp::Subtract,
                AssignOp::MultiplyAssign => BinaryOp::Multiply,
                AssignOp::DivideAssign => BinaryOp::Divide,
                AssignOp::ModAssign => BinaryOp::Modulo,
                _ => unreachable!(),
            };
            self.compile_binary_values(op, old, right, span)?
        };
        self.map_builder(self.builder.build_store(pointer, value.value))?;
        Ok(())
    }
}
