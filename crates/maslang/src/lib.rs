pub mod ast;
pub mod cli;
pub mod diagnostic;
pub mod lexer;
pub mod linker;
pub mod llvm_backend;
pub mod module_loader;
pub mod native_link;
pub mod optimization;
pub mod parser;
pub mod runtime_bootstrap;
pub mod semantic;
pub mod settings;
pub mod source;
mod source_map;
pub mod temp_directory;
pub mod token;

use std::path::PathBuf;

use ast::Program;
use diagnostic::CompilerError;
use semantic::SemanticAnalyzer;
use settings::CompilerSettings;
use source::SourceFile;

/// 保存前端流水线产出的抽象语法树，供后端直接复用。
#[derive(Debug)]
pub struct FrontendOutput {
    pub source: SourceFile,
    pub program: Program,
    pub package_roots: Vec<PathBuf>,
}

pub use optimization::CompileOptions;

/// 按固定顺序执行词法、语法和语义分析，并返回可复用的前端结果。
pub fn analyze_source(
    source: &SourceFile,
    settings: &CompilerSettings,
) -> Result<FrontendOutput, CompilerError> {
    let loaded = module_loader::load_module_graph(source, settings)?;
    SemanticAnalyzer::new(settings)
        .analyze(&loaded.program)
        .map_err(|error| source_map::restore_error(error, &loaded.origins))?;
    Ok(FrontendOutput {
        source: loaded.source,
        program: loaded.program,
        package_roots: loaded.package_roots,
    })
}

/// 把源码编译为 Windows 可执行文件，并返回最终产物路径。
pub fn compile_to_executable(
    source: &SourceFile,
    settings: &CompilerSettings,
    options: &CompileOptions,
) -> Result<PathBuf, CompilerError> {
    let frontend = analyze_source(source, settings)?;
    linker::compile_to_executable(
        &frontend.source,
        settings,
        &frontend.program,
        &frontend.package_roots,
        options,
    )
}

