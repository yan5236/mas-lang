use tower_lsp::lsp_types::Hover;

/// 在符号分析尚未接入前返回空悬停结果，保持 handler 后续可独立演进。
pub fn empty_hover() -> Option<Hover> {
    None
}
