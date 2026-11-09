use maslang::settings::CompilerSettings;
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, compile_to_executable};
use std::process::{Command, Output};

/// 原生模块返回任意字节：非法 UTF-8 与合法文本各一个入口，用来验证安全转换与严格转换的分工。
const HOST: &str = r#"
#include "mas_runtime.h"

/* 返回一段非法 UTF-8 字节；静态只读数据不归 GC 管理，字符串返回约定只借用指针与长度。 */
const uint8_t *bad_text(unsigned long long *out_len) {
    static const uint8_t bytes[] = {0xff, 0xfe, 0x41};
    *out_len = sizeof(bytes);
    return bytes;
}

/* 返回合法十进制文本，验证安全转换的成功路径未被破坏。 */
const uint8_t *good_text(unsigned long long *out_len) {
    static const uint8_t bytes[] = "42";
    *out_len = 2;
    return bytes;
}
"#;

/// 创建包含真实 C 源的临时包并走现有维护者构建链路；产物运行后自动清理工程。
fn execute(body: &str) -> Output {
    let root = TemporaryDirectory::create("safe-conversion").unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("settings.toml"),
        "[project]\nname = \"safe-conversion\"\nversion = \"0.1.0\"\n[native]\nabi = 1\ninclude = [\".\"]\n[native.sources]\ncommon = [\"host.c\"]\n",
    )
    .unwrap();
    std::fs::write(root.path().join("host.c"), HOST).unwrap();
    std::fs::write(
        root.path().join("mas_runtime.h"),
        include_str!("../native/include/mas_runtime.h"),
    )
    .unwrap();
    let path = root.path().join("src/main.ms");
    std::fs::write(&path, body).unwrap();
    let source = SourceFile::new(path.display().to_string(), body.to_string());
    let settings = CompilerSettings::default_settings();
    let options = CompileOptions::new(Some(root.path().join("main.exe")))
        .with_allow_native_source_build(true);
    let binary = compile_to_executable(&source, &settings, &options)
        .unwrap_or_else(|e| panic!("{}", e.render(settings.language, Some(&source))));
    Command::new(binary).output().unwrap()
}

/// 原生模块构造的非法 UTF-8 字节必须让 `try_int/try_float/try_bool` 返回失败并继续执行。
#[test]
fn safe_conversions_reject_invalid_utf8_without_exiting() {
    let output = execute(
        r#"extern func bad_text() -> string
extern func good_text() -> string
func main():
    var integer = try_int(bad_text())
    print(integer.ok)
    print(integer.value)
    var number = try_float(bad_text())
    print(number.ok)
    print(number.value)
    var flag = try_bool(bad_text())
    print(flag.ok)
    print(flag.value)
    var good = try_int(good_text())
    print(good.ok)
    print(good.value)
    print(42)
"#,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "false\n0\nfalse\n0\nfalse\nfalse\ntrue\n42\n42\n"
    );
}

/// 严格转换遇到非法 UTF-8 仍保持原有语义：报告中文运行时错误并以状态码 1 退出。
#[test]
fn strict_conversions_still_report_invalid_utf8() {
    for body in [
        "extern func bad_text() -> string\nfunc main():\n    print(int(bad_text()))\n",
        "extern func bad_text() -> string\nfunc main():\n    print(float(bad_text()))\n",
    ] {
        let output = execute(body);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(stderr.contains("合法的 UTF-8"), "{stderr}");
    }
}
