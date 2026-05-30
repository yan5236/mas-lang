use std::sync::Arc;

use std::collections::HashMap;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;

use tower_lsp::lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentSymbolParams,
    DocumentSymbolResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, MessageType, OneOf, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer};

use crate::analysis::AnalysisService;
use crate::completion::completion_items_for;
use crate::documents::{DocumentStore, is_mas_document};
use crate::hover::hover_for;
use crate::symbols::{SymbolSnapshot, document_symbols_for};

/// 保存 LSP 后端运行时需要的协议客户端句柄。
#[derive(Debug)]
pub struct Backend {
    client: Client,
    documents: Arc<RwLock<DocumentStore>>,
    snapshots: Arc<RwLock<HashMap<Url, SymbolSnapshot>>>,
    analysis: Arc<AnalysisService>,
}

impl Backend {
    /// 创建新的语言服务器后端实例，供 `tower-lsp` 在建立服务时注入客户端句柄。
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(DocumentStore::new())),
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            analysis: Arc::new(AnalysisService::new()),
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

    /// 分析指定打开文档并向客户端发布最新诊断，避免事件处理器重复诊断流程。
    async fn publish_document_diagnostics(&self, uri: &Url) {
        let Some(document) = self.documents.read().await.get(uri).cloned() else {
            return;
        };
        let analysis = self.analysis.analyze_document(&document);
        self.snapshots
            .write()
            .await
            .insert(uri.clone(), analysis.symbols);
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
            ..CompletionOptions::default()
        }),
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

    /// 处理文档打开事件，缓存 `.ms` 文档的完整文本并为后续分析保留当前版本。
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        if !is_mas_document(&document.language_id, &document.uri) {
            return;
        }

        let uri = document.uri.clone();
        self.documents
            .write()
            .await
            .upsert(document.uri, document.version, document.text);
        self.publish_document_diagnostics(&uri).await;
    }

    /// 处理完整文本变更事件，按 LSP FULL sync 语义用最后一条完整文本替换缓存。
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };

        let uri = params.text_document.uri;
        if !self.has_document(&uri).await {
            return;
        }

        self.documents
            .write()
            .await
            .upsert(uri.clone(), params.text_document.version, change.text);
        self.publish_document_diagnostics(&uri).await;
    }

    /// 处理文档关闭事件，删除缓存并发布空诊断以清理客户端旧问题列表。
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let removed = self.documents.write().await.remove(&uri);
        if removed.is_some() {
            self.snapshots.write().await.remove(&uri);
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
    }

    /// 返回关键字、类型、内建函数和当前文件符号组成的补全列表。
    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some(text) = self.document_text(&uri).await else {
            return Ok(Some(CompletionResponse::Array(completion_items_for(
                "",
                &SymbolSnapshot::default(),
                position,
            ))));
        };
        let snapshot = self
            .snapshots
            .read()
            .await
            .get(&uri)
            .cloned()
            .unwrap_or_default();
        Ok(Some(CompletionResponse::Array(completion_items_for(
            &text, &snapshot, position,
        ))))
    }

    /// 根据当前文件符号快照返回光标处符号的声明种类和详情。
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some(text) = self.document_text(&uri).await else {
            return Ok(None);
        };
        let snapshot = self
            .snapshots
            .read()
            .await
            .get(&uri)
            .cloned()
            .unwrap_or_default();
        Ok(hover_for(&text, &snapshot, position))
    }

    /// 返回当前文档中已经分析出的函数、变量、常量和导入符号。
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let snapshot = self
            .snapshots
            .read()
            .await
            .get(&uri)
            .cloned()
            .unwrap_or_default();
        Ok(Some(DocumentSymbolResponse::Nested(document_symbols_for(
            &snapshot,
        ))))
    }
}
