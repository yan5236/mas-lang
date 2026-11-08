use mas_lang_lsp::analysis::AnalysisService;
use mas_lang_lsp::completion::completion_items_for;
use mas_lang_lsp::documents::OpenDocument;
use mas_lang_lsp::hover::hover_for;
use mas_lang_lsp::symbols::{SymbolSnapshot, document_symbols_for};
use maslang::settings::{CompilerSettings, UiLanguage};
use tower_lsp::lsp_types::{
    CompletionItemKind, DocumentSymbol, InsertTextFormat, Position, SymbolKind, Url,
};

/// 构造测试用打开文档，保证四阶段能力都基于同一份编辑器内存文本运行。
fn open_document(text: &str) -> OpenDocument {
    let uri = Url::parse("file:///E:/xiangmu/mascript/mskaifa/yanzhenglsp/src/main.ms").unwrap();
    OpenDocument {
        uri,
        version: 1,
        path: "E:/xiangmu/mascript/mskaifa/yanzhenglsp/src/main.ms".into(),
        text: text.to_string(),
    }
}

/// 使用中文编译器设置分析文档，减少每个测试重复创建服务的样板代码。
fn analyze(text: &str) -> SymbolSnapshot {
    let service = AnalysisService::with_settings(CompilerSettings::new(UiLanguage::ZhCn));
    service.analyze_document(&open_document(text)).symbols
}

/// 验证成功分析后能提取函数、导入、变量和常量符号，并保留可展示的详情。
#[test]
fn should_extract_document_symbols_from_valid_program() {
    let snapshot = analyze(
        "import time { unix_seconds }\nconst title:string = \"demo\"\nfunc add(a:int, b:int) -> int:\n    var total:int = a + b\n    return total\n",
    );

    let symbols = snapshot.symbols();
    assert!(
        symbols
            .iter()
            .any(|symbol| { symbol.name == "time.unix_seconds" && symbol.kind == SymbolKind::MODULE })
    );
    assert!(symbols.iter().any(|symbol| {
        symbol.name == "title"
            && symbol.kind == SymbolKind::CONSTANT
            && symbol.detail.as_deref() == Some("const string")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol.name == "add"
            && symbol.kind == SymbolKind::FUNCTION
            && symbol.detail.as_deref() == Some("func add(a:int, b:int) -> int")
    }));
    assert!(symbols.iter().any(|symbol| {
        symbol.name == "total"
            && symbol.kind == SymbolKind::VARIABLE
            && symbol.detail.as_deref() == Some("var int")
    }));

    let response = document_symbols_for(&snapshot);
    assert!(response.iter().any(|symbol| symbol.name == "add"));
}

/// 验证语法错误时仍能通过降级扫描识别已经完整书写的函数和声明名。
#[test]
fn should_fallback_scan_symbols_when_program_has_syntax_error() {
    let snapshot = analyze(
        "func ready(value:int) -> int:\n    return value\nfunc broken(:\n    var local:int = 1\n",
    );

    assert!(snapshot.is_fallback());
    let symbols = snapshot.symbols();
    assert!(symbols.iter().any(|symbol| symbol.name == "ready"));
    assert!(symbols.iter().any(|symbol| symbol.name == "local"));
}

/// 验证降级文档符号仍满足 LSP 对 selectionRange 必须落在 range 内部的协议要求。
#[test]
fn should_keep_fallback_document_symbol_selection_inside_range() {
    let snapshot = analyze(
        "func ready(value:int) -> int:\n    return value\nfunc broken(:\n    var local:int = 1\nif condition :\n",
    );
    let response = document_symbols_for(&snapshot);

    assert!(snapshot.is_fallback());
    assert!(
        response
            .iter()
            .any(|symbol| symbol.name == "broken" && symbol_range_contains_selection(symbol))
    );
    assert!(
        response
            .iter()
            .any(|symbol| symbol.name == "local" && symbol_range_contains_selection(symbol))
    );
}

/// 验证补全会组合关键字、基础类型、内建函数和当前文件可见符号。
#[test]
fn should_complete_keywords_types_builtins_and_symbols() {
    let text = "func add(a:int, b:int) -> int:\n    return a + b\nfunc main():\n    var score:int = add(1, 2)\n    sc\n";
    let snapshot = analyze(text);
    let items = completion_items_for(text, &snapshot, Position::new(4, 6));

    assert!(
        items
            .iter()
            .any(|item| { item.label == "func" && item.kind == Some(CompletionItemKind::KEYWORD) })
    );
    assert!(items.iter().any(|item| {
        item.label == "int" && item.kind == Some(CompletionItemKind::TYPE_PARAMETER)
    }));
    assert!(
        items.iter().any(|item| {
            item.label == "print" && item.kind == Some(CompletionItemKind::FUNCTION)
        })
    );
    assert!(
        items.iter().any(|item| {
            item.label == "score" && item.kind == Some(CompletionItemKind::VARIABLE)
        })
    );
    assert!(
        items
            .iter()
            .any(|item| { item.label == "add" && item.kind == Some(CompletionItemKind::FUNCTION) })
    );
}

