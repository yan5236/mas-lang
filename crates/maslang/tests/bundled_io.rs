// 随附 io 回归：无项目清单的单文件、复制后的编译器二进制、重复导入只链接一次、
// 显式依赖优先、双套原生实现冲突诊断，以及 `local.io` 仍解析项目内源码。
//
// 用例只走编译器库 API 与 maslang CLI，不改生产代码；临时目录由
// `TemporaryDirectory` 在析构时清理，失败路径也不会留下垃圾。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use maslang::temp_directory::TemporaryDirectory;
use maslang::{
    CompileOptions, analyze_source,
    settings::{CompilerSettings, UiLanguage},
    source::SourceFile,
};

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

/// 在矩阵外打印清晰原因后跳过，而不是把「当前目标没有预编译对象」误报成产品缺陷。
fn skip_outside_matrix(test: &str) -> bool {
    if target_supported() {
        return false;
    }
    eprintln!("跳过 {test}：当前目标不在随附 io 预编译矩阵内");
    true
}

/// 确认临时目录及其上级都没有 settings.toml；否则无法构造“无清单单文件”场景。
fn no_manifest_above(root: &Path) -> bool {
    root.ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .all(|ancestor| !ancestor.join("settings.toml").is_file())
}

/// 固定使用中文设置，保证诊断渲染与断言语言一致。
fn zh_settings() -> CompilerSettings {
    CompilerSettings::new(UiLanguage::ZhCn)
}

/// 从磁盘读取入口源码；路径与文本都来自临时目录。
fn read_source(path: &Path) -> SourceFile {
    SourceFile::new(
        path.display().to_string(),
        fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display())),
    )
}

/// 编译入口并返回可执行文件；失败时渲染带源码定位的诊断。
fn build(source_path: &Path, output_path: &Path) -> PathBuf {
    match build_result(source_path, output_path) {
        Ok(binary) => binary,
        Err(rendered) => panic!("编译 {} 失败：{rendered}", source_path.display()),
    }
}

/// 编译入口并保留渲染后的错误文本，供冲突诊断断言。
fn build_result(source_path: &Path, output_path: &Path) -> Result<PathBuf, String> {
    let source = read_source(source_path);
    maslang::compile_to_executable(
        &source,
        &zh_settings(),
        &CompileOptions::new(Some(output_path.to_path_buf())),
    )
    .map_err(|error| error.render(UiLanguage::ZhCn, Some(&source)))
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

/// 统一换行后再断言，避免 Windows 的 `\r\n` 影响精确匹配。
fn normalized_stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

/// 在指定工作目录运行编译产物；产物必须能在无关目录下独立运行。
fn run_binary(binary: &Path, cwd: &Path) -> Output {
    Command::new(binary)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("运行 {} 失败：{error}", binary.display()))
}

/// 写入最小工程清单；`dependencies` 是完整的 `[dependencies]` 段，可为空串。
fn write_project(root: &Path, name: &str, dependencies: &str, source: &str) {
    fs::create_dir_all(root.join("src")).expect("创建 src 目录失败");
    let manifest = format!(
        "[project]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ms\"\noutput = \"build\"\nopt-level = 2\n{dependencies}"
    );
    fs::write(root.join("settings.toml"), manifest).expect("写入工程清单失败");
    fs::write(root.join("src/main.ms"), source).expect("写入入口源码失败");
}

/// 递归复制包目录，模拟 `fama install` 把源码依赖放进 `mod_module` 缓存。
fn copy_directory(source_root: &Path, target_root: &Path) {
    fs::create_dir_all(target_root).expect("创建包缓存目录失败");
    for entry in fs::read_dir(source_root).expect("读取包目录失败") {
        let entry = entry.expect("读取包目录项失败");
        let target = target_root.join(entry.file_name());
        if entry.file_type().expect("读取包目录项类型失败").is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("复制包文件失败");
        }
    }
}

/// 造一个名为 `io`、版本 0.1.0 的纯 Mas 假包，并复制到依赖缓存。
///
/// 假包只导出 `origin() -> string`：一旦编译器错误地选中随附 io，就会因缺少
/// `origin` 而直接编译失败，不会静默通过。
fn install_fake_io(project_root: &Path) {
    let package_root = project_root.join("fake-io");
    fs::create_dir_all(package_root.join("src")).expect("创建假 io 包源码目录失败");
    fs::write(
        package_root.join("settings.toml"),
        "[project]\nname = \"io\"\nversion = \"0.1.0\"\n",
    )
    .expect("写入假 io 包清单失败");
    fs::write(
        package_root.join("src/index.ms"),
        "pub func origin() -> string:\n    return \"explicit\"\n",
    )
    .expect("写入假 io 包源码失败");
    copy_directory(
        &package_root,
        &project_root.join("mod_module").join("io@0.1.0"),
    );
}

