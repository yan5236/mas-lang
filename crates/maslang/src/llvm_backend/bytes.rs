use inkwell::types::BasicMetadataTypeEnum;
use inkwell::values::{BasicValueEnum, IntValue, StructValue};

use super::*;
use crate::ast::Expr;

/// 字节运行时入口的返回形态：无返回值、整数值或字节布尔标记。
#[derive(Clone, Copy)]
enum BytesReturn {
    Void,
    Int,
    /// 返回 `i8` 的布尔标记（`0` / `1`），用于逐字节比较。
    Byte,
}

/// 字节对象的代码生成支持：类型表示复用字符串的「指针 + 长度」结构，语义与调用入口独立。
///
/// **ABI 约定**：Windows x64 的 C ABI 对 16 字节结构体返回值使用隐藏返回指针（sret），
/// 编译器 IR 里的 `declare { ptr, i64 }` 与之不一致，会读到错位的实参。因此所有被编译产物
/// 直接调用的字节运行时入口都用 `out: *mut MasBytes` 写回，再由 IR 加载字段。
impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 按需声明字节运行时函数；`parameters` 只列显式实参，返回值按 `BytesReturn` 决定。
    fn bytes_runtime_function(
        &self,
        name: &str,
        parameters: &[BasicMetadataTypeEnum<'ctx>],
        returns: BytesReturn,
    ) -> FunctionValue<'ctx> {
        self.module.get_function(name).unwrap_or_else(|| {
            let function_type = match returns {
                BytesReturn::Void => self.context.void_type().fn_type(parameters, false),
                BytesReturn::Int => self.context.i64_type().fn_type(parameters, false),
                BytesReturn::Byte => self.context.i8_type().fn_type(parameters, false),
            };
            self.module.add_function(name, function_type, None)
        })
    }

    /// 在入口块分配一个与字符串结构同布局的字节输出槽，供运行时写回结果。
    fn bytes_out_slot(&self, name: &str) -> Result<PointerValue<'ctx>, CompilerError> {
        self.allocate_entry_slot(self.string_type.into(), name)
    }

    /// 从输出槽读出字节结构值。
    fn load_bytes_slot(
        &self,
        slot: PointerValue<'ctx>,
    ) -> Result<StructValue<'ctx>, CompilerError> {
        Ok(self
            .map_builder(self.builder.build_load(self.string_type, slot, "bytes.out"))?
            .into_struct_value())
    }

    /// 构造合法的空字节值：调用运行时写回静态非空指针与零长度。
    pub(super) fn build_empty_bytes(&mut self) -> Result<StructValue<'ctx>, CompilerError> {
        let slot = self.bytes_out_slot("bytes.empty.out")?;
        let function = self.bytes_runtime_function(
            "mas_runtime_bytes_empty",
            &[self.context.ptr_type(AddressSpace::default()).into()],
            BytesReturn::Void,
        );
        self.map_builder(self.builder.build_call(function, &[slot.into()], ""))?;
        self.load_bytes_slot(slot)
    }

    /// 把字节结构拆成运行时所需的 `ptr` 与 `len` 两个实参。
    pub(super) fn bytes_parts(
        &self,
        value: StructValue<'ctx>,
    ) -> Result<(BasicValueEnum<'ctx>, BasicValueEnum<'ctx>), CompilerError> {
        Ok((
            self.extract_string_part(value, 0, "bytes.ptr")?,
            self.extract_string_part(value, 1, "bytes.len")?,
        ))
    }

    /// 分配一个 i8 有效性槽并在入口块零初始化，供运行时把「合法/非法」写回。
    pub(super) fn allocate_validity_slot(
        &self,
        name: &str,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        self.allocate_entry_slot(self.context.i8_type().into(), name)
    }

    /// 读取有效性槽：`1` 表示运行时成功产出了值。
    fn load_validity(&self, slot: PointerValue<'ctx>) -> Result<IntValue<'ctx>, CompilerError> {
        Ok(self
            .map_builder(
                self.builder
                    .build_load(self.context.i8_type(), slot, "valid"),
            )?
            .into_int_value())
    }

    /// 按运行时可空约定包装结果：成功时把载荷写入单字段记录，失败时返回空指针。
    /// `inner` 是失败语义之外的载荷类型（`bytes`、`string` 或 `int`）。
    pub(super) fn wrap_nullable_with_validity(
        &mut self,
        inner: ValueType,
        payload: BasicValueEnum<'ctx>,
        valid: IntValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let function = self.current_function.expect("function context");
        let succeeded = self
            .context
            .append_basic_block(function, "nullable.present");
        let missing = self
            .context
            .append_basic_block(function, "nullable.missing");
        let merge = self.context.append_basic_block(function, "nullable.value");
        let present = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            valid,
            self.context.i8_type().const_zero(),
            "nullable.valid",
        ))?;
        self.map_builder(
            self.builder
                .build_conditional_branch(present, succeeded, missing),
        )?;

        self.builder.position_at_end(succeeded);
        let pointer = self.build_nullable_payload(&inner, payload)?;
        let succeeded_end = self.builder.get_insert_block().expect("succeeded block");
        self.map_builder(self.builder.build_unconditional_branch(merge))?;

        self.builder.position_at_end(missing);
        let null = self.context.ptr_type(AddressSpace::default()).const_null();
        let missing_end = self.builder.get_insert_block().expect("missing block");
        self.map_builder(self.builder.build_unconditional_branch(merge))?;

        self.builder.position_at_end(merge);
        let phi = self.map_builder(
            self.builder
                .build_phi(self.list_pointer_type(), "nullable.value"),
        )?;
        phi.add_incoming(&[(&pointer, succeeded_end), (&null, missing_end)]);
        Ok(TypedValue {
            value_type: ValueType::Nullable(Box::new(inner)),
            value: phi.as_basic_value(),
        })
    }

    /// 编译 `bytes_from_list(values)`：整数列表逐项校验后复制成字节对象，越界返回 null。
    pub(super) fn compile_bytes_from_list(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if arguments.len() != 1 {
            return Err(CompilerError::build_message(
                "内建函数 `bytes_from_list` 需要且只接受一个整数列表参数",
                span,
            ));
        }
        let compiled = self.compile_expr(&arguments[0])?;
        let value = self.expect_value(compiled)?;
        let (list, _) = self.expect_list_value(value, span)?;
        let function = self.bytes_runtime_function(
            "mas_runtime_bytes_from_list",
            &[
                self.list_pointer_type().into(),
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.ptr_type(AddressSpace::default()).into(),
            ],
            BytesReturn::Void,
        );
        let validity = self.allocate_validity_slot("bytes.from_list.valid")?;
        let slot = self.bytes_out_slot("bytes.from_list.out")?;
        self.map_builder(self.builder.build_call(
            function,
            &[list.into(), validity.into(), slot.into()],
            "call.bytes_from_list",
        ))?;
        let payload = self.load_bytes_slot(slot)?.into();
        let valid = self.load_validity(validity)?;
        Ok(CodegenValue::Value(self.wrap_nullable_with_validity(
            ValueType::Bytes,
            payload,
            valid,
        )?))
    }

    /// 编译 `encode_utf8(text)`：严格校验后复制字节，非法原生字符串返回 null。
    pub(super) fn compile_encode_utf8(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if arguments.len() != 1 {
            return Err(CompilerError::build_message(
                "内建函数 `encode_utf8` 需要且只接受一个字符串参数",
                span,
            ));
        }
        let compiled = self.compile_expr(&arguments[0])?;
        let value = self.expect_value(compiled)?;
        let text = value.value.into_struct_value();
        let (ptr, len) = self.bytes_parts(text)?;
        let function = self.bytes_runtime_function(
            "mas_runtime_bytes_encode_utf8",
            &[
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.i64_type().into(),
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.ptr_type(AddressSpace::default()).into(),
            ],
            BytesReturn::Void,
        );
        let validity = self.allocate_validity_slot("bytes.encode.valid")?;
        let slot = self.bytes_out_slot("bytes.encode.out")?;
        self.map_builder(self.builder.build_call(
            function,
            &[ptr.into(), len.into(), validity.into(), slot.into()],
            "call.encode_utf8",
        ))?;
        let payload = self.load_bytes_slot(slot)?.into();
        let valid = self.load_validity(validity)?;
        Ok(CodegenValue::Value(self.wrap_nullable_with_validity(
            ValueType::Bytes,
            payload,
            valid,
        )?))
    }

    /// 编译 `decode_utf8(data)`：严格校验字节内容，失败返回 null，不做损失性替换。
    pub(super) fn compile_decode_utf8(
        &mut self,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if arguments.len() != 1 {
            return Err(CompilerError::build_message(
                "内建函数 `decode_utf8` 需要且只接受一个字节参数",
                span,
            ));
        }
        let compiled = self.compile_expr(&arguments[0])?;
        let value = self.expect_value(compiled)?;
        let data = value.value.into_struct_value();
        let (ptr, len) = self.bytes_parts(data)?;
        let function = self.bytes_runtime_function(
            "mas_runtime_bytes_decode_utf8",
            &[
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.i64_type().into(),
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.ptr_type(AddressSpace::default()).into(),
            ],
            BytesReturn::Void,
        );
        let validity = self.allocate_validity_slot("bytes.decode.valid")?;
        let slot = self.bytes_out_slot("bytes.decode.out")?;
        self.map_builder(self.builder.build_call(
            function,
            &[ptr.into(), len.into(), validity.into(), slot.into()],
            "call.decode_utf8",
        ))?;
        let payload = self.load_bytes_slot(slot)?.into();
        let valid = self.load_validity(validity)?;
        self.wrap_nullable_with_validity(ValueType::String, payload, valid)
            .map(CodegenValue::Value)
    }

    /// 编译字节下标读取：先检查 0 <= index < len，再交给运行时读取 0..255 的整数。
    pub(super) fn compile_bytes_index(
        &mut self,
        target_value: TypedValue<'ctx>,
        index_value: TypedValue<'ctx>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        if index_value.value_type != ValueType::Int {
            return Err(CompilerError::build_message("索引必须是整数", span));
        }
        let index = index_value.value.into_int_value();
        let data = target_value.value.into_struct_value();
        let (ptr, len) = self.bytes_parts(data)?;
        let len = len.into_int_value();
        let non_negative = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::SGE,
            index,
            self.context.i64_type().const_zero(),
            "bytes.index.non_negative",
        ))?;
        let below_len = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::SLT,
            index,
            len,
            "bytes.index.in_range",
        ))?;
        let valid = self.map_builder(self.builder.build_and(
            non_negative,
            below_len,
            "bytes.index.valid",
        ))?;
        self.emit_safety_check(valid, 7, span)?;

        let function = self.bytes_runtime_function(
            "mas_runtime_bytes_at",
            &[
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.i64_type().into(),
                self.context.i64_type().into(),
            ],
            BytesReturn::Int,
        );
        let call = self.map_builder(self.builder.build_call(
            function,
            &[ptr.into(), len.into(), index.into()],
            "call.bytes_at",
        ))?;
        Ok(TypedValue {
            value_type: ValueType::Int,
            value: call.try_as_basic_value().unwrap_basic(),
        })
    }

    /// 编译 `data.slice(start, end)`：半开区间非法时返回 null，合法时返回复制后的字节。
    pub(super) fn compile_bytes_slice(
        &mut self,
        target_value: TypedValue<'ctx>,
        arguments: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if arguments.len() != 2 {
            return Err(CompilerError::build_message(
                "字节方法 `slice` 需要且只接受起始与结束两个整数下标",
                span,
            ));
        }
        let mut bounds = Vec::new();
        for argument in arguments {
            let compiled = self.compile_expr(argument)?;
            let value = self.expect_value(compiled)?;
            if value.value_type != ValueType::Int {
                return Err(CompilerError::build_message(
                    "字节方法 `slice` 的下标必须是整数",
                    argument.span(),
                ));
            }
            bounds.push(value.value.into_int_value());
        }
        let data = target_value.value.into_struct_value();
        let (ptr, len) = self.bytes_parts(data)?;
        let function = self.bytes_runtime_function(
            "mas_runtime_bytes_slice",
            &[
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.i64_type().into(),
                self.context.i64_type().into(),
                self.context.i64_type().into(),
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.ptr_type(AddressSpace::default()).into(),
            ],
            BytesReturn::Void,
        );
        let validity = self.allocate_validity_slot("bytes.slice.valid")?;
        let slot = self.bytes_out_slot("bytes.slice.out")?;
        self.map_builder(self.builder.build_call(
            function,
            &[
                ptr.into(),
                len.into(),
                bounds[0].into(),
                bounds[1].into(),
                validity.into(),
                slot.into(),
            ],
            "call.bytes_slice",
        ))?;
        let payload = self.load_bytes_slot(slot)?.into();
        let valid = self.load_validity(validity)?;
        Ok(CodegenValue::Value(self.wrap_nullable_with_validity(
            ValueType::Bytes,
            payload,
            valid,
        )?))
    }

    /// 编译字节相等比较：逐字节比较内容，长度不同直接判定为不相等。
    pub(super) fn compile_bytes_equality(
        &mut self,
        left: TypedValue<'ctx>,
        right: TypedValue<'ctx>,
        negate: bool,
        _span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        let (left_ptr, left_len) = self.bytes_parts(left.value.into_struct_value())?;
        let (right_ptr, right_len) = self.bytes_parts(right.value.into_struct_value())?;
        let function = self.bytes_runtime_function(
            "mas_runtime_bytes_equals",
            &[
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.i64_type().into(),
                self.context.ptr_type(AddressSpace::default()).into(),
                self.context.i64_type().into(),
            ],
            BytesReturn::Byte,
        );
        let call = self.map_builder(self.builder.build_call(
            function,
            &[
                left_ptr.into(),
                left_len.into(),
                right_ptr.into(),
                right_len.into(),
            ],
            "call.bytes_equals",
        ))?;
        let raw = call.try_as_basic_value().unwrap_basic().into_int_value();
        let equal = self.map_builder(self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            raw,
            self.context.i8_type().const_zero(),
            "bytes.equal",
        ))?;
        let result = if negate {
            self.map_builder(self.builder.build_not(equal, "bytes.not_equal"))?
        } else {
            equal
        };
        Ok(TypedValue {
            value_type: ValueType::Bool,
            value: result.into(),
        })
    }
}