/// 验证块级关键字、main 函数、函数调用和内建函数使用 snippet 补全文本。
#[test]
fn should_complete_block_and_function_items_as_snippets() {
    let text = "func add(a:int, b:int) -> int:\n    return a + b\n";
    let snapshot = analyze(text);
    let items = completion_items_for(text, &snapshot, Position::new(2, 0));

    assert!(items.iter().any(|item| {
        item.label == "func"
            && item.insert_text.as_deref() == Some("func ${1}(${2}) :\n    ${0}")
            && item.insert_text_format == Some(InsertTextFormat::SNIPPET)
    }));
    assert!(items.iter().any(|item| {
        item.label == "main"
            && item.insert_text.as_deref() == Some("func main() :\n    ${0}")
            && item.insert_text_format == Some(InsertTextFormat::SNIPPET)
    }));
    assert!(items.iter().any(|item| {
        item.label == "if"
            && item.insert_text.as_deref() == Some("if ${1} :\n    ${0}")
            && item.insert_text_format == Some(InsertTextFormat::SNIPPET)
    }));
    assert!(items.iter().any(|item| {
        item.label == "print"
            && item.insert_text.as_deref() == Some("print(${1})")
            && item.insert_text_format == Some(InsertTextFormat::SNIPPET)
    }));
    assert!(items.iter().any(|item| {
        item.label == "add"
            && item.insert_text.as_deref() == Some("add(${1})")
            && item.insert_text_format == Some(InsertTextFormat::SNIPPET)
    }));
}

/// 验证函数名位置不会返回会重复插入 `func` 的 main 函数骨架。
#[test]
fn should_not_complete_main_skeleton_inside_function_name() {
    let text = "func ma";
    let snapshot = analyze(text);
    let items = completion_items_for(text, &snapshot, Position::new(0, 7));

    assert!(!items.iter().any(|item| {
        item.label == "main" && item.insert_text.as_deref() == Some("func main() :\n    ${0}")
    }));
}

/// 验证光标处于字符串字面量或注释内时不返回语法关键字补全。
#[test]
fn should_suppress_keyword_completion_inside_string_or_comment() {
    let text = "func main():\n    print(\"hello\")\n    // todo\n";
    let snapshot = analyze(text);
    let string_items = completion_items_for(text, &snapshot, Position::new(1, 12));
    let comment_items = completion_items_for(text, &snapshot, Position::new(2, 8));

    assert!(!string_items.iter().any(|item| item.label == "func"));
    assert!(!comment_items.iter().any(|item| item.label == "func"));
    assert!(string_items.iter().any(|item| item.label == "print"));
}

/// 验证悬停能根据光标所在符号返回声明种类和函数签名。
#[test]
fn should_hover_current_symbol_detail() {
    let text =
        "func add(a:int, b:int) -> int:\n    return a + b\nfunc main():\n    print(add(1, 2))\n";
    let snapshot = analyze(text);
    let hover = hover_for(text, &snapshot, Position::new(3, 11)).unwrap();
    let value = match hover.contents {
        tower_lsp::lsp_types::HoverContents::Scalar(
            tower_lsp::lsp_types::MarkedString::String(value),
        ) => value,
        other => panic!("unexpected hover contents: {other:?}"),
    };

    assert!(value.contains("函数"), "{value}");
    assert!(value.contains("func add(a:int, b:int) -> int"), "{value}");
}

/// 判断文档符号的选择范围是否被完整包含在符号范围中，复现 VS Code 对 documentSymbol 响应的协议校验。
fn symbol_range_contains_selection(symbol: &DocumentSymbol) -> bool {
    position_le(symbol.range.start, symbol.selection_range.start)
        && position_le(symbol.selection_range.end, symbol.range.end)
}

/// 按 LSP 的行列顺序比较两个位置，避免测试里重复展开边界判断。
fn position_le(left: Position, right: Position) -> bool {
    left.line < right.line || (left.line == right.line && left.character <= right.character)
}
