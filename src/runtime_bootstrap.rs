/// 拼接可独立检查的运行时源码，供链接阶段写入单个 Rust 引导文件。
pub fn bootstrap_source() -> &'static str {
    concat!(
        "#![allow(unsafe_op_in_unsafe_fn)]\n",
        include_str!("runtime/strings.rs"),
        "\n",
        include_str!("runtime/numeric_errors.rs"),
        "\n",
        include_str!("runtime/containers.rs"),
        "\n",
        include_str!("runtime/lists.rs"),
        "\n",
        include_str!("runtime/maps.rs"),
        "\n",
        include_str!("runtime/gc.rs"),
        "\n",
        include_str!("runtime/entry.rs"),
    )
}

#[cfg(test)]
mod tests {
    use super::bootstrap_source;

    /// 校验引导源码以 UTF-8 无 BOM 形式提供给写盘层。
    #[test]
    fn should_not_include_utf8_bom() {
        let bytes = bootstrap_source().as_bytes();
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    }

    /// 校验运行时引导源码已经包含字符串方法集合对应的导出符号。
    #[test]
    fn should_include_string_method_runtime_symbols() {
        let source = bootstrap_source();
        assert!(source.contains("pub extern \"C\" fn string_trim"));
        assert!(source.contains("pub extern \"C\" fn string_contains"));
        assert!(source.contains("pub extern \"C\" fn string_starts_with"));
        assert!(source.contains("pub extern \"C\" fn string_ends_with"));
        assert!(source.contains("pub extern \"C\" fn string_replace"));
    }

    /// 校验运行时引导源码已经包含原生模块复用的通用字符串与错误辅助符号。
    #[test]
    fn should_include_native_runtime_helper_symbols() {
        let source = bootstrap_source();
        assert!(source.contains("pub extern \"C\" fn mas_runtime_string_clone"));
        assert!(source.contains("pub extern \"C\" fn mas_runtime_abort"));
    }
}

#[cfg(test)]
#[allow(unsafe_op_in_unsafe_fn, dead_code)]
#[path = "runtime/tests.rs"]
mod runtime_tests;
