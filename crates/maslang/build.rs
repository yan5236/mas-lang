#[path = "src/runtime_bootstrap.rs"]
mod runtime_bootstrap;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

/// 把 Cargo 目标传给编译器，并为 Windows MSVC x64 构建各优化档的运行时库。
/// 编译或写入失败时终止 Cargo 构建，避免把不完整的运行时嵌入编译器。
fn main() {
    println!("cargo:rerun-if-changed=src/runtime");
    println!("cargo:rerun-if-changed=src/runtime_bootstrap.rs");
    let target = env::var("TARGET").unwrap();
    println!("cargo:rustc-env=MASLANG_TARGET={target}");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    emit_standard_library_assets(&out, &target);
    if target != "x86_64-pc-windows-msvc" {
        return;
    }
    let rustc = env::var_os("RUSTC").unwrap();
    let version = Command::new(&rustc).arg("-vV").output().unwrap();
    assert!(version.status.success(), "无法获取运行时构建工具链版本");
    fs::write(out.join("runtime-rustc.txt"), version.stdout).unwrap();
    let source = out.join("mas_runtime.rs");
    fs::write(&source, runtime_bootstrap::bootstrap_source()).unwrap();
    for level in 0..=3 {
        let status = Command::new(&rustc)
            .args([
                "--edition=2024",
                "--target=x86_64-pc-windows-msvc",
                "--crate-type=rlib",
                "--cfg=mas_runtime_library",
                "-C",
                "codegen-units=1",
                "-C",
            ])
            .arg(format!("opt-level={level}"))
            .arg(&source)
            .arg("-o")
            .arg(out.join(format!("libmas_runtime_{level}.rlib")))
            .status()
            .unwrap();
        assert!(status.success(), "无法构建 O{level} 运行时库");
    }
}

