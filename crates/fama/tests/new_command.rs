use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// 创建指向当前测试构建产物的 Fama 命令，并隔离全局设置，避免读取本机配置的外部编译器。
fn fama_command(workdir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fama"));
    command.env("FAMA_HOME", workdir.join("fama-home"));
    command
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

/// 为本地依赖测试创建一个最小库项目，覆盖 `pub` 与 `export import` 聚合入口。
fn create_local_library(root: &Path, name: &str) {
    fs::create_dir_all(root.join("src")).expect("创建库 src 目录失败");
    fs::write(
        root.join("mod.yml"),
        format!("name: {name}\nversion: 0.1.0\n\ndependencies: {{}}\n"),
    )
    .expect("写入库 mod.yml 失败");
    fs::write(root.join("mod-lock.yml"), "dependencies: {}\n").expect("写入库 mod-lock.yml 失败");
    fs::write(
        root.join("settings.toml"),
        format!(
            "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ms\"\noutput = \"build\"\nopt-level = 2\n"
        ),
    )
    .expect("写入库 settings.toml 失败");
    fs::write(
        root.join("src").join("math.ms"),
        "pub func add(a:int, b:int) -> int:\n    return a + b\n",
    )
    .expect("写入库 math.ms 失败");
    fs::write(
        root.join("src").join("index.ms"),
        "export import math { add }\n",
    )
    .expect("写入库 index.ms 失败");
    fs::write(root.join("src").join("main.ms"), "print(\"library\")\n")
        .expect("写入库 main.ms 失败");
}

/// 校验 `fama new` 能生成完整的项目骨架和预期模板。
#[test]
fn should_create_project_scaffold() {
    let workdir = create_temp_dir("create");
    let output = fama_command(&workdir)
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

/// 校验首次执行任意命令时会自动创建全局配置目录和默认配置文件。
#[test]
fn should_create_global_settings_home_and_file_on_first_command_run() {
    let workdir = create_temp_dir("global-settings");
    let fama_home = workdir.join("global-home");

    let output = fama_command(&workdir)
        .arg("new")
        .arg("demo")
        .current_dir(&workdir)
        .env("FAMA_HOME", &fama_home)
        .output()
        .expect("执行 fama new 失败");

    assert!(output.status.success(), "{output:?}");
    assert!(fama_home.is_dir());
    assert!(fama_home.join("setting.toml").is_file());

    fs::remove_dir_all(workdir).expect("删除临时目录失败");
}

/// 校验当目标目录已存在时命令会失败且不覆盖原目录。
#[test]
fn should_fail_when_project_directory_exists() {
    let workdir = create_temp_dir("exists");
    let existing_project = workdir.join("demo");
    fs::create_dir_all(&existing_project).expect("预创建项目目录失败");

    let output = fama_command(&workdir)
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
    let output = fama_command(&workdir)
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
    let new_output = fama_command(&workdir)
        .arg("new")
        .arg("demo")
        .current_dir(&workdir)
        .output()
        .expect("执行 fama new 失败");

    assert!(new_output.status.success(), "{new_output:?}");

    let build_output = fama_command(&workdir)
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

/// 校验 `fama run` 使用临时产物执行程序，不把运行产物写入项目 build 目录。
#[test]
fn should_run_project_binary_without_writing_configured_build_output() {
    let workdir = create_temp_dir("run");
    let new_output = fama_command(&workdir)
        .arg("new")
        .arg("demo")
        .current_dir(&workdir)
        .output()
        .expect("执行 fama new 失败");

    assert!(new_output.status.success(), "{new_output:?}");

    fs::write(
        workdir.join("demo").join("src").join("main.ms"),
        "var name:string = \"Mas lang\"\nprint(\"Hello \" + name)\n",
    )
    .expect("写入测试源码失败");

    let run_output = fama_command(&workdir)
        .arg("run")
        .current_dir(workdir.join("demo"))
        .output()
        .expect("执行 fama run 失败");

    assert!(run_output.status.success(), "{run_output:?}");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.contains("Hello Mas lang"));
    assert!(!workdir.join("demo").join("build").join("demo.exe").exists());

    fs::remove_dir_all(workdir).expect("删除临时目录失败");
}

/// 校验 `fama add/install/run` 能跑通本地依赖安装和模块导入主链路。
#[test]
fn should_install_local_dependency_and_run_imported_module() {
    let workdir = create_temp_dir("local-dependency");
    let app_output = fama_command(&workdir)
        .arg("new")
        .arg("app")
        .current_dir(&workdir)
        .output()
        .expect("执行 fama new app 失败");
    assert!(app_output.status.success(), "{app_output:?}");

    let lib_root = workdir.join("math");
    create_local_library(&lib_root, "math");

    let add_output = fama_command(&workdir)
        .arg("add")
        .arg("../math")
        .current_dir(workdir.join("app"))
        .output()
        .expect("执行 fama add 失败");
    assert!(add_output.status.success(), "{add_output:?}");

    let install_output = fama_command(&workdir)
        .arg("install")
        .current_dir(workdir.join("app"))
        .output()
        .expect("执行 fama install 失败");
    assert!(install_output.status.success(), "{install_output:?}");

    fs::write(
        workdir.join("app").join("src").join("main.ms"),
        "import math { add }\nfunc main():\n    print(add(1, 2))\n",
    )
    .expect("写入应用 main.ms 失败");

    let run_output = fama_command(&workdir)
        .arg("run")
        .current_dir(workdir.join("app"))
        .output()
        .expect("执行 fama run 失败");

    assert!(run_output.status.success(), "{run_output:?}");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.contains("3"));
    assert!(!stdout.contains("已生成可执行文件"));
    assert!(!stdout.contains("已命中预编译原生产物"));
    assert!(read_to_string(&workdir.join("app").join("mod.yml")).contains("path: ../math"));
    assert!(read_to_string(&workdir.join("app").join("mod-lock.yml")).contains("math@0.1.0"));
    assert!(
        workdir
            .join("app")
            .join("mod_module")
            .join("math@0.1.0")
            .is_dir()
    );

    fs::remove_dir_all(workdir).expect("删除临时目录失败");
}
