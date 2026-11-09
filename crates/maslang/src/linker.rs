use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::CompileOptions;
use crate::ast::Program;
use crate::diagnostic::{CompilerError, MessageId};
use crate::llvm_backend;
use crate::native_link::{DEFAULT_TARGET_TRIPLE, resolve_native_link_inputs};
use crate::runtime_bootstrap;
use crate::settings::CompilerSettings;
use crate::source::{SourceFile, Span};
use crate::temp_directory::TemporaryDirectory;

/// 检查可执行入口，执行对象文件生成与本机链接，成功时返回产物路径。
pub fn compile_to_executable(
    source: &SourceFile,
    settings: &CompilerSettings,
    program: &Program,
    package_roots: &[PathBuf],
    options: &CompileOptions,
) -> Result<PathBuf, CompilerError> {
    if !program.statements.iter().any(|statement| {
        matches!(statement, crate::ast::Stmt::FunctionDecl { name, parameters, .. } if name == "main" && parameters.is_empty())
    }) {
        return Err(CompilerError::semantic_message(
            "可执行程序必须定义无参数的 `func main()`",
            Span::new(0, 0),
        ));
    }
    let output_path = resolve_output_path(source, options)?;
    let generated_object =
        llvm_backend::compile_program_with_options(source, settings, program, options)?;
    let workspace = BuildWorkspace::create()?;

    write_bytes(
        &workspace.object_path,
        &generated_object.object_bytes,
        source,
    )?;
    let native_objects = resolve_native_link_inputs(
        package_roots,
        &workspace.native_root,
        source,
        options.allow_native_source_build,
    )?;
    ensure_parent_directory(&output_path, source)?;
    link_executable(
        &workspace.object_path,
        &workspace.bootstrap_path,
        &native_objects,
        &output_path,
        source,
        options.opt_level,
    )?;
    Ok(output_path)
}

/// 优先使用显式输出路径，否则在源码旁的 build 目录使用原文件主名及系统可执行文件后缀。
fn resolve_output_path(
    source: &SourceFile,
    options: &CompileOptions,
) -> Result<PathBuf, CompilerError> {
    if let Some(output_path) = &options.output_path {
        return Ok(output_path.clone());
    }

    let source_path = PathBuf::from(source.path());
    let source_name = source_path.file_stem().ok_or_else(|| {
        CompilerError::build(
            MessageId::BuildRustcFailed,
            vec!["无法推导默认输出文件名".to_string()],
            Span::new(0, 0),
        )
    })?;
    let parent = source_path.parent().ok_or_else(|| {
        CompilerError::build(
            MessageId::BuildRustcFailed,
            vec!["无法推导默认输出目录".to_string()],
            Span::new(0, 0),
        )
    })?;
    let mut binary_name = source_name.to_os_string();
    binary_name.push(env::consts::EXE_SUFFIX);
    Ok(parent.join("build").join(binary_name))
}

