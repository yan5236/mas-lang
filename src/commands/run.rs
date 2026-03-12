use std::process::Command;

use crate::cli::{BuildArgs, RunArgs};
use crate::error::FamaError;
use crate::project::ProjectContext;

/// 执行 `fama run`，先复用构建流程生成产物，再启动最终可执行文件。
pub fn run(args: RunArgs) -> Result<(), FamaError> {
    let context = ProjectContext::load(&args.project_dir)?;
    let binary_path = context.default_binary_path();

    super::build::run(BuildArgs {
        project_dir: args.project_dir.clone(),
        output: None,
    })?;

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
