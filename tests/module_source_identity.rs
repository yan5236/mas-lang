use mas_lang_lsp::{
    analysis::AnalysisService, completion::completion_items_for, documents::OpenDocument,
};
use mascript_compiler_llvm::{
    analyze_source,
    settings::{CompilerSettings, UiLanguage},
    source::SourceFile,
};
use tower_lsp::lsp_types::{Position, Url};

/// 使用真实模块项目复现内部名泄漏、内存文本诊断偏移和依赖源码串入补全的问题。
#[test]
fn module_names_and_diagnostics_use_original_source() {
    let root = std::env::temp_dir().join(format!("mas-source-identity-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("mod.yml"),
        "name: demo\nversion: 0.1.0\ndependencies: {}\n",
    )
    .unwrap();
    let path = root.join("src/main.ms");
    std::fs::write(&path, "func main():\n    print(0)\n").unwrap();
    std::fs::write(
        root.join("src/helper.ms"),
        "pub func value() -> int:\n    return 1\nvar private_value = 9\n",
    )
    .unwrap();
    let settings = CompilerSettings::new(UiLanguage::ZhCn);
    let service = AnalysisService::with_settings(settings);
    let mut document = OpenDocument {
        uri: Url::from_file_path(&path).unwrap(),
        path: path.clone(),
        version: 1,
        text: "import helper { value }\nvar a:int = 0\nfunc main():\n    print(a)\n".into(),
    };
    let valid = service.analyze_document(&document);
    assert!(valid.diagnostics.is_empty(), "{:?}", valid.diagnostics);
    assert!(!valid.symbols.is_fallback());
    let a = valid
        .symbols
        .symbols()
        .iter()
        .find(|s| s.name == "a")
        .unwrap();
    assert_eq!(a.selection_range.start, Position::new(1, 4));
    let items = completion_items_for(&document.text, &valid.symbols, Position::new(3, 10));
    assert!(items.iter().any(|item| item.label == "a"));
    assert!(
        items
            .iter()
            .all(|item| !format!("{item:?}").contains("__ms_") && item.label != "private_value")
    );
    for (text, line, name) in [
        (
            "var a:int = 0\nfunc main():\n    a = \"bad\"\n    print(a)\n",
            2,
            "a",
        ),
        (
            "var hello:string = \"hello\"\nvar world:string = 1\nfunc main():\n    print(hello)\n",
            1,
            "world",
        ),
        (
            "import helper { value }\nvar a:string = 1\nfunc main():\n    print(a)\n",
            1,
            "a",
        ),
    ] {
        document.text = text.into();
        let analysis = service.analyze_document(&document);
        let diagnostic = &analysis.diagnostics[0];
        assert_eq!(diagnostic.range.start.line, line, "{diagnostic:?}");
        assert!(
            diagnostic.message.contains(&format!("`{name}`")),
            "{diagnostic:?}"
        );
        assert!(!diagnostic.message.contains("__ms_"));
        let source = SourceFile::new(path.display().to_string(), text.into());
        let error = analyze_source(&source, &settings).unwrap_err();
        let rendered = error.render(settings.language, Some(&source));
        assert!(
            rendered.contains(&format!("main.ms:{}:", line + 1)),
            "{rendered}"
        );
        assert!(!rendered.contains("__ms_"));
    }
    document.text = "import helper { value }\nfunc main():\n    print(value())\n".into();
    for dependency in [
        "pub func value() -> int:\n    return 1\nvar broken:string = 7\n",
        "pub func value(\n",
    ] {
        std::fs::write(root.join("src/helper.ms"), dependency).unwrap();
        let analysis = service.analyze_document(&document);
        let diagnostic = &analysis.diagnostics[0];
        assert_eq!(diagnostic.range, tower_lsp::lsp_types::Range::default());
        let related = &diagnostic.related_information.as_ref().unwrap()[0];
        assert!(related.location.uri.path().ends_with("/helper.ms"));
        assert!(diagnostic.message.contains("helper.ms:"));
        assert!(!diagnostic.message.contains("__ms_"));
        if dependency.contains("broken") {
            assert_eq!(related.location.range.start.line, 2);
            assert!(diagnostic.message.contains("`broken`"));
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
