use std::collections::HashMap;

use inkwell::builder::BuilderError;
use inkwell::types::{ArrayType, BasicMetadataTypeEnum, BasicType, BasicTypeEnum};
use inkwell::values::{BasicValueEnum, GlobalValue, PointerValue};

use super::*;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 为全局变量创建模块私有的零值存储及独立初始化标记；声明执行完成前禁止读取或赋值。
    /// 仅限制链接可见性，不提前执行初始化，使 LLVM 能分析模块内实际读写而保留声明顺序错误。
    pub(super) fn declare_global(
        &mut self,
        name: &str,
        value_type: ValueType,
        is_mutable: bool,
    ) -> Result<VariableInfo<'ctx>, CompilerError> {
        if let Some(variable) = self.globals.get(name).cloned() {
            return Ok(variable);
        }

        let symbol = format!("__ms_global_{}_{}", name, self.global_index);
        self.global_index += 1;
        let global = self
            .module
            .add_global(self.basic_type_for(&value_type)?, None, &symbol);
        global.set_linkage(inkwell::module::Linkage::Internal);
        self.initialize_global(&global, &value_type)?;
        let initialized = self.module.add_global(
            self.context.i8_type(),
            None,
            &format!("{symbol}_initialized"),
        );
        initialized.set_linkage(inkwell::module::Linkage::Internal);
        initialized.set_initializer(&self.context.i8_type().const_zero());
        self.global_initialized
            .insert(name.to_string(), initialized.as_pointer_value());
        let variable = VariableInfo {
            value_type,
            pointer: global.as_pointer_value(),
            is_mutable,
        };
        self.globals.insert(name.to_string(), variable.clone());
        Ok(variable)
    }

    /// 为全局变量写入零值初始化器，确保在入口执行前内存布局稳定。
    pub(super) fn initialize_global(
        &self,
        global: &GlobalValue<'ctx>,
        value_type: &ValueType,
    ) -> Result<(), CompilerError> {
        match value_type {
            ValueType::Struct(_) | ValueType::Nullable(_) | ValueType::Null => {
                global.set_initializer(&self.list_pointer_type().const_null())
            }
            ValueType::Int => global.set_initializer(&self.context.i64_type().const_zero()),
            ValueType::Float => global.set_initializer(&self.context.f64_type().const_zero()),
            ValueType::Bool => global.set_initializer(&self.context.bool_type().const_zero()),
            ValueType::String => global.set_initializer(&self.string_type.const_zero()),
            ValueType::SafeConvertResult(value_type) => global.set_initializer(
                &self
                    .safe_convert_result_type(value_type.as_ref())?
                    .const_zero(),
            ),
            ValueType::List(_) => global.set_initializer(&self.list_pointer_type().const_null()),
            ValueType::Map(_, _) => global.set_initializer(&self.map_pointer_type().const_null()),
            ValueType::Array(element, size) => {
                global.set_initializer(&self.array_type_for(element.as_ref(), *size)?.const_zero())
            }
            ValueType::Unit => {}
        }
        Ok(())
    }

    /// 在函数入口为局部变量创建固定槽位；托管类型同时登记为待初始化的精确 GC 根。
    pub(super) fn allocate_local(
        &self,
        value_type: &ValueType,
        name: &str,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        let pointer = self.allocate_entry_slot(self.basic_type_for(value_type)?, name)?;
        if Self::gc_layout(value_type).is_some() {
            self.gc_roots
                .borrow_mut()
                .push((value_type.clone(), pointer));
        }
        Ok(pointer)
    }

    /// 检查声明状态后加载变量并传播已证明的借用键标记；未初始化全局仍按源码位置报错。
    pub(super) fn load_variable(
        &mut self,
        variable: VariableInfo<'ctx>,
        span: Span,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        self.check_global_initialized(&variable, span)?;
        let value = self.map_builder(self.builder.build_load(
            self.basic_type_for(&variable.value_type)?,
            variable.pointer,
            "load.var",
        ))?;
        if self.borrowed_slots.contains(&variable.pointer) {
            self.borrowed_values.insert(value);
        }
        Ok(TypedValue {
            value_type: variable.value_type,
            value,
        })
    }

    /// 把 LLVM 值写回变量指针。
    pub(super) fn store_variable(
        &self,
        variable: VariableInfo<'ctx>,
        value: BasicValueEnum<'ctx>,
    ) -> Result<(), CompilerError> {
        self.map_builder(self.builder.build_store(variable.pointer, value))?;
        Ok(())
    }

    /// 返回指定语义类型在 LLVM 中对应的基础类型对象。
    pub(super) fn basic_type_for(
        &self,
        value_type: &ValueType,
    ) -> Result<BasicTypeEnum<'ctx>, CompilerError> {
        Ok(match value_type {
            ValueType::Struct(_) | ValueType::Nullable(_) | ValueType::Null => {
                self.list_pointer_type().into()
            }
            ValueType::Int => self.context.i64_type().into(),
            ValueType::Float => self.context.f64_type().into(),
            ValueType::Bool => self.context.bool_type().into(),
            ValueType::String => self.string_type.into(),
            ValueType::SafeConvertResult(value_type) => {
                self.safe_convert_result_type(value_type.as_ref())?.into()
            }
            ValueType::List(_) => self.list_pointer_type().into(),
            ValueType::Map(_, _) => self.map_pointer_type().into(),
            ValueType::Array(element, size) => self.array_type_for(element.as_ref(), *size)?.into(),
            ValueType::Unit => {
                return Err(CompilerError::build_message(
                    "unit 不应该被映射为可存储的基础类型",
                    Span::new(0, self.source.text().len()),
                ));
            }
        })
    }

    /// 把基础类型转换为函数签名所需的元数据类型。
    pub(super) fn basic_metadata_type_for(
        &self,
        value_type: &ValueType,
    ) -> Result<BasicMetadataTypeEnum<'ctx>, CompilerError> {
        Ok(self.basic_type_for(value_type)?.into())
    }

    /// 返回原生函数 ABI 使用的 LLVM 类型，当前仅对 `bool` 走 `i8` 兼容 C ABI。
    pub(super) fn extern_basic_type_for(
        &self,
        value_type: &ValueType,
    ) -> Result<BasicTypeEnum<'ctx>, CompilerError> {
        Ok(match value_type {
            ValueType::Bool => self.context.i8_type().into(),
            _ => self.basic_type_for(value_type)?,
        })
    }

    /// 把原生函数参数类型转换为签名元数据类型，供 `extern func` 统一复用。
    pub(super) fn extern_metadata_type_for(
        &self,
        value_type: &ValueType,
    ) -> Result<BasicMetadataTypeEnum<'ctx>, CompilerError> {
        Ok(self.extern_basic_type_for(value_type)?.into())
    }

    /// 根据元素类型和固定长度构造 LLVM 数组类型。
    pub(super) fn array_type_for(
        &self,
        element_type: &ValueType,
        size: usize,
    ) -> Result<ArrayType<'ctx>, CompilerError> {
        Ok(self
            .basic_type_for(element_type)?
            .array_type(size.try_into().map_err(|_| {
                CompilerError::build_message(
                    "数组长度超出了 LLVM 支持的范围",
                    Span::new(0, self.source.text().len()),
                )
            })?))
    }

    /// 返回后端中统一使用的列表句柄指针类型。
    pub(super) fn list_pointer_type(&self) -> inkwell::types::PointerType<'ctx> {
        self.context.ptr_type(inkwell::AddressSpace::default())
    }

    /// 返回后端中统一使用的字典句柄指针类型。
    pub(super) fn map_pointer_type(&self) -> inkwell::types::PointerType<'ctx> {
        self.context.ptr_type(inkwell::AddressSpace::default())
    }

    /// 统一包装所有 Inkwell 构建器错误，转换成内部构建诊断。
    pub(super) fn map_builder<T>(
        &self,
        value: Result<T, BuilderError>,
    ) -> Result<T, CompilerError> {
        value.map_err(|error| {
            CompilerError::build(
                MessageId::BuildWriteIrFailed,
                vec![error.to_string()],
                Span::new(0, self.source.text().len()),
            )
        })
    }

    /// 确保表达式编译结果不是 `unit`，否则返回内部构建错误。
    pub(super) fn expect_value(
        &self,
        value: CodegenValue<'ctx>,
    ) -> Result<TypedValue<'ctx>, CompilerError> {
        match value {
            CodegenValue::Value(value) => Ok(value),
            CodegenValue::Unit => Err(CompilerError::build_message(
                "表达式返回了不可参与计算的 unit 值",
                Span::new(0, self.source.text().len()),
            )),
        }
    }

    /// 判断当前正在生成的语句是否位于顶层入口作用域。
    pub(super) fn is_top_level_scope(&self) -> bool {
        self.current_function.is_some()
            && self.current_function == self.entry_function
            && self.scopes.len() == 1
    }

    /// 在局部作用域链和全局变量表中查找变量。
    pub(super) fn lookup_variable(&self, name: &str) -> Option<VariableInfo<'ctx>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
            .or_else(|| self.globals.get(name).cloned())
    }

    /// 压入一个新的块级作用域。
    pub(super) fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// 弹出块级作用域并清空已失效的托管局部根；终结块由返回帧或循环出口负责清理。
    pub(super) fn pop_scope(&mut self) -> Result<(), CompilerError> {
        if let Some(scope) = self.scopes.pop() {
            if !self.current_block_has_terminator() {
                for variable in scope.values() {
                    if Self::gc_layout(&variable.value_type).is_some() {
                        self.map_builder(self.builder.build_store(
                            variable.pointer,
                            self.basic_type_for(&variable.value_type)?.const_zero(),
                        ))?;
                    }
                }
            }
        }
        Ok(())
    }

    /// 返回当前最内层作用域的可变引用。
    pub(super) fn current_scope_mut(&mut self) -> &mut HashMap<String, VariableInfo<'ctx>> {
        self.scopes
            .last_mut()
            .expect("作用域栈至少包含一个当前作用域")
    }

    /// 判断当前插入块是否已经包含终结指令。
    pub(super) fn current_block_has_terminator(&self) -> bool {
        self.builder
            .get_insert_block()
            .and_then(|block| block.get_terminator())
            .is_some()
    }
}
