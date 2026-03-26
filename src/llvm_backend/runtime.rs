use inkwell::AddressSpace;
use inkwell::module::Module;
use inkwell::types::BasicType;
use inkwell::values::{BasicValueEnum, FunctionValue, IntValue, PointerValue, StructValue};

use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 预声明运行时 ABI，供打印、字符串、列表和字典相关能力直接调用。
    pub(super) fn declare_runtime(
        context: &'ctx Context,
        module: &Module<'ctx>,
    ) -> RuntimeFunctions<'ctx> {
        let void_type = context.void_type();
        let i64_type = context.i64_type();
        let i8_type = context.i8_type();
        let f64_type = context.f64_type();
        let ptr_type = context.ptr_type(AddressSpace::default());

        let runtime = RuntimeFunctions {
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
            list_create: module.add_function(
                "list_create",
                ptr_type.fn_type(&[i8_type.into()], false),
                None,
            ),
            list_len: module.add_function(
                "list_len",
                i64_type.fn_type(&[ptr_type.into()], false),
                None,
            ),
            list_slice: module.add_function(
                "list_slice",
                ptr_type.fn_type(&[ptr_type.into(), i64_type.into(), i64_type.into()], false),
                None,
            ),
            list_reverse: module.add_function(
                "list_reverse",
                void_type.fn_type(&[ptr_type.into()], false),
                None,
            ),
            map_create: module.add_function(
                "map_create",
                ptr_type.fn_type(&[i8_type.into(), i8_type.into()], false),
                None,
            ),
            map_len: module.add_function(
                "map_len",
                i64_type.fn_type(&[ptr_type.into()], false),
                None,
            ),
            map_keys: module.add_function(
                "map_keys",
                ptr_type.fn_type(&[ptr_type.into()], false),
                None,
            ),
            map_values: module.add_function(
                "map_values",
                ptr_type.fn_type(&[ptr_type.into()], false),
                None,
            ),
        };

        let add_void = |name: &str, args: &[inkwell::types::BasicMetadataTypeEnum<'ctx>]| {
            module.add_function(name, void_type.fn_type(args, false), None);
        };
        let add_value =
            |name: &str,
             return_type: inkwell::types::BasicTypeEnum<'ctx>,
             args: &[inkwell::types::BasicMetadataTypeEnum<'ctx>]| {
                module.add_function(name, return_type.fn_type(args, false), None);
            };

        add_void("list_append_int", &[ptr_type.into(), i64_type.into()]);
        add_void("list_append_float", &[ptr_type.into(), f64_type.into()]);
        add_void("list_append_bool", &[ptr_type.into(), i8_type.into()]);
        add_void(
            "list_append_string",
            &[ptr_type.into(), ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_get_int",
            i64_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_get_float",
            f64_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_get_bool",
            i8_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_get_string",
            ptr_type.into(),
            &[ptr_type.into(), i64_type.into(), ptr_type.into()],
        );
        add_void("list_remove_int", &[ptr_type.into(), i64_type.into()]);
        add_void("list_remove_float", &[ptr_type.into(), f64_type.into()]);
        add_void("list_remove_bool", &[ptr_type.into(), i8_type.into()]);
        add_void(
            "list_remove_string",
            &[ptr_type.into(), ptr_type.into(), i64_type.into()],
        );
        add_value("list_pop_int", i64_type.into(), &[ptr_type.into()]);
        add_value("list_pop_float", f64_type.into(), &[ptr_type.into()]);
        add_value("list_pop_bool", i8_type.into(), &[ptr_type.into()]);
        add_value(
            "list_pop_string",
            ptr_type.into(),
            &[ptr_type.into(), ptr_type.into()],
        );
        add_value(
            "list_pop_at_int",
            i64_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_pop_at_float",
            f64_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_pop_at_bool",
            i8_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_pop_at_string",
            ptr_type.into(),
            &[ptr_type.into(), i64_type.into(), ptr_type.into()],
        );
        add_void("list_sort_int", &[ptr_type.into()]);
        add_void("list_sort_float", &[ptr_type.into()]);
        add_void("list_sort_bool", &[ptr_type.into()]);
        add_void("list_sort_string", &[ptr_type.into()]);
        add_value(
            "list_index_int",
            i64_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_index_float",
            i64_type.into(),
            &[ptr_type.into(), f64_type.into()],
        );
        add_value(
            "list_index_bool",
            i64_type.into(),
            &[ptr_type.into(), i8_type.into()],
        );
        add_value(
            "list_index_string",
            i64_type.into(),
            &[ptr_type.into(), ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_contains_int",
            i8_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_contains_float",
            i8_type.into(),
            &[ptr_type.into(), f64_type.into()],
        );
        add_value(
            "list_contains_bool",
            i8_type.into(),
            &[ptr_type.into(), i8_type.into()],
        );
        add_value(
            "list_contains_string",
            i8_type.into(),
            &[ptr_type.into(), ptr_type.into(), i64_type.into()],
        );
        add_void(
            "map_remove_int",
            &[
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_void(
            "map_remove_float",
            &[
                ptr_type.into(),
                f64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_void(
            "map_remove_bool",
            &[
                ptr_type.into(),
                i8_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_void(
            "map_remove_string",
            &[
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_contains_int",
            i8_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "map_contains_float",
            i8_type.into(),
            &[ptr_type.into(), f64_type.into()],
        );
        add_value(
            "map_contains_bool",
            i8_type.into(),
            &[ptr_type.into(), i8_type.into()],
        );
        add_value(
            "map_contains_string",
            i8_type.into(),
            &[ptr_type.into(), ptr_type.into(), i64_type.into()],
        );
        add_value(
            "map_get_int_int",
            i64_type.into(),
            &[
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_int_float",
            f64_type.into(),
            &[
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_int_bool",
            i8_type.into(),
            &[
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_int_string",
            ptr_type.into(),
            &[
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_float_int",
            i64_type.into(),
            &[
                ptr_type.into(),
                f64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_float_float",
            f64_type.into(),
            &[
                ptr_type.into(),
                f64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_float_bool",
            i8_type.into(),
            &[
                ptr_type.into(),
                f64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_float_string",
            ptr_type.into(),
            &[
                ptr_type.into(),
                f64_type.into(),
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_bool_int",
            i64_type.into(),
            &[
                ptr_type.into(),
                i8_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_bool_float",
            f64_type.into(),
            &[
                ptr_type.into(),
                i8_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_bool_bool",
            i8_type.into(),
            &[
                ptr_type.into(),
                i8_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_bool_string",
            ptr_type.into(),
            &[
                ptr_type.into(),
                i8_type.into(),
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_string_int",
            i64_type.into(),
            &[
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_string_float",
            f64_type.into(),
            &[
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_string_bool",
            i8_type.into(),
            &[
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_value(
            "map_get_string_string",
            ptr_type.into(),
            &[
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_void(
            "map_set_int_int",
            &[ptr_type.into(), i64_type.into(), i64_type.into()],
        );
        add_void(
            "map_set_int_float",
            &[ptr_type.into(), i64_type.into(), f64_type.into()],
        );
        add_void(
            "map_set_int_bool",
            &[ptr_type.into(), i64_type.into(), i8_type.into()],
        );
        add_void(
            "map_set_int_string",
            &[
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
            ],
        );
        add_void(
            "map_set_float_int",
            &[ptr_type.into(), f64_type.into(), i64_type.into()],
        );
        add_void(
            "map_set_float_float",
            &[ptr_type.into(), f64_type.into(), f64_type.into()],
        );
        add_void(
            "map_set_float_bool",
            &[ptr_type.into(), f64_type.into(), i8_type.into()],
        );
        add_void(
            "map_set_float_string",
            &[
                ptr_type.into(),
                f64_type.into(),
                ptr_type.into(),
                i64_type.into(),
            ],
        );
        add_void(
            "map_set_bool_int",
            &[ptr_type.into(), i8_type.into(), i64_type.into()],
        );
        add_void(
            "map_set_bool_float",
            &[ptr_type.into(), i8_type.into(), f64_type.into()],
        );
        add_void(
            "map_set_bool_bool",
            &[ptr_type.into(), i8_type.into(), i8_type.into()],
        );
        add_void(
            "map_set_bool_string",
            &[
                ptr_type.into(),
                i8_type.into(),
                ptr_type.into(),
                i64_type.into(),
            ],
        );
        add_void(
            "map_set_string_int",
            &[
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_void(
            "map_set_string_float",
            &[
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                f64_type.into(),
            ],
        );
        add_void(
            "map_set_string_bool",
            &[
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i8_type.into(),
            ],
        );
        add_void(
            "map_set_string_string",
            &[
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
            ],
        );

        runtime
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

    /// 构造字典运行时错误需要的源码上下文参数，供缺失键等错误精确定位到源码。
    pub(super) fn build_map_error_site_arguments(
        &mut self,
        span: Span,
    ) -> Result<Vec<inkwell::values::BasicMetadataValueEnum<'ctx>>, CompilerError> {
        let path_value = if let Some(value) = self.source_path_value {
            value
        } else {
            let value = self.build_string_literal(self.source.path())?;
            self.source_path_value = Some(value);
            value
        };
        let source_value = if let Some(value) = self.source_text_value {
            value
        } else {
            let value = self.build_string_literal(self.source.text())?;
            self.source_text_value = Some(value);
            value
        };

        let path_ptr = self.extract_string_part(path_value, 0, "map.error.path.ptr")?;
        let path_len = self.extract_string_part(path_value, 1, "map.error.path.len")?;
        let source_ptr = self.extract_string_part(source_value, 0, "map.error.source.ptr")?;
        let source_len = self.extract_string_part(source_value, 1, "map.error.source.len")?;

        Ok(vec![
            path_ptr.into(),
            path_len.into(),
            source_ptr.into(),
            source_len.into(),
            self.context.i64_type().const_int(span.start as u64, false).into(),
            self.context.i64_type().const_int(span.end as u64, false).into(),
        ])
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

    /// 返回列表元素类型在运行时中的固定类型标记。
    pub(super) fn list_type_tag(&self, value_type: &ValueType) -> Result<u8, CompilerError> {
        match value_type {
            ValueType::Int => Ok(1),
            ValueType::Float => Ok(2),
            ValueType::Bool => Ok(3),
            ValueType::String => Ok(4),
            _ => Err(CompilerError::build_message(
                "当前阶段列表元素只支持基础类型",
                Span::new(0, self.source.text().len()),
            )),
        }
    }

    /// 把列表元素类型转换为运行时函数名使用的后缀。
    pub(super) fn list_type_suffix(
        &self,
        value_type: &ValueType,
    ) -> Result<&'static str, CompilerError> {
        match value_type {
            ValueType::Int => Ok("int"),
            ValueType::Float => Ok("float"),
            ValueType::Bool => Ok("bool"),
            ValueType::String => Ok("string"),
            _ => Err(CompilerError::build_message(
                "当前阶段列表元素只支持基础类型",
                Span::new(0, self.source.text().len()),
            )),
        }
    }

    /// 根据基础名称和元素类型查找已声明的列表运行时函数。
    pub(super) fn list_runtime_function(
        &self,
        base_name: &str,
        value_type: &ValueType,
    ) -> Result<FunctionValue<'ctx>, CompilerError> {
        let suffix = self.list_type_suffix(value_type)?;
        let function_name = format!("{base_name}_{suffix}");
        self.module.get_function(&function_name).ok_or_else(|| {
            CompilerError::build_message(
                format!("代码生成阶段找不到运行时函数 `{function_name}`"),
                Span::new(0, self.source.text().len()),
            )
        })
    }

    /// 根据键类型和值类型查找已声明的字典运行时函数。
    pub(super) fn map_runtime_function(
        &self,
        base_name: &str,
        key_type: &ValueType,
        value_type: Option<&ValueType>,
    ) -> Result<FunctionValue<'ctx>, CompilerError> {
        let key_suffix = self.list_type_suffix(key_type)?;
        let function_name = if let Some(value_type) = value_type {
            let value_suffix = self.list_type_suffix(value_type)?;
            format!("{base_name}_{key_suffix}_{value_suffix}")
        } else {
            format!("{base_name}_{key_suffix}")
        };
        self.module.get_function(&function_name).ok_or_else(|| {
            CompilerError::build_message(
                format!("代码生成阶段找不到运行时函数 `{function_name}`"),
                Span::new(0, self.source.text().len()),
            )
        })
    }
}
