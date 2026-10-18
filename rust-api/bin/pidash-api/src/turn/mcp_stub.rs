//! Stub MCP server for the parity suite (`mcp_parity`).
//!
//! A hand-rolled HTTP/1.1 server over tokio, mirroring the Python probe's
//! stub one behavior at a time: Streamable POST/GET/DELETE, legacy SSE,
//! redirects, sleeps, kills, per-method statuses and raw bodies. Every
//! request is logged for the wire assertions. Nothing here ships; it only
//! feeds `#[tokio::test]`s.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex, Notify};

/// One logged exchange: method, path, lowercased header names, raw body.
#[derive(Debug, Clone)]
pub struct LoggedRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

/// The stub half of a fixture: every knob the probe's stub modes had.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct StubDsl {
    pub framing: String,
    pub call: String,
    pub sleep: HashMap<String, f64>,
    pub status_all: Option<u16>,
    pub status_methods: HashMap<String, u16>,
    pub raw: HashMap<String, RawResponse>,
    pub notif_status: Option<u16>,
    pub delete_status: Option<u16>,
    pub die_on: Vec<String>,
    pub init_error: bool,
    pub init_version: Option<String>,
    pub instructions: Option<String>,
    pub fail_list: bool,
    pub pages: Option<Vec<Vec<Value>>>,
    pub dup_cursor: bool,
    pub output_schema: bool,
    pub sse_no_endpoint: bool,
    pub sse_silent: bool,
    pub sse_post_status: Option<u16>,
    pub sse_endpoint: Option<String>,
    pub sse_empty: Option<String>,
    pub sse_redirect: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct RawResponse {
    pub status: u16,
    pub content_type: String,
    pub body: String,
}

impl StubDsl {
    fn framing(&self) -> &str {
        if self.framing.is_empty() {
            "json"
        } else {
            &self.framing
        }
    }

    fn call(&self) -> &str {
        if self.call.is_empty() {
            "text"
        } else {
            &self.call
        }
    }
}

fn echo_tool() -> Value {
    json!({
        "name": "echo",
        "description": "Echo back the input text.",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        },
    })
}

fn add_tool() -> Value {
    json!({
        "name": "add",
        "description": "Add two numbers.",
        "inputSchema": {
            "type": "object",
            "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
        },
    })
}

fn output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"a": {"type": "integer"}},
        "required": ["a"],
    })
}

/// The running stub: its base URL, the request log, and teardown.
pub struct StubServer {
    /// `http://127.0.0.1:<port>`
    pub base: String,
    log: Arc<Mutex<Vec<LoggedRequest>>>,
    #[allow(dead_code)] // fixture stub bookkeeping; only the senders matter
    queues: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
    done: Arc<Notify>,
    finished: Arc<Mutex<bool>>,
    listener: Option<tokio::task::JoinHandle<()>>,
    conns: Arc<Mutex<tokio::task::JoinSet<()>>>,
}

impl StubServer {
    pub async fn start(dsl: StubDsl) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("stub binds");
        let port = listener.local_addr().expect("stub addr").port();
        let dsl = Arc::new(dsl);
        let log = Arc::new(Mutex::new(Vec::new()));
        let queues: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let done = Arc::new(Notify::new());
        let finished = Arc::new(Mutex::new(false));
        let conns = Arc::new(Mutex::new(tokio::task::JoinSet::new()));
        let accept = tokio::spawn({
            let dsl = dsl.clone();
            let log = log.clone();
            let queues = queues.clone();
            let done = done.clone();
            let finished = finished.clone();
            let conns = conns.clone();
            async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        break;
                    };
                    if *finished.lock().await {
                        break;
                    }
                    let mut conns = conns.lock().await;
                    conns.spawn(handle_conn(
                        stream,
                        dsl.clone(),
                        log.clone(),
                        queues.clone(),
                        done.clone(),
                        finished.clone(),
                        port,
                    ));
                }
            }
        });
        Self {
            base: format!("http://127.0.0.1:{port}"),
            log,
            queues,
            done,
            finished,
            listener: Some(accept),
            conns,
        }
    }

    pub async fn requests(&self) -> Vec<LoggedRequest> {
        self.log.lock().await.clone()
    }

    /// Graceful stop: no new connections, held streams released.
    pub async fn shutdown(mut self) {
        *self.finished.lock().await = true;
        self.done.notify_waiters();
        if let Some(listener) = self.listener.take() {
            listener.abort();
        }
        let mut conns = self.conns.lock().await;
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            while conns.join_next().await.is_some() {}
        })
        .await;
        conns.abort_all();
    }

    /// Hard kill: listener and every connection dropped at once, so the
    /// port refuses and pooled connections die together.
    pub async fn kill(&self) {
        *self.finished.lock().await = true;
        self.done.notify_waiters();
        self.conns.lock().await.abort_all();
    }
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Option<Request> {
    loop {
        if let Some(end) = find_headers_end(buf) {
            let head = String::from_utf8_lossy(&buf[..end]).into_owned();
            let mut lines = head.split("\r\n");
            let request_line = lines.next().unwrap_or_default().to_string();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_string();
            let path = parts.next().unwrap_or_default().to_string();
            let mut headers = HashMap::new();
            for line in lines {
                if line.is_empty() {
                    continue;
                }
                if let Some((name, value)) = line.split_once(':') {
                    headers.insert(name.trim().to_lowercase(), value.trim().to_string());
                }
            }
            let length: usize = headers
                .get("content-length")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let total = end + length;
            while buf.len() < total {
                let mut chunk = [0u8; 4096];
                match stream.read(&mut chunk).await.ok()? {
                    0 => return None,
                    n => buf.extend_from_slice(&chunk[..n]),
                }
            }
            let body = buf[end..total].to_vec();
            *buf = buf[total..].to_vec();
            return Some(Request {
                method,
                path,
                headers,
                body,
            });
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await.ok()? {
            0 => return None,
            n => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

fn find_headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|pos| pos + 4)
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, String)],
    body: &[u8],
) {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\ncontent-length: {}\r\nconnection: keep-alive\r\n",
        status_text(status),
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
}

