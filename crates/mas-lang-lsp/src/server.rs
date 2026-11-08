use std::sync::{Arc, Weak};
use std::time::Duration;

use maslang::source::SourceFile;
use std::collections::HashMap;
use tokio::sync::{Mutex, RwLock};
use tower_lsp::jsonrpc::Result;

use tower_lsp::lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams, Location,
    MessageType, OneOf, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer};

use crate::analysis::AnalysisService;
use crate::completion::completion_items_for;
use crate::documents::{DocumentStore, OpenDocument, is_mas_document};
use crate::hover::hover_for;
use crate::symbols::{SymbolSnapshot, document_symbols_for};

/// 保存 LSP 后端运行时需要的协议客户端句柄。
#[derive(Clone, Debug)]
pub struct Backend {
    client: Client,
    documents: Arc<RwLock<DocumentStore>>,
    snapshots: Arc<RwLock<HashMap<Url, (Weak<OpenDocument>, SymbolSnapshot)>>>,
    analysis: Arc<AnalysisService>,
    analysis_gate: Arc<Mutex<()>>,
}

impl Backend {
    /// 创建新的语言服务器后端实例，供 `tower-lsp` 在建立服务时注入客户端句柄。
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(DocumentStore::new())),
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            analysis: Arc::new(AnalysisService::new()),
            analysis_gate: Arc::new(Mutex::new(())),
        }
    }

    /// 判断 URI 是否对应当前已经缓存的打开文档，主要供协议级测试和后续能力复用。
    pub async fn has_document(&self, uri: &Url) -> bool {
        self.documents.read().await.get(uri).is_some()
    }

    /// 读取指定 URI 的当前文档文本，供请求型能力在后续阶段复用最新编辑器内容。
    pub async fn document_text(&self, uri: &Url) -> Option<String> {
        self.documents
            .read()
            .await
            .get(uri)
            .map(|document| document.text.clone())
    }

    /// 返回同一份当前文本及其符号；后台诊断尚未完成时，只解析当前文件以响应交互请求。
    async fn document_with_symbols(
        &self,
        uri: &Url,
    ) -> Option<(Arc<OpenDocument>, SymbolSnapshot)> {
        let document = self.documents.read().await.snapshot(uri)?.upgrade()?;
        let snapshots = self.snapshots.read().await;
        let symbols = snapshots
            .get(uri)
            .filter(|(snapshot, _)| snapshot.as_ptr() == Arc::as_ptr(&document))
            .map(|(_, symbols)| symbols.clone());
        drop(snapshots);
        let symbols = symbols.unwrap_or_else(|| {
            let source =
                SourceFile::new(document.path.display().to_string(), document.text.clone());
            self.analysis.symbols_for_source(&source)
        });
        Some((document, symbols))
    }

    /// 安排当前快照的后台诊断；连续编辑期间仅分析安静期结束时仍有效的文档。
    fn schedule_diagnostics(&self, document: Weak<OpenDocument>, delay: Duration) {
        let backend = self.clone();
        tokio::spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            backend.publish_document_diagnostics(document).await;
        });
    }

    /// 每次只运行一份阻塞前端分析，发布前再次检查快照身份，避免覆盖新版本或已关闭文档。
    async fn publish_document_diagnostics(&self, pending: Weak<OpenDocument>) {
        let _permit = self.analysis_gate.lock().await;
        let Some(document) = pending.upgrade() else {
            return;
        };
        if !self
            .documents
            .read()
            .await
            .snapshot(&document.uri)
            .is_some_and(|current| current.ptr_eq(&pending))
        {
            return;
        }
        let service = Arc::clone(&self.analysis);
        let input = Arc::clone(&document);
        let Ok(analysis) =
            tokio::task::spawn_blocking(move || service.analyze_document(&input)).await
        else {
            return;
        };
        let documents = self.documents.read().await;
        let uri = &document.uri;
        if !documents
            .get(uri)
            .is_some_and(|current| std::ptr::eq(current, document.as_ref()))
        {
            return;
        }
        self.snapshots
            .write()
            .await
            .insert(uri.clone(), (pending, analysis.symbols));
        self.client
            .publish_diagnostics(uri.clone(), analysis.diagnostics, Some(document.version))
            .await;
    }
}

/// 构造初始化响应中的能力声明，避免 handler 与测试重复拼装能力字段。
pub fn initial_server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![".".into()]),
            ..CompletionOptions::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    /// 响应客户端初始化请求，并只声明当前阶段已经规划暴露的基础能力。
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: initial_server_capabilities(),
            server_info: Some(ServerInfo {
                name: "mas-lang-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    /// 在客户端完成初始化后发送协议内日志，避免向 stdout 写入非 JSON-RPC 文本。
    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "mas-lang language server initialized")
            .await;
    }

    /// 响应标准关闭请求，当前阶段不持有需要额外释放的外部资源。
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    /// 缓存打开的 Mas lang 文档并立即安排首轮后台诊断。
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        if !is_mas_document(&document.language_id, &document.uri) {
            return;
        }

        let uri = document.uri.clone();
        let mut documents = self.documents.write().await;
        documents.upsert(document.uri, document.version, document.text);
        if let Some(snapshot) = documents.snapshot(&uri) {
            self.schedule_diagnostics(snapshot, Duration::ZERO);
        }
    }

    /// 立即缓存较新的完整文本，等连续编辑停顿后再运行完整模块分析。
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };

        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let mut documents = self.documents.write().await;
        if !documents
            .get(&uri)
            .is_some_and(|current| version > current.version)
        {
            return;
        }

        documents.upsert(uri.clone(), version, change.text);
        if let Some(snapshot) = documents.snapshot(&uri) {
            self.schedule_diagnostics(snapshot, Duration::from_millis(120));
        }
    }

    /// 关闭文档并按同一顺序清除符号与诊断，避免后台结果在关闭后重新出现。
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let mut documents = self.documents.write().await;
        let removed = documents.remove(&uri);
        if removed.is_some() {
            self.snapshots.write().await.remove(&uri);
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
    }

    /// 返回关键字、类型、内建函数和当前文件符号组成的补全列表。
    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some((document, snapshot)) = self.document_with_symbols(&uri).await else {
            return Ok(Some(CompletionResponse::Array(completion_items_for(
                "",
                &SymbolSnapshot::default(),
                position,
            ))));
        };
        Ok(Some(CompletionResponse::Array(completion_items_for(
            &document.text,
            &snapshot,
            position,
        ))))
    }

    /// 根据当前文件符号快照返回光标处符号的声明种类和详情。
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some((document, snapshot)) = self.document_with_symbols(&uri).await else {
            return Ok(None);
        };
        Ok(hover_for(&document.text, &snapshot, position))
    }

    /// 跳转到当前内存文档中的结构体或字段声明；未知名称和跨文件目标返回空结果。
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some(text) = self.document_text(&uri).await else {
            return Ok(None);
        };
        Ok(crate::record_support::definition_range(&text, position)
            .map(|range| GotoDefinitionResponse::Scalar(Location { uri, range })))
    }

    /// 返回当前文档中已经分析出的结构体、字段、函数、变量、常量和导入符号。
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let snapshot = self
            .document_with_symbols(&uri)
            .await
            .map(|(_, symbols)| symbols)
            .unwrap_or_default();
        Ok(Some(DocumentSymbolResponse::Nested(document_symbols_for(
            &snapshot,
        ))))
    }
}
