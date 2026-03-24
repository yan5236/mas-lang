use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::CompileOptions;
use crate::ast::Program;
use crate::diagnostic::{CompilerError, MessageId};
use crate::llvm_backend;
use crate::runtime_bootstrap;
use crate::settings::CompilerSettings;
use crate::source::{SourceFile, Span};

/// 执行完整的对象文件生成与本机链接流程，并返回最终可执行文件路径。
pub fn compile_to_executable(
    source: &SourceFile,
    settings: &CompilerSettings,
    program: &Program,
    options: &CompileOptions,
) -> Result<PathBuf, CompilerError> {
    let output_path = resolve_output_path(source, options)?;
    let generated_object = llvm_backend::compile_program(source, settings, program)?;
    let workspace = BuildWorkspace::create()?;

    write_bytes(
        &workspace.object_path,
        &generated_object.object_bytes,
        source,
    )?;
    write_text(
        &workspace.bootstrap_path,
        runtime_bootstrap::bootstrap_source(),
        source,
    )?;
    ensure_parent_directory(&output_path, source)?;
    link_executable(
        &workspace.object_path,
        &workspace.bootstrap_path,
        &output_path,
        source,
    )?;
    Ok(output_path)
}

/// 根据源码路径和命令行选项推导最终输出文件位置。
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
    Ok(parent
        .join("build")
        .join(format!("{}.exe", source_name.to_string_lossy())))
}

/// 调用 `rustc` 把目标文件和引导源码链接为最终的 `.exe`。
fn link_executable(
    object_path: &Path,
    bootstrap_path: &Path,
    output_path: &Path,
    source: &SourceFile,
) -> Result<(), CompilerError> {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let status = Command::new(rustc)
        .arg("--edition")
        .arg("2024")
        .arg("--target")
        .arg("x86_64-pc-windows-msvc")
        .arg(bootstrap_path)
        .arg("-o")
        .arg(output_path)
        .arg("-C")
        .arg(format!("link-arg={}", object_path.display()))
        .status()
        .map_err(|error| {
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
    _root: PathBuf,
    object_path: PathBuf,
    bootstrap_path: PathBuf,
}

impl BuildWorkspace {
    /// 创建新的唯一临时工作目录，并分配目标文件和引导源码路径。
    fn create() -> Result<Self, CompilerError> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| {
                CompilerError::build(
                    MessageId::BuildRustcFailed,
                    vec![error.to_string()],
                    Span::new(0, 0),
                )
            })?
            .as_nanos();
        let root = env::temp_dir().join(format!("mascript-llvm-build-{unique}"));
        fs::create_dir_all(&root).map_err(|error| {
            CompilerError::build(
                MessageId::BuildCreateOutputDirFailed,
                vec![root.display().to_string(), error.to_string()],
                Span::new(0, 0),
            )
        })?;
        Ok(Self {
            object_path: root.join("program.obj"),
            bootstrap_path: root.join("bootstrap.rs"),
            _root: root,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::CompileOptions;
    use crate::settings::{CompilerSettings, UiLanguage};
    use crate::source::SourceFile;
    use crate::{analyze_source, compile_to_executable};

    /// 编译源码并立即运行产物，返回收集到的标准输出文本。
    fn build_and_run(name: &str, text: &str) -> String {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("mascript-llvm-e2e-{name}-{unique}"));
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

    /// 返回 `msceshi` 压力样例对应的源码文本。
    ///
    /// `cargo test` 在当前环境里会把 crate 放进独立沙盒目录，因此这里内嵌同内容样例，
    /// 同时仓库中的 `msceshi/compiler-stress/*.ms` 继续保留，方便人工查看和单独手跑。
    fn msceshi_fixture_source(name: &str) -> &'static str {
        match name {
            "branch-return-matrix.ms" => include_str!("testdata/branch-return-matrix.ms.txt"),
            "string-list-matrix.ms" => include_str!("testdata/string-list-matrix.ms.txt"),
            "short-circuit-guard.ms" => include_str!("testdata/short-circuit-guard.ms.txt"),
            other => panic!("未知的 msceshi 样例 `{other}`"),
        }
    }

    /// 读取内嵌的 `msceshi` 样例源码，先做前端分析再编译运行，最终返回标准输出文本。
    fn build_and_run_msceshi_fixture(name: &str) -> String {
        let source = SourceFile::new(
            format!("<msceshi/compiler-stress/{name}>"),
            msceshi_fixture_source(name).to_string(),
        );
        let settings = CompilerSettings::new(UiLanguage::ZhCn);

        analyze_source(&source, &settings).unwrap();

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let output_path = std::env::temp_dir()
            .join(format!("mascript-llvm-msceshi-{unique}"))
            .join(name.replace(".ms", ".exe"));
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
        let output = build_and_run("hello", "print(\"Hello world.\")\n");
        assert_eq!(output, "Hello world.\n");
    }

    /// 校验字符串拼接能够通过 LLVM 后端和运行时 ABI 协同工作。
    #[test]
    fn should_build_and_run_string_concat_program() {
        let output = build_and_run(
            "string",
            "var name = \"Mascript\"\nprint(\"Hello \" + name)\n",
        );
        assert_eq!(output, "Hello Mascript\n");
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

    /// 校验递归函数结合 `if / else` 全分支返回时，仍能通过 LLVM 后端完整编译并运行。
    #[test]
    fn should_build_and_run_recursive_program() {
        let output = build_and_run(
            "recursive",
            "func factorial(n:int) -> int:\n    if n <= 1:\n        return 1\n    else:\n        return n * factorial(n - 1)\nfunc main():\n    print(factorial(5))\n",
        );
        assert_eq!(output, "120\n");
    }

    /// 校验 `msceshi` 中专门构造的分支返回矩阵样例能够稳定编译运行，避免 `if/return` 回归。
    #[test]
    fn should_build_and_run_msceshi_branch_return_matrix() {
        let output = build_and_run_msceshi_fixture("branch-return-matrix.ms");
        assert_eq!(output, "-1\n0\n1\n10\n11\n20\n21\n30\n999\n300\n");
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

    /// 校验 `msceshi` 中短路保护样例运行时不会错误求值右侧危险表达式，避免隐藏的越界/空指针回归。
    #[test]
    fn should_build_and_run_msceshi_short_circuit_guard() {
        let output = build_and_run_msceshi_fixture("short-circuit-guard.ms");
        assert_eq!(output, "false\ntrue\n1\n7\n8\n");
    }
}
