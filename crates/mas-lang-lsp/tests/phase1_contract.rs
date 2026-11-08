use mas_lang_lsp::documents::is_mas_document;
use mas_lang_lsp::server::initial_server_capabilities;
use tower_lsp::lsp_types::{
    HoverProviderCapability, OneOf, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};

/// 验证基础能力与当前文件结构体定义跳转已声明，未实现的工作区能力仍不声明。
#[test]
fn should_declare_only_phase_one_capabilities() {
    let capabilities = initial_server_capabilities();

    assert_eq!(
        capabilities.text_document_sync,
        Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
    );
    assert!(capabilities.completion_provider.is_some());
    assert_eq!(
        capabilities.hover_provider,
        Some(HoverProviderCapability::Simple(true))
    );
    assert_eq!(
        capabilities.document_symbol_provider,
        Some(OneOf::Left(true))
    );
    assert_eq!(capabilities.definition_provider, Some(OneOf::Left(true)));
    assert!(capabilities.references_provider.is_none());
    assert!(capabilities.rename_provider.is_none());
    assert!(capabilities.document_formatting_provider.is_none());
}

/// 验证文档过滤逻辑只接收 Mas lang 源文件，避免无关文件进入语言分析流程。
#[test]
fn should_filter_non_mas_documents() {
    let mas_uri = Url::parse("file:///workspace/src/main.ms").unwrap();
    let txt_uri = Url::parse("file:///workspace/src/main.txt").unwrap();

    assert!(is_mas_document("mas-lang", &mas_uri));
    assert!(is_mas_document("mascript", &mas_uri));
    assert!(is_mas_document("", &mas_uri));
    assert!(!is_mas_document("plaintext", &mas_uri));
    assert!(!is_mas_document("mas-lang", &txt_uri));
}
