use std::fs;

use compiler::backend::CompileOptions;
use compiler::compile_to_executable;
use compiler::source::SourceFile;

use crate::cli::BuildArgs;
use crate::error::FamaError;
use crate::project::ProjectContext;

/// 执行 `fama build`，读取项目配置并把入口源码编译成可执行文件。
pub fn run(args: BuildArgs) -> Result<(), FamaError> {
    let context = ProjectContext::load(&args.project_dir)?;
    let source = load_entry_source(&context)?;
    let output_path = context.resolve_output_path(args.output.as_deref());
    let options = CompileOptions::new(Some(output_path.clone()));

    compile_to_executable(&source, &options)
        .map_err(|error| FamaError::Compiler(error.render(&source)))?;

    println!("已生成可执行文件: {}", output_path.display());
    Ok(())
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
