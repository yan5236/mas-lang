/// 预留编译器诊断到 LSP 诊断的转换边界，后续阶段会在这里集中处理中文错误消息。
pub fn diagnostics_module_ready() -> bool {
    true
}
