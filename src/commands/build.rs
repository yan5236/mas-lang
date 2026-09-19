use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use mascript_compiler_llvm::source::SourceFile;
use mascript_compiler_llvm::{CompileOptions, compile_to_executable};

use crate::cli::BuildArgs;
use crate::config::load_settings;
use crate::error::FamaError;
use crate::install::install_project_dependencies;
use crate::project::ProjectContext;

/// 执行 `fama build`，读取项目配置并把入口源码编译成可执行文件。
pub fn run(args: BuildArgs) -> Result<(), FamaError> {
    let context = ProjectContext::load(&args.project_dir)?;
    let output_path = context.resolve_output_path(args.output.as_deref());
    compile_project_entry(&context, &output_path, args.allow_native_source_build)?;

    println!("已生成可执行文件: {}", output_path.display());
    Ok(())
}

/// 编译当前项目入口到指定可执行文件路径，供 `fama build` 和 `fama run` 复用同一编译链路。
pub(crate) fn compile_project_entry(
    context: &ProjectContext,
    output_path: &Path,
    allow_native_source_build: bool,
) -> Result<(), FamaError> {
    install_project_dependencies(&context.root)?;
    let global_settings = load_settings()?;
    let source = load_entry_source(context)?;
    compile_project(
        &global_settings,
        allow_native_source_build,
        context,
        &source,
        output_path,
    )
}

/// 根据全局设置选择外部编译器或嵌入式主力编译器来构建当前项目。
fn compile_project(
    global_settings: &crate::config::FamaSettings,
    allow_native_source_build: bool,
    context: &ProjectContext,
    source: &SourceFile,
    output_path: &Path,
) -> Result<(), FamaError> {
    if let Some(binary_path) = &global_settings.compiler_binary {
        run_external_compiler(binary_path, context, output_path, allow_native_source_build)?;
        return Ok(());
    }

    let settings = mascript_compiler_llvm::load_settings();
    compile_to_executable(
        source,
        &settings,
        &CompileOptions::new(Some(output_path.to_path_buf()))
            .with_allow_native_source_build(allow_native_source_build)
            .with_opt_level(context.opt_level),
    )
    .map_err(|error| FamaError::Compiler(error.render(settings.language, Some(source))))?;
    Ok(())
}

/// 调用全局配置中声明的主力编译器二进制，完成当前项目的编译动作。
fn run_external_compiler(
    binary_path: &std::path::Path,
    context: &ProjectContext,
    output_path: &std::path::Path,
    allow_native_source_build: bool,
) -> Result<(), FamaError> {
    if !binary_path.is_file() {
        return Err(FamaError::MissingFile(binary_path.to_path_buf()));
    }

    let mut command = Command::new(binary_path);
    command
        .arg("build")
        .arg(&context.entry_path)
        .arg(format!("-O{}", context.opt_level.as_str()))
        .arg("-o")
        .arg(output_path)
        .current_dir(&context.root);
    if allow_native_source_build {
        command.arg("--allow-native-source-build");
    }
    let output = command
        .output()
        .map_err(|source| FamaError::io(binary_path.to_path_buf(), source))?;
    write_external_compiler_output(&output.stdout, &output.stderr)?;

    if output.status.success() {
        Ok(())
    } else {
        Err(FamaError::CommandFailed {
            command: binary_path.display().to_string(),
            code: output.status.code(),
        })
    }
}

/// 转发外部编译器输出，同时过滤普通预编译命中提示，避免 run/build 结果出现无关噪声。
fn write_external_compiler_output(stdout: &[u8], stderr: &[u8]) -> Result<(), FamaError> {
    io::stdout()
        .write_all(stdout)
        .map_err(|source| FamaError::io(Path::new("<stdout>").to_path_buf(), source))?;
    let stderr_text = String::from_utf8_lossy(stderr);
    let filtered = stderr_text
        .lines()
        .filter(|line| !line.contains("已命中预编译原生产物"))
        .collect::<Vec<_>>()
        .join("\n");
    if !filtered.is_empty() {
        io::stderr()
            .write_all(filtered.as_bytes())
            .map_err(|source| FamaError::io(Path::new("<stderr>").to_path_buf(), source))?;
        io::stderr()
            .write_all(b"\n")
            .map_err(|source| FamaError::io(Path::new("<stderr>").to_path_buf(), source))?;
    }
    Ok(())
}

/// 调用全局配置中声明的主力编译器二进制执行 `run`，保持临时产物语义由编译器负责。
pub(crate) fn run_external_compiler_entry(
    binary_path: &Path,
    context: &ProjectContext,
    allow_native_source_build: bool,
) -> Result<(), FamaError> {
    if !binary_path.is_file() {
        return Err(FamaError::MissingFile(binary_path.to_path_buf()));
    }

    let mut command = Command::new(binary_path);
    command
        .arg("run")
        .arg(&context.entry_path)
        .arg(format!("-O{}", context.opt_level.as_str()))
        .current_dir(&context.root);
    if allow_native_source_build {
        command.arg("--allow-native-source-build");
    }
    let output = command
        .output()
        .map_err(|source| FamaError::io(binary_path.to_path_buf(), source))?;
    write_external_compiler_output(&output.stdout, &output.stderr)?;

    if output.status.success() {
        Ok(())
    } else {
        Err(FamaError::CommandFailed {
            command: binary_path.display().to_string(),
            code: output.status.code(),
        })
    }
}

/// 读取项目入口源码文件，并构造编译器需要的 `SourceFile` 对象。
fn load_entry_source(context: &ProjectContext) -> Result<SourceFile, FamaError> {
    let text = fs::read_to_string(&context.entry_path)
        .map_err(|source| FamaError::io(context.entry_path.clone(), source))?;
    Ok(SourceFile::new(
        context.entry_path.display().to_string(),
        text,
    ))
}
