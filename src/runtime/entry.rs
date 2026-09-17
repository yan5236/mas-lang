unsafe extern "C" {
    fn mascript_entry();
}

/// 执行语言入口，并在全部语言栈帧退出后释放剩余托管对象。
fn main() {
    unsafe {
        mascript_entry();
    }
    gc_shutdown();
}