/// 仓库外的单文件没有 settings.toml：裸 `io` 必须指向随附包，并真实完成标准流与文件读写。
#[test]
fn single_file_without_manifest_runs_bundled_io() {
    if skip_outside_matrix("single_file_without_manifest_runs_bundled_io") {
        return;
    }
    let root = TemporaryDirectory::create("mas-bundled-single").expect("创建临时目录失败");
    if !no_manifest_above(root.path()) {
        eprintln!(
            "跳过 single_file_without_manifest_runs_bundled_io：临时目录上级存在 settings.toml"
        );
        return;
    }
    let source_path = root.path().join("standalone.ms");
    fs::write(
        &source_path,
        r#"import io {
    read_text,
    stdout,
    write_text,
    write_text_to,
}

func main():
    var stream_ok = write_text_to(stdout(), "bundled io").ok
    print("")
    print("stream ok=" + string(stream_ok))
    var stored = write_text("roundtrip.txt", "文本内容")
    print("write ok=" + string(stored.ok))
    var loaded = read_text("roundtrip.txt")
    print("read ok=" + string(loaded.ok))
    print("read value=" + loaded.value)
"#,
    )
    .expect("写入单文件源码失败");

    let output_path = root
        .path()
        .join(format!("standalone{}", std::env::consts::EXE_SUFFIX));
    let binary = build(&source_path, &output_path);
    let output = run_binary(&binary, root.path());
    assert!(output.status.success(), "{}", render_output(&output));
    assert_eq!(
        normalized_stdout(&output),
        "bundled io\nstream ok=true\nwrite ok=true\nread ok=true\nread value=文本内容\n",
        "随附 io 的标准流与文件读写结果不符"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("roundtrip.txt")).expect("读取回写文件失败"),
        "文本内容",
        "write_text/read_text 必须真实落盘并读回"
    );
}

