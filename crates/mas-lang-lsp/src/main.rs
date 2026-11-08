use mas_lang_lsp::server::Backend;
use tower_lsp::{LspService, Server};

/// 通过标准输入输出启动 mas-lang 语言服务器，确保 stdout 只承载 JSON-RPC 协议数据。
#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
