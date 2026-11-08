/// 表示源码中的半开区间，供词法、语法和语义阶段统一标记位置。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// 使用起止字节偏移构造一个新的源码区间对象。
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// 将两个已有区间合并为一个覆盖范围更大的新区间。
    pub fn merge(left: Span, right: Span) -> Self {
        Self {
            start: left.start.min(right.start),
            end: left.end.max(right.end),
        }
    }
}

/// 保存单个源码文件的路径和文本内容，为诊断渲染提供上下文。
#[derive(Clone, Debug)]
pub struct SourceFile {
    path: String,
    text: String,
}

impl SourceFile {
    /// 根据路径标识和源码文本创建新的源码文件对象。
    pub fn new(path: impl Into<String>, text: String) -> Self {
        Self {
            path: path.into(),
            text,
        }
    }

    /// 返回当前源码文件对应的路径标识文本。
    pub fn path(&self) -> &str {
        &self.path
    }

    /// 返回完整源码内容，供各编译阶段顺序读取。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 把字节偏移转换为 1 基的行号和列号。
    pub fn line_col(&self, position: usize) -> (usize, usize) {
        let clamped = position.min(self.text.len());
        let mut line = 1usize;
        let mut column = 1usize;

        for character in self.text[..clamped].chars() {
            if character == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }

        (line, column)
    }

    /// 返回指定行的原始文本，行号按 1 开始计算。
    pub fn line_text(&self, target_line: usize) -> &str {
        self.text
            .lines()
            .nth(target_line.saturating_sub(1))
            .unwrap_or("")
    }
}
