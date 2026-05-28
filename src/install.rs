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
    locked_dependencies: BTreeMap<String, LockedDependency>,
    source_paths: BTreeMap<String, PathBuf>,
    active_stack: HashSet<PathBuf>,
}

impl Installer {
    /// 为单次安装流程创建新的安装器状态对象。
    fn new(project_root: &Path, existing_lock_file: LockFile) -> Self {
        let source_paths = existing_lock_file
            .dependencies
            .iter()
            .map(|(name, dependency)| {
                (
                    name.clone(),
                    project_root.join(dependency.source.path.replace('/', "\\")),
                )
            })
            .collect();
        Self {
            project_root: project_root.to_path_buf(),
            locked_dependencies: existing_lock_file.dependencies,
            source_paths,
            active_stack: HashSet::new(),
        }
    }

    /// 递归安装单个依赖，并同步安装其传递依赖。
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

        if self.should_reuse_locked_dependency(dependency_name, &source_root)? {
            return Ok(());
        }

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
        prepare_dependency_store(&store_dir)?;
        copy_dependency_project(&source_root, &store_dir)?;

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

    /// 判断锁文件中的依赖是否可以直接复用，只有缓存存在且内容仍与源依赖一致时才允许短路。
    fn should_reuse_locked_dependency(
        &self,
        dependency_name: &str,
        source_root: &Path,
    ) -> Result<bool, FamaError> {
        let Some(dependency) = self.locked_dependencies.get(dependency_name) else {
            return Ok(false);
        };
        let Some(store_dir) = resolve_locked_store_dir(&self.project_root, dependency) else {
            return Ok(false);
        };
        if !is_dependency_store_usable(&store_dir) {
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

/// 判断依赖缓存目录是否包含最小可用结构，避免复用空目录或半拷贝目录。
fn is_dependency_store_usable(store_dir: &Path) -> bool {
    store_dir.is_dir() && store_dir.join("mod.yml").is_file() && store_dir.join("src").is_dir()
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

/// 在重新复制依赖前清理残缺缓存目录，保证目标目录处于干净状态。
fn prepare_dependency_store(store_dir: &Path) -> Result<(), FamaError> {
    if is_dependency_store_usable(store_dir) {
        fs::remove_dir_all(store_dir)
            .map_err(|source| FamaError::io(store_dir.to_path_buf(), source))?;
        return Ok(());
    }

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
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::manifest::{DependencySpec, ProjectManifest, save_project_manifest};

    use super::install_project_dependencies;

    /// 生成唯一临时目录，避免安装测试的缓存目录彼此污染。
    fn create_temp_dir(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间异常")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("fama-install-{name}-{unique}"));
        fs::create_dir_all(&path).expect("创建临时目录失败");
        path
    }

    /// 创建带最小 `src/main.ms` 的测试项目，并写入依赖清单。
    fn create_project(root: &std::path::Path, name: &str, version: &str) {
        fs::create_dir_all(root.join("src")).expect("创建 src 目录失败");
        fs::write(root.join("src").join("main.ms"), "print(\"hello\")\n").expect("写入源码失败");
        save_project_manifest(
            root,
            &ProjectManifest {
                name: name.to_string(),
                version: version.to_string(),
                dependencies: std::collections::BTreeMap::new(),
            },
        )
        .expect("写入项目清单失败");
    }

    /// 校验本地路径依赖会被安装到 `mod_module/<name>@<version>` 平铺缓存目录。
    #[test]
    fn should_install_local_dependency_into_flat_store() {
        let workspace = create_temp_dir("flat-store");
        let app_root = workspace.join("app");
        let lib_root = workspace.join("math");
        create_project(&app_root, "app", "0.1.0");
        create_project(&lib_root, "math", "0.1.0");

        save_project_manifest(
            &app_root,
            &ProjectManifest {
                name: "app".to_string(),
                version: "0.1.0".to_string(),
                dependencies: std::collections::BTreeMap::from([(
                    "math".to_string(),
                    DependencySpec {
                        path: "../math".to_string(),
                        version: "0.1.0".to_string(),
                    },
                )]),
            },
        )
        .expect("更新应用依赖失败");

        let lock_file = install_project_dependencies(&app_root).unwrap();
        assert!(app_root.join("mod_module").join("math@0.1.0").is_dir());
        assert_eq!(
            lock_file.dependencies["math"].store_path,
            "mod_module/math@0.1.0"
        );
    }

    /// 校验循环依赖会被明确拒绝，避免安装过程陷入无限递归。
    #[test]
    fn should_reject_cyclic_local_dependencies() {
        let workspace = create_temp_dir("cycle");
        let app_root = workspace.join("app");
        let alpha_root = workspace.join("alpha");
        let beta_root = workspace.join("beta");
        create_project(&app_root, "app", "0.1.0");
        create_project(&alpha_root, "alpha", "0.1.0");
        create_project(&beta_root, "beta", "0.1.0");

        save_project_manifest(
            &app_root,
            &ProjectManifest {
                name: "app".to_string(),
                version: "0.1.0".to_string(),
                dependencies: std::collections::BTreeMap::from([(
                    "alpha".to_string(),
                    DependencySpec {
                        path: "../alpha".to_string(),
                        version: "0.1.0".to_string(),
                    },
                )]),
            },
        )
        .expect("更新应用依赖失败");

        save_project_manifest(
            &alpha_root,
            &ProjectManifest {
                name: "alpha".to_string(),
                version: "0.1.0".to_string(),
                dependencies: std::collections::BTreeMap::from([(
                    "beta".to_string(),
                    DependencySpec {
                        path: "../beta".to_string(),
                        version: "0.1.0".to_string(),
                    },
                )]),
            },
        )
        .expect("更新 alpha 依赖失败");

        save_project_manifest(
            &beta_root,
            &ProjectManifest {
                name: "beta".to_string(),
                version: "0.1.0".to_string(),
                dependencies: std::collections::BTreeMap::from([(
                    "alpha".to_string(),
                    DependencySpec {
                        path: "../alpha".to_string(),
                        version: "0.1.0".to_string(),
                    },
                )]),
            },
        )
        .expect("更新 beta 依赖失败");

        let error = install_project_dependencies(&app_root).unwrap_err();
        assert!(error.to_string().contains("循环依赖"));
    }

    /// 校验锁文件命中但缓存目录缺失时，安装流程会重新复制依赖而不是被旧状态短路。
    #[test]
    fn should_reinstall_when_lock_file_exists_but_store_dir_is_missing() {
        let workspace = create_temp_dir("repair-missing-store");
        let app_root = workspace.join("app");
        let lib_root = workspace.join("time");
        create_project(&app_root, "app", "0.1.0");
        create_project(&lib_root, "time", "0.1.0");

        save_project_manifest(
            &app_root,
            &ProjectManifest {
                name: "app".to_string(),
                version: "0.1.0".to_string(),
                dependencies: std::collections::BTreeMap::from([(
                    "time".to_string(),
                    DependencySpec {
                        path: "../time".to_string(),
                        version: "0.1.0".to_string(),
                    },
                )]),
            },
        )
        .expect("更新应用依赖失败");

        install_project_dependencies(&app_root).expect("首次安装依赖失败");
        let store_dir = app_root.join("mod_module").join("time@0.1.0");
        fs::remove_dir_all(&store_dir).expect("删除缓存目录失败");

        install_project_dependencies(&app_root).expect("补装依赖失败");

        assert!(store_dir.is_dir());
        assert!(store_dir.join("mod.yml").is_file());
        assert!(store_dir.join("src").is_dir());
    }

    /// 校验依赖缓存复制会保留包内稳定预编译目录，而不会把 `native/artifacts` 当成构建垃圾跳过。
    #[test]
    fn should_keep_native_artifacts_when_installing_dependency() {
        let workspace = create_temp_dir("native-artifacts");
        let app_root = workspace.join("app");
        let lib_root = workspace.join("time");
        create_project(&app_root, "app", "0.1.0");
        create_project(&lib_root, "time", "0.1.0");
        let artifact_root = lib_root
            .join("native")
            .join("artifacts")
            .join("x86_64-pc-windows-msvc");
        fs::create_dir_all(&artifact_root).expect("创建预编译目录失败");
        fs::write(artifact_root.join("manifest.toml"), "target = \"x\"\n")
            .expect("写入预编译清单失败");
        fs::write(artifact_root.join("time_common.obj"), [1u8, 2, 3]).expect("写入预编译对象失败");

        save_project_manifest(
            &app_root,
            &ProjectManifest {
                name: "app".to_string(),
                version: "0.1.0".to_string(),
                dependencies: std::collections::BTreeMap::from([(
                    "time".to_string(),
                    DependencySpec {
                        path: "../time".to_string(),
                        version: "0.1.0".to_string(),
                    },
                )]),
            },
        )
        .expect("更新应用依赖失败");

        install_project_dependencies(&app_root).expect("安装依赖失败");

        let installed_artifact_root = app_root
            .join("mod_module")
            .join("time@0.1.0")
            .join("native")
            .join("artifacts")
            .join("x86_64-pc-windows-msvc");
        assert!(installed_artifact_root.join("manifest.toml").is_file());
        assert!(installed_artifact_root.join("time_common.obj").is_file());
    }

    /// 校验路径依赖源内容发生变化后，重复安装会刷新缓存而不是继续复用旧副本。
    #[test]
    fn should_refresh_store_when_dependency_source_changes() {
        let workspace = create_temp_dir("refresh-store-on-source-change");
        let app_root = workspace.join("app");
        let lib_root = workspace.join("time");
        create_project(&app_root, "app", "0.1.0");
        create_project(&lib_root, "time", "0.1.0");

        save_project_manifest(
            &app_root,
            &ProjectManifest {
                name: "app".to_string(),
                version: "0.1.0".to_string(),
                dependencies: std::collections::BTreeMap::from([(
                    "time".to_string(),
                    DependencySpec {
                        path: "../time".to_string(),
                        version: "0.1.0".to_string(),
                    },
                )]),
            },
        )
        .expect("更新应用依赖失败");

        install_project_dependencies(&app_root).expect("首次安装依赖失败");

        let artifact_root = lib_root
            .join("native")
            .join("artifacts")
            .join("x86_64-pc-windows-msvc");
        fs::create_dir_all(&artifact_root).expect("创建新增预编译目录失败");
        fs::write(
            lib_root.join("settings.toml"),
            "[native.prebuilt.x86_64-pc-windows-msvc]\nmanifest = \"native/artifacts/x86_64-pc-windows-msvc/manifest.toml\"\nobjects = [\"native/artifacts/x86_64-pc-windows-msvc/time_common.obj\"]\n",
        )
        .expect("写入 settings.toml 失败");
        fs::write(artifact_root.join("manifest.toml"), "target = \"x\"\n")
            .expect("写入新增预编译清单失败");
        fs::write(artifact_root.join("time_common.obj"), [9u8, 8, 7])
            .expect("写入新增预编译对象失败");

        install_project_dependencies(&app_root).expect("刷新依赖缓存失败");

        let installed_root = app_root.join("mod_module").join("time@0.1.0");
        assert!(installed_root.join("settings.toml").is_file());
        assert!(
            installed_root
                .join("native")
                .join("artifacts")
                .join("x86_64-pc-windows-msvc")
                .join("manifest.toml")
                .is_file()
        );
        assert_eq!(
            fs::read(
                installed_root
                    .join("native")
                    .join("artifacts")
                    .join("x86_64-pc-windows-msvc")
                    .join("time_common.obj")
            )
            .expect("读取缓存对象文件失败"),
            vec![9u8, 8, 7]
        );
    }
}
