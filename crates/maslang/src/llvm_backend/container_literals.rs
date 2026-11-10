use super::*;
use crate::ast::Expr;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 编译列表字面量，每个元素从左到右仅求值一次；首项同时确定类型并作为首个追加值。
    pub(super) fn compile_list_literal(
        &mut self,
        elements: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        self.compile_list_literal_expected(elements, None, span)
    }

    /// 按显式或推断元素类型构造列表；元素仅求值一次，可空提升及嵌套空容器由期望类型处理。
    pub(super) fn compile_list_literal_expected(
        &mut self,
        elements: &[Expr],
        expected: Option<&ValueType>,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if let Some(element_type) = expected {
            let list = self.build_empty_list(element_type)?;
            let result = TypedValue {
                value_type: ValueType::List(Box::new(element_type.clone())),
                value: list.into(),
            };
            self.protect_gc_value(&result)?;
            for element in elements {
                let value = self.compile_expected(element, element_type)?;
                self.build_list_append(list, element_type, value, span)?;
            }
            return Ok(CodegenValue::Value(result));
        }
        let first_value = if let Some(first) = elements.first() {
            let compiled_first = self.compile_expr(first)?;
            self.expect_value(compiled_first)?
        } else {
            return Err(CompilerError::build_message(
                "代码生成阶段无法直接编译缺少类型信息的空列表",
                span,
            ));
        };

        let element_type = self.expect_list_element_type(first_value.value_type.clone(), span)?;
        let list_value = self.build_empty_list(&element_type)?;
        self.protect_gc_value(&TypedValue {
            value_type: ValueType::List(Box::new(element_type.clone())),
            value: list_value.into(),
        })?;
        self.build_list_append(list_value, &element_type, first_value, span)?;
        for element in &elements[1..] {
            let value = self.compile_expected(element, &element_type)?;
            self.build_list_append(list_value, &element_type, value, span)?;
        }

        Ok(CodegenValue::Value(TypedValue {
            value_type: ValueType::List(Box::new(element_type)),
            value: list_value.into(),
        }))
    }

    /// 编译字典字面量，并逐项调用运行时写入键值对。
    pub(super) fn compile_map_literal(
        &mut self,
        entries: &[crate::ast::MapEntry],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        self.compile_map_literal_expected(entries, None, span)
    }

    /// 按键和值的期望类型构造字典，保留可空转换、空容器提示及半构造对象的临时根。
    pub(super) fn compile_map_literal_expected(
        &mut self,
        entries: &[crate::ast::MapEntry],
        expected: Option<(&ValueType, &ValueType)>,
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        if let Some((key_type, value_type)) = expected {
            let map = self.build_empty_map(key_type, value_type)?;
            let result = TypedValue {
                value_type: ValueType::Map(
                    Box::new(key_type.clone()),
                    Box::new(value_type.clone()),
                ),
                value: map.into(),
            };
            self.protect_gc_value(&result)?;
            for entry in entries {
                let key = self.compile_expected(&entry.key, key_type)?;
                let value = self.compile_expected(&entry.value, value_type)?;
                self.build_map_set(map, key_type, value_type, key, value, entry.span)?;
            }
            return Ok(CodegenValue::Value(result));
        }
        let Some(first_entry) = entries.first() else {
            return Err(CompilerError::build_message(
                "代码生成阶段无法直接编译缺少类型信息的空字典",
                span,
            ));
        };

        let first_key_compiled = self.compile_expr(&first_entry.key)?;
        let first_key = self.expect_value(first_key_compiled)?;
        let first_value_compiled = self.compile_expr(&first_entry.value)?;
        let first_value = self.expect_value(first_value_compiled)?;
        let key_type =
            self.expect_list_element_type(first_key.value_type.clone(), first_entry.key.span())?;
        let value_type = self
            .expect_list_element_type(first_value.value_type.clone(), first_entry.value.span())?;
        let map_value = self.build_empty_map(&key_type, &value_type)?;
        self.protect_gc_value(&TypedValue {
            value_type: ValueType::Map(Box::new(key_type.clone()), Box::new(value_type.clone())),
            value: map_value.into(),
        })?;
        self.build_map_set(
            map_value,
            &key_type,
            &value_type,
            first_key,
            first_value,
            span,
        )?;

        for entry in &entries[1..] {
            let key = self.compile_expected(&entry.key, &key_type)?;
            let value = self.compile_expected(&entry.value, &value_type)?;
            if key.value_type != key_type {
                return Err(CompilerError::build_message(
                    "字典字面量中的键类型必须保持一致",
                    entry.key.span(),
                ));
            }
            if value.value_type != value_type {
                return Err(CompilerError::build_message(
                    "字典字面量中的值类型必须保持一致",
                    entry.value.span(),
                ));
            }
            self.build_map_set(map_value, &key_type, &value_type, key, value, entry.span)?;
        }

        Ok(CodegenValue::Value(TypedValue {
            value_type: ValueType::Map(Box::new(key_type), Box::new(value_type)),
            value: map_value.into(),
        }))
    }

    /// 编译固定长度数组初始化表达式，并把结果组装为 LLVM 原生数组值。
    pub(super) fn compile_array_initializer(
        &mut self,
        element_type: &TypeName,
        size: usize,
        elements: &[Expr],
        span: Span,
    ) -> Result<CodegenValue<'ctx>, CompilerError> {
        let element_value_type = self.value_type_from_annotation(element_type);
        if elements.len() != size {
            return Err(CompilerError::build_message(
                format!(
                    "数组初始化需要 {size} 个元素，但实际提供了 {} 个",
                    elements.len()
                ),
                span,
            ));
        }

        let array_value_type = ValueType::Array(Box::new(element_value_type.clone()), size);
        let array_pointer = self.allocate_local(&array_value_type, "array.init")?;
        for (index, element) in elements.iter().enumerate() {
            let value = self.compile_expected(element, &element_value_type)?;
            if value.value_type != element_value_type {
                return Err(CompilerError::build_message(
                    "数组初始化中的元素类型必须与声明类型一致",
                    element.span(),
                ));
            }
            self.build_array_store(
                array_pointer,
                &element_value_type,
                size,
                self.context.i64_type().const_int(index as u64, false),
                value.value,
            )?;
        }

        let array_value = self.map_builder(self.builder.build_load(
            self.array_type_for(&element_value_type, size)?,
            array_pointer,
            "load.array.init",
        ))?;
        Ok(CodegenValue::Value(TypedValue {
            value_type: array_value_type,
            value: array_value,
        }))
    }
}
