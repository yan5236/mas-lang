use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diagnostic::{CompilerError, MessageId};
use crate::source::{SourceFile, Span};

pub const DEFAULT_TARGET_TRIPLE: &str = "x86_64-pc-windows-msvc";

/// 按目标平台解析模块图中所有包的原生链接输入，默认优先消费包内预编译产物。
pub fn resolve_native_link_inputs(
    package_roots: &[PathBuf],
    native_root: &Path,
    source: &SourceFile,
    allow_native_source_build: bool,
) -> Result<Vec<PathBuf>, CompilerError> {
    let mut objects = Vec::new();

    for package_root in package_roots {
        let Some(config) = load_package_native_config(package_root, source)? else {
            continue;
        };
        let package_objects = resolve_package_native_inputs(
            package_root,
            &config,
            native_root,
            source,
            allow_native_source_build,
        )?;
        objects.extend(package_objects);
    }

    Ok(objects)
}

/// 保存单个包解析后的原生输入配置，供链接阶段统一选择预编译或源码刷新路径。
#[derive(Debug)]
struct PackageNativeConfig {
    package_name: String,
    package_version: String,
    abi: u32,
    source_fallback: NativeSourceFallback,
    include_dirs: Vec<PathBuf>,
    source_files: Vec<PathBuf>,
    prebuilt: BTreeMap<String, NativePrebuiltTargetConfig>,
}

/// 表示 `settings.toml` 中与包级元信息及原生输入相关的最小字段集合。
#[derive(Debug, Default, Deserialize)]
struct PackageSettingsFile {
    project: Option<ProjectSection>,
    native: Option<NativeSection>,
}

/// 保存 `settings.toml` 里的项目名和版本，供预编译元数据校验复用。
#[derive(Debug, Default, Deserialize)]
struct ProjectSection {
    name: Option<String>,
    version: Option<String>,
}

/// 保存单个包的原生包含目录、源码列表和预编译矩阵声明。
#[derive(Debug, Default, Deserialize)]
struct NativeSection {
    #[serde(default = "default_native_abi")]
    abi: u32,
    #[serde(default = "default_source_fallback")]
    source_fallback: String,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    sources: NativeSourcesSection,
    #[serde(default)]
    prebuilt: BTreeMap<String, NativePrebuiltTargetSection>,
}

/// 保存公共源码与平台专属源码列表，供不同目标平台筛选使用。
#[derive(Debug, Default, Deserialize)]
struct NativeSourcesSection {
    #[serde(default)]
    common: Vec<String>,
    #[serde(default)]
    windows: Vec<String>,
    #[serde(default)]
    posix: Vec<String>,
}

/// 保存单个目标平台的预编译产物入口，路径统一相对包根目录解析。
#[derive(Debug, Default, Deserialize)]
struct NativePrebuiltTargetSection {
    manifest: String,
    #[serde(default)]
    objects: Vec<String>,
}

/// 保存单个目标平台在内存中解析后的预编译路径集合，避免后续重复拼接路径。
#[derive(Debug)]
struct NativePrebuiltTargetConfig {
    manifest_path: PathBuf,
    object_paths: Vec<PathBuf>,
    artifact_dir: PathBuf,
}

/// 表示普通消费路径是否允许显式回退到源码编译，仅维护者模式可以触发该能力。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeSourceFallback {
    Never,
    Explicit,
}

/// 表示预编译产物旁路元数据文件的最小字段集合，用于链接前一致性校验。
#[derive(Debug, Deserialize, Serialize)]
struct NativeArtifactManifest {
    package: String,
    package_version: String,
    target: String,
    abi: u32,
    source_hash: String,
    header_hash: String,
    objects: Vec<String>,
}

/// 为缺省 ABI 版本提供统一入口，避免配置缺省时散落魔法值。
fn default_native_abi() -> u32 {
    1
}

/// 为原生源码回退策略提供统一默认值，保持“仅显式维护者模式才允许源码构建”。
fn default_source_fallback() -> String {
    "explicit".to_string()
}

