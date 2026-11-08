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
    assert!(store_dir.join("settings.toml").is_file());
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
    fs::write(artifact_root.join("manifest.toml"), "target = \"x\"\n").expect("写入预编译清单失败");
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
    use std::io::Write;
    fs::OpenOptions::new().append(true).open(lib_root.join("settings.toml"))
        .expect("打开 settings.toml 失败").write_all(
        b"\n[native.prebuilt.x86_64-pc-windows-msvc]\nmanifest = \"native/artifacts/x86_64-pc-windows-msvc/manifest.toml\"\nobjects = [\"native/artifacts/x86_64-pc-windows-msvc/time_common.obj\"]\n",
    )
    .expect("写入 settings.toml 失败");
    fs::write(artifact_root.join("manifest.toml"), "target = \"x\"\n")
        .expect("写入新增预编译清单失败");
    fs::write(artifact_root.join("time_common.obj"), [9u8, 8, 7]).expect("写入新增预编译对象失败");

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

/// 设置测试项目的路径依赖，保持项目自身名称与版本，供依赖图变更回归复用。
fn set_dependencies(root: &std::path::Path, dependencies: &[(&str, &str, &str)]) {
    let mut manifest = crate::manifest::load_project_manifest(root).unwrap();
    manifest.dependencies = dependencies
        .iter()
        .map(|(name, path, version)| {
            (
                name.to_string(),
                DependencySpec {
                    path: path.to_string(),
                    version: version.to_string(),
                },
            )
        })
        .collect();
    save_project_manifest(root, &manifest).unwrap();
}

/// 父包缓存命中仍须刷新间接依赖，且移除依赖后锁文件仅保留本轮可达节点。
#[test]
fn cached_parent_refreshes_children_and_prunes_lock() {
    let root = create_temp_dir("graph-refresh");
    let app = root.join("app");
    let a = root.join("a");
    let b = root.join("b");
    for (path, name) in [(&app, "app"), (&a, "a"), (&b, "b")] {
        create_project(path, name, "0.1.0");
    }
    set_dependencies(&app, &[("a", "../a", "0.1.0")]);
    set_dependencies(&a, &[("b", "../b", "0.1.0")]);
    install_project_dependencies(&app).unwrap();
    fs::write(b.join("src/main.ms"), "print(2)\n").unwrap();
    let lock = install_project_dependencies(&app).unwrap();
    assert_eq!(lock.dependencies.len(), 2);
    assert_eq!(
        fs::read_to_string(app.join("mod_module/b@0.1.0/src/main.ms")).unwrap(),
        "print(2)\n"
    );
    set_dependencies(&app, &[]);
    assert!(
        install_project_dependencies(&app)
            .unwrap()
            .dependencies
            .is_empty()
    );
    assert!(
        crate::manifest::load_lock_file(&app)
            .unwrap()
            .dependencies
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

/// 旧锁不阻止升级版本或切换来源；缓存和锁文件必须同时指向新内容。
#[test]
fn old_lock_allows_version_and_source_changes() {
    let root = create_temp_dir("graph-upgrade");
    let app = root.join("app");
    let lib = root.join("lib");
    let replacement = root.join("replacement");
    create_project(&app, "app", "0.1.0");
    create_project(&lib, "lib", "0.1.0");
    set_dependencies(&app, &[("lib", "../lib", "0.1.0")]);
    install_project_dependencies(&app).unwrap();
    create_project(&lib, "lib", "0.2.0");
    set_dependencies(&app, &[("lib", "../lib", "0.2.0")]);
    let lock = install_project_dependencies(&app).unwrap();
    assert_eq!(lock.dependencies["lib"].version, "0.2.0");
    assert!(app.join("mod_module/lib@0.2.0/src/main.ms").is_file());
    create_project(&replacement, "lib", "0.2.0");
    fs::write(replacement.join("src/main.ms"), "print(42)\n").unwrap();
    set_dependencies(&app, &[("lib", "../replacement", "0.2.0")]);
    let lock = install_project_dependencies(&app).unwrap();
    assert_eq!(
        lock.dependencies["lib"].source.path,
        crate::manifest::normalize_dependency_path(&app, &fs::canonicalize(&replacement).unwrap())
    );
    assert_eq!(
        fs::read_to_string(app.join("mod_module/lib@0.2.0/src/main.ms")).unwrap(),
        "print(42)\n"
    );
    fs::remove_dir_all(root).unwrap();
}

/// 缓存命中不能掩盖新引入的循环；同轮同名不同来源仍须冲突且不覆盖旧锁。
#[test]
fn cached_graph_still_rejects_cycles_and_conflicts() {
    let root = create_temp_dir("graph-invalid");
    let app = root.join("app");
    let a = root.join("a");
    let b = root.join("b");
    let other = root.join("other");
    for (path, name) in [(&app, "app"), (&a, "a"), (&b, "b"), (&other, "b")] {
        create_project(path, name, "0.1.0");
    }
    set_dependencies(&app, &[("a", "../a", "0.1.0")]);
    set_dependencies(&a, &[("b", "../b", "0.1.0")]);
    install_project_dependencies(&app).unwrap();
    let old_lock = fs::read(app.join("mod-lock.yml")).unwrap();
    set_dependencies(&b, &[("a", "../a", "0.1.0")]);
    assert!(
        install_project_dependencies(&app)
            .unwrap_err()
            .to_string()
            .contains("循环依赖")
    );
    set_dependencies(&b, &[]);
    set_dependencies(&app, &[("a", "../a", "0.1.0"), ("b", "../other", "0.1.0")]);
    assert!(matches!(
        install_project_dependencies(&app),
        Err(crate::error::FamaError::DependencyConflict { .. })
    ));
    assert_eq!(fs::read(app.join("mod-lock.yml")).unwrap(), old_lock);
    fs::remove_dir_all(root).unwrap();
}

/// 恶意依赖版本不能用于拼接缓存路径，即使依赖源本身存在。
#[test]
fn rejects_dependency_versions_with_path_components() {
    let app = create_temp_dir("unsafe-version");
    fs::write(
        app.join("settings.toml"),
        r#"[project]
name = "app"
version = "0.1.0"

[dependencies]
lib = { path = ".", version = "0/../../outside" }
"#,
    )
    .unwrap();

    let error = install_project_dependencies(&app).unwrap_err();
    assert!(error.to_string().contains("不安全的缓存路径字符"));
    assert!(!app.join("mod_module").exists());
    fs::remove_dir_all(app).unwrap();
}
