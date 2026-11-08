use maslang::source::Span;
use tower_lsp::lsp_types::{Position, Range};

/// 构造零长度 LSP range，供后续诊断转换在缺少源码跨度时使用。
pub fn zero_range() -> Range {
    Range {
        start: Position::new(0, 0),
        end: Position::new(0, 0),
    }
}

/// 将编译器使用的字节跨度转换为 LSP 规定的零基 UTF-16 文本范围。
pub fn span_to_range(text: &str, span: Span) -> Range {
    Range {
        start: byte_offset_to_position(text, span.start),
        end: byte_offset_to_position(text, span.end),
    }
}

/// 把 LSP 位置转换为源码字节偏移，供补全和悬停从编辑器位置定位源码词元。
pub fn position_to_byte_offset(text: &str, position: Position) -> usize {
    let mut line = 0u32;
    let mut character = 0u32;

    for (offset, ch) in text.char_indices() {
        if line == position.line && character >= position.character {
            return offset;
        }

        if ch == '\n' {
            if line == position.line {
                return offset;
            }
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    text.len()
}

/// 把源码字节偏移转换为 LSP 位置，列号按 UTF-16 code unit 计算。
fn byte_offset_to_position(text: &str, offset: usize) -> Position {
    let clamped = clamp_to_char_boundary(text, offset);
    let mut line = 0u32;
    let mut character = 0u32;

    for ch in text[..clamped].chars() {
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    Position::new(line, character)
}

/// 将任意字节偏移收敛到合法字符边界，避免切片时因半个 UTF-8 字符崩溃。
fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    let mut clamped = offset.min(text.len());
    while clamped > 0 && !text.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}