/// 读取包级 `settings.toml`，并把原生声明转换成绝对路径配置对象。
fn load_package_native_config(
    package_root: &Path,
    source: &SourceFile,
) -> Result<Option<PackageNativeConfig>, CompilerError> {
    let settings_path = package_root.join("settings.toml");
    let text = match fs::read_to_string(&settings_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(build_native_error(
                source,
                format!("读取包配置失败：{}: {}", settings_path.display(), error),
            ));
        }
    };

    let parsed: PackageSettingsFile = toml::from_str(&text).map_err(|error| {
        build_native_error(
            source,
            format!("解析包配置失败：{}: {}", settings_path.display(), error),
        )
    })?;
    let Some(native) = parsed.native else {
        return Ok(None);
    };

    let source_fallback = parse_source_fallback(&native.source_fallback, package_root, source)?;
    let package_name = parsed
        .project
        .as_ref()
        .and_then(|project| project.name.clone())
        .unwrap_or_else(|| {
            package_root
                .file_name()
                .and_then(|item| item.to_str())
                .unwrap_or("unknown-package")
                .to_string()
        });
    let package_version = parsed
        .project
        .as_ref()
        .and_then(|project| project.version.clone())
        .unwrap_or_else(|| "0.0.0".to_string());

    let mut source_files = native
        .sources
        .common
        .iter()
        .map(|item| resolve_native_path(package_root, item, source))
        .collect::<Result<Vec<_>, _>>()?;
    let platform_sources = if DEFAULT_TARGET_TRIPLE.contains("windows") {
        &native.sources.windows
    } else {
        &native.sources.posix
    };
    source_files.extend(
        platform_sources
            .iter()
            .map(|item| resolve_native_path(package_root, item, source))
            .collect::<Result<Vec<_>, _>>()?,
    );

    let mut prebuilt = BTreeMap::new();
    for (target, entry) in native.prebuilt {
        let manifest_path = normalize_windows_path(&package_root.join(&entry.manifest));
        let artifact_dir = manifest_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                build_native_error(
                    source,
                    format!("预编译清单路径缺少父目录：{}", manifest_path.display()),
                )
            })?;
        let object_paths = entry
            .objects
            .iter()
            .map(|item| normalize_windows_path(&package_root.join(item)))
            .collect::<Vec<_>>();
        prebuilt.insert(
            target,
            NativePrebuiltTargetConfig {
                manifest_path,
                object_paths,
                artifact_dir,
            },
        );
    }

    Ok(Some(PackageNativeConfig {
        package_name,
        package_version,
        abi: native.abi,
        source_fallback,
        include_dirs: native
            .include
            .iter()
            .map(|item| resolve_native_path(package_root, item, source))
            .collect::<Result<Vec<_>, _>>()?,
        source_files,
        prebuilt,
    }))
}

