use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::FamaError;
use crate::manifest::{
    DependencySpec, LockFile, LockedDependency, LockedDependencySource, dependency_store_dir,
    load_lock_file, load_project_manifest, normalize_dependency_path, resolve_dependency_path,
    save_lock_file,
};

/// 执行当前项目的本地依赖安装流程，并返回最终生成的锁文件内容。
pub fn install_project_dependencies(project_root: &Path) -> Result<LockFile, FamaError> {
    let manifest = load_project_manifest(project_root)?;
    let existing_lock_file = load_lock_file(project_root)?;
    let mut installer = Installer::new(project_root, existing_lock_file);

    for (dependency_name, dependency) in &manifest.dependencies {
        installer.install_dependency(project_root, dependency_name, dependency)?;
    }

    let lock_file = LockFile {
        dependencies: installer.locked_dependencies,
    };
    save_lock_file(project_root, &lock_file)?;
    Ok(lock_file)
}

/// 维护一次安装过程中的去重状态、冲突检测和循环依赖栈。
struct Installer {
    project_root: PathBuf,
    cached_dependencies: BTreeMap<String, LockedDependency>,
    locked_dependencies: BTreeMap<String, LockedDependency>,
    source_paths: BTreeMap<String, PathBuf>,
    active_stack: HashSet<PathBuf>,
}

impl Installer {
    /// 分离旧缓存索引和本轮依赖图；冲突检测与最终锁文件只使用本轮可达依赖。
    fn new(project_root: &Path, existing_lock_file: LockFile) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            cached_dependencies: existing_lock_file.dependencies,
            locked_dependencies: BTreeMap::new(),
            source_paths: BTreeMap::new(),
            active_stack: HashSet::new(),
        }
    }

    /// 递归安装依赖及其子依赖，拒绝循环、版本冲突及不安全的缓存目标。
    fn install_dependency(
        &mut self,
        owner_root: &Path,
        dependency_name: &str,
        dependency: &DependencySpec,
    ) -> Result<(), FamaError> {
        let source_root = resolve_dependency_path(owner_root, Path::new(&dependency.path))?;
        let dependency_manifest = load_project_manifest(&source_root)?;

        if dependency_manifest.name != dependency_name {
            return Err(FamaError::DependencyInstall(format!(
                "依赖 `{dependency_name}` 的目录声明名为 `{}`，名称不一致",
                dependency_manifest.name
            )));
        }

        if dependency_manifest.version != dependency.version {
            return Err(FamaError::DependencyInstall(format!(
                "依赖 `{dependency_name}` 的版本不匹配：清单要求 `{}`，目标项目声明 `{}`",
                dependency.version, dependency_manifest.version
            )));
        }

        self.ensure_dependency_has_source(&source_root, dependency_name)?;
        self.ensure_no_dependency_cycle(&source_root, dependency_name)?;
        self.ensure_no_dependency_conflict(
            dependency_name,
            &dependency_manifest.version,
            &source_root,
        )?;

        self.active_stack.insert(source_root.clone());
        for (child_name, child_dependency) in &dependency_manifest.dependencies {
            self.install_dependency(&source_root, child_name, child_dependency)?;
        }
        self.active_stack.remove(&source_root);

        let store_dir = dependency_store_dir(
            &self.project_root,
            dependency_name,
            &dependency_manifest.version,
        );
        ensure_dependency_store_is_local(&self.project_root, &store_dir)?;
        if source_root.starts_with(&store_dir) {
            return Err(FamaError::DependencyInstall(format!(
                "依赖源位于待更新的缓存目录中：{}",
                source_root.display()
            )));
        }
        if !self.should_reuse_locked_dependency(dependency_name, &source_root, &store_dir)? {
            prepare_dependency_store(&store_dir)?;
            copy_dependency_project(&source_root, &store_dir)?;
        }

        self.source_paths
            .insert(dependency_name.to_string(), source_root.clone());
        self.locked_dependencies.insert(
            dependency_name.to_string(),
            LockedDependency {
                version: dependency_manifest.version,
                source: LockedDependencySource {
                    source_type: "path".to_string(),
                    path: normalize_dependency_path(&self.project_root, &source_root),
                },
                store_path: normalize_dependency_path(&self.project_root, &store_dir),
            },
        );
        Ok(())
    }

    /// 校验依赖项目至少具备 `src/` 目录，保证后续模块解析可以落地。
    fn ensure_dependency_has_source(
        &self,
        source_root: &Path,
        dependency_name: &str,
    ) -> Result<(), FamaError> {
        let source_dir = source_root.join("src");
        if source_dir.is_dir() {
            Ok(())
        } else {
            Err(FamaError::DependencyInstall(format!(
                "依赖 `{dependency_name}` 缺少 src 目录：{}",
                source_dir.display()
            )))
        }
    }

    /// 检测路径依赖递归过程中是否出现循环依赖，避免无限递归安装。
    fn ensure_no_dependency_cycle(
        &self,
        source_root: &Path,
        dependency_name: &str,
    ) -> Result<(), FamaError> {
        if self.active_stack.contains(source_root) {
            return Err(FamaError::DependencyInstall(format!(
                "依赖 `{dependency_name}` 出现循环依赖：{}",
                source_root.display()
            )));
        }

        Ok(())
    }

    /// 检测同名依赖是否尝试以不同版本或不同来源重复安装。
    fn ensure_no_dependency_conflict(
        &self,
        dependency_name: &str,
        version: &str,
        source_root: &Path,
    ) -> Result<(), FamaError> {
        let existing = self.locked_dependencies.get(dependency_name);
        let existing_source = self.source_paths.get(dependency_name);
        if let (Some(existing), Some(existing_source)) = (existing, existing_source) {
            if existing.version != version || existing_source != source_root {
                return Err(FamaError::DependencyConflict {
                    name: dependency_name.to_string(),
                    existing: format!(
                        "version={}, path={}",
                        existing.version,
                        existing_source.display()
                    ),
                    incoming: format!("version={version}, path={}", source_root.display()),
                });
            }
        }

        Ok(())
    }

    /// 仅当旧锁指向本轮目标缓存且内容一致时复用文件；不会跳过子依赖遍历。
    fn should_reuse_locked_dependency(
        &self,
        dependency_name: &str,
        source_root: &Path,
        expected_store: &Path,
    ) -> Result<bool, FamaError> {
        let Some(dependency) = self.cached_dependencies.get(dependency_name) else {
            return Ok(false);
        };
        let Some(store_dir) = resolve_locked_store_dir(&self.project_root, dependency) else {
            return Ok(false);
        };
        if store_dir != expected_store || !is_dependency_store_usable(&store_dir) {
            return Ok(false);
        }

        is_dependency_store_fresh(source_root, &store_dir)
    }
}

