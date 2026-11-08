use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

use crate::error::FamaError;

/// 保存项目 `settings.toml` 的核心结构，供命令层统一读写依赖清单。
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProjectManifest {
    pub name: String,
    pub version: String,
    pub dependencies: BTreeMap<String, DependencySpec>,
}

/// 保存单个依赖的来源路径与期望版本。
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub struct DependencySpec {
    pub path: String,
    pub version: String,
}

/// 保存 `mod-lock.yml` 顶层结构，集中记录已解析的安装结果。
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct LockFile {
    pub dependencies: BTreeMap<String, LockedDependency>,
}

/// 保存锁文件中单个依赖的固定版本、来源和缓存目录。
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LockedDependency {
    pub version: String,
    pub source: LockedDependencySource,
    pub store_path: String,
}

/// 保存锁文件中单个依赖的来源信息，当前阶段只支持路径源。
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LockedDependencySource {
    pub source_type: String,
    pub path: String,
}

/// 从项目根目录读取并解析 `settings.toml`。
pub fn load_project_manifest(project_root: &Path) -> Result<ProjectManifest, FamaError> {
    let path = manifest_path(project_root);
    let text = read_required_utf8_file(&path)?;
    let manifest = parse_project_manifest(&path, &text)?;

    validate_manifest(&manifest, &path)?;
    Ok(manifest)
}

/// 更新项目 TOML 的依赖表，保留构建、原生配置和未修改条目的注释。
pub fn save_project_manifest(
    project_root: &Path,
    manifest: &ProjectManifest,
) -> Result<(), FamaError> {
    let path = manifest_path(project_root);
    validate_manifest(manifest, &path)?;
    let mut document = match fs::read_to_string(&path) {
        Ok(text) => text
            .parse::<DocumentMut>()
            .map_err(|error| invalid_manifest(&path, error.to_string()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
        Err(error) => return Err(FamaError::io(path, error)),
    };
    let project = document
        .entry("project")
        .or_insert(Item::Table(Table::new()));
    let project = project
        .as_table_like_mut()
        .ok_or_else(|| invalid_manifest(&path, "project 必须是 TOML 表"))?;
    for (field, value) in [("name", &manifest.name), ("version", &manifest.version)] {
        if project.get(field).and_then(Item::as_str) != Some(value.as_str()) {
            project.insert(field, Item::Value(Value::from(value)));
        }
    }
    if !manifest.dependencies.is_empty() || document.contains_key("dependencies") {
        let dependencies = document
            .entry("dependencies")
            .or_insert(Item::Table(Table::new()));
        let dependencies = dependencies
            .as_table_like_mut()
            .ok_or_else(|| invalid_manifest(&path, "dependencies 必须是 TOML 表"))?;
        let removed: Vec<_> = dependencies
            .iter()
            .filter(|(name, _)| !manifest.dependencies.contains_key(*name))
            .map(|(name, _)| name.to_string())
            .collect();
        for name in removed {
            dependencies.remove(&name);
        }
        for (name, dependency) in &manifest.dependencies {
            if let Some(item) = dependencies.get(name) {
                if item.get("path").and_then(Item::as_str) == Some(dependency.path.as_str())
                    && item.get("version").and_then(Item::as_str)
                        == Some(dependency.version.as_str())
                {
                    continue;
                }
            }
            let mut value = InlineTable::new();
            value.insert("path", Value::from(&dependency.path));
            value.insert("version", Value::from(&dependency.version));
            dependencies.insert(name, Item::Value(Value::InlineTable(value)));
        }
    }
    write_utf8_file(&path, &document.to_string())
}

/// 从项目根目录读取并解析 `mod-lock.yml`，缺失时返回空锁文件。
pub fn load_lock_file(project_root: &Path) -> Result<LockFile, FamaError> {
    let path = lock_file_path(project_root);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LockFile::default());
        }
        Err(error) => return Err(FamaError::io(path, error)),
    };

    parse_lock_file(&lock_file_path(project_root), &text)
}

/// 把锁文件内容以 UTF-8 无 BOM 形式写回 `mod-lock.yml`。
pub fn save_lock_file(project_root: &Path, lock_file: &LockFile) -> Result<(), FamaError> {
    let path = lock_file_path(project_root);
    let content = render_lock_file(lock_file);
    write_utf8_file(&path, &content)
}

/// 返回项目 `settings.toml` 的标准路径。
pub fn manifest_path(project_root: &Path) -> PathBuf {
    project_root.join("settings.toml")
}

/// 返回项目 `mod-lock.yml` 的标准路径。
pub fn lock_file_path(project_root: &Path) -> PathBuf {
    project_root.join("mod-lock.yml")
}

/// 返回指定依赖在 `mod_module/` 下的平铺缓存目录路径。
pub fn dependency_store_dir(project_root: &Path, name: &str, version: &str) -> PathBuf {
    project_root
        .join("mod_module")
        .join(format!("{name}@{version}"))
}

