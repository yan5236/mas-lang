//! 随工具链分发的标准库登记与链接期物化。
//!
//! `stdlib/io` 是随附标准库的唯一维护来源；构建编译器时由 `build.rs` 把它的 Mas 源码、
//! 包清单、当前目标的 C 源与头文件、预编译清单和对象内嵌进二进制。运行时只使用这些
//! 内嵌资产，不读取仓库路径、工作目录或环境变量。
//!
//! 模块层把随附包登记在独立于磁盘路径的虚拟根 `<bundled>/<包名>` 下，用户目录无法用
//! 同名路径冒充；链接层把包按原目录结构写进本次构建的独占工作区，再交给普通原生
//! 依赖解析复用版本、目标、ABI 和指纹校验。

use std::fs;
use std::path::{Path, PathBuf};

use crate::diagnostic::CompilerError;
use crate::source::{SourceFile, Span};

/// 随附包虚拟根的标记组件；`<` 与 `>` 不是合法文件名，磁盘路径无法与它重合。
const BUNDLED_ROOT_MARKER: &str = "<bundled>";

/// 保存随附包中的一个 Mas 模块源码。
#[derive(Clone, Copy)]
pub(crate) struct BundledModuleSource {
    /// 相对包根目录的路径，使用 `/` 分隔，例如 `src/index.ms`。
    pub(crate) relative: &'static str,
    pub(crate) text: &'static str,
}

/// 保存随附包里随包分发的非 Mas 资产，例如 C 源、头文件、预编译对象与清单。
#[derive(Clone, Copy)]
pub(crate) struct BundledAsset {
    pub(crate) relative: &'static str,
    pub(crate) bytes: &'static [u8],
}

/// 保存当前目标的预编译产物；目标不在发布矩阵内时为 `None`。
#[derive(Clone, Copy)]
pub(crate) struct BundledPrebuilt {
    pub(crate) target: &'static str,
    pub(crate) manifest: BundledAsset,
    pub(crate) objects: &'static [BundledAsset],
}

/// 描述一个随附标准库包的全部内嵌资产。
#[derive(Clone, Copy)]
pub(crate) struct BundledPackage {
    pub(crate) name: &'static str,
    pub(crate) version: &'static str,
    /// 包清单原文，物化时原样写回，供普通原生依赖解析读取。
    pub(crate) manifest: &'static str,
    pub(crate) modules: &'static [BundledModuleSource],
    pub(crate) assets: &'static [BundledAsset],
    pub(crate) prebuilt: Option<BundledPrebuilt>,
}

/// 解析随附包内单个模块的结果，包含稳定虚拟路径与内嵌源码。
pub(crate) struct BundledModule {
    pub(crate) file: PathBuf,
    pub(crate) text: &'static str,
}

/// 由 `build.rs` 生成的资产登记表；未随附标准库时该常量为 `None`。
mod assets {
    include!(concat!(env!("OUT_DIR"), "/standard_library_assets.rs"));
}

/// 返回当前工具链内嵌的全部随附标准库包。
fn packages() -> impl Iterator<Item = BundledPackage> {
    [assets::IO].into_iter().flatten()
}

/// 按包名查找随附标准库包；名称未随附时返回 `None`。
pub(crate) fn find(name: &str) -> Option<BundledPackage> {
    packages().find(|package| package.name == name)
}

/// 返回随附包在模块图中的虚拟包根，例如 `<bundled>/io`。
pub(crate) fn package_root(package: &BundledPackage) -> PathBuf {
    PathBuf::from(format!("{BUNDLED_ROOT_MARKER}/{}", package.name))
}

/// 判断给定包根是否属于随附标准库，用于避免把它登记成真实包路径。
pub(crate) fn is_bundled_package_root(root: &Path) -> bool {
    package_for_root(root).is_some()
}

/// 从虚拟包根反查随附包；用户磁盘路径不会命中该前缀。
pub(crate) fn package_for_root(root: &Path) -> Option<BundledPackage> {
    let mut components = root.components();
    if components.next()?.as_os_str() != BUNDLED_ROOT_MARKER {
        return None;
    }
    let name = components.next()?.as_os_str().to_str()?;
    if components.next().is_some() {
        return None;
    }
    find(name)
}

