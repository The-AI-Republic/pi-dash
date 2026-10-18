//! Test-only replay harness for the turn module (PIDASHCONV-863).
//!
//! The `fixtures/assistant/turn-runtime/` captures hold, per scenario,
//! the ground-truth provider exchanges (request bytes plus the scripted
//! stub's SSE reply), the stored turn row, and the `model_messages`
//! blobs. This module replays an exchange through an axum stub server
//! and rebuilds the loop's inputs from the same capture, so a test can
//! assert both directions: the Rust loop sends the captured bytes and
//! produces the captured messages/usage.
//!
//! Volatile fields (timestamps, run/conversation ids, the stub port in
//! `provider_url`, the stub's response id) are normalised before
//! comparison; everything else is compared verbatim.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use pidash_jobs::oss_worker::model_runtime::provider::WireTool;

/// The fixtures directory of the crate (`rust-api/fixtures`).
pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/assistant/turn-runtime")
}

/// Load one `turn-runtime` capture.
pub fn load_fixture(name: &str) -> Value {
    let path = fixture_dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

/// One request a stub server recorded.
#[derive(Debug, Clone)]
#[allow(dead_code)] // capture record: the assertions read body/matches, not these
pub struct StubRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// One scripted stub reply.
#[derive(Debug, Clone)]
pub struct StubReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct StubState {
    pub requests: Arc<Mutex<Vec<StubRequest>>>,
    script: Arc<Mutex<VecDeque<StubReply>>>,
}

impl StubState {
    pub fn seen(&self) -> Vec<StubRequest> {
        self.requests.lock().unwrap().clone()
    }
}

async fn stub_handler(
    axum::extract::State(state): axum::extract::State<StubState>,
    req: axum::http::Request<axum::body::Body>,
) -> impl axum::response::IntoResponse {
    use axum::body::to_bytes;
    let method = req.method().to_string();
    let path = req
        .uri()
        .path_and_query()
        .map(|pq| pq.to_string())
        .unwrap_or_default();
    let mut headers = HashMap::new();
    for name in [
        "content-type",
        "content-length",
        "user-agent",
        "authorization",
        "x-api-key",
        "anthropic-version",
    ] {
        if let Some(value) = req.headers().get(name).and_then(|v| v.to_str().ok()) {
            headers.insert(name.to_owned(), value.to_owned());
        }
    }
    let body = to_bytes(req.into_body(), usize::MAX).await.unwrap();
    state.requests.lock().unwrap().push(StubRequest {
        method,
        path,
        headers,
        body: body.to_vec(),
    });
    let reply = state
        .script
        .lock()
        .unwrap()
        .pop_front()
        .expect("stub has a scripted reply for every request");
    let mut response = axum::response::Response::builder().status(reply.status);
    for (name, value) in &reply.headers {
        response = response.header(name, value);
    }
    response.body(axum::body::Body::from(reply.bytes)).unwrap()
}

/// Start a stub server replaying `script`, returning its base URL
/// (`http://127.0.0.1:port`, no trailing slash) and its recorded state.
pub async fn stub_server(script: Vec<StubReply>) -> (String, StubState) {
    let state = StubState {
        requests: Arc::new(Mutex::new(Vec::new())),
        script: Arc::new(Mutex::new(script.into())),
    };
    let app = axum::Router::new()
        .fallback(stub_handler)
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), state)
}

/// The stub reply for one captured exchange: the recorded status plus
/// the verbatim SSE text, or the parsed error body re-encoded (the
/// captures pin error bodies as JSON values, not bytes).
pub fn reply_for(exchange: &Value) -> StubReply {
    let response = exchange.get("response").expect("exchange has a response");
    let status = response
        .get("status")
        .and_then(Value::as_u64)
        .expect("response has a status") as u16;
    match response.get("sse_text") {
        Some(Value::String(sse)) => StubReply {
            status,
            headers: vec![("content-type".to_owned(), "text/event-stream".to_owned())],
            bytes: sse.as_bytes().to_vec(),
        },
        _ => StubReply {
            status,
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            bytes: serde_json::to_vec(response.get("body").expect("error response has a body"))
                .unwrap(),
        },
    }
}

