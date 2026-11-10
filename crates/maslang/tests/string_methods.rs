use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, analyze_source, compile_to_executable};
use std::process::{Command, Output};

/// 把 Mas 源码编译到临时目录并运行，返回完整进程输出；编译失败时直接带源码渲染诊断。
fn run_program(name: &str, body: &str) -> Output {
    let root = TemporaryDirectory::create(name).unwrap();
    let source = SourceFile::new(
        root.path().join("main.ms").display().to_string(),
        body.to_string(),
    );
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.path().join("main.exe"))),
    )
    .unwrap_or_else(|error| panic!("{}", error.render(settings.language, Some(&source))));
    Command::new(binary).output().unwrap()
}

/// 断言程序成功退出且标准输出与预期逐字节一致。
fn assert_stdout(output: &Output, expected: &str) {
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

/// 断言源码在语义分析阶段被拒绝，且诊断包含指定文本。
fn assert_semantic_error(body: &str, expected: &str) {
    let source = SourceFile::new("invalid.ms", body.to_string());
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let error = analyze_source(&source, &settings).expect_err("应报告语义错误");
    let rendered = error.render(settings.language, Some(&source));
    assert!(
        rendered.contains(expected),
        "expected={expected:?}\nrendered={rendered:?}"
    );
}

/// 切分保留头尾和连续分隔符产生的空项，覆盖空输入、无分隔符、多字节分隔符与 emoji 内容。
#[test]
fn split_keeps_head_tail_and_empty_items() {
    let output = run_program(
        "string-split",
        r#"func main():
    var parts = "a,b,,c,".split(",")
    print(len(parts))
    for part in parts:
        print("[" + part + "]")
    var whole = "整段".split(";")
    print(len(whole))
    print(whole[0])
    var empty = "".split(",")
    print(len(empty))
    print(len(empty[0]))
    var emoji = "你😀好😀".split("😀")
    print(len(emoji))
    print(emoji[0])
    print(emoji[1])
    print(len(emoji[1]))
    print(emoji[2])
    var multi = "a::b::".split("::")
    print(len(multi))
    print(multi[2])
"#,
    );
    assert_stdout(
        &output,
        "5\n[a]\n[b]\n[]\n[c]\n[]\n1\n整段\n1\n0\n3\n你\n好\n1\n\n3\n\n",
    );
}

/// 空分隔符是带源码位置的参数错误：报告中文诊断并以状态码 1 退出，不产生标准输出。
#[test]
fn split_reports_empty_separator_with_source_position() {
    let output = run_program(
        "string-split-empty",
        "func main():\n    var parts = \"abc\".split(\"\")\n    print(len(parts))\n",
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("运行时错误: 字符串方法 `split` 的分隔符不能为空"),
        "{stderr}"
    );
    assert!(stderr.contains("--> "), "{stderr}");
    assert!(stderr.contains(":2:"), "{stderr}");
    assert!(stderr.contains('^'), "{stderr}");
}

/// index_of 返回 Unicode 标量位置，支持重叠搜索、空 needle、找不到和起点越界。
#[test]
fn index_of_uses_unicode_scalars() {
    let output = run_program(
        "string-index-of",
        r#"func main():
    var text = "你好aaa"
    var first = text.index_of("小")
    print(first == null)
    var han = text.index_of("好")
    if han != null:
        print(han.value)
    else:
        print(-1)
    print(text.index_of("a").value)
    print(text.index_of("aa", 1).value)
    print(text.index_of("aa", 3).value)
    print(text.index_of("a", 3).value)
    print("aaa".index_of("aa", 1).value)
    var missing = text.index_of("😀")
    print(missing == null)
    var out_of_range = text.index_of("a", 99)
    print(out_of_range == null)
    var empty_needle = text.index_of("", 2)
    if empty_needle != null:
        print(empty_needle.value)
    else:
        print(-1)
    print(text.index_of("").value)
    var end_start = "ab".index_of("", 2)
    print(end_start.value)
    var past_end = "ab".index_of("", 3)
    print(past_end == null)
"#,
    );
    assert_stdout(
        &output,
        "true\n1\n2\n2\n3\n3\n1\ntrue\ntrue\n2\n0\n2\ntrue\n",
    );
}

/// substring 使用半开区间与标量下标，负数、倒序、越界返回 null，组合字符和 emoji 可被截取。
#[test]
fn substring_follows_scalar_half_open_ranges() {
    let body = format!(
        "func main():\n    var text = \"你e\u{301}😀\"\n    print(len(text))\n    var piece = text.substring(1, 3)\n    if piece != null:\n        print(piece.value)\n        print(len(piece.value))\n    else:\n        print(\"null\")\n    var whole = text.substring(0, 4)\n    print(len(whole.value))\n    var empty = text.substring(2, 2)\n    print(empty == null)\n    print(len(empty.value))\n    var negative = text.substring(-1, 2)\n    print(negative == null)\n    var reversed = text.substring(3, 1)\n    print(reversed == null)\n    var too_far = text.substring(0, 5)\n    print(too_far == null)\n    var emoji = text.substring(3, 4)\n    if emoji != null:\n        print(emoji.value)\n        print(len(emoji.value))\n"
    );
    let output = run_program("string-substring", &body);
    assert_stdout(
        &output,
        "4\ne\u{301}\n2\n4\nfalse\n0\ntrue\ntrue\ntrue\n😀\n1\n",
    );
}

/// to_lower/to_upper 使用 Unicode 默认规则，允许长度变化，中文与 ASCII 都保持正确。
#[test]
fn case_conversion_allows_length_changes() {
    let output = run_program(
        "string-case",
        r#"func main():
    print("Hello".to_lower())
    print("Hello".to_upper())
    print("中文AbC".to_upper())
    print("中文AbC".to_lower())
    var sharp = "ß".to_upper()
    print(sharp)
    print(len(sharp))
    var dotted = "İ".to_lower()
    print(len(dotted))
    print(dotted.to_upper())
"#,
    );
    assert_stdout(
        &output,
        "hello\nHELLO\n中文ABC\n中文abc\nSS\n2\n2\nI\u{307}\n",
    );
}

/// repeat 覆盖零次、一次、多次、负数与极大次数；极大次数返回 null 而不是申请巨量内存。
#[test]
fn repeat_covers_counts_and_allocation_limits() {
    let output = run_program(
        "string-repeat",
        r#"func main():
    var text = "ab"
    var zero = text.repeat(0)
    print(zero == null)
    print(len(zero.value))
    var once = text.repeat(1)
    print(once.value)
    var three = text.repeat(3)
    print(three.value)
    print(len(three.value))
    var negative = text.repeat(-1)
    print(negative == null)
    var huge = text.repeat(9223372036854775807)
    print(huge == null)
    var chinese = "你".repeat(2)
    print(chinese.value)
    print(len(chinese.value))
"#,
    );
    assert_stdout(&output, "false\n0\nab\nababab\n6\ntrue\ntrue\n你你\n2\n");
}

/// 既有字符串方法保持原有行为，新增方法不改变 trim/contains/starts_with/ends_with/replace。
#[test]
fn existing_string_methods_keep_their_behavior() {
    let output = run_program(
        "string-existing",
        r#"func main():
    var text = "  你好，世界  "
    var cleaned = text.trim()
    print(cleaned)
    print(cleaned.contains("好"))
    print(cleaned.starts_with("你好"))
    print(cleaned.ends_with("世界"))
    print(cleaned.replace("世界", "Mas lang"))
    print("中文".replace("", "X"))
"#,
    );
    assert_stdout(
        &output,
        "你好，世界\ntrue\ntrue\ntrue\n你好，Mas lang\n中文\n",
    );
}

/// 新方法在语义阶段校验参数个数、参数类型与接收者类型，并保证可空结果不会被当成非空值。
#[test]
fn new_string_methods_validate_arguments() {
    assert_semantic_error(
        "func main():\n    var parts = \"a\".split()\n",
        "字符串方法 `split` 需要且只接受一个分隔符参数",
    );
    assert_semantic_error(
        "func main():\n    var parts = \"a\".split(1)\n",
        "字符串方法 `split` 的分隔符参数必须是 string",
    );
    assert_semantic_error(
        "func main():\n    var index = \"a\".index_of(\"a\", \"b\")\n",
        "字符串方法 `index_of` 的起始下标必须是整数",
    );
    assert_semantic_error(
        "func main():\n    var piece = \"a\".substring(1)\n",
        "字符串方法 `substring` 需要起始和结束两个整数下标",
    );
    assert_semantic_error(
        "func main():\n    var upper = \"a\".to_upper(1)\n",
        "字符串方法 `to_upper` 不接受参数",
    );
    assert_semantic_error(
        "func main():\n    var repeated = \"a\".repeat(\"x\")\n",
        "字符串方法 `repeat` 的次数参数必须是整数",
    );
    assert_semantic_error(
        "func main():\n    var parts = [1, 2].split(\",\")\n",
        "列表类型不支持方法 `split`",
    );
    assert_semantic_error(
        "func main():\n    var index: int = \"a\".index_of(\"a\")\n",
        "变量 `index` 的类型与初始化表达式不匹配",
    );
    assert_semantic_error(
        "func main():\n    print(\"a\".repeat(2))\n",
        "只支持打印基础类型",
    );
}

/// 原生模块返回非法 UTF-8 字节；静态只读数据不归 GC 管理，字符串返回只借用指针与长度。
const INVALID_UTF8_HOST: &str = r#"
#include "mas_runtime.h"

/* 返回一段非法 UTF-8 字节，供新字符串方法验证带源码位置的运行时报错。 */
const uint8_t *bad_text(unsigned long long *out_len) {
    static const uint8_t bytes[] = {0xff, 0xfe, 0x41};
    *out_len = sizeof(bytes);
    return bytes;
}
"#;

/// 创建含真实 C 源的临时工程并编译运行，路径与 `safe_conversion_bytes` 的既有夹具保持一致。
fn run_native_program(name: &str, body: &str) -> Output {
    let root = TemporaryDirectory::create(name).unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("settings.toml"),
        "[project]\nname = \"string-native\"\nversion = \"0.1.0\"\n[native]\nabi = 1\ninclude = [\".\"]\n[native.sources]\ncommon = [\"host.c\"]\n",
    )
    .unwrap();
    std::fs::write(root.path().join("host.c"), INVALID_UTF8_HOST).unwrap();
    std::fs::write(
        root.path().join("mas_runtime.h"),
        include_str!("../native/include/mas_runtime.h"),
    )
    .unwrap();
    let path = root.path().join("src/main.ms");
    std::fs::write(&path, body).unwrap();
    let source = SourceFile::new(path.display().to_string(), body.to_string());
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let options = CompileOptions::new(Some(root.path().join("main.exe")))
        .with_allow_native_source_build(true);
    let binary = compile_to_executable(&source, &settings, &options)
        .unwrap_or_else(|error| panic!("{}", error.render(settings.language, Some(&source))));
    Command::new(binary).output().unwrap()
}