/// 在单个包内优先尝试预编译输入，必要时再通过维护者模式刷新稳定产物目录。
fn resolve_package_native_inputs(
    package_root: &Path,
    config: &PackageNativeConfig,
    native_root: &Path,
    source: &SourceFile,
    allow_native_source_build: bool,
) -> Result<Vec<PathBuf>, CompilerError> {
    if config.source_files.is_empty() && config.prebuilt.is_empty() {
        return Ok(Vec::new());
    }

    let expected_manifest_path = config
        .prebuilt
        .get(DEFAULT_TARGET_TRIPLE)
        .map(|entry| entry.manifest_path.clone())
        .unwrap_or_else(|| default_manifest_path(package_root));

    if let Some(prebuilt) = config.prebuilt.get(DEFAULT_TARGET_TRIPLE) {
        match validate_prebuilt_target(package_root, config, prebuilt, source) {
            Ok(objects) => {
                return Ok(objects);
            }
            Err(error) if !allow_native_source_build => return Err(error),
            Err(_) => {}
        }
    } else if !allow_native_source_build && !config.source_files.is_empty() {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "目标 `{}` 未声明预编译产物，缺失清单路径 `{}`；普通依赖消费不支持自动回退到源码编译。调用方请改用受支持目标或升级依赖；维护者请使用 `--allow-native-source-build` 刷新产物。",
                DEFAULT_TARGET_TRIPLE,
                expected_manifest_path.display()
            ),
        ));
    }

    if !allow_native_source_build {
        return Ok(Vec::new());
    }

    if config.source_files.is_empty() {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "目标 `{}` 没有可用预编译产物，且包内没有可用于维护者刷新的原生源码；缺失清单路径 `{}`。",
                DEFAULT_TARGET_TRIPLE,
                expected_manifest_path.display()
            ),
        ));
    }

    if config.source_fallback == NativeSourceFallback::Never {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            "当前包把 `source_fallback` 声明为 `never`，即使开启维护者模式也禁止源码刷新。"
                .to_string(),
        ));
    }

    let refreshed = refresh_native_prebuilt(package_root, config, native_root, source)?;
    eprintln!(
        "已刷新预编译原生产物：包 `{}`，目标 `{}`，目录 `{}`",
        config.package_name,
        DEFAULT_TARGET_TRIPLE,
        refreshed
            .first()
            .and_then(|item| item.parent())
            .unwrap_or_else(|| package_root)
            .display()
    );
    Ok(refreshed)
}

/// 校验当前目标平台命中的预编译清单、对象文件和摘要字段是否与包源码一致。
fn validate_prebuilt_target(
    package_root: &Path,
    config: &PackageNativeConfig,
    prebuilt: &NativePrebuiltTargetConfig,
    source: &SourceFile,
) -> Result<Vec<PathBuf>, CompilerError> {
    if prebuilt.object_paths.is_empty() {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "目标 `{}` 的预编译对象文件列表为空：{}",
                DEFAULT_TARGET_TRIPLE,
                prebuilt.manifest_path.display()
            ),
        ));
    }

    let manifest_text = fs::read_to_string(&prebuilt.manifest_path).map_err(|error| {
        build_consumer_error(
            source,
            &config.package_name,
            format!(
                "缺少预编译清单：{}，原因：{}；普通依赖消费不支持自动回退到源码编译。",
                prebuilt.manifest_path.display(),
                error
            ),
        )
    })?;
    let manifest: NativeArtifactManifest = toml::from_str(&manifest_text).map_err(|error| {
        build_consumer_error(
            source,
            &config.package_name,
            format!(
                "解析预编译清单失败：{}，原因：{}",
                prebuilt.manifest_path.display(),
                error
            ),
        )
    })?;

    if manifest.package != config.package_name {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "预编译清单中的包名为 `{}`，但当前包为 `{}`：{}",
                manifest.package,
                config.package_name,
                prebuilt.manifest_path.display()
            ),
        ));
    }
    if manifest.package_version != config.package_version {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "预编译清单中的包版本为 `{}`，但当前包版本为 `{}`：{}",
                manifest.package_version,
                config.package_version,
                prebuilt.manifest_path.display()
            ),
        ));
    }
    if manifest.target != DEFAULT_TARGET_TRIPLE {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "预编译清单声明的目标为 `{}`，当前目标为 `{}`：{}",
                manifest.target,
                DEFAULT_TARGET_TRIPLE,
                prebuilt.manifest_path.display()
            ),
        ));
    }
    if manifest.abi != config.abi {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "预编译清单 ABI 版本为 `{}`，但当前编译器期望 `{}`：{}；维护者请先刷新对应目标的预编译产物。",
                manifest.abi,
                config.abi,
                prebuilt.manifest_path.display()
            ),
        ));
    }

    let expected_source_hash = compute_fingerprint(package_root, &config.source_files, source)?;
    let header_files = collect_include_files(&config.include_dirs, source)?;
    let expected_header_hash = compute_fingerprint(package_root, &header_files, source)?;
    if manifest.source_hash != expected_source_hash {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "预编译源码指纹不一致：清单为 `{}`，当前源码为 `{}`；维护者请使用 `--allow-native-source-build` 刷新产物。",
                manifest.source_hash, expected_source_hash
            ),
        ));
    }
    if manifest.header_hash != expected_header_hash {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "预编译头文件指纹不一致：清单为 `{}`，当前头文件为 `{}`；维护者请使用 `--allow-native-source-build` 刷新产物。",
                manifest.header_hash, expected_header_hash
            ),
        ));
    }

    let configured_objects = prebuilt
        .object_paths
        .iter()
        .map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect::<Vec<_>>();
    if configured_objects != manifest.objects {
        return Err(build_consumer_error(
            source,
            &config.package_name,
            format!(
                "预编译对象文件清单不一致：settings 声明为 {:?}，manifest 声明为 {:?}。",
                configured_objects, manifest.objects
            ),
        ));
    }

    for object_path in &prebuilt.object_paths {
        if !object_path.is_file() {
            return Err(build_consumer_error(
                source,
                &config.package_name,
                format!(
                    "缺少预编译对象文件：{}；普通依赖消费不支持自动回退到源码编译。",
                    object_path.display()
                ),
            ));
        }
    }

    Ok(prebuilt
        .object_paths
        .iter()
        .map(|path| normalize_windows_path(path))
        .collect())
}