/// True when the capture is on the Anthropic lane.
pub fn is_anthropic(fixture: &Value) -> bool {
    fixture
        .get("_meta")
        .and_then(|meta| meta.get("lane"))
        .and_then(Value::as_str)
        .map(|lane| lane.starts_with("anthropic"))
        .unwrap_or(false)
}

/// Rebuild the loop's wire tools from a captured request body: the
/// captures hold the lane-transformed schema, which is exactly what
/// [`WireTool`] carries into the builders.
pub fn wire_tools_from_body(anthropic: bool, body: &Value) -> Vec<WireTool> {
    body.get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    let (name, description, parameters, strict) = if anthropic {
                        (
                            tool.get("name"),
                            tool.get("description"),
                            tool.get("input_schema"),
                            tool.get("strict"),
                        )
                    } else {
                        let function = tool.get("function").expect("openai tool has `function`");
                        (
                            function.get("name"),
                            function.get("description"),
                            function.get("parameters"),
                            function.get("strict"),
                        )
                    };
                    WireTool {
                        name: name.and_then(Value::as_str).unwrap().to_owned(),
                        description: description
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        parameters: parameters.cloned().unwrap_or(Value::Null),
                        strict: match strict {
                            Some(Value::Bool(true)) => Some(true),
                            Some(Value::Bool(false)) => Some(false),
                            _ => None,
                        },
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The instruction parts a capture's request body carries: the OpenAI
/// lane's leading system messages, or the Anthropic lane's `system`
/// text blocks.
pub fn instructions_from_body(anthropic: bool, body: &Value) -> Vec<String> {
    if anthropic {
        match body.get("system").and_then(Value::as_array) {
            Some(blocks) => blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .map(str::to_owned)
                .collect(),
            None => Vec::new(),
        }
    } else {
        body.get("messages")
            .and_then(Value::as_array)
            .map(|messages| {
                messages
                    .iter()
                    .take_while(|message| {
                        message.get("role").and_then(Value::as_str) == Some("system")
                    })
                    .filter_map(|message| message.get("content").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The user prompt text of a captured first request.
pub fn prompt_from_body(body: &Value) -> String {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .expect("request has messages");
    // The turn's prompt is the newest user-role text: for a turn's first
    // request it is the only user text, and for a later request it is the
    // prompt appended after the replayed history. Anthropic's tool results
    // ride in a `user` message with no text block, so they are skipped.
    messages
        .iter()
        .rev()
        .find_map(user_text)
        .or_else(|| messages.iter().find_map(user_text))
        .expect("request has a user message")
}

/// The text of a user message, if it carries any: OpenAI's plain string
/// content or the first Anthropic text block.
fn user_text(message: &Value) -> Option<String> {
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    match message.get("content") {
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .find_map(|block| block.get("text").and_then(Value::as_str))
            .map(str::to_owned),
        _ => None,
    }
}

/// The captured request body of one exchange, as the value the builder
/// must produce.
pub fn request_body_of(exchange: &Value) -> Value {
    exchange
        .get("request")
        .and_then(|request| request.get("body"))
        .expect("exchange captures the request body")
        .clone()
}

/// The exact bytes the capture recorded for one exchange's request.
pub fn request_bytes_of(exchange: &Value) -> Vec<u8> {
    exchange
        .get("request")
        .and_then(|request| request.get("body_bytes"))
        .and_then(Value::as_str)
        .expect("exchange captures request bytes")
        .as_bytes()
        .to_vec()
}

/// Replace every volatile field with a sentinel so two dumps compare
/// structurally: timestamps, run/conversation ids, the stub
/// `provider_url`, and the provider response id.
pub fn normalize_volatile(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(normalize_volatile).collect()),
        Value::Object(object) => {
            let mut out = Map::new();
            for (key, item) in object {
                let normalized = match key.as_str() {
                    "timestamp"
                    | "run_id"
                    | "conversation_id"
                    | "provider_url"
                    | "provider_response_id" => Value::String(format!("<{key}>")),
                    _ => normalize_volatile(item),
                };
                out.insert(key.clone(), normalized);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}
