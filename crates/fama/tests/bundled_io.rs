// 全新 Fama 工程消费随附 io：没有 `[dependencies]`、没有依赖缓存、不先跑
// `fama install`，直接 `fama build` / `fama run` 就应成功，并且不得生成
// `mod_module/io@*` 目录或 `mod-lock.yml` 里的 io 项。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use maslang::temp_directory::TemporaryDirectory;

/// 由 Cargo 注入的 `fama` 可执行文件路径，避免在测试里再次调用 `cargo run` 造成锁等待。
const FAMA_BIN: &str = env!("CARGO_BIN_EXE_fama");

/// 随附 io 只在预编译产物矩阵内承诺默认链接；矩阵外不运行本组链接用例。
fn target_supported() -> bool {
    cfg!(all(
        target_os = "windows",
        target_env = "msvc",
        target_arch = "x86_64"
    )) || (cfg!(target_os = "linux")
        && (cfg!(target_arch = "x86_64") || cfg!(target_arch = "aarch64")))
        || (cfg!(target_os = "macos")
            && (cfg!(target_arch = "x86_64") || cfg!(target_arch = "aarch64")))
}

/// 使用隔离的全局配置目录执行一次 `fama` 子命令。
///
/// `~/.fama/setting.toml` 可能配置了外部编译器路径，测试必须避免读到用户环境，
/// 否则结果会随机器变化；这里把 `FAMA_HOME` 指向空目录以使用内置编译器。
fn run_fama(home: &Path, project_dir: &Path, arguments: &[&str]) -> Output {
    Command::new(FAMA_BIN)
        .args(arguments)
        .current_dir(project_dir)
        .env("FAMA_HOME", home)
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

/// 按当前平台的构建产物路径拼接可执行文件名，便于确认 build 真产出二进制。
fn project_binary(project: &Path, name: &str) -> PathBuf {
    project.join(format!("build/{name}{}", std::env::consts::EXE_SUFFIX))
}

/// 断言锁文件没有 io 依赖项或 io 缓存引用；避免 `version` 这类普通子串造成误判。
fn assert_lock_has_no_io_item(text: &str) {
    for line in text.lines() {
        let trimmed = line.trim();
        assert!(
            !trimmed.starts_with("io:")
                && !trimmed.starts_with("\"io\":")
                && !trimmed.starts_with("'io':"),
            "mod-lock.yml 不应记录 io 依赖项：\n{text}"
        );
    }
    assert!(
        !text.contains("io@") && !text.contains("mod_module/io"),
        "mod-lock.yml 不应引用 io 安装缓存：\n{text}"
    );
}

/// 无声明、无安装、无缓存的全新工程必须直接构建运行随附 io 并输出正确内容。
#[test]
fn fresh_project_without_declared_io_builds_and_runs() {
    if !target_supported() {
        eprintln!(
            "跳过 fresh_project_without_declared_io_builds_and_runs：当前目标不在随附 io 预编译矩阵内"
        );
        return;
    }
    let root = TemporaryDirectory::create("fama-bundled-io").expect("创建临时工程失败");
    let home = TemporaryDirectory::create("fama-bundled-home").expect("创建隔离配置目录失败");
    let project = root.path();
    fs::create_dir_all(project.join("src")).expect("创建 src 目录失败");
    fs::write(
        project.join("settings.toml"),
        "[project]\nname = \"bundled-io-fama\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ms\"\noutput = \"build\"\nopt-level = 2\n",
    )
    .expect("写入工程清单失败");
    fs::write(
        project.join("src/main.ms"),
        r#"import io { stdout, write_text_to }

func main():
    var result = write_text_to(stdout(), "bundled-io-fama-ok")
    print("")
    print("ok=" + string(result.ok))
"#,
    )
    .expect("写入入口源码失败");

    // 关键：不跑 `fama install`，也不打开 `--allow-native-source-build`。
    let build = run_fama(home.path(), project, &["build", "."]);
    assert!(
        build.status.success(),
        "全新工程直接 fama build 失败：{}",
        render_output(&build)
    );
    assert!(
        project_binary(project, "bundled-io-fama").is_file(),
        "fama build 必须生成可执行文件"
    );

    let run = run_fama(home.path(), project, &["run", "."]);
    assert!(
        run.status.success(),
        "全新工程直接 fama run 失败：{}",
        render_output(&run)
    );
    let stdout = normalized_stdout(&run);
    assert!(
        stdout.contains("bundled-io-fama-ok"),
        "运行输出缺少随附 io 写入的标记：\n{stdout}"
    );
    assert!(stdout.contains("ok=true"), "随附 io 写入应成功：\n{stdout}");

    // 随附 io 不进入安装器：不得出现任何 io@版本 缓存目录。
    let mod_module = project.join("mod_module");
    if mod_module.is_dir() {
        let entries: Vec<String> = fs::read_dir(&mod_module)
            .expect("读取 mod_module 失败")
            .map(|entry| {
                entry
                    .expect("读取 mod_module 项失败")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(
            !entries.iter().any(|name| name.starts_with("io@")),
            "不得生成随附 io 的安装目录：{entries:?}"
        );
    }

    // 若生成了锁文件，其中不得出现 io 依赖项。
    let lock = project.join("mod-lock.yml");
    if lock.is_file() {
        let text = fs::read_to_string(&lock).expect("读取 mod-lock.yml 失败");
        assert_lock_has_no_io_item(&text);
    }
}
