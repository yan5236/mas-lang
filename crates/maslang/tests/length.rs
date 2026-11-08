use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, analyze_source, compile_to_executable};
use std::process::Command;

/// 编译运行包含长 Unicode 字面量的循环，校验经变量取长及改写为托管字符串后都返回实际标量数。
#[test]
fn long_string_lengths_follow_the_current_value() {
    let root = TemporaryDirectory::create("mascript-long-length-test").unwrap();
    let literal = "你e\u{301}😀".repeat(300);
    let source = SourceFile::new(
        root.path().join("main.ms").display().to_string(),
        format!(
            "func main():\n    var text = \"{literal}\"\n    var total = 0\n    for i in 0..3:\n        total += len(text)\n    print(total)\n    text = text + \"好\"\n    print(len(text))\n"
        ),
    );
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.path().join("main.exe"))),
    )
    .unwrap();
    let output = Command::new(binary).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"3600\n1201\n");
}

/// 真正编译并运行全部 len 类型，确认 Unicode 计数、空串、动态字符串和数组实参副作用。
#[test]
fn length_matches_documented_types() {
    let root = TemporaryDirectory::create("mascript-length-test").unwrap();
    let text = "func make() -> array<int,2>:\n    print(42)\n    return array<int,2>(1,2)\nfunc main():\n    print(len(\"hello\"))\n    print(len(\"你好\"))\n    print(len(\"😀\"))\n    print(len(\"é\"))\n    print(len(\"\"))\n    var s = \"你\" + \"好\"\n    print(len(s))\n    print(len([1,2,3]))\n    print(len({1:2}))\n    var a = array<int,3>(1,2,3)\n    print(len(a))\n    print(len(make()))\n";
    let source = SourceFile::new(
        root.path().join("main.ms").display().to_string(),
        text.to_string(),
    );
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.path().join("main.exe"))),
    )
    .unwrap();
    let output = Command::new(binary).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "5\n2\n1\n2\n0\n2\n3\n1\n3\n42\n2\n"
    );
    let invalid = SourceFile::new("invalid.ms", "func main():\n    print(len(1))\n".to_string());
    assert!(
        analyze_source(&invalid, &settings)
            .unwrap_err()
            .render(settings.language, Some(&invalid))
            .contains("len")
    );
}
