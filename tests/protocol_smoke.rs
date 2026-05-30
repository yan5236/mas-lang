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

/// 验证真实 stdio 语言服务器能完成初始化、基础补全和关闭协议往返。
#[test]
fn should_handle_stdio_initialize_completion_and_shutdown() {
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

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": "file:///E:/xiangmu/mascript/mskaifa/yanzhenglsp",
                "capabilities": {}
            }
        }),
    );
    let initialize = recv_response_with_id(&receiver, 1);
    assert_eq!(initialize["id"], 1);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "mas-lang-lsp");
    assert!(initialize["result"]["capabilities"]["completionProvider"].is_object());

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );
    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/completion",
            "params": {
                "textDocument": {
                    "uri": "file:///E:/xiangmu/mascript/mskaifa/yanzhenglsp/src/main.ms"
                },
                "position": { "line": 4, "character": 4 }
            }
        }),
    );
    let completion = recv_response_with_id(&receiver, 2);
    assert_eq!(completion["id"], 2);
    let items = completion["result"].as_array().unwrap();
    assert!(items.iter().any(|item| item["label"] == "func"));

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "shutdown",
            "params": null
        }),
    );
    let shutdown = recv_response_with_id(&receiver, 3);
    assert_eq!(shutdown["id"], 3);
    assert!(shutdown["result"].is_null());

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    drop(stdin);
    let status = server.wait_timeout(Duration::from_secs(5)).unwrap();
    assert!(status.is_some_and(|status| status.success()));
}

/// 验证真实 stdio 语言服务器能用验证项目样例完成打开、完整变更和关闭事件链路。
#[test]
fn should_handle_validation_project_document_lifecycle() {
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

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": "file:///E:/xiangmu/mascript/mskaifa/yanzhenglsp",
                "capabilities": {}
            }
        }),
    );
    let initialize = recv_response_with_id(&receiver, 10);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "mas-lang-lsp");

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    );
    write_lsp_message(
        &mut stdin,
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
    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {
                    "uri": document_uri,
                    "version": 2
                },
                "contentChanges": [{
                    "text": sample_text + "\n"
                }]
            }
        }),
    );
    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": {
                "textDocument": {
                    "uri": document_uri
                }
            }
        }),
    );

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "shutdown",
            "params": null
        }),
    );
    let shutdown = recv_response_with_id(&receiver, 11);
    assert!(shutdown["result"].is_null());

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    drop(stdin);
    let status = server.wait_timeout(Duration::from_secs(5)).unwrap();
    assert!(status.is_some_and(|status| status.success()));
}

/// 验证真实 stdio 语言服务器会在打开和修改 `.ms` 文档后发布编译器诊断。
#[test]
fn should_publish_diagnostics_after_open_and_change() {
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

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": "file:///E:/xiangmu/mascript/mskaifa/yanzhenglsp",
                "capabilities": {}
            }
        }),
    );
    let initialize = recv_response_with_id(&receiver, 20);
    assert_eq!(initialize["result"]["serverInfo"]["name"], "mas-lang-lsp");

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": document_uri,
                    "languageId": "mas-lang",
                    "version": 1,
                    "text": "func main():\n    print(@)\n"
                }
            }
        }),
    );
    let open_diagnostics =
        recv_notification_with_method(&receiver, "textDocument/publishDiagnostics");
    assert_eq!(open_diagnostics["params"]["uri"], document_uri);
    assert_eq!(
        open_diagnostics["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        open_diagnostics["params"]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("无法识别的字符")
    );

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": {
                    "uri": document_uri,
                    "version": 2
                },
                "contentChanges": [{
                    "text": "func main():\n    print(1)\n"
                }]
            }
        }),
    );
    let change_diagnostics =
        recv_notification_with_method(&receiver, "textDocument/publishDiagnostics");
    assert_eq!(
        change_diagnostics["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "shutdown",
            "params": null
        }),
    );
    let shutdown = recv_response_with_id(&receiver, 21);
    assert!(shutdown["result"].is_null());

    write_lsp_message(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    drop(stdin);
    let status = server.wait_timeout(Duration::from_secs(5)).unwrap();
    assert!(status.is_some_and(|status| status.success()));
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