async fn handle_conn(
    mut stream: TcpStream,
    dsl: Arc<StubDsl>,
    log: Arc<Mutex<Vec<LoggedRequest>>>,
    queues: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
    done: Arc<Notify>,
    finished: Arc<Mutex<bool>>,
    port: u16,
) {
    let mut buf = Vec::new();
    loop {
        if *finished.lock().await {
            return;
        }
        let Some(request) = read_request(&mut stream, &mut buf).await else {
            return;
        };
        if dsl.sse_silent && request.method == "GET" {
            // Silence, not even headers: hold without answering.
            log_request(&log, &request).await;
            done.notified().await;
            return;
        }
        // The legacy SSE stream takes over its connection.
        if request.method == "GET" && request.path == "/sse" {
            log_request(&log, &request).await;
            if let Some(target) = dsl.sse_redirect.as_deref() {
                write_response(&mut stream, 307, &[("location", target.to_string())], b"").await;
                return;
            }
            serve_sse_get(&mut stream, &dsl, &queues, &done).await;
            return;
        }
        if !serve_one(&mut stream, &request, &dsl, &queues, &log, port).await {
            return;
        }
    }
}

async fn log_request(log: &Arc<Mutex<Vec<LoggedRequest>>>, request: &Request) {
    log.lock().await.push(LoggedRequest {
        method: request.method.clone(),
        path: request.path.clone(),
        headers: request.headers.clone(),
        body: String::from_utf8_lossy(&request.body).into_owned(),
    });
}

