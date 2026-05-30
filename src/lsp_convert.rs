use tower_lsp::lsp_types::{Position, Range};

/// 构造零长度 LSP range，供后续诊断转换在缺少源码跨度时使用。
pub fn zero_range() -> Range {
    Range {
        start: Position::new(0, 0),
        end: Position::new(0, 0),
    }
}