/// 把依赖源路径转换为尽量稳定的相对路径字符串，便于写入清单和锁文件。
pub fn normalize_dependency_path(base_root: &Path, target_root: &Path) -> String {
    diff_paths(target_root, base_root)
        .unwrap_or_else(|| target_root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

/// 将用户输入的依赖路径解析为绝对目录路径，并要求目标必须存在。
pub fn resolve_dependency_path(base_root: &Path, raw_path: &Path) -> Result<PathBuf, FamaError> {
    let candidate = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        base_root.join(raw_path)
    };
    if !candidate.exists() {
        return Err(FamaError::MissingFile(candidate));
    }

    fs::canonicalize(&candidate).map_err(|source| FamaError::io(candidate, source))
}

/// 从项目 TOML 读取身份和依赖，构建及原生配置由各自的读取方解析。
fn parse_project_manifest(path: &Path, text: &str) -> Result<ProjectManifest, FamaError> {
    #[derive(Deserialize)]
    struct Settings {
        project: Identity,
        #[serde(default)]
        dependencies: BTreeMap<String, DependencySpec>,
    }

    #[derive(Deserialize)]
    struct Identity {
        name: String,
        version: String,
    }

    let settings: Settings =
        toml::from_str(text).map_err(|error| invalid_manifest(path, error.to_string()))?;
    Ok(ProjectManifest {
        name: settings.project.name,
        version: settings.project.version,
        dependencies: settings.dependencies,
    })
}

/// 解析 `mod-lock.yml` 文本，只覆盖当前阶段写出的锁文件结构。
fn parse_lock_file(path: &Path, text: &str) -> Result<LockFile, FamaError> {
    if text.trim().is_empty() || text.trim() == "{}" {
        return Ok(LockFile::default());
    }

    let mut dependencies: BTreeMap<String, LockedDependency> = BTreeMap::new();
    let mut current_dependency = None::<String>;
    let mut in_source_block = false;

    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed == "dependencies:" || trimmed == "dependencies: {}" {
            continue;
        }

        if raw_line.starts_with("  ") && line.ends_with(':') && !raw_line.starts_with("    ") {
            let dependency_name = line.trim_end_matches(':').trim().to_string();
            dependencies.insert(
                dependency_name.clone(),
                LockedDependency {
                    version: String::new(),
                    source: LockedDependencySource {
                        source_type: String::new(),
                        path: String::new(),
                    },
                    store_path: String::new(),
                },
            );
            current_dependency = Some(dependency_name);
            in_source_block = false;
            continue;
        }

        if trimmed == "source:" {
            current_dependency
                .as_ref()
                .ok_or_else(|| invalid_lock_file(path, "source 块缺少所属依赖"))?;
            in_source_block = true;
            continue;
        }

        let dependency_name = current_dependency
            .as_ref()
            .ok_or_else(|| invalid_lock_file(path, "锁文件字段缺少所属依赖"))?;
        let dependency = dependencies
            .get_mut(dependency_name)
            .ok_or_else(|| invalid_lock_file(path, "锁文件字段缺少初始化节点"))?;

        if let Some(rest) = trimmed.strip_prefix("version:") {
            dependency.version = rest.trim().to_string();
            in_source_block = false;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("store_path:") {
            dependency.store_path = rest.trim().to_string();
            in_source_block = false;
            continue;
        }

        if in_source_block {
            if let Some(rest) = trimmed.strip_prefix("type:") {
                dependency.source.source_type = rest.trim().to_string();
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("path:") {
                dependency.source.path = rest.trim().to_string();
                continue;
            }
        }

        return Err(invalid_lock_file(
            path,
            format!("无法解析的 mod-lock.yml 行：{line}"),
        ));
    }

    Ok(LockFile { dependencies })
}

/// 把锁文件渲染为稳定、可读的 `mod-lock.yml` 文本。
fn render_lock_file(lock_file: &LockFile) -> String {
    if lock_file.dependencies.is_empty() {
        return "dependencies: {}\n".to_string();
    }

    let mut output = String::from("dependencies:\n");
    for (dependency_name, dependency) in &lock_file.dependencies {
        output.push_str(&format!("  {dependency_name}:\n"));
        output.push_str(&format!("    version: {}\n", dependency.version));
        output.push_str("    source:\n");
        output.push_str(&format!("      type: {}\n", dependency.source.source_type));
        output.push_str(&format!("      path: {}\n", dependency.source.path));
        output.push_str(&format!("    store_path: {}\n", dependency.store_path));
    }
    output
}

/// 校验项目身份、依赖字段和缓存路径成分，失败时返回带清单路径的错误。
fn validate_manifest(manifest: &ProjectManifest, path: &Path) -> Result<(), FamaError> {
    if manifest.name.trim().is_empty() {
        return Err(FamaError::InvalidManifest {
            path: path.to_path_buf(),
            message: "name 不能为空".to_string(),
        });
    }

    if manifest.version.trim().is_empty() {
        return Err(FamaError::InvalidManifest {
            path: path.to_path_buf(),
            message: "version 不能为空".to_string(),
        });
    }

    for (dependency_name, dependency) in &manifest.dependencies {
        if !maslang::settings::is_dependency_cache_component(dependency_name)
            || !maslang::settings::is_dependency_cache_component(&dependency.version)
        {
            return Err(FamaError::InvalidManifest {
                path: path.to_path_buf(),
                message: "依赖名称或版本含有不安全的缓存路径字符".to_string(),
            });
        }

        if dependency.path.trim().is_empty() {
            return Err(FamaError::InvalidManifest {
                path: path.to_path_buf(),
                message: format!("依赖 `{dependency_name}` 缺少 path"),
            });
        }
    }

    Ok(())
}

/// 读取必需的 UTF-8 文本文件，缺失时保留路径上下文信息。
fn read_required_utf8_file(path: &Path) -> Result<String, FamaError> {
    fs::read_to_string(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            FamaError::MissingFile(path.to_path_buf())
        } else {
            FamaError::io(path.to_path_buf(), source)
        }
    })
}

