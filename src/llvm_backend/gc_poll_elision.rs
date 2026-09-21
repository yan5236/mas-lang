use super::*;
use inkwell::values::{CallSiteValue, InstructionOpcode, InstructionValue};
use std::collections::HashSet;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 沿控制流求所有前驱的分配状态并删除冗余自动安全点；入口和无前驱块从未知状态开始。
    /// 只有上次安全点之后所有路径均无潜在分配/未知调用时才删除；保留根、帧和显式 GC 调用。
    /// 字典写入即使通常只是更新也视作分配，循环回边和条件分配通过不动点传播，不跨函数推断。
    pub(super) fn elide_redundant_gc_polls(&self) {
        let polls: HashSet<_> = self
            .gc_management
            .borrow()
            .iter()
            .copied()
            .filter(|&instruction| {
                CallSiteValue::try_from(instruction)
                    .ok()
                    .and_then(|call| call.get_called_fn_value())
                    .is_some_and(|callee| callee.get_name().to_bytes() == b"mas_gc_poll")
            })
            .collect();
        let mut removed = HashSet::new();
        for function in self.module.get_functions() {
            let blocks = function.get_basic_blocks();
            let positions: HashMap<_, _> = blocks
                .iter()
                .enumerate()
                .map(|(i, &block)| (block, i))
                .collect();
            let mut predecessors = vec![Vec::new(); blocks.len()];
            for (index, block) in blocks.iter().enumerate() {
                if let Some(terminator) = block.get_terminator() {
                    for successor in terminator
                        .get_operands()
                        .flatten()
                        .filter_map(|operand| operand.block())
                    {
                        predecessors[positions[&successor]].push(index);
                    }
                }
            }
            // true 表示可能仍有分配预算或构造保护待处理；从保守上界收敛。
            let mut dirty_out = vec![true; blocks.len()];
            loop {
                let previous = dirty_out.clone();
                for (index, block) in blocks.iter().enumerate() {
                    let mut dirty = index == 0
                        || predecessors[index].is_empty()
                        || predecessors[index].iter().any(|&parent| previous[parent]);
                    for instruction in block.get_instructions() {
                        if polls.contains(&instruction) {
                            dirty = false;
                        } else if may_change_gc(instruction) {
                            dirty = true;
                        }
                    }
                    dirty_out[index] = dirty;
                }
                if previous == dirty_out {
                    break;
                }
            }
            for (index, block) in blocks.iter().enumerate() {
                let mut dirty = index == 0
                    || predecessors[index].is_empty()
                    || predecessors[index].iter().any(|&parent| dirty_out[parent]);
                for instruction in block.get_instructions() {
                    if polls.contains(&instruction) {
                        if !dirty {
                            removed.insert(instruction);
                        }
                        dirty = false;
                    } else if may_change_gc(instruction) {
                        dirty = true;
                    }
                }
            }
        }
        self.gc_management
            .borrow_mut()
            .retain(|instruction| !removed.contains(instruction));
        for instruction in removed {
            instruction.erase_from_basic_block();
        }
    }
}

/// 未知调用、间接调用及异常调用保守视作可能分配；仅允许确定不分配、不收集的字典读取、栈格式化和整数溢出 intrinsic。
/// 白名单检查完整的 ABI 类型后缀，不放行名称相近的用户 extern 函数或 keys/values 快照分配。
fn may_change_gc(instruction: InstructionValue<'_>) -> bool {
    if matches!(
        instruction.get_opcode(),
        InstructionOpcode::Invoke | InstructionOpcode::CallBr
    ) {
        return true;
    }
    let Ok(call) = CallSiteValue::try_from(instruction) else {
        return false;
    };
    let Some(callee) = call.get_called_fn_value() else {
        return true;
    };
    let name = callee.get_name().to_bytes();
    if matches!(
        name,
        b"map_len" | b"int_to_string_into" | b"map_get_int_int_site"
    ) || numeric_safety::is_checked_arithmetic_intrinsic(name)
    {
        return false;
    }
    let scalar = |name: &[u8]| matches!(name, b"int" | b"float" | b"bool" | b"string");
    if let Some(kind) = name.strip_prefix(b"map_contains_") {
        return !scalar(kind);
    }
    if let Some(kinds) = name.strip_prefix(b"map_get_") {
        if let Some(split) = kinds.iter().position(|&byte| byte == b'_') {
            return !(scalar(&kinds[..split]) && scalar(&kinds[split + 1..]));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::values::AnyValue;

    /// 只读字典循环仅保留入口自动检查，同时保留帧/根；显式 poll 与未知调用均不能被删除。
    #[test]
    fn read_only_loop_elides_polls_but_explicit_calls_survive() {
        let source = SourceFile::new(
            "<poll-elision>",
            r#"
extern func mas_gc_poll()
extern func unknown()
func query(m:map<int,int>, n:int) -> int:
    var sum = 0
    for i in 0..n:
        if m.contains(i):
            sum += m[i]
    return sum
func explicit():
    mas_gc_poll()
    unknown()
"#
            .into(),
        );
        let settings = CompilerSettings::default_settings();
        let frontend = crate::analyze_source(&source, &settings).unwrap();
        let context = Context::create();
        let mut generator = CodeGenerator::new(&context, &frontend.source).unwrap();
        generator.compile_program(&frontend.program).unwrap();
        generator.elide_redundant_gc_polls();
        for info in generator
            .functions
            .values()
            .filter(|info| info.abi == FunctionAbi::User)
        {
            let ir = info.function.print_to_string().to_string();
            if info.function.get_name().to_bytes().ends_with(b"_query") {
                assert_eq!(ir.matches("call void @mas_gc_poll()").count(), 1, "{ir}");
                assert!(ir.contains("call void @mas_gc_root"));
                assert!(ir.contains("call void @mas_gc_enter"));
                assert!(ir.contains("call void @mas_gc_leave"));
            }
            if info.function.get_name().to_bytes().ends_with(b"_explicit") {
                assert!(ir.contains("call void @unknown()"), "{ir}");
                assert!(ir.matches("call void @mas_gc_poll()").count() >= 2, "{ir}");
            }
        }
        generator.module.verify().unwrap();
    }
}