/// 生成随附标准库登记代码：把 `stdlib/io` 的 Mas 源码、包清单和当前目标的原生资产内嵌进编译器。
///
/// 只有仓库内构建才能读到 `stdlib/io`；发布的是编译好的工具链二进制，因此运行时不再
/// 依赖仓库路径、工作目录或环境变量。找不到包清单时退化为不内嵌，并在运行时给出明确诊断。
fn emit_standard_library_assets(out: &Path, target: &str) {
    let package_root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("..")
        .join("..")
        .join("stdlib")
        .join("io");
    let generated_path = out.join("standard_library_assets.rs");
    let settings_path = package_root.join("settings.toml");
    if !settings_path.is_file() {
        println!(
            "cargo:warning=未找到随附标准库 io 的包清单 {}，本次构建的编译器不内嵌 io 标准库",
            settings_path.display()
        );
        println!("cargo:rerun-if-changed={}", package_root.display());
        fs::write(
            generated_path,
            "pub(crate) const IO: Option<super::BundledPackage> = None;\n",
        )
        .unwrap();
        return;
    }
    println!("cargo:rerun-if-changed={}", package_root.display());

    let settings_text = fs::read_to_string(&settings_path).unwrap();
    let version = read_project_version(&settings_text).unwrap_or_else(|| "0.0.0".to_string());

    let modules = collect_files(&package_root, |path| {
        path.starts_with(package_root.join("src")) && has_extension(path, "ms")
    });
    let assets = collect_files(&package_root, |path| {
        (path.starts_with(package_root.join("native").join("include"))
            || path.starts_with(package_root.join("native").join("common"))
            || path.starts_with(package_root.join("native").join("platform")))
            && (has_extension(path, "c") || has_extension(path, "h"))
    });

    let artifact_dir = package_root.join("native").join("artifacts").join(target);
    let mut objects = Vec::new();
    let mut manifest_path = None;
    if artifact_dir.is_dir() {
        let manifest = artifact_dir.join("manifest.toml");
        if !manifest.is_file() {
            panic!(
                "随附标准库 io 的目标 `{target}` 缺少预编译清单：{}",
                manifest.display()
            );
        }
        manifest_path = Some(manifest);
        objects = collect_files(&artifact_dir, |path| {
            has_extension(path, "obj") || has_extension(path, "o")
        });
        if objects.is_empty() {
            panic!(
                "随附标准库 io 的目标 `{target}` 没有任何预编译对象文件：{}",
                artifact_dir.display()
            );
        }
    } else {
        println!(
            "cargo:warning=随附标准库 io 没有目标 `{target}` 的预编译产物，导入 io 的程序将在链接前报错"
        );
    }

    let mut generated = String::new();
    generated.push_str(
        "pub(crate) const IO: Option<super::BundledPackage> = Some(super::BundledPackage {\n",
    );
    generated.push_str("    name: \"io\",\n");
    generated.push_str(&format!("    version: {},\n", rust_string(&version)));
    generated.push_str(&format!(
        "    manifest: include_str!({}),\n",
        rust_path_literal(&settings_path)
    ));
    generated.push_str("    modules: &[\n");
    for path in &modules {
        generated.push_str(&format!(
            "        super::BundledModuleSource {{ relative: {}, text: include_str!({}) }},\n",
            rust_string(&relative_path(&package_root, path)),
            rust_path_literal(path)
        ));
        println!("cargo:rerun-if-changed={}", path.display());
    }
    generated.push_str("    ],\n    assets: &[\n");
    for path in &assets {
        generated.push_str(&format!(
            "        super::BundledAsset {{ relative: {}, bytes: include_bytes!({}) }},\n",
            rust_string(&relative_path(&package_root, path)),
            rust_path_literal(path)
        ));
        println!("cargo:rerun-if-changed={}", path.display());
    }
    generated.push_str("    ],\n");
    match manifest_path {
        Some(manifest) => {
            println!("cargo:rerun-if-changed={}", manifest.display());
            generated.push_str(&format!(
                "    prebuilt: Some(super::BundledPrebuilt {{\n        target: {},\n        manifest: super::BundledAsset {{ relative: {}, bytes: include_bytes!({}) }},\n        objects: &[\n",
                rust_string(target),
                rust_string(&relative_path(&package_root, &manifest)),
                rust_path_literal(&manifest)
            ));
            for path in &objects {
                generated.push_str(&format!(
                    "            super::BundledAsset {{ relative: {}, bytes: include_bytes!({}) }},\n",
                    rust_string(&relative_path(&package_root, path)),
                    rust_path_literal(path)
                ));
                println!("cargo:rerun-if-changed={}", path.display());
            }
            generated.push_str("        ],\n    }),\n");
        }
        None => generated.push_str("    prebuilt: None,\n"),
    }
    generated.push_str("});\n");
    fs::write(&generated_path, generated).unwrap();
}

/// 递归收集包内满足条件的文件，并按相对路径排序保证生成代码稳定可复现。
fn collect_files(root: &Path, accept: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_into(root, &accept, &mut files);
    files.sort_by(|left, right| relative_path(root, left).cmp(&relative_path(root, right)));
    files
}

/// 递归目录遍历的实际实现，保持与排序逻辑解耦。
fn collect_files_into(root: &Path, accept: &impl Fn(&Path) -> bool, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => panic!("读取随附标准库目录失败 {}: {error}", root.display()),
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files_into(&path, accept, files);
        } else if accept(&path) {
            files.push(path);
        }
    }
}

/// 统一用 `/` 分隔包内相对路径，使生成结果与宿主平台无关。
fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// 判断文件是否以给定扩展名结尾。
fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

/// 把文本转成可直接写进 Rust 源码的字符串字面量。
fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

/// 把路径转成可放进 `include_str!` / `include_bytes!` 的字面量。
fn rust_path_literal(path: &Path) -> String {
    format!("{:?}", path.to_string_lossy())
}

/// 读取包清单 `[project]` 段的版本号，供运行时诊断引用随附包版本。
fn read_project_version(text: &str) -> Option<String> {
    let mut in_project = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_project = trimmed == "[project]";
            continue;
        }
        if in_project
            && let Some(rest) = trimmed.strip_prefix("version")
            && let Some(value) = rest.trim_start().strip_prefix('=')
            && let Some(start) = value.find('"')
            && let Some(end) = value[start + 1..].find('"')
        {
            return Some(value[start + 1..start + 1 + end].to_string());
        }
    }
    None
}
