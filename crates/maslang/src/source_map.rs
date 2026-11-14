use crate::diagnostic::CompilerError;
use crate::source::{SourceFile, Span};
use std::collections::BTreeMap;

/// 保存合并源码中一个模块的起始偏移、原文和源名称到内部名称的映射。
pub struct ModuleOrigin {
    pub offset: usize,
    pub source: SourceFile,
    pub bindings: BTreeMap<String, String>,
}

/// 将语义错误还原到所属文件及源码名称；未命中映射的错误保持原样。
pub fn restore_error(mut error: CompilerError, origins: &[ModuleOrigin]) -> CompilerError {
    if let Some(origin) = origins.iter().find(|origin| {
        error.span.start >= origin.offset
            && error.span.start <= origin.offset + origin.source.text().len()
    }) {
        error.span = Span::new(
            error.span.start - origin.offset,
            error
                .span
                .end
                .saturating_sub(origin.offset)
                .min(origin.source.text().len()),
        );
        // 先按所属模块映射，再回退到整个模块图：跨模块引用的类型与函数只在导入方
        // 模块里记着内部名，只查当前模块会把 `__ms_*` 内部标识泄漏到诊断里。
        let visible_name = |inner: &str| -> Option<String> {
            origin
                .bindings
                .iter()
                .find(|(_, internal)| internal.as_str() == inner)
                .or_else(|| {
                    origins
                        .iter()
                        .flat_map(|candidate| candidate.bindings.iter())
                        .find(|(_, internal)| internal.as_str() == inner)
                })
                .map(|(name, _)| name.clone())
        };
        for argument in &mut error.args {
            // 按完整诊断标识符替换，避免名称前缀或下划线产生歧义。
            let parts: Vec<String> = argument
                .split('`')
                .enumerate()
                .map(|(index, part)| {
                    if index % 2 == 1 {
                        visible_name(part).unwrap_or_else(|| part.to_string())
                    } else {
                        part.to_string()
                    }
                })
                .collect();
            *argument = parts.join("`");
            if let Some(name) = visible_name(argument) {
                *argument = name;
            }
        }
        error.source = Some(origin.source.clone());
    }
    error
}
