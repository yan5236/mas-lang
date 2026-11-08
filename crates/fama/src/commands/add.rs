use crate::cli::AddArgs;
use crate::error::FamaError;
use crate::manifest::{
    DependencySpec, load_project_manifest, normalize_dependency_path, resolve_dependency_path,
    save_project_manifest,
};
use crate::project::resolve_project_root;

/// 执行 `fama add`，把本地路径依赖写入当前项目的 `mod.yml`。
pub fn run(args: AddArgs) -> Result<(), FamaError> {
    let project_root = resolve_project_root(&args.project_dir)?;
    let dependency_root = resolve_dependency_path(&project_root, &args.dependency_path)?;
    let dependency_manifest = load_project_manifest(&dependency_root)?;
    let mut project_manifest = load_project_manifest(&project_root)?;

    project_manifest.dependencies.insert(
        dependency_manifest.name.clone(),
        DependencySpec {
            path: normalize_dependency_path(&project_root, &dependency_root),
            version: dependency_manifest.version.clone(),
        },
    );
    save_project_manifest(&project_root, &project_manifest)?;

    println!(
        "已添加本地依赖: {} {}",
        dependency_manifest.name, dependency_manifest.version
    );
    Ok(())
}