/// 使用当前包里的 C 源和头文件刷新稳定目录下的对象文件与元数据，并返回最终链接输入。
fn refresh_native_prebuilt(
    package_root: &Path,
    config: &PackageNativeConfig,
    native_root: &Path,
    source: &SourceFile,
) -> Result<Vec<PathBuf>, CompilerError> {
    let stage_dir = native_root.join(sanitize_package_name(package_root));
    fs::create_dir_all(&stage_dir).map_err(|error| {
        build_native_error(
            source,
            format!("创建原生中间目录失败：{}: {}", stage_dir.display(), error),
        )
    })?;

    let mut build = cc::Build::new();
    build
        .cargo_metadata(false)
        .warnings(false)
        .target(DEFAULT_TARGET_TRIPLE)
        .host(DEFAULT_TARGET_TRIPLE)
        .opt_level(2)
        .debug(false)
        .out_dir(&stage_dir);
    if DEFAULT_TARGET_TRIPLE.contains("windows") {
        build.flag_if_supported("/utf-8");
    }

    for include_dir in &config.include_dirs {
        build.include(include_dir);
    }
    for source_file in &config.source_files {
        build.file(source_file);
    }

    let built_objects = build.try_compile_intermediates().map_err(|error| {
        build_native_error(
            source,
            format!(
                "维护者源码构建失败（{}）：{}",
                package_root.display(),
                error
            ),
        )
    })?;

    let artifact_plan = build_artifact_plan(package_root, config);
    fs::create_dir_all(&artifact_plan.artifact_dir).map_err(|error| {
        build_native_error(
            source,
            format!(
                "创建预编译产物目录失败：{}: {}",
                artifact_plan.artifact_dir.display(),
                error
            ),
        )
    })?;

    if built_objects.len() != artifact_plan.object_paths.len() {
        return Err(build_native_error(
            source,
            format!(
                "源码刷新生成的对象文件数量为 {}，但预期写入 {} 个稳定产物。",
                built_objects.len(),
                artifact_plan.object_paths.len()
            ),
        ));
    }

    let mut final_objects = Vec::new();
    for (built_object, object_path) in built_objects.iter().zip(&artifact_plan.object_paths) {
        fs::copy(built_object, object_path).map_err(|error| {
            build_native_error(
                source,
                format!(
                    "写入预编译对象文件失败：{}: {}",
                    object_path.display(),
                    error
                ),
            )
        })?;
        final_objects.push(normalize_windows_path(object_path));
    }

    let header_files = collect_include_files(&config.include_dirs, source)?;
    let manifest = NativeArtifactManifest {
        package: config.package_name.clone(),
        package_version: config.package_version.clone(),
        target: DEFAULT_TARGET_TRIPLE.to_string(),
        abi: config.abi,
        source_hash: compute_fingerprint(package_root, &config.source_files, source)?,
        header_hash: compute_fingerprint(package_root, &header_files, source)?,
        objects: artifact_plan.object_file_names,
    };
    let manifest_text = toml::to_string(&manifest).map_err(|error| {
        build_native_error(
            source,
            format!(
                "序列化预编译清单失败：{}: {}",
                artifact_plan.manifest_path.display(),
                error
            ),
        )
    })?;
    fs::write(&artifact_plan.manifest_path, manifest_text.as_bytes()).map_err(|error| {
        build_native_error(
            source,
            format!(
                "写入预编译清单失败：{}: {}",
                artifact_plan.manifest_path.display(),
                error
            ),
        )
    })?;

    Ok(final_objects)
}

