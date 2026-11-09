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
                i8_type.fn_type(&[i64_type.into()], false),
                None,
            ),
            print_float: module.add_function(
                "print_float",
                i8_type.fn_type(&[f64_type.into()], false),
                None,
            ),
            print_bool: module.add_function(
                "print_bool",
                i8_type.fn_type(&[i8_type.into()], false),
                None,
            ),
            print_string: module.add_function(
                "print_string",
                i8_type.fn_type(&[ptr_type.into(), i64_type.into()], false),
                None,
            ),
            string_to_int: module.add_function(
                "string_to_int",
                i64_type.fn_type(&[ptr_type.into(), i64_type.into()], false),
                None,
            ),
            string_to_float: module.add_function(
                "string_to_float",
                f64_type.fn_type(&[ptr_type.into(), i64_type.into()], false),
                None,
            ),
            try_string_to_int: module.add_function(
                "try_string_to_int",
                i8_type.fn_type(&[ptr_type.into(), i64_type.into(), ptr_type.into()], false),
                None,
            ),
            try_string_to_float: module.add_function(
                "try_string_to_float",
                i8_type.fn_type(&[ptr_type.into(), i64_type.into(), ptr_type.into()], false),
                None,
            ),
            try_string_to_bool: module.add_function(
                "try_string_to_bool",
                i8_type.fn_type(&[ptr_type.into(), i64_type.into(), ptr_type.into()], false),
                None,
            ),
            int_to_string: module.add_function(
                "int_to_string",
                ptr_type.fn_type(&[i64_type.into(), ptr_type.into()], false),
                None,
            ),
            float_to_string: module.add_function(
                "float_to_string",
                ptr_type.fn_type(&[f64_type.into(), ptr_type.into()], false),
                None,
            ),
            bool_to_string: module.add_function(
                "bool_to_string",
                ptr_type.fn_type(&[i8_type.into(), ptr_type.into()], false),
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
            string_trim: module.add_function(
                "string_trim",
                ptr_type.fn_type(&[ptr_type.into(), i64_type.into(), ptr_type.into()], false),
                None,
            ),
            string_contains: module.add_function(
                "string_contains",
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
            string_starts_with: module.add_function(
                "string_starts_with",
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
            string_ends_with: module.add_function(
                "string_ends_with",
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
            string_replace: module.add_function(
                "string_replace",
                ptr_type.fn_type(
                    &[
                        ptr_type.into(),
                        i64_type.into(),
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

        let scalar_abis: [(&str, inkwell::types::BasicTypeEnum<'ctx>); 5] = [
            ("int", i64_type.into()),
            ("float", f64_type.into()),
            ("bool", i8_type.into()),
            ("string", ptr_type.into()),
            ("record", ptr_type.into()),
        ];
        for (key_name, key_type) in scalar_abis {
            for (value_name, value_type) in scalar_abis {
                if key_name != "record" && value_name != "record" {
                    continue;
                }
                let mut key_args = vec![ptr_type.into(), key_type.into()];
                if key_name == "string" {
                    key_args.push(i64_type.into());
                }
                let mut set_args = key_args.clone();
                set_args.push(value_type.into());
                if value_name == "string" {
                    set_args.push(i64_type.into());
                }
                add_void(&format!("map_set_{key_name}_{value_name}"), &set_args);
                if value_name == "string" {
                    key_args.push(ptr_type.into());
                }
                key_args.extend::<[inkwell::types::BasicMetadataTypeEnum<'ctx>; 6]>([
                    ptr_type.into(),
                    i64_type.into(),
                    ptr_type.into(),
                    i64_type.into(),
                    i64_type.into(),
                    i64_type.into(),
                ]);
                add_value(
                    &format!("map_get_{key_name}_{value_name}"),
                    value_type,
                    &key_args,
                );
            }
        }
        add_value(
            "map_contains_record",
            i8_type.into(),
            &[ptr_type.into(), ptr_type.into()],
        );
        add_void(
            "map_remove_record",
            &[
                ptr_type.into(),
                ptr_type.into(),
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
            ],
        );
        add_void("list_append_record", &[ptr_type.into(), ptr_type.into()]);
        add_void(
            "list_set_record",
            &[ptr_type.into(), i64_type.into(), ptr_type.into()],
        );
        add_void("list_remove_record", &[ptr_type.into(), ptr_type.into()]);
        add_value(
            "list_get_record",
            ptr_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value("list_pop_record", ptr_type.into(), &[ptr_type.into()]);
        add_value(
            "list_pop_at_record",
            ptr_type.into(),
            &[ptr_type.into(), i64_type.into()],
        );
        add_value(
            "list_index_record",
            i64_type.into(),
            &[ptr_type.into(), ptr_type.into()],
        );
        add_value(
            "list_contains_record",
            i8_type.into(),
            &[ptr_type.into(), ptr_type.into()],
        );
        add_void("list_append_int", &[ptr_type.into(), i64_type.into()]);
        add_void("list_append_float", &[ptr_type.into(), f64_type.into()]);
        add_void("list_append_bool", &[ptr_type.into(), i8_type.into()]);
        add_void(
            "list_append_string",
            &[ptr_type.into(), ptr_type.into(), i64_type.into()],
        );
        add_void(
            "list_set_int",
            &[ptr_type.into(), i64_type.into(), i64_type.into()],
        );
        add_void(
            "list_set_float",
            &[ptr_type.into(), i64_type.into(), f64_type.into()],
        );
        add_void(
            "list_set_bool",
            &[ptr_type.into(), i64_type.into(), i8_type.into()],
        );
        add_void(
            "list_set_string",
            &[
                ptr_type.into(),
                i64_type.into(),
                ptr_type.into(),
                i64_type.into(),
            ],
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

    /// 构造 `{ ptr, len }` 字符串；对用户长字面量记录已知标量数，诊断专用文本跳过登记。
    pub(super) fn build_string_literal(
        &mut self,
        value: &str,
        user_literal: bool,
    ) -> Result<StructValue<'ctx>, CompilerError> {
        let global = self.map_builder(
            self.builder
                .build_global_string_ptr(value, &format!("string.{}", self.string_index)),
        )?;
        self.string_index += 1;
        // shortcut: 只登记长字面量以限制启动开销；短字面量扫描成为热点时再按测量调整阈值。
        if user_literal && value.len() >= 256 {
            self.long_string_literals.push((
                global.as_pointer_value(),
                value.len(),
                value.chars().count(),
            ));
        }
        self.build_runtime_string(
            global.as_pointer_value(),
            self.context.i64_type().const_int(value.len() as u64, false),
        )
    }

    /// 程序需要运行时取长时，在执行用户代码前登记已知长字面量的字节数与标量数。
    /// 使用独立入口块，避免移动现有的全局初始化和 GC 根帧指令。
    pub(super) fn register_string_literal_lengths(&self) -> Result<(), CompilerError> {
        if self.long_string_literals.is_empty() || self.module.get_function("string_len").is_none() {
            return Ok(());
        }
        let entry = self.entry_function.unwrap().get_first_basic_block().unwrap();
        let setup = self.context.prepend_basic_block(entry, "string.lengths");
        self.builder.position_at_end(setup);
        let i64_type = self.context.i64_type();
        let function = self.module.add_function(
            "mas_register_string_length",
            self.context.void_type().fn_type(
                &[
                    self.context.ptr_type(AddressSpace::default()).into(),
                    i64_type.into(),
                    i64_type.into(),
                ],
                false,
            ),
            None,
        );
        for &(pointer, bytes, scalars) in &self.long_string_literals {
            self.map_builder(self.builder.build_call(
                function,
                &[
                    pointer.into(),
                    i64_type.const_int(bytes as u64, false).into(),
                    i64_type.const_int(scalars as u64, false).into(),
                ],
                "",
            ))?;
        }
        self.map_builder(self.builder.build_unconditional_branch(entry))?;
        Ok(())
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

    /// 返回安全转换结果在 LLVM 中使用的 `{ ok, value }` 结构类型。
    pub(super) fn safe_convert_result_type(
        &self,
        value_type: &ValueType,
    ) -> Result<StructType<'ctx>, CompilerError> {
        Ok(self.context.struct_type(
            &[
                self.context.bool_type().into(),
                self.basic_type_for(value_type)?.into(),
            ],
            false,
        ))
    }

    /// 构造字典运行时错误需要的源码上下文参数，供缺失键等错误精确定位到源码。
    pub(super) fn build_map_error_site_arguments(
        &mut self,
        span: Span,
    ) -> Result<Vec<inkwell::values::BasicMetadataValueEnum<'ctx>>, CompilerError> {
        let path_value = if let Some(value) = self.source_path_value {
            value
        } else {
            let value = self.build_string_literal(self.source.path(), false)?;
            self.source_path_value = Some(value);
            value
        };
        let source_value = if let Some(value) = self.source_text_value {
            value
        } else {
            let value = self.build_string_literal(self.source.text(), false)?;
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
            self.context
                .i64_type()
                .const_int(span.start as u64, false)
                .into(),
            self.context
                .i64_type()
                .const_int(span.end as u64, false)
                .into(),
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
            ValueType::Nullable(inner) => Ok(match inner.as_ref() {
                ValueType::Int => 6,
                ValueType::Float => 7,
                ValueType::Bool => 8,
                ValueType::String => 9,
                _ => 5,
            }),
            ValueType::Struct(_) | ValueType::List(_) | ValueType::Map(_, _) => Ok(5),
            _ => Err(CompilerError::build_message(
                "列表元素只支持基础类型或结构体",
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
            ValueType::Struct(_)
            | ValueType::Nullable(_)
            | ValueType::List(_)
            | ValueType::Map(_, _) => Ok("record"),
            _ => Err(CompilerError::build_message(
                "列表元素只支持基础类型或结构体",
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
