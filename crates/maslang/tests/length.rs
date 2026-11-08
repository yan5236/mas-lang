use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, analyze_source, compile_to_executable};
use std::process::Command;

/// 真正编译并运行全部 len 类型，确认 Unicode 计数、空串、动态字符串和数组实参副作用。
#[test]
fn length_matches_documented_types() {
    let root = TemporaryDirectory::create("mascript-length-test").unwrap();
    let text = "print(len(\"hello\"))\nprint(len(\"你好\"))\nprint(len(\"😀\"))\nprint(len(\"é\"))\nprint(len(\"\"))\nvar s = \"你\" + \"好\"\nprint(len(s))\nprint(len([1,2,3]))\nprint(len({1:2}))\nvar a = array<int,3>(1,2,3)\nprint(len(a))\nfunc make() -> array<int,2>:\n    print(42)\n    return array<int,2>(1,2)\nprint(len(make()))\n";
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
    let invalid = SourceFile::new("invalid.ms", "print(len(1))\n".to_string());
    assert!(
        analyze_source(&invalid, &settings)
            .unwrap_err()
            .render(settings.language, Some(&invalid))
            .contains("len")
    );
}