/// Serve one keep-alive request. Returns false when the connection dies.
async fn serve_one(
    stream: &mut TcpStream,
    request: &Request,
    dsl: &StubDsl,
    queues: &Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
    log: &Arc<Mutex<Vec<LoggedRequest>>>,
    port: u16,
) -> bool {
    log_request(log, request).await;
    let body_text = String::from_utf8_lossy(&request.body).into_owned();
    let message: Option<Value> = serde_json::from_str(&body_text).ok();
    let method = message
        .as_ref()
        .and_then(|message| message.get("method"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // Redirect endpoints.
    if request.path == "/mcp307" {
        let location = format!("http://127.0.0.1:{port}/mcp2");
        write_response(stream, 307, &[("location", location)], b"").await;
        return true;
    }
    if request.path == "/mcpx" {
        // Userinfo, query and fragment ride along so the redirect message
        // assertion proves they are stripped from the reported location.
        write_response(
            stream,
            307,
            &[(
                "location",
                "http://probe:secret@127.0.0.1:9/mcp?session=abc#frag".to_string(),
            )],
            b"",
        )
        .await;
        return true;
    }

    // Legacy SSE posts: 202 plus the answer on the held stream.
    if request.path.starts_with("/messages") {
        if let Some(status) = dsl.sse_post_status {
            write_response(stream, status, &[], b"").await;
            return true;
        }
        if let Some(message) = &message {
            if let Some(answer) = answer_message(dsl, message) {
                let data = serde_json::to_string(&answer).expect("answer serializes");
                if let Some(sender) = queues.lock().await.get("legacy-1") {
                    let _ = sender.send(format!("event: message\ndata: {data}\n\n"));
                }
            }
        }
        write_response(stream, 202, &[], b"").await;
        return true;
    }

    if request.method == "GET" {
        write_response(stream, 405, &[], b"").await;
        return true;
    }

    if request.method == "DELETE" {
        if dsl.die_on.iter().any(|d| d == "DELETE") {
            return false;
        }
        write_response(stream, dsl.delete_status.unwrap_or(204), &[], b"").await;
        return true;
    }

    // Streamable POSTs.
    if request.method != "POST" || (request.path != "/mcp" && request.path != "/mcp2") {
        write_response(stream, 404, &[], b"").await;
        return true;
    }
    if let Some(status) = dsl.status_all {
        write_response(stream, status, &[], b"").await;
        return true;
    }
    if let Some(status) = dsl.status_methods.get(method.as_str()) {
        write_response(stream, *status, &[], b"").await;
        return true;
    }
    if let Some(raw) = dsl.raw.get(method.as_str()) {
        write_response(
            stream,
            raw.status,
            &[("content-type", raw.content_type.clone())],
            raw.body.as_bytes(),
        )
        .await;
        return true;
    }
    if let Some(seconds) = dsl.sleep.get(method.as_str()) {
        tokio::time::sleep(Duration::from_secs_f64(*seconds)).await;
    }
    if dsl.die_on.iter().any(|d| d == &method) {
        return false;
    }
    let Some(message) = message else {
        write_response(stream, 400, &[], b"").await;
        return true;
    };
    if message.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
        write_response(stream, dsl.notif_status.unwrap_or(202), &[], b"").await;
        return true;
    }
    let mut extra = Vec::new();
    if method == "initialize" {
        extra.push(("mcp-session-id", "probe-session-1".to_string()));
    }
    if dsl.sse_empty.as_deref() == Some(method.as_str()) {
        write_response(
            stream,
            200,
            &[("content-type", "text/event-stream".to_string())],
            b"",
        )
        .await;
        return true;
    }
    let answer = answer_message(dsl, &message);
    match answer {
        None => {
            write_response(stream, 202, &[], b"").await;
        }
        Some(answer) => {
            let data = serde_json::to_string(&answer).expect("answer serializes");
            if dsl.framing() == "sse" {
                extra.push(("content-type", "text/event-stream".to_string()));
                let body = format!("event: message\ndata: {data}\n\n");
                write_response(stream, 200, &extra, body.as_bytes()).await;
            } else {
                extra.push(("content-type", "application/json".to_string()));
                write_response(stream, 200, &extra, data.as_bytes()).await;
            }
        }
    }
    true
}

/// Hold a legacy SSE stream: the `endpoint` event, then queued answers and
/// pings until the case ends.
async fn serve_sse_get(
    stream: &mut TcpStream,
    dsl: &StubDsl,
    queues: &Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
    done: &Arc<Notify>,
) {
    if dsl.sse_no_endpoint {
        let body = b": hello\n\n";
        write_response(
            stream,
            200,
            &[("content-type", "text/event-stream".to_string())],
            body,
        )
        .await;
        return;
    }
    // Close-delimited stream: headers without a length, events to EOF.
    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n";
    if stream.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    let endpoint = dsl
        .sse_endpoint
        .clone()
        .unwrap_or_else(|| "/messages?sessionId=legacy-1".to_string());
    if stream
        .write_all(format!("event: endpoint\ndata: {endpoint}\n\n").as_bytes())
        .await
        .is_err()
    {
        return;
    }
    let (sender, mut receiver) = mpsc::unbounded_channel::<String>();
    queues.lock().await.insert("legacy-1".to_string(), sender);
    let mut ping = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            _ = done.notified() => break,
            _ = ping.tick() => {
                if stream.write_all(b": ping\n\n").await.is_err() {
                    break;
                }
            }
            answer = receiver.recv() => {
                match answer {
                    Some(answer) => {
                        if stream.write_all(answer.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
    queues.lock().await.remove("legacy-1");
}

/// Answer one JSON-RPC message, mirroring the probe stub: `None` means the
/// message is a notification with no answer.
fn answer_message(dsl: &StubDsl, message: &Value) -> Option<Value> {
    let object = message.as_object()?;
    let method = object.get("method")?.as_str()?;
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    match method {
        "initialize" => {
            if dsl.init_error {
                return Some(json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": -32602, "message": "Bad init"},
                }));
            }
            let version = dsl.init_version.clone().unwrap_or_else(|| {
                message
                    .get("params")
                    .and_then(|params| params.get("protocolVersion"))
                    .and_then(Value::as_str)
                    .unwrap_or("2025-11-25")
                    .to_string()
            });
            let mut result = json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "probe-stub", "version": "0.0.0"},
            });
            if let Some(instructions) = &dsl.instructions {
                result["instructions"] = Value::String(instructions.clone());
            }
            Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
        }
        "notifications/initialized" => None,
        "tools/list" => {
            if dsl.fail_list {
                return Some(json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": -32602, "message": "Bad list"},
                }));
            }
            if let Some(pages) = &dsl.pages {
                let cursor = message
                    .get("params")
                    .and_then(|params| params.get("cursor"))
                    .and_then(Value::as_str);
                let index = match cursor {
                    None => 0,
                    Some(cursor) => cursor[1..].parse::<usize>().unwrap_or(0),
                };
                let mut result = json!({"tools": pages[index]});
                if dsl.dup_cursor {
                    result["nextCursor"] = Value::String("c1".to_string());
                } else if index + 1 < pages.len() {
                    result["nextCursor"] = Value::String(format!("c{}", index + 1));
                }
                return Some(json!({"jsonrpc": "2.0", "id": id, "result": result}));
            }
            let mut tools = vec![echo_tool(), add_tool()];
            if dsl.output_schema {
                tools[0]["outputSchema"] = output_schema();
            }
            Some(json!({"jsonrpc": "2.0", "id": id, "result": {"tools": tools}}))
        }
        "tools/call" => {
            let name = message
                .get("params")
                .and_then(|params| params.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = message
                .get("params")
                .and_then(|params| params.get("arguments"))
                .cloned()
                .unwrap_or(Value::Null);
            call_result(dsl, &id, name, &args)
        }
        _ => Some(json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": "Method not found"},
        })),
    }
}

