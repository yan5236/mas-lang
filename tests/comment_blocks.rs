use mascript_compiler_llvm::settings::{CompilerSettings, UiLanguage};
use mascript_compiler_llvm::source::SourceFile;
use mascript_compiler_llvm::temp_directory::TemporaryDirectory;
use mascript_compiler_llvm::{CompileOptions, analyze_source, compile_to_executable};
use std::process::Command;

/// 函数头与首条语句之间的空行、不同缩进注释和 CRLF 都不得阻止函数体识别。
#[test]
fn function_body_starts_after_comments_and_blank_lines() {
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    for prefix in [
        "// 顶格注释\n",
        "    // 注释\n",
        "\n        // 注释不决定缩进\n\n",
        "\r\n// 注释\r\n    // 连续注释\r\n",
    ] {
        let source = SourceFile::new(
            "comments.ms",
            format!("func main():\n{prefix}    print(7)\n"),
        );
        analyze_source(&source, &settings)
            .unwrap_or_else(|error| panic!("{}", error.render(settings.language, Some(&source))));
    }
}

/// 注释不能充当函数体，错误应落在缺少缩进的真实代码或 EOF，而不是注释行。
#[test]
fn comments_do_not_make_an_empty_body_valid() {
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    for text in [
        "func main():\n// 注释\n\nprint(1)\n",
        "func main():\n    // 注释\n",
    ] {
        let source = SourceFile::new("missing.ms", text.to_string());
        let error = analyze_source(&source, &settings).unwrap_err();
        assert!(
            error
                .render(settings.language, Some(&source))
                .contains("缺少函数体")
        );
        assert_eq!(error.span.start, text.find("print").unwrap_or(text.len()));
    }
}

/// 使用真实 EXE 验证截图场景和条件、循环、else 块首注释，共用解析规则且不改变运算结果。
#[test]
fn commented_suites_compile_and_execute() {
    let root = TemporaryDirectory::create("mascript-comment-test").unwrap();
    let text = "func fib(n:int) -> int:\n    if n < 2:\n// 条件体前注释\n        return n\n    return fib(n - 1) + fib(n - 2)\nfunc main():\n    // 注释行\n    var a:int = 7\n    var b:int = 2\n    print(\"int_div=\" + string(a / b))\n    var total = 0\n    for i in 0..3:\n// 循环体前注释\n\n        total += i\n    while total < 4:\n        // 注释\n        total += 1\n    if total == 4:\n        // 注释\n        print(fib(7))\n    elif total == 5:\n// 注释\n        print(0)\n    else:\n// 注释\n        print(-1)\n";
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
    .unwrap_or_else(|error| panic!("{}", error.render(settings.language, Some(&source))));
    let output = Command::new(binary).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "int_div=3.5\n13\n"
    );
}
