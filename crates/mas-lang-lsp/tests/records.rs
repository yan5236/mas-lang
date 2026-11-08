use mas_lang_lsp::{
    analysis::AnalysisService, completion::completion_items_for, documents::OpenDocument,
    hover::hover_for, record_support::definition_range,
};
use maslang::settings::CompilerSettings;
use tower_lsp::lsp_types::{Position, SymbolKind, Url};

/// 构造仅内存的文档，避免依赖工作区样例及磁盘内容。
fn document(text: &str) -> OpenDocument {
    OpenDocument {
        uri: Url::parse("file:///E:/record-lsp/main.ms").unwrap(),
        path: "E:/record-lsp/main.ms".into(),
        version: 1,
        text: text.into(),
    }
}

/// 根据唯一片段和片段内字节偏移获取 UTF-16 光标位置，支持前置中文。
fn at(text: &str, marker: &str, extra: usize) -> Position {
    let offset = text.find(marker).unwrap() + extra;
    let prefix = &text[..offset];
    Position::new(
        prefix.bytes().filter(|b| *b == b'\n').count() as u32,
        prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
    )
}

/// 结构体字段、可空函数签名和有效程序诊断复用编译器，类型和字段定位保持原始名称。
#[test]
fn diagnostics_symbols_hover_and_definition() {
    let text = "// 中文\nstruct Person:\n    age: int?\nfunc create() -> Person:\n    return Person(18)\nvar person = create()\nprint(person.age.value)\n";
    let analysis = AnalysisService::with_settings(CompilerSettings::default_settings())
        .analyze_document(&document(text));
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert!(
        analysis
            .symbols
            .symbols()
            .iter()
            .any(|s| s.name == "Person" && s.kind == SymbolKind::STRUCT)
    );
    assert!(
        analysis
            .symbols
            .symbols()
            .iter()
            .any(|s| s.name == "Person.age" && s.detail.as_deref() == Some("age: int?"))
    );
    let position = at(text, "person.age", 8);
    let hover = hover_for(text, &analysis.symbols, position).unwrap();
    assert!(format!("{:?}", hover.contents).contains("age: int?"));
    let range = definition_range(text, position).unwrap();
    assert_eq!(range.start, Position::new(2, 4));
    assert_eq!(range.end, Position::new(2, 7));
    assert_eq!(
        definition_range(text, at(text, "-> Person", 4))
            .unwrap()
            .start,
        Position::new(1, 7)
    );
    let bad = text.replace("Person(18)", "Person(false)");
    assert!(
        !AnalysisService::new()
            .analyze_document(&document(&bad))
            .diagnostics
            .is_empty()
    );
}

/// 编辑到点号时保留字段补全，可空对象只给 value，取值后才给结构体字段。
#[test]
fn incomplete_member_chains_and_nullable_types_complete() {
    let prefix = "struct Person:\n    age: int?\n    name: string\nvar person: Person? = null\n";
    for (tail, expected) in [
        ("person.", vec!["value"]),
        ("person.value.", vec!["age", "name"]),
    ] {
        let text = format!("{prefix}{tail}");
        let analysis = AnalysisService::new().analyze_document(&document(&text));
        let items = completion_items_for(&text, &analysis.symbols, at(&text, tail, tail.len()));
        let labels: Vec<_> = items.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, expected);
    }
    let text = format!("{prefix}var next: ");
    let analysis = AnalysisService::new().analyze_document(&document(&text));
    let items = completion_items_for(&text, &analysis.symbols, at(&text, "var next: ", 10));
    assert!(items.iter().any(|x| x.label == "int?"));
    assert!(items.iter().any(|x| x.label == "Person"));
}

/// 同名字段必须按接收者类型定位；同名参数的作用域不能混淆。
#[test]
fn same_named_fields_resolve_by_receiver() {
    let text = "struct A:\n    value: int\nstruct B:\n    value: string\nfunc first(x: A):\n    print(x.value)\nfunc second(x: B):\n    print(x.value)\n";
    let first = at(text, "print(x.value)", 9);
    let last = text.rfind("print(x.value)").unwrap();
    let second = Position::new(
        text[..last].bytes().filter(|b| *b == b'\n').count() as u32,
        13,
    );
    assert_eq!(definition_range(text, first).unwrap().start.line, 1);
    assert_eq!(definition_range(text, second).unwrap().start.line, 3);
}

/// 字段声明本身可悬停，字符串和注释中的同名文字不能伪装成类型定义引用。
#[test]
fn declaration_hover_and_non_code_definition() {
    let text = "struct Person:\n    age: int?\nprint(\"Person\")\n// Person\n";
    let analysis = AnalysisService::new().analyze_document(&document(text));
    let hover = hover_for(text, &analysis.symbols, Position::new(1, 5)).unwrap();
    assert!(format!("{:?}", hover.contents).contains("age: int?"));
    assert!(definition_range(text, Position::new(2, 9)).is_none());
    assert!(definition_range(text, Position::new(3, 5)).is_none());
}