/// 保存刷新产物时最终采用的对象文件和清单路径，确保源码刷新与配置声明共用同一协议。
struct ArtifactPlan {
    artifact_dir: PathBuf,
    manifest_path: PathBuf,
    object_paths: Vec<PathBuf>,
    object_file_names: Vec<String>,
}

/// 根据配置中的预编译声明或默认路径规则计算本次刷新应写入的稳定产物位置。
fn build_artifact_plan(package_root: &Path, config: &PackageNativeConfig) -> ArtifactPlan {
    if let Some(prebuilt) = config.prebuilt.get(DEFAULT_TARGET_TRIPLE) {
        return ArtifactPlan {
            artifact_dir: prebuilt.artifact_dir.clone(),
            manifest_path: prebuilt.manifest_path.clone(),
            object_paths: prebuilt.object_paths.clone(),
            object_file_names: prebuilt
                .object_paths
                .iter()
                .map(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default()
                        .to_string()
                })
                .collect(),
        };
    }

    let artifact_dir = default_manifest_path(package_root)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| package_root.to_path_buf());
    let extension = object_extension();
    let object_file_names = config
        .source_files
        .iter()
        .filter_map(|path| {
            path.file_stem()
                .and_then(|name| name.to_str())
                .map(|stem| format!("{stem}.{extension}"))
        })
        .collect::<Vec<_>>();
    let object_paths = object_file_names
        .iter()
        .map(|name| artifact_dir.join(name))
        .collect::<Vec<_>>();

    ArtifactPlan {
        artifact_dir,
        manifest_path: default_manifest_path(package_root),
        object_paths,
        object_file_names,
    }
}

/// 计算当前目标平台默认的清单落盘位置，供未显式声明目标时生成试验产物使用。
fn default_manifest_path(package_root: &Path) -> PathBuf {
    package_root
        .join("native")
        .join("artifacts")
        .join(DEFAULT_TARGET_TRIPLE)
        .join("manifest.toml")
}

/// 返回当前目标平台的对象文件扩展名，保证 Windows 与 POSIX 产物命名保持一致。
fn object_extension() -> &'static str {
    if DEFAULT_TARGET_TRIPLE.contains("windows") {
        "obj"
    } else {
        "o"
    }
}

/// 解析 `source_fallback` 字段，遇到未知值时立即给出中文配置错误。
fn parse_source_fallback(
    value: &str,
    package_root: &Path,
    source: &SourceFile,
) -> Result<NativeSourceFallback, CompilerError> {
    match value {
        "explicit" => Ok(NativeSourceFallback::Explicit),
        "never" => Ok(NativeSourceFallback::Never),
        other => Err(build_native_error(
            source,
            format!(
                "包 `{}` 的 `source_fallback` 值无效：`{}`，只支持 `explicit` 或 `never`",
                package_root.display(),
                other
            ),
        )),
    }
}

/// 把包配置中的相对原生路径解析为绝对路径，并确保目标文件或目录真实存在。
fn resolve_native_path(
    package_root: &Path,
    relative: &str,
    source: &SourceFile,
) -> Result<PathBuf, CompilerError> {
    let path = package_root.join(relative);
    if path.exists() {
        Ok(normalize_windows_path(&path))
    } else {
        Err(build_native_error(
            source,
            format!("原生路径不存在：{}", path.display()),
        ))
    }
}

