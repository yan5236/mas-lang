// time 0.2.0（原生 ABI 2）的端到端验收：走真实 `fama install` / `fama run` 链路，
// 使用包内 Windows 预编译产物，不开启 `--allow-native-source-build`。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use maslang::temp_directory::TemporaryDirectory;

/// 由 Cargo 注入的 `fama` 可执行文件路径，避免在测试里再次调用 `cargo run` 造成锁等待。
const FAMA_BIN: &str = env!("CARGO_BIN_EXE_fama");

/// 返回仓库根目录，供测试定位标准库包与示例工程。
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("fama 应位于 crates/fama 目录下")
        .to_path_buf()
}

/// 使用隔离的全局配置目录执行一次 `fama` 子命令。
///
/// `~/.fama/setting.toml` 可能配置了外部编译器路径，测试必须避免读到用户环境，
/// 否则结果会随机器变化；这里把 `FAMA_HOME` 指向空目录以使用内置编译器。
fn run_fama(project_dir: &Path, arguments: &[&str]) -> Output {
    let home = TemporaryDirectory::create("fama-time-home").expect("创建临时全局配置目录失败");
    Command::new(FAMA_BIN)
        .args(arguments)
        .current_dir(project_dir)
        .env("FAMA_HOME", home.path())
        .output()
        .expect("执行 fama 子命令失败")
}

/// 把子进程输出整理成便于断言的文本，断言失败时能直接看到前后文。
fn render_output(output: &Output) -> String {
    format!(
        "状态码: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// 统一换行后再断言，避免 Windows 的 `\r\n` 影响按行匹配。
fn normalized_stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

/// 断言输出包含全部标记，任一缺失都会打印完整输出便于定位。
fn assert_markers(text: &str, markers: &[&str]) {
    for marker in markers {
        assert!(text.contains(marker), "输出缺少标记 `{marker}`：\n{text}");
    }
}

/// 示例工程必须同时覆盖严格接口与 `try_*`：负等待走错误分支后仍能继续执行到末尾。
#[test]
fn time_example_reports_recoverable_failures_and_keeps_strict_api() {
    let project = repository_root().join("examples/projects/time-api");

    let install = run_fama(&project, &["install", "."]);
    assert!(
        install.status.success(),
        "fama install 失败：{}",
        render_output(&install)
    );

    let run = run_fama(&project, &["run", "."]);
    assert!(run.status.success(), "fama run 失败：{}", render_output(&run));

    let stdout = normalized_stdout(&run);
    assert_markers(
        &stdout,
        &[
            "sleep(-1) -> false",
            "sleep(-1) code=5",
            "after sleep(-1)",
            "sleep(0) -> true",
            "sleep(0) code=0",
            "sleep(20) -> true",
            "try_unix_seconds ok=true",
            "try_unix_seconds positive=true",
            "try_unix_millis ok=true",
            "try_monotonic_millis ok=true",
            "try_monotonic_millis non_negative=true",
            "try_now_utc ok=true",
            "try_now_utc utc_offset=true",
            "try_now_email_date ok=true",
            "try_now_email_date utc_offset=true",
            "try_now_http_date ok=true",
            "try_now_http_date gmt=true",
        ],
    );

    assert!(
        stdout.trim_end().ends_with("done"),
        "负等待之后必须继续执行到末尾标记：\n{stdout}"
    );
    assert!(
        !stdout.contains("sleep(-1) unreachable"),
        "负等待不得进入成功分支：\n{stdout}"
    );
}

/// 严格 `sleep(-1)` 必须保持旧语义：输出中文运行时错误并以状态码 1 结束进程。
#[test]
fn strict_sleep_keeps_chinese_error_and_exit_code() {
    let root = TemporaryDirectory::create("fama-time-strict").expect("创建临时工程失败");
    fs::create_dir_all(root.path().join("src")).expect("创建 src 目录失败");
    let dependency = repository_root().join("stdlib/time");
    fs::write(
        root.path().join("settings.toml"),
        format!(
            "[project]\nname = \"time-strict\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ms\"\noutput = \"build\"\nopt-level = 2\n\n[dependencies]\n\"time\" = {{ path = \"{}\", version = \"0.2.0\" }}\n",
            dependency.display().to_string().replace('\\', "/")
        ),
    )
    .expect("写入工程配置失败");
    fs::write(
        root.path().join("src/main.ms"),
        "import time { sleep }\nfunc main():\n    print(\"before\")\n    sleep(-1)\n    print(\"after\")\n",
    )
    .expect("写入入口源码失败");

    let run = run_fama(root.path(), &["run", "."]);
    let stdout = normalized_stdout(&run);
    let stderr = String::from_utf8_lossy(&run.stderr).replace("\r\n", "\n");

    assert!(
        !run.status.success(),
        "严格 sleep(-1) 必须以非零状态退出：{}",
        render_output(&run)
    );
    assert_eq!(
        run.status.code(),
        Some(1),
        "退出码必须为 1：{}",
        render_output(&run)
    );
    assert!(
        stderr.contains("运行时错误: sleep(ms) 只接受大于等于 0 的毫秒整数"),
        "必须保留原有中文运行时错误：{}",
        render_output(&run)
    );
    assert!(
        stdout.contains("before"),
        "出错前的输出必须已经写出来：{}",
        render_output(&run)
    );
    assert!(
        !stdout.contains("after"),
        "严格接口出错后不得继续执行：{}",
        render_output(&run)
    );
}