/// 链接程序及原生依赖；工具链匹配时复用相同优化档的预构建运行时，否则用当前 rustc 编译完整运行时。
/// GNU/Linux 在原生对象之后重申 libc 依赖，确保 C 对象所需的符号也参与解析。链接失败时返回构建诊断。
fn link_executable(
    object_path: &Path,
    bootstrap_path: &Path,
    native_objects: &[PathBuf],
    output_path: &Path,
    source: &SourceFile,
    opt_level: crate::optimization::OptLevel,
) -> Result<(), CompilerError> {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let mut command = Command::new(&rustc);
    if let Some(runtime) = bundled_runtime(&rustc, opt_level) {
        let library = bootstrap_path.with_file_name("libmas_runtime.rlib");
        write_bytes(&library, runtime, source)?;
        write_text(bootstrap_path, "fn main() { mas_runtime::run(); }", source)?;
        command
            .arg("--extern")
            .arg(format!("mas_runtime={}", library.display()));
    } else {
        write_text(bootstrap_path, runtime_bootstrap::bootstrap_source(), source)?;
    }
    command
        .arg("--edition")
        .arg("2024")
        .arg("--target")
        .arg(DEFAULT_TARGET_TRIPLE)
        .arg("-C")
        .arg(format!("opt-level={}", opt_level.as_str()))
        // 引导运行时是单文件 crate；固定单 codegen unit，避免私有 `#[cold]` 辅助函数被
        // 内部化成带 `.llvm.` 后缀的符号后被其它 CGU 引用，导致 macOS/ELF 链接偶发缺符号。
        .args(["-C", "codegen-units=1"])
        .arg(bootstrap_path)
        .arg("-o")
        .arg(output_path)
        .arg("-C")
        .arg(format!("link-arg={}", object_path.display()));
    for native_object in native_objects {
        command
            .arg("-C")
            .arg(format!("link-arg={}", native_object.display()));
    }
    if cfg!(all(target_os = "linux", target_env = "gnu")) && !native_objects.is_empty() {
        command.args(["-C", "link-arg=-lc"]);
    }
    let status = command.status().map_err(|error| {
        CompilerError::build(
            MessageId::BuildRustcFailed,
            vec![error.to_string()],
            Span::new(0, source.text().len()),
        )
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(CompilerError::build(
            MessageId::BuildRustcExitFailed,
            vec![status.to_string()],
            Span::new(0, source.text().len()),
        ))
    }
}

/// 查找与实际执行的 rustc 完全同版的嵌入运行时；未知工具链或不支持的宿主走源码链接。
#[cfg(all(target_arch = "x86_64", target_os = "windows", target_env = "msvc"))]
fn bundled_runtime(rustc: &str, level: crate::optimization::OptLevel) -> Option<&'static [u8]> {
    use crate::optimization::OptLevel::*;
    let version = Command::new(rustc).arg("-vV").output().ok()?;
    if !version.status.success()
        || version.stdout != include_bytes!(concat!(env!("OUT_DIR"), "/runtime-rustc.txt"))
    {
        return None;
    }
    Some(match level {
        O0 => include_bytes!(concat!(env!("OUT_DIR"), "/libmas_runtime_0.rlib")),
        O1 => include_bytes!(concat!(env!("OUT_DIR"), "/libmas_runtime_1.rlib")),
        O2 => include_bytes!(concat!(env!("OUT_DIR"), "/libmas_runtime_2.rlib")),
        O3 => include_bytes!(concat!(env!("OUT_DIR"), "/libmas_runtime_3.rlib")),
    })
}

/// 非 Windows MSVC 宿主保留原有的运行时源码链接流程。
#[cfg(not(all(target_arch = "x86_64", target_os = "windows", target_env = "msvc")))]
fn bundled_runtime(_: &str, _: crate::optimization::OptLevel) -> Option<&'static [u8]> {
    None
}

/// 确保目标输出路径的父目录存在，不存在时自动递归创建。
fn ensure_parent_directory(path: &Path, source: &SourceFile) -> Result<(), CompilerError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CompilerError::build(
                MessageId::BuildCreateOutputDirFailed,
                vec![parent.display().to_string(), error.to_string()],
                Span::new(0, source.text().len()),
            )
        })?;
    }
    Ok(())
}

/// 使用 UTF-8 无 BOM 形式把文本写入构建阶段的临时 Rust 源文件。
fn write_text(path: &Path, text: &str, source: &SourceFile) -> Result<(), CompilerError> {
    fs::write(path, text.as_bytes()).map_err(|error| {
        CompilerError::build(
            MessageId::BuildWriteBootstrapFailed,
            vec![path.display().to_string(), error.to_string()],
            Span::new(0, source.text().len()),
        )
    })
}

/// 把生成的目标文件字节写入磁盘，供后续链接步骤使用。
fn write_bytes(path: &Path, bytes: &[u8], source: &SourceFile) -> Result<(), CompilerError> {
    fs::write(path, bytes).map_err(|error| {
        CompilerError::build(
            MessageId::BuildWriteObjectFailed,
            vec![path.display().to_string(), error.to_string()],
            Span::new(0, source.text().len()),
        )
    })
}

/// 管理单次构建使用的临时目录和中间文件路径。
struct BuildWorkspace {
    _root: TemporaryDirectory,
    object_path: PathBuf,
    bootstrap_path: PathBuf,
    native_root: PathBuf,
}

