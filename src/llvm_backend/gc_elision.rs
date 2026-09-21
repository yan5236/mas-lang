use super::*;
use inkwell::values::{CallSiteValue, InstructionOpcode};
use std::collections::HashSet;

impl<'ctx, 'src> CodeGenerator<'ctx, 'src> {
    /// 对用户函数调用图求保守不动点，找出没有托管根、分配或未知调用的函数（含递归环）。
    /// 只移除记录过的自动 GC 调用；根注册、显式 GC、原生调用均使函数及其调用者退出候选集。
    /// 入口帧保留；剩余调用只有纯计算（含精确识别的整数溢出 intrinsic）或终止进程的诊断，不会清除调用者的 pending 临时保护。
    pub(super) fn elide_scalar_gc(&self) {
        let generated: HashSet<_> = self.gc_management.borrow().iter().copied().collect();
        let mut safe: HashSet<_> = self
            .functions
            .values()
            .filter(|info| info.abi == FunctionAbi::User)
            .map(|info| info.function)
            .collect();
        loop {
            let previous = safe.clone();
            safe.retain(|function| {
                function.get_basic_blocks().iter().all(|block| {
                    block.get_instructions().all(|instruction| {
                        if generated.contains(&instruction) {
                            return true;
                        }
                        if instruction.get_opcode() == InstructionOpcode::Invoke {
                            return false;
                        }
                        let Ok(call) = CallSiteValue::try_from(instruction) else {
                            return true;
                        };
                        let Some(callee) = call.get_called_fn_value() else {
                            return false;
                        };
                        numeric_safety::is_checked_arithmetic_intrinsic(
                            callee.get_name().to_bytes(),
                        ) || previous.contains(&callee)
                            || matches!(
                                callee.get_name().to_bytes(),
                                b"mas_numeric_error" | b"mas_global_uninitialized"
                            )
                    })
                })
            });
            if safe == previous {
                break;
            }
        }
        for instruction in self.gc_management.borrow_mut().drain(..) {
            let function = instruction
                .get_parent()
                .and_then(|block| block.get_parent());
            if function.is_some_and(|function| safe.contains(&function)) {
                instruction.erase_from_basic_block();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::values::AnyValue;

    /// 验证递归环整体消除、分配/原生调用沿调用链传播，以及显式 GC 调用不被误当成自动指令。
    #[test]
    fn preserves_effectful_call_graph_and_elides_scalar_cycles() {
        let source = SourceFile::new(
            "<gc-elision>",
            r#"
extern func mas_gc_collect()
extern func mas_gc_poll()
func pure(n:int) -> int:
    if n < 2:
        return n
    return pure(n-1) + pure(n-2)
func even(n:int) -> bool:
    if n == 0:
        return true
    return odd(n-1)
func odd(n:int) -> bool:
    if n == 0:
        return false
    return even(n-1)
func allocate(n:int) -> int:
    var text = string(n)
    return len(text)
func middle(n:int) -> int:
    return allocate(n)
func outer(n:int) -> int:
    return middle(n)
func native():
    mas_gc_collect()
func explicit():
    mas_gc_poll()
func mixed(n:int) -> int:
    if n == 0:
        return allocate(n)
    return mixed_caller(n-1)
func mixed_caller(n:int) -> int:
    return mixed(n)
"#
            .into(),
        );
        let settings = CompilerSettings::default_settings();
        let frontend = crate::analyze_source(&source, &settings).unwrap();
        let context = Context::create();
        let mut generator = CodeGenerator::new(&context, &frontend.source).unwrap();
        generator.compile_program(&frontend.program).unwrap();
        generator.elide_scalar_gc();
        for name in [
            "pure",
            "even",
            "odd",
            "allocate",
            "middle",
            "outer",
            "native",
            "explicit",
            "mixed",
            "mixed_caller",
        ] {
            let suffix = format!("_{name}");
            let function = generator
                .functions
                .values()
                .find(|info| {
                    info.abi == FunctionAbi::User
                        && info
                            .function
                            .get_name()
                            .to_string_lossy()
                            .ends_with(&suffix)
                })
                .unwrap()
                .function;
            let ir = function.print_to_string().to_string();
            let should_keep = !matches!(name, "pure" | "even" | "odd");
            assert_eq!(
                ir.contains("call void @mas_gc_enter"),
                should_keep,
                "{name}: {ir}"
            );
            assert_eq!(
                ir.contains("call void @mas_gc_leave"),
                should_keep,
                "{name}: {ir}"
            );
            assert_eq!(
                ir.contains("call void @mas_gc_poll"),
                should_keep,
                "{name}: {ir}"
            );
        }
        let entry = generator
            .entry_function
            .unwrap()
            .print_to_string()
            .to_string();
        assert!(entry.contains("call void @mas_gc_enter"));
        generator.module.verify().unwrap();
    }
}
