use mas_lang_lsp::documents::{DocumentStore, is_mas_document};
use mas_lang_lsp::lsp_convert::span_to_range;
use mas_lang_lsp::server::Backend;
use maslang::source::Span;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, Position,
    Range, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, Url,
    VersionedTextDocumentIdentifier,
};

/// 验证文档仓库能够按 URI 保存、完整替换和删除编辑器中的当前文本。
#[test]
fn should_store_replace_and_remove_open_documents() {
    let uri = Url::parse("file:///workspace/src/main.ms").unwrap();
    let mut store = DocumentStore::new();

    let opened = store.upsert(uri.clone(), 1, "func main() {}".to_string());
    assert_eq!(opened.version, 1);
    assert_eq!(store.get(&uri).unwrap().text, "func main() {}");

    let changed = store.upsert(uri.clone(), 2, "func main() {\n  return\n}".to_string());
    assert_eq!(changed.version, 2);
    assert_eq!(store.get(&uri).unwrap().text, "func main() {\n  return\n}");

    let removed = store.remove(&uri).unwrap();
    assert_eq!(removed.version, 2);
    assert!(store.get(&uri).is_none());
}

/// 验证 LSP 文档事件会驱动后端缓存打开、完整替换和关闭当前文档。
#[tokio::test]
async fn should_update_backend_documents_from_lsp_events() {
    let (service, _socket) = tower_lsp::LspService::new(Backend::new);
    let backend = service.inner();
    let uri = Url::parse("file:///workspace/src/main.ms").unwrap();

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "mas-lang".to_string(),
                version: 1,
                text: "func main() {}".to_string(),
            },
        })
        .await;

    assert_eq!(
        backend.document_text(&uri).await,
        Some("func main() {}".to_string())
    );

    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "func main() {\n  return\n}".to_string(),
            }],
        })
        .await;

    assert_eq!(
        backend.document_text(&uri).await,
        Some("func main() {\n  return\n}".to_string())
    );

    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await;

    assert!(!backend.has_document(&uri).await);
}

/// 验证语言过滤同时检查 languageId 与 `.ms` 扩展名，避免无关文件触发分析。
#[test]
fn should_accept_only_mas_language_ms_documents() {
    let mas_uri = Url::parse("file:///workspace/src/main.ms").unwrap();
    let md_uri = Url::parse("file:///workspace/src/readme.md").unwrap();

    assert!(is_mas_document("mas-lang", &mas_uri));
    assert!(is_mas_document("ms", &mas_uri));
    assert!(!is_mas_document("plaintext", &mas_uri));
    assert!(!is_mas_document("mas-lang", &md_uri));
}

/// 验证源码字节跨度会转换为 LSP 使用的零基 UTF-16 range。
#[test]
fn should_convert_ascii_span_to_lsp_range() {
    let text = "func main() {\n  return 1\n}\n";
    let start = text.find("return").unwrap();
    let end = start + "return".len();

    assert_eq!(
        span_to_range(text, Span::new(start, end)),
        Range {
            start: Position::new(1, 2),
            end: Position::new(1, 8),
        }
    );
}

/// 验证中文字符按 UTF-16 单元计列号，而不是按 UTF-8 字节数计列号。
#[test]
fn should_convert_chinese_span_to_utf16_columns() {
    let text = "变量一 = 1\n打印(变量一)\n";
    let start = text.find("打印").unwrap();
    let end = start + "打印".len();

    assert_eq!(
        span_to_range(text, Span::new(start, end)),
        Range {
            start: Position::new(1, 0),
            end: Position::new(1, 2),
        }
    );
}

/// 验证多行跨度的结束位置会落在最后一行的 UTF-16 列号。
#[test]
fn should_convert_multiline_span_to_lsp_range() {
    let text = "func main() {\n  变量一 = 1\n  return 变量一\n}\n";
    let start = text.find("变量一").unwrap();
    let end = text.find("return").unwrap() + "return 变量一".len();

    assert_eq!(
        span_to_range(text, Span::new(start, end)),
        Range {
            start: Position::new(1, 2),
            end: Position::new(2, 12),
        }
    );
}
