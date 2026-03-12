use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// 返回集成测试要调用的 fama 可执行文件路径。
fn fama_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fama"))
}

/// 为每个测试创建唯一的临时工作目录，避免不同测试相互干扰。
fn create_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间异常")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("fama-tests-{prefix}-{unique}"));
    fs::create_dir_all(&path).expect("创建临时目录失败");
    path
}

/// 读取 UTF-8 文本文件内容，供断言模板内容使用。
fn read_to_string(path: &Path) -> String {
    fs::read_to_string(path).expect("读取生成文件失败")
}

/// 校验 `fama new` 能生成完整的项目骨架和预期模板。
#[test]
fn should_create_project_scaffold() {
    let workdir = create_temp_dir("create");
    let output = Command::new(fama_bin())
        .arg("new")
        .arg("demo")
        .current_dir(&workdir)
        .output()
        .expect("执行 fama new 失败");

    assert!(output.status.success(), "{output:?}");

    let project_root = workdir.join("demo");
    assert!(project_root.is_dir());
    assert!(project_root.join("src").is_dir());
    assert!(project_root.join("mod_module").is_dir());
    assert_eq!(
        read_to_string(&project_root.join("src").join("main.ms")),
        "print(\"Hello world.\")\n"
    );
    assert!(read_to_string(&project_root.join("mod.yml")).contains("name: demo"));
    assert!(read_to_string(&project_root.join("settings.toml")).contains("name = \"demo\""));

    fs::remove_dir_all(workdir).expect("删除临时目录失败");
}

/// 校验当目标目录已存在时命令会失败且不覆盖原目录。
#[test]
fn should_fail_when_project_directory_exists() {
    let workdir = create_temp_dir("exists");
    let existing_project = workdir.join("demo");
    fs::create_dir_all(&existing_project).expect("预创建项目目录失败");

    let output = Command::new(fama_bin())
        .arg("new")
        .arg("demo")
        .current_dir(&workdir)
        .output()
        .expect("执行 fama new 失败");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("目标目录已存在"));

    fs::remove_dir_all(workdir).expect("删除临时目录失败");
}

/// 校验缺少项目名时 clap 会返回帮助和错误状态码。
#[test]
fn should_fail_when_project_name_is_missing() {
    let workdir = create_temp_dir("missing");
    let output = Command::new(fama_bin())
        .arg("new")
        .current_dir(&workdir)
        .output()
        .expect("执行 fama new 失败");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage:"));

    fs::remove_dir_all(workdir).expect("删除临时目录失败");
}

/// 校验 `fama build` 能读取项目配置并在约定目录下生成可执行文件。
#[test]
fn should_build_project_binary_into_configured_output_directory() {
    let workdir = create_temp_dir("build");
    let new_output = Command::new(fama_bin())
        .arg("new")
        .arg("demo")
        .current_dir(&workdir)
        .output()
        .expect("执行 fama new 失败");

    assert!(new_output.status.success(), "{new_output:?}");

    let build_output = Command::new(fama_bin())
        .arg("build")
        .current_dir(workdir.join("demo"))
        .output()
        .expect("执行 fama build 失败");

    assert!(build_output.status.success(), "{build_output:?}");
    assert!(
        workdir
            .join("demo")
            .join("build")
            .join("demo.exe")
            .is_file()
    );

    fs::remove_dir_all(workdir).expect("删除临时目录失败");
}

/// 校验 `fama run` 能够完整跑通编译和执行流程，并输出程序结果。
#[test]
fn should_run_project_binary_after_building() {
    let workdir = create_temp_dir("run");
    let new_output = Command::new(fama_bin())
        .arg("new")
        .arg("demo")
        .current_dir(&workdir)
        .output()
        .expect("执行 fama new 失败");

    assert!(new_output.status.success(), "{new_output:?}");

    fs::write(
        workdir.join("demo").join("src").join("main.ms"),
        "var name:string = \"Mascript\"\nprint(\"Hello \" + name)\n",
    )
    .expect("写入测试源码失败");

    let run_output = Command::new(fama_bin())
        .arg("run")
        .current_dir(workdir.join("demo"))
        .output()
        .expect("执行 fama run 失败");

    assert!(run_output.status.success(), "{run_output:?}");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.contains("Hello Mascript"));

    fs::remove_dir_all(workdir).expect("删除临时目录失败");
}