/// 新字符串方法遇到原生输入的非法 UTF-8 时，统一报告带源码位置的运行时错误并以状态码 1 退出。
#[test]
fn new_string_methods_report_invalid_utf8_with_source_position() {
    let statements = [
        "var parts = bad_text().split(\",\")",
        "var position = bad_text().index_of(\"a\")",
        "var piece = bad_text().substring(0, 1)",
        "var lower = bad_text().to_lower()",
        "var repeated = bad_text().repeat(1)",
    ];
    for (index, statement) in statements.iter().enumerate() {
        let body = format!(
            "extern func bad_text() -> string\nfunc main():\n    {statement}\n    print(42)\n"
        );
        let output = run_native_program(&format!("string-native-utf8-{index}"), &body);
        assert_eq!(output.status.code(), Some(1), "{statement}: {output:?}");
        assert!(output.stdout.is_empty(), "{statement}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("字符串不是合法的 UTF-8 文本"),
            "{statement}: {stderr}"
        );
        assert!(stderr.contains("--> "), "{statement}: {stderr}");
        // 模块装载器为入口模块追加一行 `// module: ...` 头，因此源码第 3 行在合成源码中报第 4 行。
        assert!(stderr.contains("main.ms:4:"), "{statement}: {stderr}");
    }
}
