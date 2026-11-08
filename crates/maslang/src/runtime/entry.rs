unsafe extern "C" {
    fn mascript_entry();
}

/// 执行语言入口，并在全部语言栈帧退出后释放剩余托管对象。
pub fn run() {
    unsafe {
        mascript_entry();
    }
    gc_shutdown();
}

/// 完整源码链接模式的进程入口，复用运行时库的入口及清理流程。
#[cfg(not(mas_runtime_library))]
fn main() {
    run();
}
