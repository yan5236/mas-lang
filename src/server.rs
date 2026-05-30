use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DocumentSymbolParams,
    DocumentSymbolResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, MessageType, OneOf, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind,
};
use tower_lsp::{Client, LanguageServer};

use crate::completion::keyword_items;

/// 保存 LSP 后端运行时需要的协议客户端句柄。
#[derive(Debug)]
pub struct Backend {
    client: Client,
}

impl Backend {
    /// 创建新的语言服务器后端实例，供 `tower-lsp` 在建立服务时注入客户端句柄。
    pub fn new(client: Client) -> Self {
        Self { client }
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

    /// 返回一阶段的基础补全候选，后续阶段会合并当前文件符号和类型信息。
    async fn completion(&self, _params: CompletionParams) -> Result<Option<CompletionResponse>> {
        Ok(Some(CompletionResponse::Array(keyword_items())))
    }

    /// 当前阶段先返回空悬停，保持能力通道可用且不伪造尚未分析出的符号信息。
    async fn hover(&self, _params: HoverParams) -> Result<Option<Hover>> {
        Ok(None)
    }

    /// 当前阶段先返回空文档符号列表，后续符号模型接入后在同一 handler 内填充。
    async fn document_symbol(
        &self,
        _params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        Ok(Some(DocumentSymbolResponse::Nested(Vec::new())))
    }
}
