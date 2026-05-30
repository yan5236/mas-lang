use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};

/// 向 LSP 子进程写入一条带 Content-Length 头的 JSON-RPC 消息。
fn write_lsp_message(input: &mut impl Write, value: Value) {
    let body = value.to_string();
    write!(input, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
    input.flush().unwrap();
}

/// 从 LSP 子进程读取下一条 JSON-RPC 消息。
fn read_lsp_message(output: &mut impl BufRead) -> Value {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = Some(rest.trim().parse::<usize>().unwrap());
        }
    }

    let mut body = vec![0; content_length.unwrap()];
    output.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// 持续接收服务端消息，直到找到指定 id 的响应为止。
fn recv_response_with_id(receiver: &Receiver<Value>, id: i64) -> Value {
    loop {
        let message = receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("等待 LSP 响应超时: id={id}"));
        if message.get("id").and_then(Value::as_i64) == Some(id) {
            return message;
        }
    }
}

/// 持续接收服务端消息，直到找到指定方法的通知为止。
fn recv_notification_with_method(receiver: &Receiver<Value>, method: &str) -> Value {
    loop {
        let message = receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("等待 LSP 通知超时: method={method}"));
        if message.get("method").and_then(Value::as_str) == Some(method) {
            return message;
        }
    }
}

/// 验证真实 stdio 语言服务器能在验证工程中返回符号、补全和悬停结果。
#[test]
fn should_serve_phase_four_language_features_for_validation_project() {
    let sample_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../mskaifa/yanzhenglsp/src/main.ms");
    let sample_text = std::fs::read_to_string(&sample_path).unwrap();
    let document_uri = "file:///E:/xiangmu/mascript/mskaifa/yanzhenglsp/src/main.ms";

    let mut server = Command::new(env!("CARGO_BIN_EXE_mas-lang-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = server.stdin.take().unwrap();
    let stdout = server.stdout.take().unwrap();
    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        while let Ok(message) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read_lsp_message(&mut stdout)
        })) {
            if sender.send(message).is_err() {
                break;
            }
        }
    });

    initialize_server(&mut stdin, &receiver);
    open_validation_document(&mut stdin, &receiver, document_uri, sample_text);
    assert_validation_document_symbols(&mut stdin, &receiver, document_uri);
    assert_validation_completion(&mut stdin, &receiver, document_uri);
    assert_validation_hover(&mut stdin, &receiver, document_uri);
    shutdown_server(&mut stdin, &receiver);

    drop(stdin);
    let status = server.wait_timeout(Duration::from_secs(5)).unwrap();
    assert!(status.is_some_and(|status| status.success()));
}

/// 初始化测试语言服务器，并确认服务端名称符合当前协议入口。
fn initialize_server(stdin: &mut impl Write, receiver: &Receiver<Value>) {
    write_lsp_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": "file:///E:/xiangmu/mascript/mskaifa/yanzhenglsp",
                "capabilities": {}
            }
        }),
    );
    let initialize = recv_response_with_id(receiver, 30);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "mas-lang-lsp");
}

/// 打开验证工程文档，并等待诊断通知表示服务端已完成一次分析。
fn open_validation_document(
    stdin: &mut impl Write,
    receiver: &Receiver<Value>,
    document_uri: &str,
    sample_text: String,
) {
    write_lsp_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": document_uri,
                    "languageId": "mas-lang",
                    "version": 1,
                    "text": sample_text
                }
            }
        }),
    );
    let _ = recv_notification_with_method(receiver, "textDocument/publishDiagnostics");
}

/// 请求文档符号并确认验证样例中的函数可以被 LSP 返回。
fn assert_validation_document_symbols(
    stdin: &mut impl Write,
    receiver: &Receiver<Value>,
    document_uri: &str,
) {
    write_lsp_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 31,
            "method": "textDocument/documentSymbol",
            "params": {
                "textDocument": { "uri": document_uri }
            }
        }),
    );
    let symbols = recv_response_with_id(receiver, 31);
    assert!(symbols["result"].as_array().unwrap().iter().any(|item| {
        item["name"] == "add_score" || item["name"].as_str().unwrap_or("").ends_with("add_score")
    }));
}

/// 请求补全并确认当前文件局部变量会出现在候选列表中。
fn assert_validation_completion(
    stdin: &mut impl Write,
    receiver: &Receiver<Value>,
    document_uri: &str,
) {
    write_lsp_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 32,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": document_uri },
                "position": { "line": 17, "character": 8 }
            }
        }),
    );
    let completion = recv_response_with_id(receiver, 32);
    assert!(
        completion["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["label"] == "score" })
    );
}

/// 请求悬停并确认函数调用位置能展示对应函数签名。
fn assert_validation_hover(stdin: &mut impl Write, receiver: &Receiver<Value>, document_uri: &str) {
    write_lsp_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 33,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": document_uri },
                "position": { "line": 17, "character": 25 }
            }
        }),
    );
    let hover = recv_response_with_id(receiver, 33);
    assert!(
        hover["result"]["contents"]
            .as_str()
            .unwrap_or("")
            .contains("add_score"),
        "{hover}"
    );
}

/// 关闭测试语言服务器，避免 stdio 子进程在测试结束后残留。
fn shutdown_server(stdin: &mut impl Write, receiver: &Receiver<Value>) {
    write_lsp_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 34,
            "method": "shutdown",
            "params": null
        }),
    );
    let shutdown = recv_response_with_id(receiver, 34);
    assert!(shutdown["result"].is_null());

    write_lsp_message(
        stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
}

trait ChildWaitTimeout {
    fn wait_timeout(
        &mut self,
        timeout: Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl ChildWaitTimeout for std::process::Child {
    /// 轮询等待子进程退出，超过指定时间后主动杀掉进程并返回空结果。
    fn wait_timeout(
        &mut self,
        timeout: Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(Some(status));
            }
            if std::time::Instant::now() >= deadline {
                self.kill()?;
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
