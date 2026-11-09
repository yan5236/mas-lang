use maslang::settings::CompilerSettings;
use maslang::source::SourceFile;
use maslang::temp_directory::TemporaryDirectory;
use maslang::{CompileOptions, compile_to_executable};
use std::process::{Command, Output, Stdio};

/// 编译源码并返回可执行文件与持有临时目录的守卫；守卫在测试结束时统一清理。
/// 目录通过 `TemporaryDirectory` 原子创建（PID + 计数器），避免并发用例共用目录导致链接中间文件互相覆盖。
fn build(text: &str) -> (TemporaryDirectory, std::path::PathBuf) {
    let workspace = TemporaryDirectory::create("mascript-output").unwrap();
    let root = workspace.path();
    let source = SourceFile::new(root.join("main.ms").display().to_string(), text.to_string());
    let settings = CompilerSettings::new(maslang::settings::UiLanguage::ZhCn);
    let binary = compile_to_executable(
        &source,
        &settings,
        &CompileOptions::new(Some(root.join("main.exe"))),
    )
    .unwrap_or_else(|e| panic!("{}", e.render(settings.language, Some(&source))));
    (workspace, binary)
}

/// 正常输出覆盖四种基础类型，确认打印入口返回值变化不影响既有调用方式。
#[test]
fn print_keeps_existing_output_for_scalar_types() {
    let (_workspace, binary) = build(
        "func main():\n    print(42)\n    print(1.5)\n    print(true)\n    print(\"你好\")\n    print(try_print(\"ok\"))\n",
    );
    let output: Output = Command::new(binary).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "42\n1.5\ntrue\n你好\nok\ntrue\n"
    );
}

/// 关闭标准输出读端后 `try_print` 必须返回 false 让调用方继续；未观察到失败才会落到除零错误。
#[test]
fn try_print_reports_closed_pipe_and_caller_continues() {
    let (_workspace, binary) = build(
        "func main():\n    var count = 0\n    while count < 20000:\n        if not try_print(\"0123456789\"):\n            return\n        count += 1\n    var zero = 0\n    print(1 %/% zero)\n",
    );
    let mut child = Command::new(binary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("整数除零"),
        "{output:?}"
    );
}

/// 严格 `print` 在管道关闭时报告中文语言错误并以状态码 1 退出，而不是 Rust panic。
#[test]
fn strict_print_reports_language_error_instead_of_panic() {
    let (_workspace, binary) = build(
        "func main():\n    var count = 0\n    while count < 100000:\n        print(\"0123456789\")\n        count += 1\n",
    );
    let mut child = Command::new(binary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr.contains("无法写入标准输出"), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
}
