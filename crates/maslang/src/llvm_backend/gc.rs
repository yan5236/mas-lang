use super::*;
use inkwell::types::BasicTypeEnum;
use inkwell::values::{FunctionValue, InstructionOpcode};

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 返回托管槽的扫描种类与数量；递归展开同构数组，字符串按双字布局、其他引用按指针布局扫描。
    pub(super) fn gc_layout(value_type: &ValueType) -> Option<(u8, u64)> {
        match value_type {
            ValueType::String => Some((1, 1)),
            ValueType::Struct(_)
            | ValueType::Nullable(_)
            | ValueType::List(_)
            | ValueType::Map(_, _) => Some((2, 1)),
            ValueType::Array(element, count) => {
                Self::gc_layout(element).map(|(kind, inner)| (kind, inner * *count as u64))
            }
            _ => None,
        }
    }

    /// 在函数入口创建固定大小栈槽，循环只复用槽位，不按迭代次数消耗栈。
    pub(super) fn allocate_entry_slot(
        &self,
        value_type: BasicTypeEnum<'ctx>,
        name: &str,
    ) -> Result<PointerValue<'ctx>, CompilerError> {
        let entry = self
            .current_function
            .expect("function context")
            .get_first_basic_block()
            .expect("entry block");
        let builder = self.context.create_builder();
        if let Some(first) = entry.get_first_instruction() {
            builder.position_before(&first);
        } else {
            builder.position_at_end(entry);
        }
        self.map_builder(builder.build_alloca(value_type, name))
    }

    /// 为托管表达式保留根以跨越嵌套调用；已证明借用的栈字符串无需堆根，实际入图复制由字典写入负责。
    pub(super) fn protect_gc_value(&self, value: &TypedValue<'ctx>) -> Result<(), CompilerError> {
        if Self::gc_layout(&value.value_type).is_some()
            && !self.borrowed_values.contains(&value.value)
        {
            let slot = self.allocate_local(&value.value_type, "gc.temp")?;
            self.map_builder(self.builder.build_store(slot, value.value))?;
            self.gc_temporaries
                .borrow_mut()
                .push((value.value_type.clone(), slot));
        }
        Ok(())
    }

    /// 清空本条语句产生的临时根；外层语句的实参和遍历快照继续受到保护。
    pub(super) fn clear_gc_temporaries(&self, start: usize) -> Result<(), CompilerError> {
        for (value_type, pointer) in &self.gc_temporaries.borrow()[start..] {
            self.map_builder(
                self.builder
                    .build_store(*pointer, self.basic_type_for(value_type)?.const_zero()),
            )?;
        }
        Ok(())
    }

    /// 声明无参 GC 辅助函数，复用相同符号，避免与原生诊断声明重复。
    fn gc_void_function(&self, name: &str) -> FunctionValue<'ctx> {
        self.module.get_function(name).unwrap_or_else(|| {
            self.module
                .add_function(name, self.context.void_type().fn_type(&[], false), None)
        })
    }

    /// 在当前语句边界或循环头插入安全点；此时没有未保护的 LLVM 中间结果。
    pub(super) fn emit_gc_poll(&self) -> Result<(), CompilerError> {
        self.emit_gc_management(&self.builder, "mas_gc_poll")
    }

    /// 记录编译器自动插入的帧/安全点指令，供无分配函数优化使用；显式 extern 调用不在此集合内。
    fn emit_gc_management(&self, builder: &Builder<'ctx>, name: &str) -> Result<(), CompilerError> {
        let call = self.map_builder(builder.build_call(self.gc_void_function(name), &[], ""))?;
        self.gc_management.borrow_mut().push(
            call.try_as_basic_value()
                .instruction()
                .expect("void GC call"),
        );
        Ok(())
    }

    /// 函数生成结束后统一零初始化根、注册栈帧，并在每个返回边之前移除根帧。
    pub(super) fn finish_gc_frame(&self) -> Result<(), CompilerError> {
        let function = self.current_function.expect("function context");
        let entry = function.get_first_basic_block().expect("entry block");
        let mut instruction = entry.get_first_instruction();
        while instruction.is_some_and(|value| value.get_opcode() == InstructionOpcode::Alloca) {
            instruction = instruction.unwrap().get_next_instruction();
        }
        let builder = self.context.create_builder();
        builder.position_before(&instruction.expect("function body"));
        let roots = self.gc_roots.borrow();
        for (value_type, pointer) in roots.iter() {
            self.map_builder(
                builder.build_store(*pointer, self.basic_type_for(value_type)?.const_zero()),
            )?;
        }
        for pointer in self.gc_handle_roots.borrow().iter() {
            self.map_builder(builder.build_store(
                *pointer,
                self.context.ptr_type(AddressSpace::default()).const_null(),
            ))?;
        }
        self.emit_gc_management(&builder, "mas_gc_enter")?;
        let root_function = self.module.get_function("mas_gc_root").unwrap_or_else(|| {
            self.module.add_function(
                "mas_gc_root",
                self.context.void_type().fn_type(
                    &[
                        self.context.ptr_type(AddressSpace::default()).into(),
                        self.context.i8_type().into(),
                        self.context.i64_type().into(),
                    ],
                    false,
                ),
                None,
            )
        });
        let mut all_roots = roots.clone();
        if Some(function) == self.entry_function {
            all_roots.extend(
                self.globals
                    .values()
                    .filter(|value| Self::gc_layout(&value.value_type).is_some())
                    .map(|value| (value.value_type.clone(), value.pointer)),
            );
        }
        for (value_type, pointer) in all_roots {
            let (kind, count) = Self::gc_layout(&value_type).expect("managed root");
            self.map_builder(builder.build_call(
                root_function,
                &[
                    pointer.into(),
                    self.context.i8_type().const_int(kind as u64, false).into(),
                    self.context.i64_type().const_int(count, false).into(),
                ],
                "",
            ))?;
        }
        for pointer in self.gc_handle_roots.borrow().iter() {
            self.map_builder(builder.build_call(
                root_function,
                &[
                    (*pointer).into(),
                    self.context.i8_type().const_int(2, false).into(),
                    self.context.i64_type().const_int(1, false).into(),
                ],
                "",
            ))?;
        }
        for block in function.get_basic_blocks() {
            if let Some(terminator) = block.get_terminator() {
                if terminator.get_opcode() == InstructionOpcode::Return {
                    builder.position_before(&terminator);
                    self.emit_gc_management(&builder, "mas_gc_leave")?;
                }
            }
        }
        Ok(())
    }
}