/// 收集所有包含目录中的头文件，供预编译清单在链接前校验头文件摘要使用。
fn collect_include_files(
    include_dirs: &[PathBuf],
    source: &SourceFile,
) -> Result<Vec<PathBuf>, CompilerError> {
    let mut files = Vec::new();
    for include_dir in include_dirs {
        collect_directory_files(include_dir, &mut files, source)?;
    }
    files.sort();
    Ok(files)
}

/// 递归遍历单个目录并收集文件路径，保证摘要计算覆盖所有头文件层级。
fn collect_directory_files(
    root: &Path,
    files: &mut Vec<PathBuf>,
    source: &SourceFile,
) -> Result<(), CompilerError> {
    for entry in fs::read_dir(root).map_err(|error| {
        build_native_error(
            source,
            format!("读取原生目录失败：{}: {}", root.display(), error),
        )
    })? {
        let entry = entry.map_err(|error| {
            build_native_error(
                source,
                format!("读取原生目录条目失败：{}: {}", root.display(), error),
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            build_native_error(
                source,
                format!("读取原生目录类型失败：{}: {}", path.display(), error),
            )
        })?;
        if file_type.is_dir() {
            collect_directory_files(&path, files, source)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

/// 按稳定顺序计算一组源文件或头文件的内容指纹，避免把过期对象误当成最新产物。
fn compute_fingerprint(
    package_root: &Path,
    files: &[PathBuf],
    source: &SourceFile,
) -> Result<String, CompilerError> {
    let normalized_root = normalize_windows_path(package_root);
    let mut hasher = StableHasher::new();
    let mut sorted = files.to_vec();
    sorted.sort();
    for path in sorted {
        let normalized_path = normalize_windows_path(&path);
        let bytes = fs::read(&normalized_path).map_err(|error| {
            build_native_error(
                source,
                format!("读取原生文件失败：{}: {}", normalized_path.display(), error),
            )
        })?;
        let relative = normalized_path
            .strip_prefix(&normalized_root)
            .unwrap_or(&normalized_path);
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(&bytes);
        hasher.update(b"\0");
    }
    Ok(format!("fnv1a64:{:016x}", hasher.finish()))
}

/// 实现稳定的 FNV-1a 64 位哈希，保证摘要值可跨进程、跨机器复现。
struct StableHasher {
    state: u64,
}

impl StableHasher {
    /// 创建新的 FNV-1a 哈希器，初始值固定为标准 offset basis。
    fn new() -> Self {
        Self {
            state: 0xcbf29ce484222325,
        }
    }

    /// 追加一段字节到当前哈希状态，统一按 FNV-1a 规则滚动更新。
    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(0x100000001b3);
        }
    }

    /// 返回当前累计出的稳定 64 位摘要值。
    fn finish(self) -> u64 {
        self.state
    }
}

/// 把包根目录转换为稳定的临时目录名，避免不同包的原生产物互相覆盖。
fn sanitize_package_name(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// 去掉 Windows `\\?\` 长路径前缀，避免 `cl.exe` 在源码路径解析上出现兼容性问题。
fn normalize_windows_path(path: &Path) -> PathBuf {
    let text = path.display().to_string();
    if let Some(stripped) = text.strip_prefix("\\\\?\\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

/// 构造统一的原生构建错误，避免各分支散落重复的 `CompilerError::build` 样板代码。
fn build_native_error(source: &SourceFile, message: String) -> CompilerError {
    CompilerError::build(
        MessageId::BuildRustcFailed,
        vec![message],
        Span::new(0, source.text().len()),
    )
}

/// 构造面向普通调用方的中文诊断，明确指出当前角色、目标平台和维护者处理建议。
fn build_consumer_error(source: &SourceFile, package_name: &str, detail: String) -> CompilerError {
    build_native_error(
        source,
        format!(
            "普通依赖消费链接 `{package_name}` 时无法使用目标 `{}` 的预编译原生产物：{detail}",
            DEFAULT_TARGET_TRIPLE
        ),
    )
}
