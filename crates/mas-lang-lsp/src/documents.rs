use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use tower_lsp::lsp_types::Url;

/// 保存一份已经由编辑器打开的 mas-lang 文档状态。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenDocument {
    pub uri: Url,
    pub version: i32,
    pub path: PathBuf,
    pub text: String,
}

/// 维护按 URI 索引的打开文档集合，后续分析与补全统一从这里读取当前文本。
#[derive(Debug, Default)]
pub struct DocumentStore {
    documents: HashMap<Url, Arc<OpenDocument>>,
}

impl DocumentStore {
    /// 创建空文档仓库，用于语言服务器启动后的内存状态初始化。
    pub fn new() -> Self {
        Self::default()
    }

    /// 写入或替换一份打开文档，并返回被缓存的文档副本。
    pub fn upsert(&mut self, uri: Url, version: i32, text: String) -> OpenDocument {
        let path = uri
            .to_file_path()
            .unwrap_or_else(|_| PathBuf::from(uri.path()));
        let document = OpenDocument {
            uri: uri.clone(),
            version,
            path,
            text,
        };
        self.documents.insert(uri, Arc::new(document.clone()));
        document
    }

    /// 按 URI 读取当前缓存文档，供请求型 LSP 能力复用最新文本。
    pub fn get(&self, uri: &Url) -> Option<&OpenDocument> {
        self.documents.get(uri).map(Arc::as_ref)
    }

    /// 返回当前快照的弱引用；替换或关闭文档后，等待中的诊断不会保留旧文本。
    pub(crate) fn snapshot(&self, uri: &Url) -> Option<Weak<OpenDocument>> {
        self.documents.get(uri).map(Arc::downgrade)
    }

    /// 删除已经关闭的文档，并把旧文档返回给调用方用于清理诊断。
    pub fn remove(&mut self, uri: &Url) -> Option<OpenDocument> {
        self.documents.remove(uri).map(Arc::unwrap_or_clone)
    }
}

/// 判断客户端文档是否应该进入 mas-lang 语言分析流程。
pub fn is_mas_document(language_id: &str, uri: &Url) -> bool {
    let language_matches = matches!(language_id, "" | "mas-lang" | "mascript" | "ms");
    let extension_matches = uri.path().ends_with(".ms")
        || uri
            .to_file_path()
            .ok()
            .and_then(|path| path.extension().map(|ext| ext == "ms"))
            .unwrap_or(false);

    language_matches && extension_matches
}