impl BuildWorkspace {
    /// 原子创建独占临时目录并分配中间文件路径；守卫在成功、失败或栈展开后统一清理。
    fn create() -> Result<Self, CompilerError> {
        let root = TemporaryDirectory::create("mascript-llvm-build").map_err(|error| {
            CompilerError::build(
                MessageId::BuildCreateOutputDirFailed,
                vec![env::temp_dir().display().to_string(), error.to_string()],
                Span::new(0, 0),
            )
        })?;
        Ok(Self {
            object_path: root.path().join("program.obj"),
            bootstrap_path: root.path().join("bootstrap.rs"),
            native_root: root.path().join("native"),
            _root: root,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use crate::CompileOptions;
    use crate::settings::{CompilerSettings, UiLanguage};
    use crate::source::SourceFile;
    use crate::temp_directory::TemporaryDirectory;
    use crate::{analyze_source, compile_to_executable};

    /// 在独占目录编译运行源码，返回标准输出并清理 EXE、PDB 和测试中间文件。
    fn build_and_run(name: &str, text: &str) -> String {
        let workspace = TemporaryDirectory::create("mascript-llvm-e2e").unwrap();
        let temp_root = workspace.path();
        let source_path = temp_root.join(format!("{name}.ms"));
        let output_path = temp_root.join(format!("{name}.exe"));
        let source = SourceFile::new(source_path.display().to_string(), text.to_string());
        let settings = CompilerSettings::new(UiLanguage::ZhCn);

        let binary =
            compile_to_executable(&source, &settings, &CompileOptions::new(Some(output_path)))
                .unwrap();
        let output = Command::new(binary).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    }

    /// 在独占目录运行预期失败的程序，返回诊断后清理全部测试产物。
    fn build_and_run_expect_failure(name: &str, text: &str) -> String {
        let workspace = TemporaryDirectory::create("mascript-llvm-e2e").unwrap();
        let temp_root = workspace.path();
        let source_path = temp_root.join(format!("{name}.ms"));
        let output_path = temp_root.join(format!("{name}.exe"));
        let source = SourceFile::new(source_path.display().to_string(), text.to_string());
        let settings = CompilerSettings::new(UiLanguage::ZhCn);

        let binary =
            compile_to_executable(&source, &settings, &CompileOptions::new(Some(output_path)))
                .unwrap();
        let output = Command::new(binary).output().unwrap();
        assert!(!output.status.success());
        String::from_utf8(output.stderr).unwrap()
    }

    /// 返回 `msceshi` 压力样例对应的源码文本。
    ///
    /// `cargo test` 在当前环境里会把 crate 放进独立沙盒目录，因此这里内嵌同内容样例，
    /// 同时仓库中的 `msceshi/compiler-stress/*.ms` 继续保留，方便人工查看和单独手跑。
    fn msceshi_fixture_source(name: &str) -> &'static str {
        match name {
            "branch-return-matrix.ms" => include_str!("testdata/branch-return-matrix.ms.txt"),
            "array-matrix.ms" => include_str!("testdata/array-matrix.ms.txt"),
            "string-list-matrix.ms" => include_str!("testdata/string-list-matrix.ms.txt"),
            "string-method-matrix.ms" => include_str!("testdata/string-method-matrix.ms.txt"),
            "short-circuit-guard.ms" => include_str!("testdata/short-circuit-guard.ms.txt"),
            other => panic!("未知的 msceshi 样例 `{other}`"),
        }
    }

    /// 分析并运行内嵌样例，返回标准输出；临时目录在成功或失败时均由守卫清理。
    fn build_and_run_msceshi_fixture(name: &str) -> String {
        let source = SourceFile::new(
            format!("<msceshi/compiler-stress/{name}>"),
            msceshi_fixture_source(name).to_string(),
        );
        let settings = CompilerSettings::new(UiLanguage::ZhCn);

        analyze_source(&source, &settings).unwrap();

        let workspace = TemporaryDirectory::create("mascript-llvm-msceshi").unwrap();
        let output_path = workspace.path().join(name.replace(".ms", ".exe"));
        let binary =
            compile_to_executable(&source, &settings, &CompileOptions::new(Some(output_path)))
                .unwrap();
        let output = Command::new(binary).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    }

    /// 校验最小 Hello World 程序能够被编译并以原生方式运行。
    #[test]
    fn should_build_and_run_hello_world_program() {
        let output = build_and_run("hello", "func main():\n    print(\"Hello world.\")\n");
        assert_eq!(output, "Hello world.\n");
    }

    /// 校验字符串拼接能够通过 LLVM 后端和运行时 ABI 协同工作。
    #[test]
    fn should_build_and_run_string_concat_program() {
        let output = build_and_run(
            "string",
            "var name = \"Mas lang\"\nfunc main():\n    print(\"Hello \" + name)\n",
        );
        assert_eq!(output, "Hello Mas lang\n");
    }

    /// 校验第二阶段的控制流、函数和运算符可以通过 LLVM 后端完整运行。
    #[test]
    fn should_build_and_run_phase2_program() {
        let output = build_and_run(
            "phase2",
            "func add(a:int, b:int) -> int:\n    return a + b\nfunc main():\n    var total:int = 0\n    for i in 0..5:\n        if i == 2:\n            continue\n        total += i\n    while total < 10:\n        total += 1\n    print(add(total, 1))\n    print(7 %/% 2)\n    print(7 % 2)\n",
        );
        assert_eq!(output, "11\n3\n1\n");
    }

    /// 校验列表字面量、遍历、索引、切片和方法调用能够通过 LLVM 后端完整运行。
    #[test]
    fn should_build_and_run_list_program() {
        let output = build_and_run(
            "list",
            "func main():\n    var items:list<int> = [3, 1, 2]\n    print(len(items))\n    print(items[0])\n    items.append(4)\n    print(len(items))\n    var part = items[1:3]\n    print(part[0])\n    print(part[1])\n    print(items.contains(2))\n    print(items.index(4))\n    items.remove(1)\n    print(len(items))\n    print(items.pop())\n    items.append(1)\n    items.sort()\n    print(items[0])\n    print(items[2])\n    items.reverse()\n    print(items[0])\n    print(items.pop(1))\n    for item in items:\n        print(item)\n",
        );
        assert_eq!(output, "3\n3\n4\n1\n2\ntrue\n3\n3\n4\n1\n3\n3\n2\n3\n1\n");
    }

    /// 校验固定长度数组的默认声明、初始化和元素访问能够通过 LLVM 后端完整运行。
    #[test]
    fn should_build_and_run_array_program() {
        let output = build_and_run(
            "array",
            "func main():\n    var fixed:array<int,3>\n    var values = array<int,3>(7, 8, 9)\n    print(fixed[0])\n    print(values[0])\n    print(values[1])\n    print(values[2])\n",
        );
        assert_eq!(output, "0\n7\n8\n9\n");
    }

    /// 校验字典字面量、增删改查、keys/values 与遍历能够通过 LLVM 后端完整运行。
    #[test]
    fn should_build_and_run_map_program() {
        let output = build_and_run(
            "map",
            "func main():\n    var user:map<string,int> = {\"alice\": 10, \"bob\": 20}\n    print(len(user))\n    print(user[\"alice\"])\n    user[\"alice\"] = 11\n    user[\"carol\"] = 30\n    print(user.contains(\"carol\"))\n    var keys = user.keys()\n    print(len(keys))\n    print(keys[0])\n    print(keys[1])\n    print(keys[2])\n    var values = user.values()\n    print(values[0])\n    print(values[1])\n    print(values[2])\n    user.remove(\"bob\")\n    print(len(user))\n    for key, value in user:\n        print(key)\n        print(value)\n",
        );
        assert_eq!(
            output,
            "2\n10\ntrue\n3\nalice\nbob\ncarol\n11\n20\n30\n2\nalice\n11\ncarol\n30\n"
        );
    }

    /// 校验读取不存在的字典键时会触发明确的运行时失败，避免静默返回脏值。
    #[test]
    fn should_fail_when_map_key_is_missing() {
        let error = build_and_run_expect_failure(
            "map-missing-key",
            "func main():\n    var user:map<string,int> = {\"age\": 18}\n    print(user[\"name\"])\n",
        );
        assert!(error.contains("运行时错误: 字典中不存在目标键"));
        assert!(error.contains("-->"));
        assert!(error.contains("print(user[\"name\"])"));
    }

    /// 校验递归函数结合 `if / else` 全分支返回时，仍能通过 LLVM 后端完整编译并运行。
    #[test]
    fn should_build_and_run_recursive_program() {
        let output = build_and_run(
            "recursive",
            "func factorial(n:int) -> int:\n    if n <= 1:\n        return 1\n    else:\n        return n * factorial(n - 1)\nfunc main():\n    print(factorial(5))\n",
        );
        assert_eq!(output, "120\n");
    }

    /// 校验基础类型转换在 LLVM 后端和运行时 ABI 协同下能够输出正确结果。
    #[test]
    fn should_build_and_run_type_convert_program() {
        let output = build_and_run(
            "type-convert",
            "func main():\n    print(int(\"123\"))\n    print(float(\"3.14\"))\n    print(string(42))\n    print(string(false))\n    print(bool(1))\n    print(bool(0))\n    print(bool(\"hello\"))\n    print(bool(\"\"))\n",
        );
        assert_eq!(output, "123\n3.14\n42\nfalse\ntrue\nfalse\ntrue\nfalse\n");
    }

    /// 校验安全类型转换在成功与失败两条路径上都能返回稳定的 `ok/value` 结果。
    #[test]
    fn should_build_and_run_safe_type_convert_program() {
        let output = build_and_run(
            "safe-type-convert",
            "func main():\n    var parsed = try_int(\"123\")\n    print(parsed.ok)\n    print(parsed.value)\n    var failed = try_int(\"abc\")\n    print(failed.ok)\n    var float_parsed = try_float(\"3.5\")\n    print(float_parsed.ok)\n    print(float_parsed.value)\n    var bool_parsed = try_bool(\"true\")\n    print(bool_parsed.ok)\n    print(bool_parsed.value)\n    var bool_failed = try_bool(\"hello\")\n    print(bool_failed.ok)\n",
        );
        assert_eq!(output, "true\n123\nfalse\ntrue\n3.5\ntrue\ntrue\nfalse\n");
    }

    /// 校验 `msceshi` 中专门构造的分支返回矩阵样例能够稳定编译运行，避免 `if/return` 回归。
    #[test]
    fn should_build_and_run_msceshi_branch_return_matrix() {
        let output = build_and_run_msceshi_fixture("branch-return-matrix.ms");
        assert_eq!(output, "-1\n0\n1\n10\n11\n20\n21\n30\n999\n300\n");
    }

    /// 校验 `msceshi` 中数组压力样例能够稳定运行，避免固定长度数组的零值声明或索引回归。
    #[test]
    fn should_build_and_run_msceshi_array_matrix() {
        let output = build_and_run_msceshi_fixture("array-matrix.ms");
        assert_eq!(output, "0\n0\n11\n22\n33\n55\n");
    }

    /// 校验 `msceshi` 中字符串列表压力样例能够稳定运行，避免字符串列表再次出现类型错乱或空指针。
    #[test]
    fn should_build_and_run_msceshi_string_list_matrix() {
        let output = build_and_run_msceshi_fixture("string-list-matrix.ms");
        assert_eq!(
            output,
            "3\ngamma\ntrue\n2\nalpha\nbeta\nbeta\n2\ndelta\ngamma\ngamma\ndelta\ngamma\n"
        );
    }

    /// 校验 `msceshi` 中字符串方法样例能够处理中文 UTF-8 文本，并覆盖空替换保护逻辑。
    #[test]
    fn should_build_and_run_msceshi_string_method_matrix() {
        let output = build_and_run_msceshi_fixture("string-method-matrix.ms");
        assert_eq!(
            output,
            "你好，世界\ntrue\ntrue\ntrue\n你好，Mas lang\n中文\n"
        );
    }

    /// 校验 `msceshi` 中短路保护样例运行时不会错误求值右侧危险表达式，避免隐藏的越界/空指针回归。
    #[test]
    fn should_build_and_run_msceshi_short_circuit_guard() {
        let output = build_and_run_msceshi_fixture("short-circuit-guard.ms");
        assert_eq!(output, "false\ntrue\n1\n7\n8\n");
    }
}
