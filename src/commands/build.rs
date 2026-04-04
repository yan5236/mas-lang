use std::fs;
use std::process::Command;

use mascript_compiler_llvm::source::SourceFile;
use mascript_compiler_llvm::{CompileOptions, compile_to_executable};

use crate::cli::BuildArgs;
use crate::config::load_settings;
use crate::error::FamaError;
use crate::project::ProjectContext;

/// 执行 `fama build`，读取项目配置并把入口源码编译成可执行文件。
pub fn run(args: BuildArgs) -> Result<(), FamaError> {
    let context = ProjectContext::load(&args.project_dir)?;
    let global_settings = load_settings()?;
    let source = load_entry_source(&context)?;
    let output_path = context.resolve_output_path(args.output.as_deref());
    compile_project(&global_settings, &context, &source, &output_path)?;

    println!("已生成可执行文件: {}", output_path.display());
    Ok(())
}

/// 根据全局设置选择外部编译器或嵌入式主力编译器来构建当前项目。
fn compile_project(
    global_settings: &crate::config::FamaSettings,
    context: &ProjectContext,
    source: &SourceFile,
    output_path: &std::path::Path,
) -> Result<(), FamaError> {
    if let Some(binary_path) = &global_settings.compiler_binary {
        run_external_compiler(binary_path, context, output_path)?;
        return Ok(());
    }

    let settings = mascript_compiler_llvm::load_settings();
    compile_to_executable(
        source,
        &settings,
        &CompileOptions::new(Some(output_path.to_path_buf())),
    )
    .map_err(|error| FamaError::Compiler(error.render(settings.language, Some(source))))?;
    Ok(())
}

/// 调用全局配置中声明的主力编译器二进制，完成当前项目的编译动作。
fn run_external_compiler(
    binary_path: &std::path::Path,
    context: &ProjectContext,
    output_path: &std::path::Path,
) -> Result<(), FamaError> {
    if !binary_path.is_file() {
        return Err(FamaError::MissingFile(binary_path.to_path_buf()));
    }

    let status = Command::new(binary_path)
        .arg("build")
        .arg(&context.entry_path)
        .arg("-o")
        .arg(output_path)
        .current_dir(&context.root)
        .status()
        .map_err(|source| FamaError::io(binary_path.to_path_buf(), source))?;

    if status.success() {
        Ok(())
    } else {
        Err(FamaError::CommandFailed {
            command: binary_path.display().to_string(),
            code: status.code(),
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