/// 复制依赖项目到平铺缓存目录，并显式忽略依赖自身的缓存和构建输出目录。
fn copy_dependency_project(source_root: &Path, target_root: &Path) -> Result<(), FamaError> {
    if let Some(parent) = target_root.parent() {
        fs::create_dir_all(parent).map_err(|source| FamaError::io(parent.to_path_buf(), source))?;
    }

    fs::create_dir_all(target_root)
        .map_err(|source| FamaError::io(target_root.to_path_buf(), source))?;
    copy_directory_recursive(source_root, target_root)
}

/// 递归复制依赖项目目录树，并过滤掉不应该进入缓存的目录。
fn copy_directory_recursive(source_root: &Path, target_root: &Path) -> Result<(), FamaError> {
    for entry in fs::read_dir(source_root)
        .map_err(|source| FamaError::io(source_root.to_path_buf(), source))?
    {
        let entry = entry.map_err(|source| FamaError::io(source_root.to_path_buf(), source))?;
        let source_path = entry.path();
        let target_path = target_root.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|source| FamaError::io(source_path.clone(), source))?;

        if should_skip_dependency_entry(&source_path) {
            continue;
        }

        if file_type.is_dir() {
            fs::create_dir_all(&target_path)
                .map_err(|source| FamaError::io(target_path.clone(), source))?;
            copy_directory_recursive(&source_path, &target_path)?;
            continue;
        }

        fs::copy(&source_path, &target_path)
            .map_err(|source| FamaError::io(target_path.clone(), source))?;
    }

    Ok(())
}

/// 根据锁文件中的 `store_path` 还原依赖缓存目录的绝对路径，缺失时回退到固定平铺规则。
fn resolve_locked_store_dir(project_root: &Path, dependency: &LockedDependency) -> Option<PathBuf> {
    let trimmed = dependency.store_path.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(project_root.join(trimmed.replace('/', "\\")))
}

/// 检查缓存目录是否包含项目设置与源码目录，避免复用不完整的副本。
fn is_dependency_store_usable(store_dir: &Path) -> bool {
    store_dir.is_dir()
        && store_dir.join("settings.toml").is_file()
        && store_dir.join("src").is_dir()
}

/// 比较源依赖目录与缓存目录是否仍然一致，避免路径依赖更新后继续复用陈旧缓存。
fn is_dependency_store_fresh(source_root: &Path, store_dir: &Path) -> Result<bool, FamaError> {
    compare_dependency_directories(source_root, store_dir)
}