/// `tools/call` answers, one per probe `call` mode.
fn call_result(dsl: &StubDsl, id: &Value, name: &str, args: &Value) -> Option<Value> {
    let result =
        |content: Value| json!({"jsonrpc": "2.0", "id": id, "result": {"content": content}});
    let error_result = |content: Value| json!({"jsonrpc": "2.0", "id": id, "result": {"content": content, "isError": true}});
    match dsl.call() {
        "text" => {
            let text = if name == "echo" {
                format!(
                    "echo:{}",
                    args.get("text").and_then(Value::as_str).unwrap_or_default()
                )
            } else {
                format!("called:{name}")
            };
            Some(result(json!([{"type": "text", "text": text}])))
        }
        "is_error" => Some(error_result(json!([{"type": "text", "text": "boom"}]))),
        "is_error_multi" => Some(error_result(
            json!([{"type": "text", "text": "first"}, {"type": "text", "text": "second"}]),
        )),
        "is_error_structured" => Some(json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "content": [{"type": "text", "text": "bad"}],
                "structuredContent": {"a": 1},
                "isError": true,
            },
        })),
        "is_error_image" => Some(error_result(
            json!([{"type": "image", "data": "aGk=", "mimeType": "image/png"}]),
        )),
        "structured" => Some(json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "content": [{"type": "text", "text": "{\"a\": 1, \"b\": 2}"}],
                "structuredContent": {"a": 1, "b": 2},
            },
        })),
        "structured_bad" => Some(json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "content": [{"type": "text", "text": "{\"a\": \"nope\"}"}],
                "structuredContent": {"a": "nope"},
            },
        })),
        "json_text" => Some(result(json!([{"type": "text", "text": "{\"a\": 1}"}]))),
        "multi_text" => Some(result(
            json!([{"type": "text", "text": "one"}, {"type": "text", "text": "two"}]),
        )),
        "image" => Some(result(
            json!([{"type": "image", "data": "aGk=", "mimeType": "image/png"}]),
        )),
        "audio" => Some(result(
            json!([{"type": "audio", "data": "aGk=", "mimeType": "audio/mp3"}]),
        )),
        "embedded_text" => Some(result(json!([{
            "type": "resource",
            "resource": {"uri": "file:///t.txt", "text": "file-words", "mimeType": "text/plain"},
        }]))),
        "embedded_blob_png" => Some(result(json!([{
            "type": "resource",
            "resource": {"uri": "file:///t.png", "blob": "aGk=", "mimeType": "image/png"},
        }]))),
        "embedded_blob_bin" => Some(result(json!([{
            "type": "resource",
            "resource": {"uri": "file:///t.bin", "blob": "aGk="},
        }]))),
        "resource_link" => Some(result(json!([{
            "type": "resource_link", "uri": "https://x.test/r", "name": "r",
        }]))),
        "result_unwrap" => Some(json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "content": [{"type": "text", "text": "42"}],
                "structuredContent": {"result": 42},
            },
        })),
        other => panic!("unknown stub call mode: {other}"),
    }
}
