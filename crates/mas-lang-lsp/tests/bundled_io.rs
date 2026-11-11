// 随附 io 的编辑器侧回归：没有 settings.toml 的单个 .ms 文件也必须解析
// `import io { ... }` 的真实公开导出，并让 `write_text_to`、`stdout`、
// `WriteResult.ok`、`FileMode.Read`、`IOErrorKind.NotFound` 参与语义分析。

use std::fs;
use std::path::Path;

use mas_lang_lsp::analysis::AnalysisService;
use mas_lang_lsp::documents::OpenDocument;
use maslang::settings::{CompilerSettings, UiLanguage};
use maslang::temp_directory::TemporaryDirectory;
use tower_lsp::lsp_types::Url;

/// 固定中文设置，保证诊断语言与断言一致。
fn zh_settings() -> CompilerSettings {
    CompilerSettings::new(UiLanguage::ZhCn)
}

/// 确认临时目录到盘符根的各级上级都没有项目清单，确保用例真的处于“无清单”场景。
fn no_manifest_above(path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .all(|ancestor| !ancestor.join("settings.toml").is_file())
}

/// 在临时目录写一份源码并以编辑器内存文本打开；文件本身存在，供路径解析使用。
fn open_document(root: &Path, text: &str) -> OpenDocument {
    let path = root.join("main.ms");
    fs::write(&path, text).expect("写入临时源码失败");
    OpenDocument {
        uri: Url::from_file_path(&path).expect("临时路径必须能转成 file URL"),
        path,
        version: 1,
        text: text.to_string(),
    }
}

/// 无项目清单的单文件导入随附 io：诊断为空，且公开符号与字段都真正参与语义分析。
#[test]
fn analysis_without_manifest_resolves_bundled_io_exports() {
    let root = TemporaryDirectory::create("lsp-bundled-io").expect("创建临时目录失败");
    if !no_manifest_above(root.path()) {
        eprintln!(
            "跳过 analysis_without_manifest_resolves_bundled_io_exports：临时目录上级存在 settings.toml"
        );
        return;
    }
    let text = r#"import io {
    FileMode,
    IOErrorKind,
    open,
    stdout,
    write_text_to,
}

func main():
    var result = write_text_to(stdout(), "x")
    if not result.ok:
        print("write failed")
    var opened = open("state.bin", FileMode.Read)
    if opened.code == IOErrorKind.NotFound:
        print("missing")
    print(string(result.ok))
"#;
    let document = open_document(root.path(), text);
    let service = AnalysisService::with_settings(zh_settings());
    let analysis = service.analyze_document(&document);
    assert!(
        analysis.diagnostics.is_empty(),
        "无清单单文件导入随附 io 不应产生诊断：{:?}",
        analysis.diagnostics
    );
}

/// 对照用例：同名但写错的公开符号必须产生诊断，证明上一条用例不是“只要 import 就静默通过”。
#[test]
fn analysis_reports_unknown_symbol_after_bundled_io_import() {
    let root = TemporaryDirectory::create("lsp-bundled-io-negative").expect("创建临时目录失败");
    if !no_manifest_above(root.path()) {
        eprintln!(
            "跳过 analysis_reports_unknown_symbol_after_bundled_io_import：临时目录上级存在 settings.toml"
        );
        return;
    }
    let text = r#"import io { stdout, write_text_to }

func main():
    write_text_to(stdout(), "x")
    no_such_io_symbol()
"#;
    let document = open_document(root.path(), text);
    let service = AnalysisService::with_settings(zh_settings());
    let analysis = service.analyze_document(&document);
    assert!(
        !analysis.diagnostics.is_empty(),
        "不存在的符号必须出现在编辑器诊断中"
    );
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("no_such_io_symbol")),
        "诊断应指出不存在的符号名：{:?}",
        analysis.diagnostics
    );
}