/// 从用户目录读取编译器设置；当设置不存在或无效时自动回退到默认值。
pub fn load_settings() -> CompilerSettings {
    settings::load_settings()
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{CompileOptions, analyze_source, compile_to_executable};
    use crate::settings::{CompilerSettings, UiLanguage};
    use crate::source::SourceFile;

    /// 构造测试环境统一使用的中文设置，减少重复样板代码。
    fn zh_settings() -> CompilerSettings {
        CompilerSettings::new(UiLanguage::ZhCn)
    }

    /// 创建一个带 `mod.yml` 和 `src/` 的临时模块项目目录，供模块导入测试复用。
    fn create_temp_project(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("mascript-llvm-project-{name}-{unique}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("mod.yml"),
            "name: demo\nversion: 0.1.0\n\ndependencies: {}\n",
        )
        .unwrap();
        fs::write(root.join("mod-lock.yml"), "dependencies: {}\n").unwrap();
        fs::write(
            root.join("settings.toml"),
            "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ms\"\noutput = \"build\"\nopt-level = 2\n",
        )
        .unwrap();
        root
    }

    /// 递归复制目录树中的全部文件，保证测试依赖里的预编译对象文件按原字节保留下来。
    fn copy_directory_recursive(source_root: &Path, target_root: &Path) {
        fs::create_dir_all(target_root).unwrap();
        for entry in fs::read_dir(source_root).unwrap() {
            let entry = entry.unwrap();
            let source_path = entry.path();
            let target_path = target_root.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_directory_recursive(&source_path, &target_path);
            } else {
                fs::copy(&source_path, &target_path).unwrap();
            }
        }
    }

    /// 从 workspace 的标准库目录复制 `time` 到临时项目的 `mod_module`，保留预编译对象文件以模拟本地依赖安装。
    fn install_time_dependency(root: &Path) {
        let package_root = root.join("mod_module").join("time@0.1.0");
        let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/time");
        copy_directory_recursive(&source_root, &package_root);
    }

    /// 为依赖 `time` 的测试项目写入固定依赖清单，避免每个用例重复拼接样板文件。
    fn write_time_dependency_manifest(root: &Path) {
        fs::write(
            root.join("mod.yml"),
            "name: demo\nversion: 0.1.0\n\ndependencies:\n  time:\n    path: ../time\n    version: 0.1.0\n",
        )
        .unwrap();
        fs::write(
            root.join("mod-lock.yml"),
            "dependencies:\n  time:\n    version: 0.1.0\n    source:\n      type: path\n      path: ../time\n    store_path: mod_module/time@0.1.0\n",
        )
        .unwrap();
    }

    /// 把给定主程序写入临时项目并返回可直接传给编译器的 `SourceFile`。
    fn write_time_app_source(root: &Path, text: &str) -> SourceFile {
        let source_path = root.join("src").join("main.ms");
        fs::write(&source_path, text).unwrap();
        SourceFile::new(
            source_path.display().to_string(),
            fs::read_to_string(&source_path).unwrap(),
        )
    }

    /// 校验最小程序能够完整通过前端分析流水线。
    #[test]
    fn should_analyze_minimal_program() {
        let source = SourceFile::new("<test>", "var value = 1\nprint(value)\n".to_string());
        let output = analyze_source(&source, &zh_settings()).unwrap();
        assert_eq!(output.program.statements.len(), 2);
    }

    /// 校验原生后端能够把最小程序编译为可执行文件。
    #[test]
    fn should_compile_minimal_program() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let source_path = env::temp_dir()
            .join(format!("mascript-llvm-minimal-{unique}"))
            .join("minimal.ms");
        let source = SourceFile::new(
            source_path.display().to_string(),
            "print(\"Hello world.\")\n".to_string(),
        );
        let binary =
            compile_to_executable(&source, &zh_settings(), &CompileOptions::new(None)).unwrap();
        assert!(binary.extension().is_some_and(|ext| ext == "exe"));
    }

    /// 校验本地模块、`index.ms` 聚合入口与 `export import` 能在前端被正确装载。
    #[test]
    fn should_analyze_project_with_local_module_imports() {
        let root = create_temp_project("local-import");
        fs::create_dir_all(root.join("src").join("utils")).unwrap();
        fs::write(
            root.join("src").join("utils").join("math.ms"),
            "pub func add(a:int, b:int) -> int:\n    return a + b\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("utils").join("index.ms"),
            "export import utils.math { add }\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("main.ms"),
            "import utils { add }\nfunc main():\n    print(add(1, 2))\n",
        )
        .unwrap();

        let source_path = root.join("src").join("main.ms");
        let source = SourceFile::new(
            source_path.display().to_string(),
            fs::read_to_string(&source_path).unwrap(),
        );
        let output = analyze_source(&source, &zh_settings()).unwrap();
        assert!(
            output
                .program
                .statements
                .iter()
                .filter_map(|statement| match statement {
                    crate::ast::Stmt::FunctionDecl { name, .. } => Some(name.as_str()),
                    _ => None,
                })
                .any(|name| name.contains("add") || name == "main")
        );

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验别名导入与全部导入能够在模块装载后被正确重写并编译运行。
    #[test]
    fn should_build_project_with_alias_and_star_imports() {
        let root = create_temp_project("alias-star");
        fs::create_dir_all(root.join("src").join("utils")).unwrap();
        fs::write(
            root.join("src").join("utils").join("math.ms"),
            "pub func add(a:int, b:int) -> int:\n    return a + b\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("main.ms"),
            "import utils.math { add as plus }\nimport utils.math.*\nfunc main():\n    print(plus(1, 2))\n    print(add(3, 4))\n",
        )
        .unwrap();

        let source_path = root.join("src").join("main.ms");
        let source = SourceFile::new(
            source_path.display().to_string(),
            fs::read_to_string(&source_path).unwrap(),
        );
        let output_path = root.join("build").join("demo.exe");
        let binary = compile_to_executable(
            &source,
            &zh_settings(),
            &CompileOptions::new(Some(output_path)),
        )
        .unwrap();
        let output = std::process::Command::new(binary).output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "3\n7\n");

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验导入未公开符号时会在编译阶段给出明确失败。
    #[test]
    fn should_reject_importing_private_symbol() {
        let root = create_temp_project("private-symbol");
        fs::create_dir_all(root.join("src").join("utils")).unwrap();
        fs::write(
            root.join("src").join("utils").join("math.ms"),
            "func add(a:int, b:int) -> int:\n    return a + b\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("main.ms"),
            "import utils.math { add }\nfunc main():\n    print(add(1, 2))\n",
        )
        .unwrap();

        let source_path = root.join("src").join("main.ms");
        let source = SourceFile::new(
            source_path.display().to_string(),
            fs::read_to_string(&source_path).unwrap(),
        );
        let error = analyze_source(&source, &zh_settings()).unwrap_err();
        assert!(
            error
                .render(UiLanguage::ZhCn, Some(&source))
                .contains("未公开")
        );

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验本地模块与依赖同名时，`local.` 前缀能够强制选择本地模块。
    #[test]
    fn should_prefer_local_prefix_over_dependency_module() {
        let root = create_temp_project("local-prefix");
        fs::create_dir_all(root.join("mod_module").join("time@0.1.0").join("src")).unwrap();
        fs::write(
            root.join("mod.yml"),
            "name: demo\nversion: 0.1.0\n\ndependencies:\n  time:\n    path: ../time\n    version: 0.1.0\n",
        )
        .unwrap();
        fs::write(
            root.join("mod-lock.yml"),
            "dependencies:\n  time:\n    version: 0.1.0\n    source:\n      type: path\n      path: ../time\n    store_path: mod_module/time@0.1.0\n",
        )
        .unwrap();
        fs::write(
            root.join("mod_module")
                .join("time@0.1.0")
                .join("src")
                .join("index.ms"),
            "pub func now() -> int:\n    return 1\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("time.ms"),
            "pub func now() -> int:\n    return 2\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("main.ms"),
            "import local.time { now }\nfunc main():\n    print(now())\n",
        )
        .unwrap();

        let source_path = root.join("src").join("main.ms");
        let source = SourceFile::new(
            source_path.display().to_string(),
            fs::read_to_string(&source_path).unwrap(),
        );
        let output_path = root.join("build").join("demo.exe");
        let binary = compile_to_executable(
            &source,
            &zh_settings(),
            &CompileOptions::new(Some(output_path)),
        )
        .unwrap();
        let output = std::process::Command::new(binary).output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "2\n");

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验循环导入会在模块装载阶段被明确拒绝。
    #[test]
    fn should_reject_cyclic_module_imports() {
        let root = create_temp_project("cyclic-import");
        fs::create_dir_all(root.join("src").join("utils")).unwrap();
        fs::write(
            root.join("src").join("a.ms"),
            "import b { value_b }\npub func value_a() -> int:\n    return value_b()\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("b.ms"),
            "import a { value_a }\npub func value_b() -> int:\n    return value_a()\n",
        )
        .unwrap();
        fs::write(
            root.join("src").join("main.ms"),
            "import a { value_a }\nfunc main():\n    print(value_a())\n",
        )
        .unwrap();

        let source_path = root.join("src").join("main.ms");
        let source = SourceFile::new(
            source_path.display().to_string(),
            fs::read_to_string(&source_path).unwrap(),
        );
        let error = analyze_source(&source, &zh_settings()).unwrap_err();
        assert!(
            error
                .render(UiLanguage::ZhCn, Some(&source))
                .contains("循环依赖")
        );

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验应用工程能够通过依赖缓存导入 `time` 标准库，并拿到非空的时间格式输出与非负耗时差值。
    #[test]
    fn should_build_project_with_time_standard_library_dependency() {
        let root = create_temp_project("time-stdlib");
        install_time_dependency(&root);
        write_time_dependency_manifest(&root);
        let source = write_time_app_source(
            &root,
            "import time { monotonic_millis, now_iso8601, now_rfc2822, now_rfc7231, now_unix, now_unix_millis, sleep }\n\nfunc main():\n    print(\"unix=\" + string(now_unix()))\n    print(\"unix_millis=\" + string(now_unix_millis()))\n    print(\"iso8601=\" + now_iso8601())\n    print(\"rfc2822=\" + now_rfc2822())\n    print(\"rfc7231=\" + now_rfc7231())\n    var started = monotonic_millis()\n    sleep(10)\n    print(\"elapsed_ms=\" + string(monotonic_millis() - started))\n",
        );
        let output_path = root.join("build").join("demo.exe");
        let binary = compile_to_executable(
            &source,
            &zh_settings(),
            &CompileOptions::new(Some(output_path)),
        )
        .unwrap();
        let output = std::process::Command::new(binary).output().unwrap();
        assert!(output.status.success());

        let stdout = String::from_utf8(output.stdout).unwrap();
        let lines = stdout.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 6, "{stdout}");
        assert!(lines[0].starts_with("unix="), "{stdout}");
        assert!(lines[1].starts_with("unix_millis="), "{stdout}");
        assert!(lines[2].starts_with("iso8601="), "{stdout}");
        assert!(lines[2].contains("T"), "{stdout}");
        assert!(lines[2].ends_with("+00:00"), "{stdout}");
        assert!(lines[3].starts_with("rfc2822="), "{stdout}");
        assert!(lines[3].ends_with(" +0000"), "{stdout}");
        assert!(lines[4].starts_with("rfc7231="), "{stdout}");
        assert!(lines[4].ends_with(" GMT"), "{stdout}");
        assert!(lines[5].starts_with("elapsed_ms="), "{stdout}");

        let elapsed = lines[5]
            .trim_start_matches("elapsed_ms=")
            .parse::<i64>()
            .unwrap();
        assert!(elapsed >= 10);

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验 `sleep(-1)` 会沿着依赖导入链路返回明确的中文运行时错误，而不是静默忽略。
    #[test]
    fn should_report_negative_sleep_from_time_dependency() {
        let root = create_temp_project("time-sleep-error");
        install_time_dependency(&root);
        write_time_dependency_manifest(&root);
        let source = write_time_app_source(
            &root,
            "import time { sleep }\n\nfunc main():\n    sleep(-1)\n",
        );
        let output_path = root.join("build").join("demo.exe");
        let binary = compile_to_executable(
            &source,
            &zh_settings(),
            &CompileOptions::new(Some(output_path)),
        )
        .unwrap();
        let output = std::process::Command::new(binary).output().unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("sleep(ms) 只接受大于等于 0 的毫秒整数"));

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验测试辅助安装流程会把 `time` 的预编译清单与对象文件一并复制到依赖缓存。
    #[test]
    fn should_copy_time_prebuilt_artifacts_into_dependency_cache() {
        let root = create_temp_project("time-prebuilt-only");
        install_time_dependency(&root);
        let artifact_root = root
            .join("mod_module")
            .join("time@0.1.0")
            .join("native")
            .join("artifacts")
            .join("x86_64-pc-windows-msvc");
        assert!(artifact_root.join("manifest.toml").is_file());
        assert!(artifact_root.join("time_common.obj").is_file());
        assert!(artifact_root.join("time_windows.obj").is_file());

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验缺少预编译矩阵声明时，普通消费路径会返回明确的中文错误而不是隐式源码编译。
    #[test]
    fn should_fail_when_time_prebuilt_matrix_is_missing() {
        let root = create_temp_project("time-missing-matrix");
        install_time_dependency(&root);
        write_time_dependency_manifest(&root);
        let package_root = root.join("mod_module").join("time@0.1.0");
        let settings_path = package_root.join("settings.toml");
        let settings = fs::read_to_string(&settings_path).unwrap();
        fs::write(
            &settings_path,
            settings.replace(
                "\n[native.prebuilt.x86_64-pc-windows-msvc]\nmanifest = \"native/artifacts/x86_64-pc-windows-msvc/manifest.toml\"\nobjects = [\n  \"native/artifacts/x86_64-pc-windows-msvc/time_common.obj\",\n  \"native/artifacts/x86_64-pc-windows-msvc/time_windows.obj\",\n]\n",
                "",
            ),
        )
        .unwrap();

        let source = write_time_app_source(
            &root,
            "import time { now_unix }\n\nfunc main():\n    print(now_unix())\n",
        );
        let output_path = root.join("build").join("demo.exe");
        let error = compile_to_executable(
            &source,
            &zh_settings(),
            &CompileOptions::new(Some(output_path)),
        )
        .unwrap_err();
        let rendered = error.render(UiLanguage::ZhCn, Some(&source));
        assert!(rendered.contains("未声明预编译产物"));
        assert!(rendered.contains("x86_64-pc-windows-msvc"));
        assert!(rendered.contains("普通依赖消费不支持自动回退到源码编译"));

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验缺少对象文件时会返回中文诊断，并明确提示维护者刷新预编译产物。
    #[test]
    fn should_fail_when_time_prebuilt_object_is_missing() {
        let root = create_temp_project("time-missing-object");
        install_time_dependency(&root);
        write_time_dependency_manifest(&root);
        let package_root = root.join("mod_module").join("time@0.1.0");
        fs::remove_file(
            package_root
                .join("native")
                .join("artifacts")
                .join("x86_64-pc-windows-msvc")
                .join("time_common.obj"),
        )
        .unwrap();

        let source = write_time_app_source(
            &root,
            "import time { now_unix }\n\nfunc main():\n    print(now_unix())\n",
        );
        let output_path = root.join("build").join("demo.exe");
        let error = compile_to_executable(
            &source,
            &zh_settings(),
            &CompileOptions::new(Some(output_path)),
        )
        .unwrap_err();
        let rendered = error.render(UiLanguage::ZhCn, Some(&source));
        assert!(rendered.contains("缺少预编译对象文件"));
        assert!(rendered.contains("普通依赖消费"));

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验 ABI 版本不匹配时会在链接前被阻止，并提示维护者先刷新对应目标的预编译产物。
    #[test]
    fn should_fail_when_time_prebuilt_abi_mismatches() {
        let root = create_temp_project("time-abi-mismatch");
        install_time_dependency(&root);
        write_time_dependency_manifest(&root);
        let manifest_path = root
            .join("mod_module")
            .join("time@0.1.0")
            .join("native")
            .join("artifacts")
            .join("x86_64-pc-windows-msvc")
            .join("manifest.toml");
        let manifest = fs::read_to_string(&manifest_path).unwrap();
        fs::write(&manifest_path, manifest.replace("abi = 1", "abi = 999")).unwrap();

        let source = write_time_app_source(
            &root,
            "import time { now_unix }\n\nfunc main():\n    print(now_unix())\n",
        );
        let output_path = root.join("build").join("demo.exe");
        let error = compile_to_executable(
            &source,
            &zh_settings(),
            &CompileOptions::new(Some(output_path)),
        )
        .unwrap_err();
        let rendered = error.render(UiLanguage::ZhCn, Some(&source));
        assert!(rendered.contains("ABI 版本"));
        assert!(rendered.contains("维护者请先刷新对应目标的预编译产物"));

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }

    /// 校验维护者显式开启源码构建模式后，缺失的预编译对象和清单会被重新生成并立即可用。
    #[test]
    fn should_refresh_time_prebuilt_when_maintainer_mode_is_enabled() {
        let root = create_temp_project("time-refresh");
        install_time_dependency(&root);
        write_time_dependency_manifest(&root);
        let artifact_root = root
            .join("mod_module")
            .join("time@0.1.0")
            .join("native")
            .join("artifacts")
            .join("x86_64-pc-windows-msvc");
        fs::remove_dir_all(&artifact_root).unwrap();

        let source = write_time_app_source(
            &root,
            "import time { now_unix }\n\nfunc main():\n    print(now_unix())\n",
        );
        let output_path = root.join("build").join("demo.exe");
        let binary = compile_to_executable(
            &source,
            &zh_settings(),
            &CompileOptions::new(Some(output_path)).with_allow_native_source_build(true),
        )
        .unwrap();
        let output = std::process::Command::new(binary).output().unwrap();
        assert!(output.status.success());
        assert!(artifact_root.join("manifest.toml").is_file());
        assert!(artifact_root.join("time_common.obj").is_file());
        assert!(artifact_root.join("time_windows.obj").is_file());

        fs::remove_dir_all(Path::new(&root)).unwrap();
    }
}
