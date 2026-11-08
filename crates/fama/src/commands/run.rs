use maslang::temp_directory::TemporaryDirectory;
use std::env;
use std::process::Command;

use crate::cli::RunArgs;
use crate::config::load_settings;
use crate::error::FamaError;
use crate::project::ProjectContext;

/// 刷新依赖后运行项目；内嵌模式在子进程退出后清理临时产物，外部模式由编译器负责清理。
pub fn run(args: RunArgs) -> Result<(), FamaError> {
    let context = ProjectContext::load(&args.project_dir)?;
    let global_settings = load_settings()?;
    if let Some(binary_path) = &global_settings.compiler_binary {
        crate::install::install_project_dependencies(&context.root)?;
        return super::build::run_external_compiler_entry(
            binary_path,
            &context,
            args.allow_native_source_build,
        );
    }

    let workspace = TemporaryDirectory::create("fama-run")
        .map_err(|source| FamaError::io(env::temp_dir(), source))?;
    let binary_path = workspace
        .path()
        .join(format!("program{}", env::consts::EXE_SUFFIX));

    super::build::compile_project_entry(&context, &binary_path, args.allow_native_source_build)?;

    let status = Command::new(&binary_path)
        .current_dir(&context.root)
        .status()
        .map_err(|source| FamaError::io(binary_path.clone(), source))?;

    if status.success() {
        Ok(())
    } else {
        Err(FamaError::CommandFailed {
            command: binary_path.display().to_string(),
            code: status.code(),
        })
    }
}
