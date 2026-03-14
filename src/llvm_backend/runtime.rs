use inkwell::AddressSpace;
use inkwell::module::Module;
use inkwell::values::{BasicValueEnum, IntValue, PointerValue, StructValue};

use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 预声明最小运行时 ABI，供 `print`、字符串拼接和字符串比较直接调用。
    pub(super) fn declare_runtime(
        context: &'ctx Context,
        module: &Module<'ctx>,
    ) -> RuntimeFunctions<'ctx> {
        let void_type = context.void_type();
        let i64_type = context.i64_type();
        let i8_type = context.i8_type();
        let f64_type = context.f64_type();
        let ptr_type = context.ptr_type(AddressSpace::default());

        RuntimeFunctions {
            print_int: module.add_function(
                "print_int",
                void_type.fn_type(&[i64_type.into()], false),
                None,
            ),
            print_float: module.add_function(
                "print_float",
                void_type.fn_type(&[f64_type.into()], false),
                None,
            ),
            print_bool: module.add_function(
                "print_bool",
                void_type.fn_type(&[i8_type.into()], false),
                None,
            ),
            print_string: module.add_function(
                "print_string",
                void_type.fn_type(&[ptr_type.into(), i64_type.into()], false),
                None,
            ),
            string_concat: module.add_function(
                "string_concat",
                ptr_type.fn_type(
                    &[
                        ptr_type.into(),
                        i64_type.into(),
                        ptr_type.into(),
                        i64_type.into(),
                        ptr_type.into(),
                    ],
                    false,
                ),
                None,
            ),
            string_equals: module.add_function(
                "string_equals",
                i8_type.fn_type(
                    &[
                        ptr_type.into(),
                        i64_type.into(),
                        ptr_type.into(),
                        i64_type.into(),
                    ],
                    false,
                ),
                None,
            ),
        }
    }

    /// 把字符串字面量构造成 `{ ptr, len }` 结构值。
    pub(super) fn build_string_literal(
        &mut self,
        value: &str,
    ) -> Result<StructValue<'ctx>, CompilerError> {
        let global = self.map_builder(
            self.builder
                .build_global_string_ptr(value, &format!("string.{}", self.string_index)),
        )?;
        self.string_index += 1;
        self.build_runtime_string(
            global.as_pointer_value(),
            self.context.i64_type().const_int(value.len() as u64, false),
        )
    }

    /// 使用运行期值构造内部字符串结构。
    pub(super) fn build_runtime_string(
        &self,
        pointer: PointerValue<'ctx>,
        length: IntValue<'ctx>,
    ) -> Result<StructValue<'ctx>, CompilerError> {
        let initial = self.string_type.get_undef();
        let with_ptr = self
            .map_builder(
                self.builder
                    .build_insert_value(initial, pointer, 0, "string.ptr"),
            )?
            .into_struct_value();
        let with_len = self
            .map_builder(
                self.builder
                    .build_insert_value(with_ptr, length, 1, "string.len"),
            )?
            .into_struct_value();
        Ok(with_len)
    }

    /// 从内部字符串结构值中提取指定字段，避免重复写 `extractvalue` 指令。
    pub(super) fn extract_string_part(
        &self,
        value: StructValue<'ctx>,
        index: u32,
        name: &str,
    ) -> Result<BasicValueEnum<'ctx>, CompilerError> {
        self.map_builder(self.builder.build_extract_value(value, index, name))
    }
}
