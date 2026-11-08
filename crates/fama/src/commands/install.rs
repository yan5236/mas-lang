use crate::cli::InstallArgs;
use crate::error::FamaError;
use crate::install::install_project_dependencies;
use crate::project::resolve_project_root;

/// 执行 `fama install`，安装当前项目声明的全部本地路径依赖。
pub fn run(args: InstallArgs) -> Result<(), FamaError> {
    let project_root = resolve_project_root(&args.project_dir)?;
    let lock_file = install_project_dependencies(&project_root)?;
    println!("已安装依赖数量: {}", lock_file.dependencies.len());
    Ok(())
}