/// 递归比较两个依赖目录的有效内容，忽略 `build/`、`target/` 等不应进入缓存的目录。
fn compare_dependency_directories(
    source_root: &Path,
    store_root: &Path,
) -> Result<bool, FamaError> {
    let source_entries = list_dependency_entries(source_root)?;
    let store_entries = list_dependency_entries(store_root)?;
    if source_entries.len() != store_entries.len() {
        return Ok(false);
    }

    for (source_entry, store_entry) in source_entries.iter().zip(store_entries.iter()) {
        if source_entry.name != store_entry.name
            || source_entry.is_dir != store_entry.is_dir
            || source_entry.is_file != store_entry.is_file
        {
            return Ok(false);
        }

        if source_entry.is_dir {
            if !compare_dependency_directories(&source_entry.path, &store_entry.path)? {
                return Ok(false);
            }
            continue;
        }

        if !source_entry.is_file {
            return Ok(false);
        }

        if !files_have_same_content(&source_entry.path, &store_entry.path)? {
            return Ok(false);
        }
    }

    Ok(true)
}

/// 读取单层目录下所有应参与缓存比较的条目，并按名称排序保证遍历顺序稳定。
fn list_dependency_entries(root: &Path) -> Result<Vec<DependencyEntrySnapshot>, FamaError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root).map_err(|source| FamaError::io(root.to_path_buf(), source))? {
        let entry = entry.map_err(|source| FamaError::io(root.to_path_buf(), source))?;
        let path = entry.path();
        if should_skip_dependency_entry(&path) {
            continue;
        }

        let file_type = entry
            .file_type()
            .map_err(|source| FamaError::io(path.clone(), source))?;
        entries.push(DependencyEntrySnapshot {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
            is_dir: file_type.is_dir(),
            is_file: file_type.is_file(),
        });
    }

    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

/// 通过文件大小和字节内容双重比较两个文件，确保缓存命中时不会漏掉内容变化。
fn files_have_same_content(left: &Path, right: &Path) -> Result<bool, FamaError> {
    let left_meta =
        fs::metadata(left).map_err(|source| FamaError::io(left.to_path_buf(), source))?;
    let right_meta =
        fs::metadata(right).map_err(|source| FamaError::io(right.to_path_buf(), source))?;
    if left_meta.len() != right_meta.len() {
        return Ok(false);
    }

    let left_bytes = fs::read(left).map_err(|source| FamaError::io(left.to_path_buf(), source))?;
    let right_bytes =
        fs::read(right).map_err(|source| FamaError::io(right.to_path_buf(), source))?;
    Ok(left_bytes == right_bytes)
}

/// 保存目录比较时最小必需的条目快照，避免在递归过程里反复读取同一元信息。
struct DependencyEntrySnapshot {
    name: String,
    path: PathBuf,
    is_dir: bool,
    is_file: bool,
}

/// 检查现有缓存根和目标是否仍位于项目下的预期位置，防止链接重定向缓存清理。
fn ensure_dependency_store_is_local(
    project_root: &Path,
    store_dir: &Path,
) -> Result<(), FamaError> {
    let root = fs::canonicalize(project_root)
        .map_err(|error| FamaError::io(project_root.to_path_buf(), error))?;
    let cache = root.join("mod_module");
    let expected = cache.join(store_dir.file_name().expect("缓存目录必须包含包名"));
    for (path, expected) in [
        (project_root.join("mod_module"), cache),
        (store_dir.to_path_buf(), expected),
    ] {
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                let actual =
                    fs::canonicalize(&path).map_err(|error| FamaError::io(path.clone(), error))?;
                if metadata.is_symlink() || actual != expected {
                    return Err(FamaError::DependencyInstall(format!(
                        "依赖缓存路径被重定向：{}",
                        path.display()
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(FamaError::io(path, error)),
        }
    }
    Ok(())
}

/// 在重新复制依赖前清理残缺缓存目录，保证目标目录处于干净状态。
fn prepare_dependency_store(store_dir: &Path) -> Result<(), FamaError> {
    if store_dir.exists() {
        if store_dir.is_dir() {
            fs::remove_dir_all(store_dir)
                .map_err(|source| FamaError::io(store_dir.to_path_buf(), source))?;
        } else {
            fs::remove_file(store_dir)
                .map_err(|source| FamaError::io(store_dir.to_path_buf(), source))?;
        }
    }

    Ok(())
}

/// 判断缓存复制时是否需要跳过目标目录，避免把旧缓存或构建产物递归带入。
fn should_skip_dependency_entry(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("mod_module" | "build" | ".git" | "target")
    )
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
