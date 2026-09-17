use std::env;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::RunArgs;
use crate::config::load_settings;
use crate::error::FamaError;
use crate::project::ProjectContext;

/// 执行 `fama run`，把项目编译到临时可执行文件后立即启动，避免污染项目 build 目录。
pub fn run(args: RunArgs) -> Result<(), FamaError> {
    let context = ProjectContext::load(&args.project_dir)?;
    let global_settings = load_settings()?;
    if let Some(binary_path) = &global_settings.compiler_binary {
        return super::build::run_external_compiler_entry(
            binary_path,
            &context,
            args.allow_native_source_build,
        );
    }

    let binary_path = temp_run_binary_path(&context.name);

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

/// 为 `fama run` 生成唯一临时可执行文件名，使行为与主力编译器 `run` 子命令保持一致。
fn temp_run_binary_path(project_name: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    env::temp_dir().join(format!("fama-run-{project_name}-{unique}.exe"))
}