/// 在随附包内定位模块，`relative` 是不含包名的路径片段。
///
/// 与磁盘模块一致：先尝试 `src/<片段>.ms`，再尝试目录入口 `src/<片段>/index.ms`；
/// 空片段指向包根入口 `src/index.ms`。
pub(crate) fn lookup(package: &BundledPackage, relative: &[String]) -> Option<BundledModule> {
    let direct = module_relative_path(relative, false);
    let index = module_relative_path(relative, true);
    for candidate in [direct, index] {
        if let Some(text) = package
            .modules
            .iter()
            .find(|module| module.relative == candidate)
            .map(|module| module.text)
        {
            return Some(BundledModule {
                file: package_root(package).join(&candidate),
                text,
            });
        }
    }
    None
}

/// 返回随附模块缺失时应展示的虚拟路径，保持与磁盘模块一致的诊断写法。
pub(crate) fn missing_module_path(package: &BundledPackage, relative: &[String]) -> PathBuf {
    package_root(package).join(module_relative_path(relative, false))
}

/// 计算包内模块的相对路径；`as_index` 为真时按目录入口处理。
fn module_relative_path(relative: &[String], as_index: bool) -> String {
    if relative.is_empty() {
        return "src/index.ms".to_string();
    }
    let joined = relative.join("/");
    if as_index {
        format!("src/{joined}/index.ms")
    } else {
        format!("src/{joined}.ms")
    }
}

/// 把随附包按原目录结构写进本次构建的独占工作区，返回可交给原生解析的包根。
///
/// 只写包清单、随包资产和当前目标的预编译产物；源码刷新（`--allow-native-source-build`）
/// 也只会写进这个临时包根，不会落到用户工程、安装目录或源码树。
pub(crate) fn materialize(
    name: &str,
    destination: &Path,
    source: &SourceFile,
) -> Result<PathBuf, CompilerError> {
    let package = find(name).ok_or_else(|| {
        CompilerError::build_message(
            format!("随附标准库 `{name}` 未内嵌在当前工具链中，请改用包含该标准库的编译器版本"),
            Span::new(0, source.text().len()),
        )
    })?;
    let root = destination.join(package.name);
    write_asset(&root, "settings.toml", package.manifest.as_bytes(), source)?;
    for asset in package.assets {
        write_asset(&root, asset.relative, asset.bytes, source)?;
    }
    // 只物化与当前编译目标一致的预编译产物，避免把别的目标对象带进链接。
    if let Some(prebuilt) = package
        .prebuilt
        .filter(|prebuilt| prebuilt.target == crate::native_link::DEFAULT_TARGET_TRIPLE)
    {
        write_asset(
            &root,
            prebuilt.manifest.relative,
            prebuilt.manifest.bytes,
            source,
        )?;
        for object in prebuilt.objects {
            write_asset(&root, object.relative, object.bytes, source)?;
        }
    }
    // 返回规范路径：规范形式与依赖缓存的规范包根一致，原生指纹才能按同样的分隔符复算。
    fs::canonicalize(&root).map_err(|error| {
        CompilerError::build_message(
            format!("解析随附标准库目录失败：{}: {error}", root.display()),
            Span::new(0, source.text().len()),
        )
    })
}

/// 把单个内嵌资产写入工作区，按需要创建父目录。
fn write_asset(
    root: &Path,
    relative: &str,
    bytes: &[u8],
    source: &SourceFile,
) -> Result<(), CompilerError> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CompilerError::build_message(
                format!("创建随附标准库目录失败：{}: {error}", parent.display()),
                Span::new(0, source.text().len()),
            )
        })?;
    }
    fs::write(&path, bytes).map_err(|error| {
        CompilerError::build_message(
            format!("写随附标准库资产失败：{}: {error}", path.display()),
            Span::new(0, source.text().len()),
        )
    })
}