/// 把 maslang 二进制复制到另一个目录再用它构建，证明运行时不依赖仓库源码或资产路径。
///
/// 只改变 cwd 查不出编译期绝对路径泄漏：这里只复制可执行文件本身，并让源码与
/// 工作目录都位于互不相关的临时目录。
#[test]
fn copied_compiler_binary_builds_bundled_io_outside_repository() {
    if skip_outside_matrix("copied_compiler_binary_builds_bundled_io_outside_repository") {
        return;
    }
    let tool_root = TemporaryDirectory::create("mas-bundled-tool").expect("创建工具目录失败");
    let compiler = tool_root
        .path()
        .join(format!("maslang{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_maslang"), &compiler).expect("复制 maslang 二进制失败");
    assert!(compiler.is_file(), "复制后的 maslang 二进制必须存在");

    let project_root = TemporaryDirectory::create("mas-bundled-cli").expect("创建临时工程目录失败");
    if !no_manifest_above(project_root.path()) {
        eprintln!(
            "跳过 copied_compiler_binary_builds_bundled_io_outside_repository：临时目录上级存在 settings.toml"
        );
        return;
    }
    let source_path = project_root.path().join("hello.ms");
    fs::write(
        &source_path,
        r#"import io { stdout, write_text_to }

func main():
    var result = write_text_to(stdout(), "cli bundled io")
    print("")
    print("ok=" + string(result.ok))
"#,
    )
    .expect("写入 CLI 用例源码失败");
    let output_path = project_root
        .path()
        .join(format!("hello{}", std::env::consts::EXE_SUFFIX));

    // 隔离全局设置与临时目录：不能读到用户配置的外部编译器，也不能把中间文件散落到宿主机。
    let home = TemporaryDirectory::create("mas-bundled-home").expect("创建隔离 home 失败");
    let unrelated_cwd =
        TemporaryDirectory::create("mas-bundled-cwd").expect("创建无关工作目录失败");
    let temp = project_root.path().join("tmp");
    fs::create_dir_all(&temp).expect("创建隔离临时目录失败");

    let output = Command::new(&compiler)
        .args([
            "build".as_ref(),
            source_path.as_os_str(),
            "-o".as_ref(),
            output_path.as_os_str(),
        ])
        .current_dir(unrelated_cwd.path())
        .env("FAMA_HOME", home.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("TMP", &temp)
        .env("TEMP", &temp)
        .output()
        .expect("执行复制后的 maslang 失败");
    assert!(
        output.status.success(),
        "复制后的 maslang 构建失败：{}",
        render_output(&output)
    );

    let run = run_binary(&output_path, unrelated_cwd.path());
    assert!(run.status.success(), "{}", render_output(&run));
    assert_eq!(
        normalized_stdout(&run),
        "cli bundled io\nok=true\n",
        "复制后的编译器必须内嵌随附 io，不能依赖仓库路径"
    );
}

/// 同一工程的两个模块各自 `import io`：同一模块标识只加入一份原生对象，链接与运行成功。
///
/// MSVC 对重复对象会直接链接失败，因此「能构建并运行」就是去重的证据。
#[test]
fn duplicate_bundled_io_imports_link_once() {
    if skip_outside_matrix("duplicate_bundled_io_imports_link_once") {
        return;
    }
    let root = TemporaryDirectory::create("mas-bundled-duplicate").expect("创建临时工程失败");
    write_project(
        root.path(),
        "duplicate-io",
        "",
        r#"import helper { emit }
import io { stdout, write_text_to }

func main():
    var helper_ok = emit()
    print("")
    print("helper ok=" + string(helper_ok))
    var main_ok = write_text_to(stdout(), "main io").ok
    print("")
    print("main ok=" + string(main_ok))
"#,
    );
    fs::write(
        root.path().join("src/helper.ms"),
        r#"import io { stdout, write_text_to }

pub func emit() -> bool:
    return write_text_to(stdout(), "helper io").ok
"#,
    )
    .expect("写入 helper 模块失败");

    let output_path = root
        .path()
        .join("build")
        .join(format!("duplicate-io{}", std::env::consts::EXE_SUFFIX));
    let binary = build(&root.path().join("src/main.ms"), &output_path);
    let output = run_binary(&binary, root.path());
    assert!(output.status.success(), "{}", render_output(&output));
    assert_eq!(
        normalized_stdout(&output),
        "helper io\nhelper ok=true\nmain io\nmain ok=true\n",
        "多处导入随附 io 必须只链接一份原生对象并正常运行"
    );
}

/// 当前包显式声明 `[dependencies].io` 时，必须优先使用已安装的精确版本。
///
/// 假包只导出 `origin()`；如果错误地选中随附包，会因找不到 `origin` 而编译失败。
#[test]
fn explicit_io_dependency_wins_over_bundled_package() {
    if skip_outside_matrix("explicit_io_dependency_wins_over_bundled_package") {
        return;
    }
    let root = TemporaryDirectory::create("mas-bundled-explicit").expect("创建临时工程失败");
    write_project(
        root.path(),
        "explicit-io",
        "\n[dependencies]\nio = { path = \"fake-io\", version = \"0.1.0\" }\n",
        r#"import io { origin }

func main():
    print(origin())
"#,
    );
    install_fake_io(root.path());

    let output_path = root
        .path()
        .join("build")
        .join(format!("explicit-io{}", std::env::consts::EXE_SUFFIX));
    let binary = build(&root.path().join("src/main.ms"), &output_path);
    let output = run_binary(&binary, root.path());
    assert!(output.status.success(), "{}", render_output(&output));
    assert_eq!(
        normalized_stdout(&output),
        "explicit\n",
        "显式声明 io 依赖时必须加载已安装的 0.1.0 包，而不是随附包"
    );
}

/// 同一模块图实际同时加载随附 io 与显式 io 时，必须在链接前给出可定位的冲突诊断。
///
/// 根工程声明显式 io 并导入它；依赖包 helper 的清单没有 io 声明，所以它导入的是随附 io。
#[test]
fn bundled_and_explicit_io_conflict_is_reported_before_link() {
    if skip_outside_matrix("bundled_and_explicit_io_conflict_is_reported_before_link") {
        return;
    }
    let root = TemporaryDirectory::create("mas-bundled-conflict").expect("创建临时工程失败");
    write_project(
        root.path(),
        "conflict-io",
        "\n[dependencies]\nio = { path = \"fake-io\", version = \"0.1.0\" }\nhelper = { path = \"helper\", version = \"0.1.0\" }\n",
        r#"import helper { emit }
import io { origin }

func main():
    print(origin())
    print("helper=" + string(emit()))
"#,
    );
    install_fake_io(root.path());
    let helper_root = root.path().join("mod_module").join("helper@0.1.0");
    fs::create_dir_all(helper_root.join("src")).expect("创建 helper 包源码目录失败");
    fs::write(
        helper_root.join("settings.toml"),
        "[project]\nname = \"helper\"\nversion = \"0.1.0\"\n",
    )
    .expect("写入 helper 包清单失败");
    fs::write(
        helper_root.join("src/index.ms"),
        r#"import io { stdout, write_text_to }

pub func emit() -> bool:
    return write_text_to(stdout(), "helper io").ok
"#,
    )
    .expect("写入 helper 包源码失败");

    let output_path = root
        .path()
        .join("build")
        .join(format!("conflict-io{}", std::env::consts::EXE_SUFFIX));
    let rendered = build_result(&root.path().join("src/main.ms"), &output_path)
        .expect_err("同时加载随附 io 与显式 io 必须在链接前失败");
    for keyword in ["随附", "显式依赖", "重复 C 符号", "[dependencies].io"] {
        assert!(
            rendered.contains(keyword),
            "冲突诊断缺少关键词 `{keyword}`：\n{rendered}"
        );
    }
    assert!(
        !output_path.exists(),
        "冲突必须在链接前报错，不能留下可执行产物：{}",
        output_path.display()
    );
}

/// 项目内 `src/io.ms` 仍可经 `local.io` 导入；同项目的裸 `io` 优先指向随附包。
#[test]
fn local_io_resolves_project_source_while_bare_io_uses_bundled_package() {
    if skip_outside_matrix("local_io_resolves_project_source_while_bare_io_uses_bundled_package") {
        return;
    }
    let root = TemporaryDirectory::create("mas-bundled-local").expect("创建临时工程失败");
    write_project(
        root.path(),
        "local-io",
        "",
        r#"import io { stdout, write_text_to }
import local.io { marker }

func main():
    var stream_ok = write_text_to(stdout(), "bundled io").ok
    print("")
    print("stream ok=" + string(stream_ok))
    print("local=" + marker())
"#,
    );
    fs::write(
        root.path().join("src/io.ms"),
        "pub func marker() -> string:\n    return \"local\"\n",
    )
    .expect("写入项目内 io 源码失败");

    let output_path = root
        .path()
        .join("build")
        .join(format!("local-io{}", std::env::consts::EXE_SUFFIX));
    let binary = build(&root.path().join("src/main.ms"), &output_path);
    let output = run_binary(&binary, root.path());
    assert!(output.status.success(), "{}", render_output(&output));
    assert_eq!(
        normalized_stdout(&output),
        "bundled io\nstream ok=true\nlocal=local\n",
        "裸 io 必须让位于随附包，local.io 必须解析项目内源码"
    );
}

/// 没有项目清单时，普通模块导入保持原来的报错行为，不能被随附 io 的解析扩展捎带放宽。
#[test]
fn single_file_without_manifest_does_not_resolve_ordinary_modules() {
    let root = TemporaryDirectory::create("mas-bundled-no-manifest").expect("创建临时目录失败");
    if !no_manifest_above(root.path()) {
        eprintln!(
            "跳过 single_file_without_manifest_does_not_resolve_ordinary_modules：临时目录上级存在 settings.toml"
        );
        return;
    }
    let source_path = root.path().join("main.ms");
    fs::write(
        &source_path,
        "import helper { value }\n\nfunc main():\n    print(value())\n",
    )
    .expect("写入入口源码失败");
    // 同时在入口旁和 src/ 下放 helper，任何“顺手扩大本地搜索”的实现都会误命中。
    let helper = "pub func value() -> int:\n    return 1\n";
    fs::write(root.path().join("helper.ms"), helper).expect("写入同级 helper 失败");
    fs::create_dir_all(root.path().join("src")).expect("创建 src 目录失败");
    fs::write(root.path().join("src/helper.ms"), helper).expect("写入 src helper 失败");

    let source = read_source(&source_path);
    let result = analyze_source(&source, &zh_settings());
    assert!(
        result.is_err(),
        "无清单单文件不应解析普通模块导入；只有随附 io 属于该场景的标准库能力"
    );
}