/// 以 UTF-8 无 BOM 形式写入文本文件，统一控制编码输出。
fn write_utf8_file(path: &Path, content: &str) -> Result<(), FamaError> {
    fs::write(path, content.as_bytes()).map_err(|source| FamaError::io(path.to_path_buf(), source))
}

/// 构造带路径上下文的 `settings.toml` 解析错误，避免重复拼接错误消息。
fn invalid_manifest(path: &Path, message: impl Into<String>) -> FamaError {
    FamaError::InvalidManifest {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

/// 构造带路径上下文的 `mod-lock.yml` 解析错误，避免重复拼接错误消息。
fn invalid_lock_file(path: &Path, message: impl Into<String>) -> FamaError {
    FamaError::InvalidLockFile {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

/// 计算两个路径之间的相对路径，便于把依赖来源写成稳定的相对地址。
fn diff_paths(path: &Path, base: &Path) -> Option<PathBuf> {
    let path_components: Vec<_> = path.components().collect();
    let base_components: Vec<_> = base.components().collect();
    let common_len = path_components
        .iter()
        .zip(base_components.iter())
        .take_while(|(left, right)| left == right)
        .count();

    if common_len == 0
        && path_components
            .first()
            .is_some_and(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
        && base_components
            .first()
            .is_some_and(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
        && path_components.first() != base_components.first()
    {
        return None;
    }

    let mut result = PathBuf::new();
    for component in &base_components[common_len..] {
        if !matches!(component, Component::RootDir | Component::Prefix(_)) {
            result.push("..");
        }
    }
    for component in &path_components[common_len..] {
        result.push(component.as_os_str());
    }

    if result.as_os_str().is_empty() {
        result.push(".");
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        DependencySpec, LockFile, LockedDependency, LockedDependencySource, ProjectManifest,
        dependency_store_dir, normalize_dependency_path, save_lock_file, save_project_manifest,
    };

    /// 创建唯一的临时项目目录，避免清单和锁文件测试互相覆盖。
    fn create_temp_dir(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时间异常")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("fama-manifest-{name}-{unique}"));
        fs::create_dir_all(&path).expect("创建临时目录失败");
        path
    }

    /// 校验项目依赖清单可以被成功写入，并且不会携带 UTF-8 BOM。
    #[test]
    fn should_write_manifest_without_bom() {
        let root = create_temp_dir("manifest");
        let manifest = ProjectManifest {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            dependencies: std::collections::BTreeMap::from([(
                "math".to_string(),
                DependencySpec {
                    path: "../math".to_string(),
                    version: "0.1.0".to_string(),
                },
            )]),
        };

        save_project_manifest(&root, &manifest).unwrap();

        let bytes = fs::read(root.join("settings.toml")).unwrap();
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    }

    /// 校验锁文件可以写出平铺缓存路径和路径源元数据。
    #[test]
    fn should_write_lock_file_with_store_path() {
        let root = create_temp_dir("lock");
        let lock_file = LockFile {
            dependencies: std::collections::BTreeMap::from([(
                "math".to_string(),
                LockedDependency {
                    version: "0.1.0".to_string(),
                    source: LockedDependencySource {
                        source_type: "path".to_string(),
                        path: "../math".to_string(),
                    },
                    store_path: "mod_module/math@0.1.0".to_string(),
                },
            )]),
        };

        save_lock_file(&root, &lock_file).unwrap();
        let text = fs::read_to_string(root.join("mod-lock.yml")).unwrap();
        assert!(text.contains("mod_module/math@0.1.0"));
    }

    /// 校验依赖缓存目录采用 `mod_module/<name>@<version>` 的平铺规则。
    #[test]
    fn should_build_flat_store_directory_path() {
        let root = create_temp_dir("store");
        let path = dependency_store_dir(&root, "math", "0.1.0");
        assert!(path.ends_with("mod_module\\math@0.1.0"));
    }

    /// 校验依赖来源路径会优先写成相对于当前项目根目录的相对路径。
    #[test]
    fn should_normalize_dependency_path_to_relative_path() {
        let base = std::path::Path::new("E:/workspace/app");
        let dependency = std::path::Path::new("E:/workspace/libs/math");
        let relative = normalize_dependency_path(base, dependency);
        assert_eq!(relative, "../libs/math");
    }
}
