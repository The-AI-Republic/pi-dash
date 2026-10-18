//! MCP tool-server client for the assistant turn runtime (PIDASHCONV-1112).
//!
//! Ports the live half of `apps/api/pi_dash/assistant/runtime/mcp.py:1-309`
//! behind the handler layer: a JSON-RPC MCP client speaking `initialize`,
//! `tools/list` and `tools/call` over Streamable HTTP with the legacy SSE
//! fallback, the fail-open [`ResilientSession`] policy, tool-name prefixing,
//! and per-call timeouts. The pure decision surface (timeout/cap values,
//! prefix assignment, skip planning, [`ResilientState`]) already lives in
//! `pidash_services::assistant::mcp` and is reused, not reimplemented.
//!
//! Wire contract (see `fixtures/assistant/mcp-transport/*.json`, captured
//! from the real `build_toolset`/`ResilientToolset` path on
//! pydantic-ai-slim 1.107.0 + fastmcp 3.4.4 + mcp 1.30.0):
//!
//! * Streamable HTTP: `POST initialize` (id 0, protocol `2025-11-25`,
//!   capabilities `{}`, client `mcp/0.1.0`), `POST notifications/initialized`,
//!   `POST tools/list` (no params; `{"cursor"}` pages, 250-page cap,
//!   duplicate-cursor break), `POST tools/call`
//!   (`{name, arguments, _meta: {progressToken: id}}`), `DELETE` terminate
//!   when a session id was captured. `Accept: application/json,
//!   text/event-stream`; the `mcp-session-id` response header is echoed and
//!   the server's protocol version is stamped on every later request.
//!   Responses arrive as one JSON object or an SSE stream.
//! * Legacy SSE (URL path matches `/sse(/|?|&|$)`): `GET` the event stream,
//!   take the `endpoint` event URL (same origin), `POST` each message there,
//!   read answers off the stream. POST failures are swallowed and the waiter
//!   times out. No `DELETE`.
//! * Fail-open: a dead server records its exception class and the turn goes
//!   on with `{}` tools or an absorbed call result. The recorded classes
//!   reach the user via `tool_servers_skipped` events, so they are matched
//!   exactly (`RuntimeError`, `McpError`, `HTTPStatusError`, `ReadTimeout`,
//!   `RemoteProtocolError`, `ConnectError`, `BrokenResourceError`,
//!   `ValidationError`, `Error`; tool errors pass through as model retries).
//!
//! Deliberate deviations from the Python stack, each pinned by a fixture:
//!
//! * No standalone `GET` event stream. Nothing in the turn path consumes
//!   server-initiated messages (tool cache is per-turn), so the client
//!   never opens it; a `202` answer or an empty/unparseable response waits
//!   out the read deadline and fails the op with `McpError`, which is the
//!   stub-observable outcome either way.
//! * A cross-origin SSE `endpoint` fails the server open immediately.
//!   Python hangs there (the reader swallows the raise and the endpoint
//!   wait has no timeout); the hang is bounded in production only by the
//!   turn limits, so failing open is strictly closer to the module's
//!   contract. The recorded class follows FastMCP `_connect` (everything
//!   but `HTTPStatusError`/`McpError` wraps in `RuntimeError`).
//! * Server-initiated requests on the SSE stream (ping, sampling) are
//!   ignored rather than answered; no turn behavior depends on them.
//! * `User-Agent`/`Accept-Encoding` are reqwest's defaults, not httpx's;
//!   no MCP server varies behavior on them.
//!
//! Not wired into `worker()` here — PIDASHCONV-863 owns the agent loop,
//! the live seam and the wiring.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use pidash_services::assistant::crypto::CryptoError;
use pidash_services::assistant::mcp::{DecryptFailure, ResilientState, ToolsetSpec};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Protocol constants
// ---------------------------------------------------------------------------

/// Protocol version the client offers (`mcp/types.py`, `LATEST_PROTOCOL_VERSION`).
pub const PROTOCOL_VERSION: &str = "2025-11-25";
/// Server versions the handshake accepts (`mcp/shared/version.py`).
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2024-11-05", "2025-03-26", "2025-06-18", PROTOCOL_VERSION];
/// Default client identity (`mcp/client/session.py`, `DEFAULT_CLIENT_INFO`).
const CLIENT_NAME: &str = "mcp";
const CLIENT_VERSION: &str = "0.1.0";
/// Requested response framings (`mcp/client/streamable_http.py`).
const ACCEPT: &str = "application/json, text/event-stream";
const JSON_MIME: &str = "application/json";
const SSE_MIME: &str = "text/event-stream";
/// Auto-pagination cap (`fastmcp/client/mixins/tools.py`).
const MAX_PAGES: u32 = 250;
/// Redirect chain cap (httpx `max_redirects` default).
const MAX_REDIRECTS: u32 = 20;

// ---------------------------------------------------------------------------
// Transport inference
// ---------------------------------------------------------------------------

/// Whether `url` selects the legacy SSE transport
/// (`fastmcp/mcp_config.py`, `infer_transport_type_from_url`):
/// `/sse` followed by `/`, `?`, `&` or end of path. The `?` arm is dead
/// in practice (`urlparse` strips the query first), but `&` can appear
/// in a path segment.
pub fn is_sse_url(url: &str) -> bool {
    let path = url_path_of(url);
    let path = path.split(['?', '#']).next().unwrap_or(path);
    path.contains("/sse/") || path.ends_with("/sse") || path.contains("/sse&")
}

/// Path-plus part of an http(s) URL (`/mcp`, `/sse?x=1`, …).
fn url_path_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, after)| after).unwrap_or(url);
    match rest.find('/') {
        Some(idx) => &rest[idx..],
        None => "/",
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC request builders (byte-exact)
// ---------------------------------------------------------------------------

use serde::Serialize;

/// `{"method":..,"params":..,"jsonrpc":"2.0","id":..}` — field order matches
/// the SDK's `model_dump` exactly, including omitting absent `params`.
#[derive(Serialize)]
struct Request<'a, P: Serialize> {
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<P>,
    jsonrpc: &'a str,
    id: i64,
}

#[derive(Serialize)]
struct Notification<'a> {
    method: &'a str,
    jsonrpc: &'a str,
}

#[derive(Serialize)]
struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    protocol_version: &'static str,
    capabilities: HashMap<String, Value>,
    #[serde(rename = "clientInfo")]
    client_info: ClientInfo,
}

#[derive(Serialize)]
struct ClientInfo {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct CursorParams<'a> {
    cursor: &'a str,
}

#[derive(Serialize)]
struct CallParams<'a> {
    name: &'a str,
    arguments: &'a Value,
    #[serde(rename = "_meta")]
    meta: ProgressMeta,
}

#[derive(Serialize)]
struct ProgressMeta {
    #[serde(rename = "progressToken")]
    progress_token: i64,
}

/// `initialize` request body (`ClientSession.initialize`, mcp 1.30).
pub fn initialize_body(id: i64) -> String {
    let request = Request {
        method: "initialize",
        params: Some(InitializeParams {
            protocol_version: PROTOCOL_VERSION,
            capabilities: HashMap::new(),
            client_info: ClientInfo {
                name: CLIENT_NAME,
                version: CLIENT_VERSION,
            },
        }),
        jsonrpc: "2.0",
        id,
    };
    serde_json::to_string(&request).expect("request serializes")
}

/// `notifications/initialized` body (no id, no params).
pub fn initialized_body() -> String {
    let notification = Notification {
        method: "notifications/initialized",
        jsonrpc: "2.0",
    };
    serde_json::to_string(&notification).expect("notification serializes")
}

/// `tools/list` request body: no `params` key without a cursor, exactly
/// `{"cursor"}` with one (`ClientSession.list_tools`, mcp 1.30).
pub fn list_body(id: i64, cursor: Option<&str>) -> String {
    let params = cursor.map(|cursor| CursorParams { cursor });
    let request = Request {
        method: "tools/list",
        params,
        jsonrpc: "2.0",
        id,
    };
    serde_json::to_string(&request).expect("request serializes")
}

/// `tools/call` request body. `_meta.progressToken` is always the request
/// id: FastMCP always installs a progress handler
/// (`default_progress_handler`), and the SDK stamps the token whenever a
/// callback is present (`mcp/shared/session.py`, `send_request`).
pub fn call_body(id: i64, name: &str, args: &Value) -> String {
    let request = Request {
        method: "tools/call",
        params: Some(CallParams {
            name,
            arguments: args,
            meta: ProgressMeta { progress_token: id },
        }),
        jsonrpc: "2.0",
        id,
    };
    serde_json::to_string(&request).expect("request serializes")
}

// ---------------------------------------------------------------------------
// SSE parsing
// ---------------------------------------------------------------------------

/// One parsed server-sent event: the optional `event:` name plus the
/// newline-joined `data:` payload. `id:`/`retry:` lines carry no MCP
/// meaning and are dropped while scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Incremental SSE framer over a response byte stream. Bytes are pushed as
/// they arrive; complete events (terminated by a blank line) are returned
/// and any partial tail is kept for the next push.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed newly arrived bytes, returning every event completed by them.
    /// Only complete blocks are decoded; the partial tail stays raw bytes
    /// so a multi-byte char or `\r\n` pair split across chunks survives.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some((start, end)) = event_boundary(&self.buf) {
            let block: Vec<u8> = self.buf.drain(..start).collect();
            self.buf.drain(..(end - start));
            // Normalize line endings: \r\n and lone \r both mean \n.
            // The split points are ASCII, so no char spans the decode.
            let text = String::from_utf8_lossy(&block).replace("\r\n", "\n");
            let text = text.replace('\r', "\n");
            if let Some(event) = parse_event_block(&text) {
                events.push(event);
            }
        }
        events
    }
}

/// Byte offsets of the first complete blank-line boundary: the start of
/// its first line break and just past its second. A `\r` at the very end
/// of the buffer is ambiguous (a split `\r\n`) and never completes one.
fn event_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let mut at = 0;
    let mut first: Option<usize> = None;
    while at < buf.len() {
        let (is_break, len) = if buf[at] == b'\n' {
            (true, 1)
        } else if buf[at] == b'\r' {
            if at + 1 < buf.len() && buf[at + 1] == b'\n' {
                (true, 2)
            } else if at + 1 == buf.len() {
                return None;
            } else {
                (true, 1)
            }
        } else {
            (false, 1)
        };
        if is_break {
            if let Some(start) = first {
                return Some((start, at + len));
            }
            first = Some(at);
        } else {
            first = None;
        }
        at += len;
    }
    None
}

/// Whether an SSE event can carry a request answer: only `message`
/// events resolve (`_handle_sse_event`, `sse_reader`); anything else —
/// including unnamed events, which httpx-sse reports with an empty
/// name — is skipped.
fn is_answer_event(event: &SseEvent) -> bool {
    event.event.as_deref() == Some("message") && !event.data.is_empty()
}

/// Parse one blank-line-delimited event block; comments and empty blocks
/// yield no event.
fn parse_event_block(block: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut data_lines = Vec::new();
    for line in block.split('\n') {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "data" => data_lines.push(value.to_string()),
            _ => {}
        }
    }
    if event.is_none() && data_lines.is_empty() {
        return None;
    }
    Some(SseEvent {
        event,
        data: data_lines.join("\n"),
    })
}

// ---------------------------------------------------------------------------
// Errors: Python exception classes as data
// ---------------------------------------------------------------------------

/// A failed MCP operation, identified the way `ResilientToolset` identifies
/// it: the Python exception class name plus its message. Only the class
/// reaches the user (via `tool_servers_skipped`); the message is for logs.
/// Constructors below mirror the SDK raise sites one by one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpError {
    /// Python exception class name (`RuntimeError`, `McpError`, …).
    pub class: &'static str,
    pub message: String,
}

impl McpError {
    fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
        }
    }

    /// Request waiter deadline (`mcp/shared/session.py`, `send_request`).
    /// `what` is always the wrapper name `ClientRequest`.
    pub fn timeout(what: &str, seconds: f64) -> Self {
        Self::new(
            "McpError",
            format!(
                "Timed out while waiting for response to {what}. Waited {} seconds.",
                py_float(seconds)
            ),
        )
    }

    /// JSON-RPC `error` object (`McpError(response_or_error.error)`).
    pub fn rpc_error(message: &str) -> Self {
        Self::new("McpError", message)
    }

    /// HTTP 404 on a request: the session is gone
    /// (`_send_session_terminated_error`).
    pub fn session_terminated() -> Self {
        Self::new("McpError", "Session terminated")
    }

    /// Non-2xx/202/404 status (`response.raise_for_status()`, httpx text).
    pub fn http_status(status: u16, url: &str) -> Self {
        let reason = canonical_reason(status);
        let kind = if status < 500 { "Client" } else { "Server" };
        Self::new(
            "HTTPStatusError",
            format!(
                "{kind} error '{status} {reason}' for url '{url}'\n\
                 For more information check: \
                 https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/{status}"
            ),
        )
    }

    /// Redirect the client refuses to follow
    /// (`_raise_for_unfollowed_redirect`). The named location drops
    /// userinfo, query and fragment (`redirect_location`); an https
    /// endpoint refusing a plain-http target gets the downgrade text.
    pub fn unfollowed_redirect(request: &reqwest::Url, target: &reqwest::Url) -> Self {
        let mut location = target.clone();
        let _ = location.set_username("");
        let _ = location.set_password(None);
        location.set_query(None);
        location.set_fragment(None);
        if request.scheme() == "https" && location.scheme() == "http" {
            let mut upgrade = location.clone();
            let _ = upgrade.set_scheme("https");
            return Self::new(
                "HTTPStatusError",
                format!(
                    "Redirect to {location} not followed: it would downgrade this HTTPS endpoint to plain HTTP.\n\
                     The server is likely behind a TLS-terminating proxy whose forwarded headers it does not trust,\n\
                     often combined with a trailing-slash difference. Try {upgrade} instead, or fix the proxy settings."
                ),
            );
        }
        Self::new(
            "HTTPStatusError",
            format!(
                "Redirect to {location} not followed; use that URL as the \
                 endpoint if it is the intended server"
            ),
        )
    }

    /// Whole-connect deadline (`FastMCPClient.initialize`).
    pub fn init_timeout() -> Self {
        Self::new(
            "RuntimeError",
            "Client failed to connect: Failed to initialize server session",
        )
    }

    /// Any transport failure while establishing the session
    /// (`FastMCPClient._connect` wraps everything in `RuntimeError`).
    pub fn connect_failed(detail: &str) -> Self {
        Self::new(
            "RuntimeError",
            format!("Client failed to connect: {detail}"),
        )
    }

    /// Server protocol version outside `SUPPORTED_PROTOCOL_VERSIONS`.
    pub fn unsupported_version(version: &str) -> Self {
        Self::new(
            "RuntimeError",
            format!(
                "Client failed to connect: Unsupported protocol version \
                 from the server: {version}"
            ),
        )
    }

    /// Op on a dead session before anything recorded a failure: the send
    /// hits the closed stream (anyio `BrokenResourceError`, empty message).
    pub fn broken_session() -> Self {
        Self::new("BrokenResourceError", "")
    }

    /// Op on a dead session after a failure was recorded: the client
    /// object is torn down (`FastMCPClient.session` guard).
    pub fn not_connected() -> Self {
        Self::new(
            "RuntimeError",
            "Client is not connected. Use the 'async with client:' context \
             manager first.",
        )
    }

    /// SSE stream ended before any `endpoint` event. The text is anyio's
    /// task-status error, repeated verbatim so cross-backend log searches
    /// keep working.
    pub fn sse_no_endpoint() -> Self {
        Self::new(
            "RuntimeError",
            "Client failed to connect: Child exited without calling \
             task_status.started()",
        )
    }

    /// Tool declares `outputSchema` but returned no `structuredContent`.
    pub fn schema_missing(tool: &str) -> Self {
        Self::new(
            "RuntimeError",
            format!(
                "Tool {tool} has an output schema but did not return \
                 structured content"
            ),
        )
    }

    /// `structuredContent` fails the tool's `outputSchema`.
    pub fn schema_invalid(tool: &str, detail: &str) -> Self {
        Self::new(
            "RuntimeError",
            format!("Invalid structured content returned by tool {tool}: {detail}"),
        )
    }

    /// The tool's `outputSchema` itself does not compile.
    pub fn schema_unusable(tool: &str, detail: &str) -> Self {
        Self::new(
            "RuntimeError",
            format!("Invalid schema for tool {tool}: {detail}"),
        )
    }

    /// Result payload that is not a valid `CallToolResult` (unknown content
    /// block, wrong shape). The SDK's pydantic parse raises raw
    /// `ValidationError` past the waiter.
    pub fn bad_result(detail: &str) -> Self {
        Self::new("ValidationError", detail)
    }

    /// Tearing down a session whose in-flight request timed out surfaces
    /// the raw transport error, not the waiter's `McpError` wrapper
    /// (httpx `ReadTimeout`, empty message).
    pub fn close_after_timeout() -> Self {
        Self::new("ReadTimeout", "")
    }

    /// Mid-session connection failure on a fresh POST (httpx `ConnectError`
    /// propagates raw past the waiter; only initial-connect errors are
    /// wrapped in `RuntimeError`).
    pub fn connect_error(detail: &str) -> Self {
        Self::new("ConnectError", detail)
    }

    /// Mid-session failure on an established exchange: reset, truncated
    /// body, mid-stream stall (httpx `RemoteProtocolError`, raw).
    pub fn remote_protocol(detail: &str) -> Self {
        Self::new("RemoteProtocolError", detail)
    }

    /// Base64 payload that does not decode (`binascii.Error`, whose class
    /// name is literally `Error`).
    pub fn base64(detail: &str) -> Self {
        Self::new("Error", detail)
    }
}

/// Python `str(float)`: shortest round-trip, integral values keep `.0`.
/// `serde_json` renders Rust floats the same way.
fn py_float(value: f64) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "0.0".to_string())
}

/// HTTP reason phrases for the statuses `http_status` can report.
fn canonical_reason(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a Teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        423 => "Locked",
        424 => "Failed Dependency",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => "Unknown",
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.class, self.message)
    }
}

impl std::error::Error for McpError {}

// ---------------------------------------------------------------------------
// Tool definitions and call results
// ---------------------------------------------------------------------------

/// One listed tool: the prefixed name the model sees, the original name
/// the server speaks, and the schemas both sides need.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolDef {
    /// `{prefix}_{original}` (or the bare name when the prefix is empty).
    pub name: String,
    pub original: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
}

/// One mapped content block (`pydantic_ai/mcp.py`, `_map_mcp_tool_result`).
/// `Json` is a text block that parsed as JSON; `Image` is `BinaryImage`
/// (direct images and image-typed blobs); `Binary` is plain `BinaryContent`
/// (audio and non-image blobs).
#[derive(Debug, Clone, PartialEq)]
pub enum McpContent {
    Text(String),
    Json(Value),
    Image { media_type: String, data: Vec<u8> },
    Binary { media_type: String, data: Vec<u8> },
}

/// A mapped call result: one block, several, or preferred structured data
/// (`MCPToolset.direct_call_tool`).
#[derive(Debug, Clone, PartialEq)]
pub enum McpValue {
    Single(McpContent),
    List(Vec<McpContent>),
    Structured(Value),
}

/// What a call yields: a value for the model, a tool-error retry
/// (`ModelRetry` passthrough — never recorded, never absorbed), or the
/// absorbed unavailable text (always paired with a recorded failure).
#[derive(Debug, Clone, PartialEq)]
pub enum CallResult {
    Value(McpValue),
    Retry { message: String },
    Absorbed { text: String },
}

/// Map one `CallToolResult` content block. Unknown block types and
/// missing or mistyped required fields fail the parse the way the SDK's
/// `CallToolResult` validation does (`TextContent.text`,
/// `ImageContent`/`AudioContent` `data` + `mimeType`, `ResourceLink.uri`
/// are all required).
fn map_content_block(block: &Value) -> Result<McpContent, McpError> {
    let kind = block
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "text" => {
            let text = require_str(block, "text", "text block without text")?;
            Ok(map_text(text))
        }
        "image" => {
            let data = decode_data(require_str(block, "data", "image block without data")?)?;
            let media_type = require_str(block, "mimeType", "image block without mimeType")?;
            Ok(McpContent::Image {
                media_type: media_type.to_string(),
                data,
            })
        }
        "audio" => {
            let data = decode_data(require_str(block, "data", "audio block without data")?)?;
            let media_type = require_str(block, "mimeType", "audio block without mimeType")?;
            Ok(McpContent::Binary {
                media_type: media_type.to_string(),
                data,
            })
        }
        "resource" => map_embedded(block),
        "resource_link" => {
            let uri = require_str(block, "uri", "resource link without uri")?;
            Ok(McpContent::Text(uri.to_string()))
        }
        other => Err(McpError::bad_result(&format!(
            "unknown content block type: {other}"
        ))),
    }
}

/// Required string field, else the SDK's `ValidationError`.
fn require_str<'a>(block: &'a Value, field: &str, missing: &str) -> Result<&'a str, McpError> {
    block
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::bad_result(missing))
}

/// Text blocks starting with `[` or `{` are JSON-parsed
/// (`pydantic_core.from_json`, strict); unparseable stays text. The check
/// runs on the raw text (`str.startswith`, no strip): leading whitespace
/// reads as plain text in Python.
fn map_text(text: &str) -> McpContent {
    if text.starts_with('[') || text.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            return McpContent::Json(value);
        }
    }
    McpContent::Text(text.to_string())
}

/// Base64 payloads decode the way `base64.b64decode(data)` with
/// `validate=False` does: alphabet characters are kept, everything else
/// discarded, then strict-decoded. Residual failure (bad padding) is the
/// same `binascii.Error` Python would raise.
fn decode_data(encoded: &str) -> Result<Vec<u8>, McpError> {
    use base64::Engine as _;
    let filtered: String = encoded
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(filtered.as_bytes())
        .map_err(|err| McpError::base64(&err.to_string()))
}

/// Embedded resources: text inlines, blobs narrow to image/binary by media
/// type (`BinaryContent.narrow_type`, `is_image`). The union tries text
/// first, like the SDK's `TextResourceContents | BlobResourceContents`;
/// `uri` is required on both, `mimeType` optional.
fn map_embedded(block: &Value) -> Result<McpContent, McpError> {
    let resource = block
        .get("resource")
        .filter(|resource| resource.is_object())
        .ok_or_else(|| McpError::bad_result("embedded resource without a resource payload"))?;
    require_str(resource, "uri", "embedded resource without a uri")?;
    if let Some(text) = resource.get("text").and_then(Value::as_str) {
        return Ok(McpContent::Text(text.to_string()));
    }
    let Some(blob) = resource.get("blob").and_then(Value::as_str) else {
        return Err(McpError::bad_result(
            "embedded resource without text or a blob",
        ));
    };
    let data = decode_data(blob)?;
    let media_type = resource
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_string();
    if media_type.starts_with("image/") {
        Ok(McpContent::Image { media_type, data })
    } else {
        Ok(McpContent::Binary { media_type, data })
    }
}

/// A mapped `tools/call` result: a value, a tool-error retry, or a
/// result payload that fails validation (an op error, not a retry).
enum MappedCall {
    Value(McpValue),
    Retry(String),
    Invalid(McpError),
}

/// Map a whole `tools/call` result: errors become retries, preferred
/// structured data wins over all-text content, otherwise blocks map
/// one-to-one (singletons unwrap).
fn map_call_result(
    content: &[Value],
    structured: Option<&Value>,
    tool: &str,
    is_error: bool,
) -> MappedCall {
    if is_error {
        // `FastMCPClient.call_tool`, `raise_on_error`: first text wins,
        // anything else names the tool. `structuredContent` is ignored.
        let message = content
            .first()
            .filter(|first| first.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|first| first.get("text").and_then(Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| format!("Tool '{tool}' returned an error"));
        return MappedCall::Retry(message);
    }
    if let Some(structured) = structured.filter(|v| is_structured_present(v)) {
        let all_text = content
            .iter()
            .all(|block| block.get("type").and_then(Value::as_str) == Some("text"));
        if all_text {
            if let Some(unwrapped) = unwrap_single_result(structured) {
                return MappedCall::Value(McpValue::Structured(unwrapped));
            }
            return MappedCall::Value(McpValue::Structured(structured.clone()));
        }
    }
    let mut mapped = Vec::with_capacity(content.len());
    for block in content {
        match map_content_block(block) {
            Ok(part) => mapped.push(part),
            Err(err) => return MappedCall::Invalid(err),
        }
    }
    if mapped.len() == 1 {
        MappedCall::Value(McpValue::Single(mapped.pop().expect("one mapped")))
    } else {
        MappedCall::Value(McpValue::List(mapped))
    }
}

/// Whether `structuredContent` counts as present for the preference
/// (`if (structured := ...)`): null and `{}` are falsy and fall through
/// to the content mapping. Validation still sees `{}` (see
/// `validate_output`), exactly like the SDK.
fn is_structured_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Object(object) => !object.is_empty(),
        _ => true,
    }
}

/// Single-key `{"result": …}` structured payloads unwrap
/// (`direct_call_tool`).
fn unwrap_single_result(structured: &Value) -> Option<Value> {
    let object = structured.as_object()?;
    if object.len() == 1 {
        if let Some(result) = object.get("result") {
            return Some(result.clone());
        }
    }
    None
}

/// Enforce the tool's `outputSchema` against its `structuredContent`
/// (`ClientSession._validate_tool_result`). Unknown tools skip: the SDK
/// only warns there.
fn validate_output(
    tool: &str,
    output_schema: Option<&Value>,
    structured: Option<&Value>,
) -> Result<(), McpError> {
    let Some(schema) = output_schema else {
        return Ok(());
    };
    let Some(structured) = structured.filter(|v| !v.is_null()) else {
        return Err(McpError::schema_missing(tool));
    };
    let validator = jsonschema::validator_for(schema)
        .map_err(|err| McpError::schema_unusable(tool, &err.to_string()))?;
    validator
        .validate(structured)
        .map_err(|err| McpError::schema_invalid(tool, &err.to_string()))
}

// ---------------------------------------------------------------------------
// HTTP layer
// ---------------------------------------------------------------------------

/// Plain client: no auto-redirects (the SDK's same-origin rules are
/// applied by hand) and no built-in timeouts (tokio deadlines govern, so
/// slow phases map to the right waiter errors).
fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client builds")
}

fn secs_to_duration(seconds: f64) -> Duration {
    if seconds.is_nan() || seconds <= 0.0 {
        return Duration::ZERO;
    }
    if !seconds.is_finite() {
        return Duration::from_secs(3600 * 24 * 365);
    }
    Duration::from_secs_f64(seconds)
}

/// Whether a redirect target may be followed
/// (`_is_same_origin_redirect`): same scheme/host/port, or http→https on
/// the same host with default ports.
fn redirect_allowed(previous: &reqwest::Url, next: &reqwest::Url) -> bool {
    if previous.scheme() == next.scheme()
        && previous.host_str() == next.host_str()
        && previous.port_or_known_default() == next.port_or_known_default()
    {
        return true;
    }
    previous.scheme() == "http"
        && next.scheme() == "https"
        && previous.host_str() == next.host_str()
        && previous.port_or_known_default() == Some(80)
        && next.port_or_known_default() == Some(443)
}

/// Whether an SSE `endpoint` URL is on the GET URL's origin
/// (`mcp/client/sse.py` compares scheme and netloc). Ports compare by
/// value: URL parsing already normalizes an explicit default port away,
/// so unlike the SDK's string compare, `:80` matches an implicit one.
fn endpoint_origin_ok(base: &reqwest::Url, endpoint: &reqwest::Url) -> bool {
    base.scheme() == endpoint.scheme()
        && base.host_str() == endpoint.host_str()
        && base.port_or_known_default() == endpoint.port_or_known_default()
}

/// Classify a reqwest transport failure mid-session: fresh-connection
/// failures are `ConnectError`, established-exchange failures are
/// `RemoteProtocolError`.
fn classify_transport(err: &reqwest::Error) -> McpError {
    let detail = err.to_string();
    if err.is_connect() {
        McpError::connect_error(&detail)
    } else {
        McpError::remote_protocol(&detail)
    }
}

// ---------------------------------------------------------------------------
// Live sessions
// ---------------------------------------------------------------------------

/// What a dead session makes `close` do.
#[derive(Debug, Clone)]
struct DeadSession {
    /// Error `close` records (`close_after_timeout` for timeouts, the
    /// original error otherwise). Ignored when `inflight` is false.
    close_error: McpError,
    /// An in-flight REQUEST wedged: no `DELETE` is attempted.
    #[allow(dead_code)] // fixture bookkeeping; the assertions read the error class
    inflight: bool,
}

/// A failed op, classified by what it leaves behind: a clean session the
/// next op may use, or a dead one.
enum SessionFault {
    /// Clean session (JSON-RPC error, validation, waiter timeout on a
    /// complete-but-empty answer, retryable connect failure).
    Logical(McpError),
    /// Dead session. `close` re-records `close_error` without `DELETE`
    /// when `inflight`, else terminates normally.
    Wedge { error: McpError, close: DeadSession },
}

impl SessionFault {
    fn logical(error: McpError) -> Self {
        SessionFault::Logical(error)
    }

    fn wedge(error: McpError, close_error: McpError, inflight: bool) -> Self {
        SessionFault::Wedge {
            error,
            close: DeadSession {
                close_error,
                inflight,
            },
        }
    }

    /// Transport/status wedge: close re-records the same error.
    fn wedge_same(error: McpError, inflight: bool) -> Self {
        SessionFault::wedge(error.clone(), error, inflight)
    }
}

/// Request phase: connect-phase transport failures wrap in `RuntimeError`
/// (FastMCP `_connect` passes only `HTTPStatusError`/`McpError` through);
/// op-phase failures surface raw.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Connect,
    Op,
}

/// Which error an elapsed or waited-out deadline reports: the whole
/// connect bound reports the init timeout, an op waiter the McpError.
#[derive(Clone, Copy)]
enum DeadlineRule {
    Init,
    Waiter(f64),
}

fn deadline_error(rule: DeadlineRule) -> McpError {
    match rule {
        DeadlineRule::Init => McpError::init_timeout(),
        DeadlineRule::Waiter(secs) => McpError::timeout("ClientRequest", secs),
    }
}

/// Map a reqwest transport failure in each phase.
fn transport_error(phase: Phase, err: &reqwest::Error) -> McpError {
    match phase {
        Phase::Connect => McpError::connect_failed(&err.to_string()),
        Phase::Op => classify_transport(err),
    }
}

/// An elapsed deadline wedges the session: the op reports the waiter
/// error while close re-records the raw transport error.
fn elapsed_wedge(phase: Phase, rule: DeadlineRule) -> SessionFault {
    SessionFault::wedge(
        deadline_error(rule),
        match phase {
            Phase::Op => McpError::close_after_timeout(),
            Phase::Connect => McpError::init_timeout(),
        },
        true,
    )
}

/// Base headers for every MCP exchange: content negotiation, the stored
/// auth header when one exists, and the negotiated session markers. The
/// `Accept` value differs by exchange: Streamable POSTs ask for both
/// framings, the SSE stream asks for events only, and SSE message POSTs
/// carry httpx's `*/*` default.
fn base_headers(
    auth: Option<&str>,
    session_id: Option<&str>,
    protocol_version: Option<&str>,
    accept: &'static str,
) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static(JSON_MIME),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static(accept),
    );
    if let Some(auth) = auth {
        if let Ok(value) = reqwest::header::HeaderValue::from_str(auth) {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
    }
    if let Some(session_id) = session_id {
        if let Ok(value) = reqwest::header::HeaderValue::from_str(session_id) {
            headers.insert("mcp-session-id", value);
        }
    }
    if let Some(protocol_version) = protocol_version {
        if let Ok(value) = reqwest::header::HeaderValue::from_str(protocol_version) {
            headers.insert("mcp-protocol-version", value);
        }
    }
    headers
}

/// One POST send with its redirect, header, and deadline policy.
struct Post<'a> {
    client: &'a reqwest::Client,
    url: reqwest::Url,
    body: &'a str,
    auth: Option<&'a str>,
    session_id: Option<&'a str>,
    protocol_version: Option<&'a str>,
    deadline: Instant,
    phase: Phase,
    rule: DeadlineRule,
    accept: &'static str,
}

/// POST with redirects followed by hand (`request_within_origin`): a POST
/// keeps its method on 307/308 same-origin only. Anything else — a
/// method-changing or off-origin redirect, or the 21st hop past the
/// budget — surfaces the redirect itself as `HTTPStatusError`
/// (`_raise_for_unfollowed_redirect`); the SDK never reports
/// `TooManyRedirects` with following off. All hops share the op deadline.
async fn post_follow(post: Post<'_>) -> Result<reqwest::Response, SessionFault> {
    let Post {
        client,
        mut url,
        body,
        auth,
        session_id,
        protocol_version,
        deadline,
        phase,
        rule,
        accept,
    } = post;
    let original_scheme = url.scheme().to_string();
    let mut followed = 0u32;
    loop {
        let strip_auth = url.scheme() != original_scheme;
        let headers = base_headers(
            if strip_auth { None } else { auth },
            session_id,
            protocol_version,
            accept,
        );
        let send = client
            .post(url.clone())
            .headers(headers)
            .body(body.to_owned());
        let response = tokio::time::timeout_at(deadline.into(), send.send())
            .await
            .map_err(|_| elapsed_wedge(phase, rule))?
            .map_err(|err| {
                let error = transport_error(phase, &err);
                SessionFault::wedge_same(error, true)
            })?;
        let status = response.status().as_u16();
        if !(300..400).contains(&status) {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|location| url.join(location).ok());
        let Some(next) = location else {
            // Redirect without a target: not a redirect at all, just an
            // unusable answer for the caller to dispatch by content.
            return Ok(response);
        };
        if followed == MAX_REDIRECTS
            || !matches!(status, 307 | 308)
            || !redirect_allowed(&url, &next)
        {
            let error = McpError::unfollowed_redirect(&url, &next);
            return Err(SessionFault::wedge_same(error, true));
        }
        url = next;
        followed += 1;
    }
}

/// Read one request's answer off a final POST response: JSON object or SSE
/// stream. Complete-but-unusable answers (202, empty stream, wrong id,
/// unparseable body, unexpected content type, bare 3xx) wait out the op
/// deadline and fail with the waiter timeout while the session stays
/// clean — the waiter outlives stream errors that never resolve it. A
/// bare 3xx without a target dispatches by content type like a 2xx
/// (`raise_for_status` is a no-op for it); only 4xx/5xx wedge.
async fn read_request_response(
    response: reqwest::Response,
    id: i64,
    deadline: Instant,
    phase: Phase,
    rule: DeadlineRule,
) -> Result<Value, SessionFault> {
    let status = response.status().as_u16();
    if status == 202 {
        return Err(no_answer(deadline, rule).await);
    }
    if status == 404 {
        return Err(SessionFault::logical(McpError::session_terminated()));
    }
    if (400..600).contains(&status) {
        let url = response.url().clone();
        return Err(SessionFault::wedge_same(
            McpError::http_status(status, url.as_str()),
            true,
        ));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_lowercase();
    if content_type.starts_with(JSON_MIME) {
        let body = tokio::time::timeout_at(deadline.into(), response.bytes())
            .await
            .map_err(|_| elapsed_wedge(phase, rule))?
            .map_err(|err| {
                let error = transport_error(phase, &err);
                SessionFault::wedge_same(error, true)
            })?;
        return match match_response_body(&body, id) {
            BodyMatch::Result(result) => Ok(result),
            BodyMatch::RpcError(message) => {
                Err(SessionFault::logical(McpError::rpc_error(&message)))
            }
            BodyMatch::NoAnswer => Err(no_answer(deadline, rule).await),
        };
    }
    if content_type.starts_with(SSE_MIME) {
        return read_sse_answer(response, id, deadline, phase, rule).await;
    }
    Err(no_answer(deadline, rule).await)
}

/// A complete response with no usable answer: sleep to the op deadline,
/// then fail with the waiter timeout. The session stays clean.
async fn no_answer(deadline: Instant, rule: DeadlineRule) -> SessionFault {
    tokio::time::sleep_until(deadline.into()).await;
    SessionFault::logical(deadline_error(rule))
}

/// How one response body relates to our request id.
enum BodyMatch {
    Result(Value),
    RpcError(String),
    NoAnswer,
}

/// Match one JSON-RPC envelope against our id. Unparseable bodies, id
/// mismatches and malformed errors are all `NoAnswer`: the waiter's
/// deadline, not a parse error, is what the op reports.
fn match_response_body(body: &[u8], id: i64) -> BodyMatch {
    let Ok(envelope) = serde_json::from_slice::<Value>(body) else {
        return BodyMatch::NoAnswer;
    };
    let Some(envelope) = envelope.as_object() else {
        return BodyMatch::NoAnswer;
    };
    if envelope.get("id") != Some(&Value::from(id)) {
        return BodyMatch::NoAnswer;
    }
    if let Some(error) = envelope.get("error").and_then(Value::as_object) {
        match error.get("message").and_then(Value::as_str) {
            Some(message) => return BodyMatch::RpcError(message.to_string()),
            None => return BodyMatch::NoAnswer,
        }
    }
    match envelope.get("result") {
        Some(result) => BodyMatch::Result(result.clone()),
        None => BodyMatch::NoAnswer,
    }
}

/// Scan a POST-response SSE stream for our id: matching responses and
/// errors resolve, everything else (progress, pings, other ids,
/// unparseable lines) is skipped. An ended stream without a match waits
/// out the deadline like any other empty answer.
async fn read_sse_answer(
    response: reqwest::Response,
    id: i64,
    deadline: Instant,
    phase: Phase,
    rule: DeadlineRule,
) -> Result<Value, SessionFault> {
    use futures_util::StreamExt as _;
    let mut stream = response.bytes_stream();
    let mut parser = SseParser::new();
    loop {
        let chunk = tokio::time::timeout_at(deadline.into(), stream.next())
            .await
            .map_err(|_| elapsed_wedge(phase, rule))?;
        let Some(chunk) = chunk else {
            return Err(no_answer(deadline, rule).await);
        };
        let chunk = chunk.map_err(|err| {
            let error = transport_error(phase, &err);
            SessionFault::wedge_same(error, true)
        })?;
        for event in parser.push(&chunk) {
            if !is_answer_event(&event) {
                continue;
            }
            match match_response_body(event.data.as_bytes(), id) {
                BodyMatch::Result(result) => return Ok(result),
                BodyMatch::RpcError(message) => {
                    return Err(SessionFault::logical(McpError::rpc_error(&message)));
                }
                BodyMatch::NoAnswer => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Streamable HTTP session
// ---------------------------------------------------------------------------

/// One connected Streamable HTTP session: ids count from 0 per connection,
/// the session id and protocol version ride every post-handshake request.
struct StreamableSession {
    client: reqwest::Client,
    url: reqwest::Url,
    auth: Option<String>,
    session_id: Option<String>,
    protocol_version: Option<String>,
    next_id: i64,
    read_timeout: Duration,
    read_secs: f64,
    dead: Option<DeadSession>,
    instructions: Option<String>,
    include_instructions: bool,
}

impl StreamableSession {
    fn new(
        client: reqwest::Client,
        url: reqwest::Url,
        auth: Option<String>,
        read_timeout: Duration,
        read_secs: f64,
        include_instructions: bool,
    ) -> Self {
        Self {
            client,
            url,
            auth,
            session_id: None,
            protocol_version: None,
            next_id: 0,
            read_timeout,
            read_secs,
            dead: None,
            instructions: None,
            include_instructions,
        }
    }

    /// One request/response exchange: POST (redirects followed), session
    /// capture on initialize answers, answer read off the final response.
    /// Owns the whole op deadline, so waiter timeouts never race the read.
    /// During connect the per-request waiter races the whole-connect
    /// bound; the sooner picks the error (`fail_after` nesting).
    async fn exchange(
        &mut self,
        body: &str,
        id: i64,
        deadline: Instant,
        phase: Phase,
        capture_session: bool,
    ) -> Result<Value, SessionFault> {
        let (deadline, rule) = match phase {
            Phase::Connect => {
                let waiter = Instant::now() + self.read_timeout;
                if waiter < deadline {
                    (waiter, DeadlineRule::Waiter(self.read_secs))
                } else {
                    (deadline, DeadlineRule::Init)
                }
            }
            Phase::Op => (deadline, DeadlineRule::Waiter(self.read_secs)),
        };
        let response = post_follow(Post {
            client: &self.client,
            url: self.url.clone(),
            body,
            auth: self.auth.as_deref(),
            session_id: self.session_id.as_deref(),
            protocol_version: self.protocol_version.as_deref(),
            deadline,
            phase,
            rule,
            accept: ACCEPT,
        })
        .await?;
        if capture_session {
            // The id is captured off initialize answers that survive
            // `raise_for_status`: 2xx but a bare 202 (whose waiter never
            // resolves), plus bare 3xx without a target. Error bodies
            // count; 4xx/5xx raise before the extraction runs.
            let status = response.status().as_u16();
            let survives =
                (200..300).contains(&status) && status != 202 || (300..400).contains(&status);
            if survives {
                if let Some(session_id) = response
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|value| value.to_str().ok())
                {
                    self.session_id = Some(session_id.to_string());
                }
            }
        }
        read_request_response(response, id, deadline, phase, rule).await
    }

    /// `initialize` + `notifications/initialized` under one connect
    /// deadline. A captured session id is terminated even on the way out
    /// of a failed handshake.
    async fn connect(&mut self, init_timeout: Duration) -> Result<(), McpError> {
        let deadline = Instant::now() + init_timeout;
        let id = self.next_id;
        self.next_id += 1;
        let result = match self
            .exchange(&initialize_body(id), id, deadline, Phase::Connect, true)
            .await
        {
            Ok(result) => result,
            Err(SessionFault::Logical(error)) | Err(SessionFault::Wedge { error, .. }) => {
                return self.fail_connect(error).await;
            }
        };
        let Some(version) = result.get("protocolVersion").and_then(Value::as_str) else {
            // Unshaped handshake answer: the waiter never resolves, so the
            // connect reports its own deadline (same rule as 202 answers).
            tokio::time::sleep_until(deadline.into()).await;
            return self.fail_connect(McpError::init_timeout()).await;
        };
        // The server's version is stamped from here on — even when the
        // handshake rejects it (the terminating DELETE carries it).
        self.protocol_version = Some(version.to_string());
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&version) {
            return self
                .fail_connect(McpError::unsupported_version(version))
                .await;
        }
        if self.include_instructions {
            self.instructions = result
                .get("instructions")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        // The notification has no waiter: a failed POST wedges the session
        // for later ops but the connect itself reports success.
        if let Err(wedge) = self.post_notification(&initialized_body(), deadline).await {
            self.dead = Some(wedge);
        }
        Ok(())
    }

    /// Failed handshake: terminate a captured session, then report.
    async fn fail_connect(&self, error: McpError) -> Result<(), McpError> {
        self.delete_best_effort().await;
        Err(error)
    }

    /// POST a notification: any 2xx/202/404 answer is success (404s are
    /// swallowed with the session alive); anything else wedges the session
    /// without recording an op error, since no waiter exists to report one.
    async fn post_notification(&self, body: &str, deadline: Instant) -> Result<(), DeadSession> {
        let response = match post_follow(Post {
            client: &self.client,
            url: self.url.clone(),
            body,
            auth: self.auth.as_deref(),
            session_id: self.session_id.as_deref(),
            protocol_version: self.protocol_version.as_deref(),
            deadline,
            phase: Phase::Op,
            rule: DeadlineRule::Waiter(self.read_secs),
            accept: ACCEPT,
        })
        .await
        {
            Ok(response) => response,
            Err(SessionFault::Logical(_)) => unreachable!("POST send never fails logically"),
            Err(SessionFault::Wedge { close, .. }) => {
                return Err(DeadSession {
                    close_error: close.close_error,
                    inflight: false,
                });
            }
        };
        let status = response.status().as_u16();
        if status == 404 || (200..300).contains(&status) {
            Ok(())
        } else {
            let url = response.url().clone();
            let error = McpError::http_status(status, url.as_str());
            Err(DeadSession {
                close_error: error,
                inflight: false,
            })
        }
    }

    /// One op exchange: POST the body, read the answer, all under a
    /// fresh read deadline.
    async fn request_page(&mut self, body: &str, id: i64) -> Result<Value, SessionFault> {
        let deadline = Instant::now() + self.read_timeout;
        self.exchange(body, id, deadline, Phase::Op, false).await
    }

    fn take_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// `DELETE` terminate: best effort, every outcome swallowed
    /// (`terminate_session`).
    async fn delete_best_effort(&self) {
        let Some(session_id) = self.session_id.as_deref() else {
            return;
        };
        let headers = base_headers(
            self.auth.as_deref(),
            Some(session_id),
            self.protocol_version.as_deref(),
            ACCEPT,
        );
        let deadline = Instant::now() + self.read_timeout;
        let send = self.client.delete(self.url.clone()).headers(headers).send();
        let _ = tokio::time::timeout_at(deadline.into(), send).await;
    }

    /// Close: an in-flight wedge re-records without `DELETE`, anything
    /// else terminates when a session id is known and records nothing.
    async fn close(self) -> Option<McpError> {
        if let Some(dead) = &self.dead {
            if dead.inflight {
                return Some(dead.close_error.clone());
            }
        }
        self.delete_best_effort().await;
        None
    }
}

/// What the turn listing says about the called tool's output schema.
enum SchemaLookup<'a> {
    /// Listed, with or without a schema: validate or skip, no refresh.
    Listed(Option<&'a Value>),
    /// Unlisted: refresh page one first.
    Unlisted,
}

/// One tool row off a `tools/list` page, before prefixing.
struct ListedTool {
    name: String,
    description: Option<String>,
    input_schema: Value,
    output_schema: Option<Value>,
}

/// Strict `ListToolsResult` shape: tools with names and input schemas.
/// Field types follow the SDK (`Tool.inputSchema` is a required object,
/// `outputSchema`/`description`/`nextCursor` are object/string-or-null):
/// mistyped fields fail the parse instead of degrading.
fn parse_list_result(result: &Value) -> Result<(Vec<ListedTool>, Option<String>), McpError> {
    let object = result
        .as_object()
        .ok_or_else(|| McpError::bad_result("tools/list result is not an object"))?;
    let tools = object
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| McpError::bad_result("tools/list result has no tools"))?;
    let mut listed = Vec::with_capacity(tools.len());
    for tool in tools {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::bad_result("listed tool has no name"))?;
        let input_schema = tool
            .get("inputSchema")
            .filter(|schema| schema.is_object())
            .cloned()
            .ok_or_else(|| {
                McpError::bad_result(format!("listed tool {name} has no inputSchema").as_str())
            })?;
        listed.push(ListedTool {
            name: name.to_string(),
            description: opt_str(tool, "description", &format!("listed tool {name}"))?,
            input_schema,
            output_schema: opt_object(tool, "outputSchema", &format!("listed tool {name}"))?,
        });
    }
    let next = opt_str(result, "nextCursor", "tools/list result")?;
    Ok((listed, next))
}

/// Optional string-or-null field: present-but-mistyped fails the parse.
fn opt_str(block: &Value, field: &str, what: &str) -> Result<Option<String>, McpError> {
    match block.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(McpError::bad_result(&format!(
            "{what} has a mistyped {field}"
        ))),
    }
}

/// Optional object-or-null field: present-but-mistyped fails the parse.
fn opt_object(block: &Value, field: &str, what: &str) -> Result<Option<Value>, McpError> {
    match block.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(object @ Value::Object(_)) => Ok(Some(object.clone())),
        Some(_) => Err(McpError::bad_result(&format!(
            "{what} has a mistyped {field}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Legacy SSE session
// ---------------------------------------------------------------------------

/// One connected legacy-SSE session: a held `GET` event stream carries the
/// answers while each message is `POST`ed to the `endpoint` URL. Requests
/// are strictly sequential here (the turn path never pipelines), so one
/// shared stream reader serves every op — no dispatcher task needed.
struct LegacySseSession {
    client: reqwest::Client,
    #[allow(dead_code)] // parsed for the base-URL check; the session dials via `client`
    base: reqwest::Url,
    auth: Option<String>,
    endpoint: reqwest::Url,
    get: GetReader,
    next_id: i64,
    read_timeout: Duration,
    read_secs: f64,
    dead: Option<DeadSession>,
    instructions: Option<String>,
    include_instructions: bool,
}

/// The held `GET` stream plus its SSE framer.
struct GetReader {
    response: reqwest::Response,
    parser: SseParser,
}

impl LegacySseSession {
    /// Open the `GET` stream and take the `endpoint` event. The wait is
    /// bounded by read-idle per chunk only — there is no absolute bound
    /// on transport establishment, matching the SDK.
    async fn open(
        client: reqwest::Client,
        base: reqwest::Url,
        auth: Option<String>,
        read_timeout: Duration,
        read_secs: f64,
        include_instructions: bool,
    ) -> Result<Self, McpError> {
        // The event-stream GET carries no content type; `Cache-Control:
        // no-store` keeps proxies from caching it (`_SSE_HEADERS`).
        let mut headers = base_headers(auth.as_deref(), None, None, SSE_MIME);
        headers.remove(reqwest::header::CONTENT_TYPE);
        headers.insert(
            "cache-control",
            reqwest::header::HeaderValue::from_static("no-store"),
        );
        let mut url = base.clone();
        for _ in 0..=MAX_REDIRECTS {
            let send = client.get(url.clone()).headers(headers.clone()).send();
            let response = tokio::time::timeout(read_timeout, send)
                .await
                .map_err(|_| McpError::connect_failed(""))?
                .map_err(|err| McpError::connect_failed(&err.to_string()))?;
            let status = response.status().as_u16();
            if !(300..400).contains(&status) {
                if !(200..300).contains(&status) {
                    // `_connect` passes HTTPStatusError through unwrapped.
                    return Err(McpError::http_status(status, response.url().as_str()));
                }
                return Self::read_endpoint(
                    client,
                    base,
                    auth,
                    response,
                    read_timeout,
                    read_secs,
                    include_instructions,
                )
                .await;
            }
            let next = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|location| url.join(location).ok());
            // The GET stream follows any status same-origin. Anything else
            // — off-origin, targetless, or past the budget — reads as an
            // empty stream below (`raise_for_status` is a no-op for 3xx),
            // so the connect reports the same missing endpoint.
            let Some(next) = next else {
                return Err(McpError::sse_no_endpoint());
            };
            if !redirect_allowed(&url, &next) {
                return Err(McpError::sse_no_endpoint());
            }
            url = next;
        }
        Err(McpError::sse_no_endpoint())
    }

    /// Read events until the `endpoint` URL arrives.
    async fn read_endpoint(
        client: reqwest::Client,
        base: reqwest::Url,
        auth: Option<String>,
        response: reqwest::Response,
        read_timeout: Duration,
        read_secs: f64,
        include_instructions: bool,
    ) -> Result<Self, McpError> {
        let mut get = GetReader {
            response,
            parser: SseParser::new(),
        };
        loop {
            let chunk = tokio::time::timeout(read_timeout, get.response.chunk())
                .await
                .map_err(|_| McpError::connect_failed(""))?
                .map_err(|err| McpError::connect_failed(&err.to_string()))?;
            let Some(chunk) = chunk else {
                return Err(McpError::sse_no_endpoint());
            };
            for event in get.parser.push(&chunk) {
                if event.event.as_deref() != Some("endpoint") || event.data.is_empty() {
                    continue;
                }
                let endpoint = base.join(&event.data).map_err(|err| {
                    McpError::connect_failed(&format!("bad endpoint URL {}: {err}", event.data))
                })?;
                // Same origin as the GET URL. A mismatch fails open here
                // instead of hanging (see the module docs); the class
                // follows `_connect`, which wraps everything but
                // HTTPStatusError/McpError.
                if !endpoint_origin_ok(&base, &endpoint) {
                    return Err(McpError::connect_failed(&format!(
                        "Endpoint origin does not match connection origin: {endpoint}"
                    )));
                }
                // The sessionId query is parsed for the (unused here)
                // session callback only; nothing in the turn path reads it.
                return Ok(Self {
                    client,
                    base,
                    auth,
                    endpoint,
                    get,
                    next_id: 0,
                    read_timeout,
                    read_secs,
                    dead: None,
                    instructions: None,
                    include_instructions,
                });
            }
        }
    }

    /// `initialize` + `notifications/initialized` under one connect
    /// deadline. SSE posts carry no session or version headers.
    async fn connect(&mut self, init_timeout: Duration) -> Result<(), McpError> {
        let deadline = Instant::now() + init_timeout;
        let id = self.take_id();
        let result = self
            .exchange(&initialize_body(id), id, deadline, Phase::Connect)
            .await
            .map_err(|fault| match fault {
                SessionFault::Logical(error) | SessionFault::Wedge { error, .. } => error,
            })?;
        let Some(version) = result.get("protocolVersion").and_then(Value::as_str) else {
            tokio::time::sleep_until(deadline.into()).await;
            return Err(McpError::init_timeout());
        };
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&version) {
            return Err(McpError::unsupported_version(version));
        }
        if self.include_instructions {
            self.instructions = result
                .get("instructions")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if let Err(wedge) = self.post_notification(&initialized_body(), deadline).await {
            self.dead = Some(wedge);
        }
        Ok(())
    }

    /// One request/response exchange: POST the message, read the answer
    /// off the held stream. POST failures are swallowed and the waiter
    /// times out (`post_writer`); there is never a DELETE to send.
    /// During connect the per-request waiter races the whole-connect
    /// bound; the sooner picks the error (`fail_after` nesting).
    async fn exchange(
        &mut self,
        body: &str,
        id: i64,
        deadline: Instant,
        phase: Phase,
    ) -> Result<Value, SessionFault> {
        let (deadline, rule) = match phase {
            Phase::Connect => {
                let waiter = Instant::now() + self.read_timeout;
                if waiter < deadline {
                    (waiter, DeadlineRule::Waiter(self.read_secs))
                } else {
                    (deadline, DeadlineRule::Init)
                }
            }
            Phase::Op => (deadline, DeadlineRule::Waiter(self.read_secs)),
        };
        let posted = post_follow(Post {
            client: &self.client,
            url: self.endpoint.clone(),
            body,
            auth: self.auth.as_deref(),
            session_id: None,
            protocol_version: None,
            deadline,
            phase,
            rule,
            accept: "*/*",
        })
        .await;
        let response = match posted {
            Ok(response) => response,
            Err(_) => return Err(self.swallowed_post(deadline, rule).await),
        };
        if !(200..300).contains(&response.status().as_u16()) {
            return Err(self.swallowed_post(deadline, rule).await);
        }
        self.read_answer(id, deadline, phase, rule).await
    }

    /// A swallowed POST: wedge the session, wait out the deadline, and
    /// report the waiter timeout.
    async fn swallowed_post(&mut self, deadline: Instant, rule: DeadlineRule) -> SessionFault {
        let error = deadline_error(rule);
        self.dead = Some(DeadSession {
            close_error: error.clone(),
            inflight: true,
        });
        no_answer(deadline, rule).await
    }

    /// Read the held stream until our id resolves. Server requests and
    /// notifications are ignored: no turn behavior answers them.
    async fn read_answer(
        &mut self,
        id: i64,
        deadline: Instant,
        phase: Phase,
        rule: DeadlineRule,
    ) -> Result<Value, SessionFault> {
        loop {
            let chunk = tokio::time::timeout_at(deadline.into(), self.get.response.chunk())
                .await
                .map_err(|_| elapsed_wedge(phase, rule))?;
            let chunk = match chunk {
                Ok(Some(chunk)) => chunk,
                // The stream died mid-wait (derived: no fixture pins which
                // raw error Python surfaces here): wedge like any other
                // dead exchange and let the waiter report the deadline.
                Ok(None) | Err(_) => {
                    let error = deadline_error(rule);
                    self.dead = Some(DeadSession {
                        close_error: error,
                        inflight: true,
                    });
                    return Err(no_answer(deadline, rule).await);
                }
            };
            for event in self.get.parser.push(&chunk) {
                if !is_answer_event(&event) {
                    continue;
                }
                match match_response_body(event.data.as_bytes(), id) {
                    BodyMatch::Result(result) => return Ok(result),
                    BodyMatch::RpcError(message) => {
                        return Err(SessionFault::logical(McpError::rpc_error(&message)));
                    }
                    BodyMatch::NoAnswer => {}
                }
            }
        }
    }

    /// POST a notification: failures wedge silently, like requests, but
    /// the connect still reports success.
    async fn post_notification(
        &mut self,
        body: &str,
        deadline: Instant,
    ) -> Result<(), DeadSession> {
        let posted = post_follow(Post {
            client: &self.client,
            url: self.endpoint.clone(),
            body,
            auth: self.auth.as_deref(),
            session_id: None,
            protocol_version: None,
            deadline,
            phase: Phase::Op,
            rule: DeadlineRule::Waiter(self.read_secs),
            accept: "*/*",
        })
        .await;
        let wedged = match posted {
            Ok(response) => !(200..300).contains(&response.status().as_u16()),
            Err(_) => true,
        };
        if wedged {
            let error = McpError::timeout("ClientRequest", self.read_secs);
            return Err(DeadSession {
                close_error: error,
                inflight: false,
            });
        }
        Ok(())
    }

    async fn request_page(&mut self, body: &str, id: i64) -> Result<Value, SessionFault> {
        let deadline = Instant::now() + self.read_timeout;
        self.exchange(body, id, deadline, Phase::Op).await
    }

    fn take_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

// ---------------------------------------------------------------------------
// Live session: transport-agnostic shells
// ---------------------------------------------------------------------------

/// Either transport behind one fail-open driver. Pagination, shape checks,
/// validation and the dead-session rules are transport-agnostic; only the
/// byte exchange and close differ.
enum LiveSession {
    Streamable(StreamableSession),
    Sse(LegacySseSession),
}

impl LiveSession {
    fn is_dead(&self) -> bool {
        match self {
            LiveSession::Streamable(session) => session.dead.is_some(),
            LiveSession::Sse(session) => session.dead.is_some(),
        }
    }

    fn set_dead(&mut self, dead: DeadSession) {
        match self {
            LiveSession::Streamable(session) => session.dead = Some(dead),
            LiveSession::Sse(session) => session.dead = Some(dead),
        }
    }

    /// Record a fault's aftermath on the session, returning the op error.
    fn apply_fault(&mut self, fault: SessionFault) -> McpError {
        match fault {
            SessionFault::Logical(error) => error,
            SessionFault::Wedge { error, close } => {
                self.set_dead(close);
                error
            }
        }
    }

    async fn request_page(&mut self, body: &str, id: i64) -> Result<Value, SessionFault> {
        match self {
            LiveSession::Streamable(session) => session.request_page(body, id).await,
            LiveSession::Sse(session) => session.request_page(body, id).await,
        }
    }

    fn take_id(&mut self) -> i64 {
        match self {
            LiveSession::Streamable(session) => session.take_id(),
            LiveSession::Sse(session) => session.take_id(),
        }
    }

    fn instructions(&self) -> Option<&str> {
        match self {
            LiveSession::Streamable(session) => session.instructions.as_deref(),
            LiveSession::Sse(session) => session.instructions.as_deref(),
        }
    }

    /// One `tools/list` page, strict shape, faults applied.
    async fn list_page(
        &mut self,
        cursor: Option<&str>,
    ) -> Result<(Vec<ListedTool>, Option<String>), McpError> {
        let id = self.take_id();
        let body = list_body(id, cursor);
        let result = self
            .request_page(&body, id)
            .await
            .map_err(|fault| self.apply_fault(fault))?;
        parse_list_result(&result).map_err(|error| self.apply_fault(SessionFault::logical(error)))
    }

    /// `tools/list` with auto-pagination: cursors followed to the end,
    /// duplicate cursors break after extending, past 250 pages the whole
    /// listing fails (partial pages are discarded).
    async fn list_tools(&mut self, failure_set: bool) -> Result<Vec<ListedTool>, McpError> {
        if self.is_dead() {
            return Err(if failure_set {
                McpError::not_connected()
            } else {
                McpError::broken_session()
            });
        }
        let mut tools = Vec::new();
        let mut seen = HashSet::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let (page, next) = self.list_page(cursor.as_deref()).await?;
            tools.extend(page);
            let Some(next) = next else {
                return Ok(tools);
            };
            if !seen.insert(next.clone()) {
                return Ok(tools);
            }
            cursor = Some(next);
        }
        Err(McpError::new(
            "RuntimeError",
            "Reached auto-pagination limit (250 pages) for list_tools. Use \
             list_tools_mcp() with cursor for manual pagination, or increase \
             max_pages.",
        ))
    }

    /// One `tools/call`: strict result shape, output-schema validation,
    /// then the FastMCP/pydantic-ai mapping. Unlisted tools refresh page
    /// one first (`_validate_tool_result`); the refresh rides the same
    /// request counter, after the call.
    async fn call_tool(
        &mut self,
        failure_set: bool,
        name: &str,
        args: &Value,
        schema: SchemaLookup<'_>,
    ) -> Result<MappedCall, McpError> {
        if self.is_dead() {
            return Err(if failure_set {
                McpError::not_connected()
            } else {
                McpError::broken_session()
            });
        }
        let id = self.take_id();
        let body = call_body(id, name, args);
        let result = self
            .request_page(&body, id)
            .await
            .map_err(|fault| self.apply_fault(fault))?;
        let object = result
            .as_object()
            .ok_or_else(|| McpError::bad_result("tools/call result is not an object"))?;
        let content = object
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| McpError::bad_result("tools/call result has no content"))?;
        let structured = object.get("structuredContent").filter(|v| !v.is_null());
        if structured.is_some_and(|structured| !structured.is_object()) {
            return Err(McpError::bad_result(
                "tools/call structuredContent is not an object",
            ));
        }
        let is_error = match object.get("isError") {
            None => false,
            Some(Value::Bool(error)) => *error,
            // Pydantic lax bools: 0/1 coerce, everything else (null,
            // strings, other numbers) fails validation.
            Some(Value::Number(number)) if number.as_u64() == Some(0) => false,
            Some(Value::Number(number)) if number.as_u64() == Some(1) => true,
            Some(_) => {
                return Err(McpError::bad_result("tools/call isError is not a bool"));
            }
        };
        if !is_error {
            match schema {
                SchemaLookup::Listed(output_schema) => {
                    validate_output(name, output_schema, structured)?;
                }
                SchemaLookup::Unlisted => {
                    // Refresh page one; its failure is the call's failure.
                    // A still-unknown tool skips validation with a warning.
                    let (page, _) = self.list_page(None).await?;
                    let output_schema = page
                        .iter()
                        .find(|tool| tool.name == name)
                        .and_then(|tool| tool.output_schema.as_ref());
                    validate_output(name, output_schema, structured)?;
                }
            }
        }
        match map_call_result(content, structured, name, is_error) {
            MappedCall::Value(value) => Ok(MappedCall::Value(value)),
            MappedCall::Retry(message) => Ok(MappedCall::Retry(message)),
            MappedCall::Invalid(error) => Err(error),
        }
    }

    /// Close: Streamable re-records an in-flight wedge without `DELETE`
    /// and otherwise terminates silently; SSE close is always silent.
    async fn close(self) -> Option<McpError> {
        match self {
            LiveSession::Streamable(session) => session.close().await,
            LiveSession::Sse(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Fail-open driver
// ---------------------------------------------------------------------------

/// The live `ResilientToolset` (`runtime/mcp.py:74-172`): connect, list
/// (cached, prefixed) and call against one MCP server, absorbing every
/// outage into a recorded exception class so the turn survives. Tool errors
/// (`isError`) are not outages: they pass through as [`CallResult::Retry`],
/// the `ModelRetry` control flow, unrecorded.
///
/// The driver is transport-agnostic: [`is_sse_url`] picks Streamable HTTP
/// or legacy SSE at connect. Calls and lists on an unconnected session
/// connect on demand, mirroring the `async with self` re-entry.
pub struct ResilientSession {
    spec: ToolsetSpec,
    state: ResilientState,
    live: Option<LiveSession>,
    tools_cache: Option<Vec<McpToolDef>>,
    instructions: Option<String>,
    last_message: Option<String>,
    http: reqwest::Client,
}

impl ResilientSession {
    /// Build the driver (`build_toolset`, `mcp.py:175-209`). Fails — the
    /// `toolset_unavailable` skip — when the URL is not http(s) or the
    /// auth header is not a valid header value.
    pub fn new(spec: ToolsetSpec) -> Result<Self, ()> {
        let url = spec.url.parse::<reqwest::Url>().map_err(|_| ())?;
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(());
        }
        if let Some(auth) = spec.auth_header.as_deref() {
            reqwest::header::HeaderValue::from_str(auth).map_err(|_| ())?;
        }
        let state = ResilientState::new(spec.server_name.clone(), spec.prefix.clone());
        Ok(Self {
            spec,
            state,
            live: None,
            tools_cache: None,
            instructions: None,
            last_message: None,
            http: build_client(),
        })
    }

    /// Connect: handshake the server (`__aenter__`). Never fails outwardly;
    /// a dead server records its class and the turn goes on.
    pub async fn enter(&mut self) {
        if self.live.is_some() {
            self.state.connect_succeeded();
            return;
        }
        let url = self
            .spec
            .url
            .parse::<reqwest::Url>()
            .expect("URL validated");
        let read_timeout = secs_to_duration(self.spec.read_timeout);
        let init_timeout = secs_to_duration(self.spec.timeout);
        let auth = self.spec.auth_header.clone();
        let include_instructions = self.spec.include_instructions;
        let connected = if is_sse_url(&self.spec.url) {
            match LegacySseSession::open(
                self.http.clone(),
                url,
                auth,
                read_timeout,
                self.spec.read_timeout,
                include_instructions,
            )
            .await
            {
                Ok(mut session) => match session.connect(init_timeout).await {
                    Ok(()) => Ok(LiveSession::Sse(session)),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        } else {
            let mut session = StreamableSession::new(
                self.http.clone(),
                url,
                auth,
                read_timeout,
                self.spec.read_timeout,
                include_instructions,
            );
            match session.connect(init_timeout).await {
                Ok(()) => Ok(LiveSession::Streamable(session)),
                Err(error) => Err(error),
            }
        };
        match connected {
            Ok(live) => {
                self.instructions = live.instructions().map(str::to_string);
                self.live = Some(live);
                self.state.connect_succeeded();
            }
            Err(error) => self.record(error),
        }
    }

    /// List tools (`get_tools`): prefixed, cached for the session, `{}` on
    /// any failure. A recorded failure short-circuits without I/O.
    pub async fn tools(&mut self) -> Vec<McpToolDef> {
        if !self.state.should_list_tools() {
            return Vec::new();
        }
        if let Some(cached) = &self.tools_cache {
            return cached.clone();
        }
        self.ensure_connected().await;
        let Some(live) = self.live.as_mut() else {
            return Vec::new();
        };
        let failure_set = self.state.failure.is_some();
        match live.list_tools(failure_set).await {
            Ok(listed) => {
                let defs = apply_prefix(&self.spec.tool_prefix, listed);
                self.tools_cache = Some(defs.clone());
                defs
            }
            Err(error) => {
                self.state.list_failed(error.class);
                self.last_message = Some(error.message);
                Vec::new()
            }
        }
    }

    /// Call one tool by its prefixed name (`call_tool`). Tool errors pass
    /// through as retries; outages absorb into the result text.
    pub async fn call(&mut self, name: &str, args: &Value) -> CallResult {
        self.ensure_connected().await;
        let Some(live) = self.live.as_mut() else {
            let failure = self.state.failure.clone().unwrap_or_default();
            return CallResult::Absorbed {
                text: pidash_services::assistant::mcp::absorb_message(
                    &self.spec.server_name,
                    &failure,
                ),
            };
        };
        let original = strip_prefix(&self.spec.tool_prefix, name);
        let schema = match self
            .tools_cache
            .as_ref()
            .and_then(|defs| defs.iter().find(|def| def.name == name))
        {
            Some(def) => SchemaLookup::Listed(def.output_schema.as_ref()),
            None => SchemaLookup::Unlisted,
        };
        let failure_set = self.state.failure.is_some();
        match live.call_tool(failure_set, &original, args, schema).await {
            Ok(MappedCall::Value(value)) => CallResult::Value(value),
            // Control flow passes through untouched (`except
            // _CONTROL_FLOW_EXCEPTIONS: raise`): only ModelRetry has a
            // source in this stack (no hooks, approval, or deferral), and
            // it records nothing.
            Ok(MappedCall::Retry(message)) => CallResult::Retry { message },
            Ok(MappedCall::Invalid(error)) | Err(error) => {
                let text = match self.state.call_failed(error.class) {
                    pidash_services::assistant::mcp::CallOutcome::Absorbed(text) => text,
                    pidash_services::assistant::mcp::CallOutcome::Passthrough => {
                        unreachable!("op errors are never control flow")
                    }
                };
                self.last_message = Some(error.message);
                CallResult::Absorbed { text }
            }
        }
    }

    /// Close (`__aexit__`): never entered means nothing to unwind; a
    /// teardown failure records like any other outage. Never fails.
    pub async fn close(&mut self) {
        if !self.state.should_close() {
            return;
        }
        if let Some(live) = self.live.take() {
            if let Some(error) = live.close().await {
                self.state.close_failed(error.class);
                self.last_message = Some(error.message);
            }
        }
        self.tools_cache = None;
    }

    /// Connect on demand when no session is live (the `async with self`
    /// re-entry in `list_tools`/`call_tool`): calls re-attempt even after
    /// a recorded failure. Lists short-circuit before reaching here.
    async fn ensure_connected(&mut self) {
        if self.live.is_none() {
            self.enter().await;
        }
    }

    fn record(&mut self, error: McpError) {
        self.state.connect_failed(error.class);
        self.last_message = Some(error.message);
    }

    /// Recorded exception class, if the server failed at any point.
    pub fn failure(&self) -> Option<&str> {
        self.state.failure.as_deref()
    }

    pub fn server_name(&self) -> &str {
        &self.spec.server_name
    }

    #[allow(dead_code)] // ported accessor; the seam reads server_name/failure
    pub fn prefix(&self) -> &str {
        &self.spec.prefix
    }

    /// The server's `instructions`, when requested and offered.
    #[allow(dead_code)] // `include_instructions` is false on the turn path (Python's default)
    pub fn instructions(&self) -> Option<&str> {
        self.instructions.as_deref()
    }

    /// The last recorded error's message, for logs (the class is what the
    /// user sees).
    #[allow(dead_code)] // diagnostics accessor; the seam reports the exception class
    pub fn last_message(&self) -> Option<&str> {
        self.last_message.as_deref()
    }
}

/// Prefix listed tools (`PrefixedToolset.get_tools`): `{prefix}_{name}`.
/// Duplicate names keep the last def at the first position (dict order).
fn apply_prefix(prefix: &str, listed: Vec<ListedTool>) -> Vec<McpToolDef> {
    let mut defs: Vec<McpToolDef> = Vec::with_capacity(listed.len());
    for tool in listed {
        let name = if prefix.is_empty() {
            tool.name.clone()
        } else {
            format!("{prefix}_{}", tool.name)
        };
        let def = McpToolDef {
            name,
            original: tool.name,
            description: tool.description,
            input_schema: tool.input_schema,
            output_schema: tool.output_schema,
        };
        if let Some(existing) = defs.iter_mut().find(|existing| existing.name == def.name) {
            *existing = def;
        } else {
            defs.push(def);
        }
    }
    defs
}

/// Strip one `{prefix}_` for the server call (`PrefixedToolset.call_tool`).
/// Empty prefixes never wrapped, so nothing is stripped then.
fn strip_prefix(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        return name.to_string();
    }
    name.strip_prefix(&format!("{prefix}_"))
        .unwrap_or(name)
        .to_string()
}

// ---------------------------------------------------------------------------
// Config wiring
// ---------------------------------------------------------------------------

/// Decrypt the stored MCP auth header for `auth_header_for`
/// (`mcp.py:257-265` + `crypto.py:89-96`): the backend's decryptor adapts a
/// ciphertext into the header or a classified failure — a missing key is
/// `AssistantError` (surfaced by code), anything else unreadable is opaque.
#[allow(dead_code)] // the seam decrypts through `assistant::common` directly (same call)
pub fn decrypt_auth_header(
    token: &[u8],
    decrypt: impl FnOnce(&[u8]) -> Result<String, CryptoError>,
) -> Result<String, DecryptFailure> {
    match decrypt(token) {
        Ok(header) => Ok(header),
        Err(CryptoError::NotConfigured(error)) => Err(DecryptFailure::Assistant(error)),
        Err(CryptoError::Transport(_)) => Err(DecryptFailure::Opaque),
    }
}

// ---------------------------------------------------------------------------
// Unit tests (pure surface; the stub-server parity suite lives in mcp_parity)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_bodies_are_byte_exact() {
        assert_eq!(
            initialize_body(0),
            r#"{"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"mcp","version":"0.1.0"}},"jsonrpc":"2.0","id":0}"#
        );
        assert_eq!(
            initialized_body(),
            r#"{"method":"notifications/initialized","jsonrpc":"2.0"}"#
        );
        assert_eq!(
            list_body(1, None),
            r#"{"method":"tools/list","jsonrpc":"2.0","id":1}"#
        );
        assert_eq!(
            list_body(2, Some("c1")),
            r#"{"method":"tools/list","params":{"cursor":"c1"},"jsonrpc":"2.0","id":2}"#
        );
        assert_eq!(
            call_body(2, "echo", &json!({"text": "hi"})),
            r#"{"method":"tools/call","params":{"name":"echo","arguments":{"text":"hi"},"_meta":{"progressToken":2}},"jsonrpc":"2.0","id":2}"#
        );
    }

    #[test]
    fn sse_url_inference() {
        for url in [
            "http://h/sse",
            "http://h/sse/",
            "http://h/x/sse/y",
            "https://h:8443/a/sse?x=1",
            "http://h/a/sse&x=1",
        ] {
            assert!(is_sse_url(url), "{url}");
        }
        for url in [
            "http://h/mcp",
            "http://h/sse2",
            "http://h/xsse",
            "http://h/assets",
            "http://h/",
            "http://h",
        ] {
            assert!(!is_sse_url(url), "{url}");
        }
    }

    #[test]
    fn sse_parser_frames_events() {
        let mut parser = SseParser::new();
        let events = parser.push(b"event: message\ndata: {\"a\": 1}\n\n: ping\n\ndata: x\n");
        assert_eq!(
            events,
            vec![SseEvent {
                event: Some("message".to_string()),
                data: "{\"a\": 1}".to_string(),
            }]
        );
        // Partial tail completes on the next push; CRLF normalizes.
        let events = parser.push(b"data: y\r\n\r\nevent: endpoint\r\ndata: /m?q=1\r\n\r\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "x\ny");
        assert_eq!(events[1].event.as_deref(), Some("endpoint"));
        // A bare field without a colon is a field with an empty value.
        let mut parser = SseParser::new();
        let events = parser.push(b"data\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "");
    }

    #[test]
    fn sse_parser_survives_chunk_splits() {
        // A multi-byte char split across chunks decodes whole, not as
        // replacement chars ("caf\u{e9}", never "caf\u{fffd}\u{fffd}").
        let mut parser = SseParser::new();
        // "data: caf" + first byte of "\u{e9}".
        assert!(parser.push(b"data: caf\xc3").is_empty());
        // Second byte of "\u{e9}", still no event until the blank line.
        assert!(parser.push(b"\xa9").is_empty());
        let events = parser.push(b"\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "caf\u{e9}");
        // A \r\n pair split across chunks is one line break, not a blank
        // line: the two data lines stay one event.
        let mut parser = SseParser::new();
        assert!(parser.push(b"data: x\r").is_empty());
        let events = parser.push(b"\ndata: y\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "x\ny");
    }

    #[test]
    fn redirect_rules() {
        let prev: reqwest::Url = "http://h:80/mcp".parse().unwrap();
        let same: reqwest::Url = "http://h:80/mcp2".parse().unwrap();
        let upgrade: reqwest::Url = "https://h/mcp".parse().unwrap();
        let cross_port: reqwest::Url = "http://h:9/mcp".parse().unwrap();
        let cross_host: reqwest::Url = "http://x:80/mcp".parse().unwrap();
        assert!(redirect_allowed(&prev, &same));
        assert!(redirect_allowed(&prev, &upgrade));
        assert!(!redirect_allowed(&prev, &cross_port));
        assert!(!redirect_allowed(&prev, &cross_host));
    }

    #[test]
    fn endpoint_origin_rules() {
        let base: reqwest::Url = "http://h/sse".parse().unwrap();
        let same: reqwest::Url = "http://h/messages?x=1".parse().unwrap();
        let cross_scheme: reqwest::Url = "https://h/messages".parse().unwrap();
        let cross_host: reqwest::Url = "http://x/messages".parse().unwrap();
        let cross_port: reqwest::Url = "http://h:9/messages".parse().unwrap();
        let explicit_port: reqwest::Url = "http://h:80/messages".parse().unwrap();
        assert!(endpoint_origin_ok(&base, &same));
        assert!(!endpoint_origin_ok(&base, &cross_scheme));
        assert!(!endpoint_origin_ok(&base, &cross_host));
        assert!(!endpoint_origin_ok(&base, &cross_port));
        assert!(endpoint_origin_ok(&base, &explicit_port));
    }

    #[test]
    fn python_floats() {
        assert_eq!(py_float(0.3), "0.3");
        assert_eq!(py_float(0.5), "0.5");
        assert_eq!(py_float(5.0), "5.0");
        assert_eq!(py_float(300.0), "300.0");
    }

    #[test]
    fn error_texts() {
        assert_eq!(
            McpError::timeout("ClientRequest", 0.5).to_string(),
            "McpError: Timed out while waiting for response to ClientRequest. Waited 0.5 seconds."
        );
        assert_eq!(
            McpError::http_status(500, "http://h/mcp").message,
            "Server error '500 Internal Server Error' for url 'http://h/mcp'\n\
             For more information check: \
             https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/500"
        );
        assert_eq!(
            McpError::http_status(404, "http://h/mcp").class,
            "HTTPStatusError"
        );
        assert!(McpError::http_status(404, "http://h/mcp")
            .message
            .starts_with("Client error '404 Not Found'"));
        let plain: reqwest::Url = "http://h/mcp".parse().unwrap();
        let target: reqwest::Url = "http://x/mcp".parse().unwrap();
        assert_eq!(
            McpError::unfollowed_redirect(&plain, &target).message,
            "Redirect to http://x/mcp not followed; use that URL as the endpoint if it is the intended server"
        );
        // Userinfo, query and fragment never reach the message.
        let noisy: reqwest::Url = "http://u:p@x/mcp?k=v#frag".parse().unwrap();
        assert_eq!(
            McpError::unfollowed_redirect(&plain, &noisy).message,
            "Redirect to http://x/mcp not followed; use that URL as the endpoint if it is the intended server"
        );
        // An https endpoint refusing a plain-http target names the downgrade.
        let secure: reqwest::Url = "https://h/mcp".parse().unwrap();
        let downgrade: reqwest::Url = "http://h/mcp".parse().unwrap();
        assert_eq!(
            McpError::unfollowed_redirect(&secure, &downgrade).message,
            "Redirect to http://h/mcp not followed: it would downgrade this HTTPS endpoint to plain HTTP.\n\
             The server is likely behind a TLS-terminating proxy whose forwarded headers it does not trust,\n\
             often combined with a trailing-slash difference. Try https://h/mcp instead, or fix the proxy settings."
        );
        assert_eq!(
            McpError::init_timeout().message,
            "Client failed to connect: Failed to initialize server session"
        );
        assert_eq!(McpError::close_after_timeout().message, "");
        assert_eq!(McpError::broken_session().message, "");
        assert_eq!(
            McpError::not_connected().message,
            "Client is not connected. Use the 'async with client:' context manager first."
        );
        assert_eq!(McpError::session_terminated().message, "Session terminated");
        assert_eq!(
            McpError::schema_missing("echo").message,
            "Tool echo has an output schema but did not return structured content"
        );
    }

    #[test]
    fn text_mapping_parses_json_only() {
        assert_eq!(map_text("hi"), McpContent::Text("hi".to_string()));
        assert_eq!(map_text("[1, 2]"), McpContent::Json(json!([1, 2])));
        assert_eq!(
            map_text("  {\"a\": 1}"),
            McpContent::Text("  {\"a\": 1}".to_string())
        );
        assert_eq!(map_text("[oops"), McpContent::Text("[oops".to_string()));
        assert_eq!(map_text("42"), McpContent::Text("42".to_string()));
    }

    #[test]
    fn result_unwrap() {
        assert_eq!(
            unwrap_single_result(&json!({"result": 42})),
            Some(json!(42))
        );
        assert_eq!(unwrap_single_result(&json!({"a": 1, "b": 2})), None);
        assert_eq!(unwrap_single_result(&json!({"other": 1})), None);
        assert_eq!(unwrap_single_result(&json!([1])), None);
    }

    #[test]
    fn call_mapping() {
        let text = |text: &str| json!({"type": "text", "text": text});
        // Errors: first text wins, non-text names the tool, structured ignored.
        assert!(matches!(
            map_call_result(&[text("boom")], None, "echo", true),
            MappedCall::Retry(message) if message == "boom"
        ));
        assert!(matches!(
            map_call_result(&[text("a"), text("b")], None, "echo", true),
            MappedCall::Retry(message) if message == "a"
        ));
        assert!(matches!(
            map_call_result(&[text("bad")], Some(&json!({"a": 1})), "echo", true),
            MappedCall::Retry(message) if message == "bad"
        ));
        assert!(matches!(
            map_call_result(
                &[json!({"type": "image", "data": "aGk=", "mimeType": "image/png"})],
                None,
                "echo",
                true
            ),
            MappedCall::Retry(message) if message == "Tool 'echo' returned an error"
        ));
        // Structured wins over all-text content only.
        assert!(matches!(
            map_call_result(&[text("42")], Some(&json!({"result": 42})), "echo", false),
            MappedCall::Value(McpValue::Structured(value)) if value == json!(42)
        ));
        assert!(matches!(
            map_call_result(
                &[json!({"type": "image", "data": "aGk=", "mimeType": "image/png"})],
                Some(&json!({"a": 1})),
                "echo",
                false
            ),
            MappedCall::Value(McpValue::Single(McpContent::Image { .. }))
        ));
        // Unknown blocks fail validation (an op error, not a retry).
        assert!(matches!(
            map_call_result(&[json!({"type": "future"})], None, "echo", false),
            MappedCall::Invalid(error) if error.class == "ValidationError"
        ));
        // Empty structured payloads are falsy: the content mapping wins.
        assert!(matches!(
            map_call_result(&[text("hi")], Some(&json!({})), "echo", false),
            MappedCall::Value(McpValue::Single(McpContent::Text(text))) if text == "hi"
        ));
    }

    #[test]
    fn strict_shapes() {
        let invalid = |block: Value| map_content_block(&block).unwrap_err().class.to_string();
        // Required content fields: missing or mistyped fails validation.
        assert_eq!(invalid(json!({"type": "text"})), "ValidationError");
        assert_eq!(
            invalid(json!({"type": "text", "text": 5})),
            "ValidationError"
        );
        assert_eq!(
            invalid(json!({"type": "image", "data": "aGk="})),
            "ValidationError"
        );
        assert_eq!(
            invalid(json!({"type": "image", "mimeType": "image/png"})),
            "ValidationError"
        );
        assert_eq!(
            invalid(json!({"type": "audio", "data": "aGk="})),
            "ValidationError"
        );
        assert_eq!(
            invalid(json!({"type": "resource_link", "name": "r"})),
            "ValidationError"
        );
        assert_eq!(
            invalid(json!({"type": "resource", "resource": {"text": "x"}})),
            "ValidationError"
        );
        assert_eq!(
            invalid(json!({"type": "resource", "resource": {"uri": "u:"}})),
            "ValidationError"
        );
        // Well-formed blocks still map.
        assert!(map_content_block(&json!({"type": "text", "text": "hi"})).is_ok());
        assert!(
            map_content_block(&json!({"type": "resource_link", "uri": "u:", "name": "r"})).is_ok()
        );
        // Listed tools: inputSchema must be an object, the optionals keep
        // their types.
        let listed = |tool: Value| parse_list_result(&json!({"tools": [tool]}));
        assert!(listed(json!({"name": "a", "inputSchema": {}})).is_ok());
        assert!(listed(json!({"name": "a", "inputSchema": []})).is_err());
        assert!(listed(json!({"name": "a"})).is_err());
        assert!(listed(json!({"name": "a", "inputSchema": {}, "description": 5})).is_err());
        assert!(listed(json!({"name": "a", "inputSchema": {}, "outputSchema": []})).is_err());
        assert!(parse_list_result(&json!({"tools": [], "nextCursor": 5})).is_err());
        assert!(parse_list_result(&json!({"tools": [], "nextCursor": Value::Null})).is_ok());
    }

    #[test]
    fn prefixing() {
        let listed = |name: &str| ListedTool {
            name: name.to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
            output_schema: None,
        };
        let defs = apply_prefix("mcp_x", vec![listed("echo")]);
        assert_eq!(defs[0].name, "mcp_x_echo");
        assert_eq!(defs[0].original, "echo");
        assert_eq!(strip_prefix("mcp_x", "mcp_x_echo"), "echo");
        assert_eq!(strip_prefix("mcp_x", "echo"), "echo");
        // Empty prefixes never wrapped: names pass through untouched.
        let defs = apply_prefix("", vec![listed("echo")]);
        assert_eq!(defs[0].name, "echo");
        assert_eq!(strip_prefix("", "mcp_x_echo"), "mcp_x_echo");
        // Duplicate names keep the last def at the first position.
        let defs = apply_prefix("p", vec![listed("a"), listed("b"), listed("a")]);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].original, "a");
        assert_eq!(defs[1].original, "b");
    }

    #[test]
    fn durations() {
        assert_eq!(secs_to_duration(0.3), Duration::from_millis(300));
        assert_eq!(secs_to_duration(0.0), Duration::ZERO);
        assert_eq!(secs_to_duration(-1.0), Duration::ZERO);
        assert_eq!(secs_to_duration(f64::NAN), Duration::ZERO);
        assert!(secs_to_duration(f64::INFINITY) > Duration::from_secs(3600));
    }

    #[test]
    fn answer_events() {
        let event = |name: Option<&str>, data: &str| SseEvent {
            event: name.map(str::to_string),
            data: data.to_string(),
        };
        assert!(is_answer_event(&event(Some("message"), "{}")));
        assert!(!is_answer_event(&event(Some("message"), "")));
        assert!(!is_answer_event(&event(Some("endpoint"), "{}")));
        assert!(!is_answer_event(&event(None, "{}")));
    }

    #[test]
    fn response_matching() {
        assert!(matches!(
            match_response_body(br#"{"jsonrpc":"2.0","id":2,"result":{"a":1}}"#, 2),
            BodyMatch::Result(_)
        ));
        assert!(matches!(
            match_response_body(br#"{"jsonrpc":"2.0","id":2,"error":{"code":-1,"message":"x"}}"#, 2),
            BodyMatch::RpcError(message) if message == "x"
        ));
        // Wrong id, unparseable, and malformed errors never resolve.
        assert!(matches!(
            match_response_body(br#"{"jsonrpc":"2.0","id":3,"result":{}}"#, 2),
            BodyMatch::NoAnswer
        ));
        assert!(matches!(
            match_response_body(b"{oops", 2),
            BodyMatch::NoAnswer
        ));
        assert!(matches!(
            match_response_body(b"[1,2]", 2),
            BodyMatch::NoAnswer
        ));
        assert!(matches!(
            match_response_body(br#"{"jsonrpc":"2.0","id":2,"error":{"code":-1}}"#, 2),
            BodyMatch::NoAnswer
        ));
    }

    #[test]
    fn output_validation() {
        let schema =
            json!({"type": "object", "properties": {"a": {"type": "integer"}}, "required": ["a"]});
        assert!(validate_output("echo", None, None).is_ok());
        assert!(validate_output("echo", None, Some(&json!({"a": 1}))).is_ok());
        assert!(validate_output("echo", Some(&schema), Some(&json!({"a": 1}))).is_ok());
        let missing = validate_output("echo", Some(&schema), None).unwrap_err();
        assert_eq!(missing.class, "RuntimeError");
        assert!(missing
            .message
            .contains("did not return structured content"));
        let invalid = validate_output("echo", Some(&schema), Some(&json!({"a": "x"}))).unwrap_err();
        assert_eq!(invalid.class, "RuntimeError");
        assert!(invalid.message.contains("Invalid structured content"));
        let unusable =
            validate_output("echo", Some(&json!({"type": "nonsense"})), Some(&json!({})))
                .unwrap_err();
        assert_eq!(unusable.class, "RuntimeError");
        assert!(unusable.message.contains("Invalid schema"));
    }

    #[test]
    fn auth_decrypt_mapping() {
        use pidash_services::assistant::crypto::CryptoError;
        use pidash_types::assistant::errors::AssistantError;
        let ok = decrypt_auth_header(b"token", |_| Ok("Bearer [REDACTED]".to_string()));
        assert_eq!(ok, Ok("Bearer [REDACTED]".to_string()));
        let missing = decrypt_auth_header(b"token", |_| {
            Err(CryptoError::NotConfigured(
                AssistantError::AssistantNotConfigured("no key".to_string()),
            ))
        });
        assert!(matches!(missing, Err(DecryptFailure::Assistant(_))));
        let broken = decrypt_auth_header(b"token", |_| {
            Err(CryptoError::Transport("kms down".to_string()))
        });
        assert!(matches!(broken, Err(DecryptFailure::Opaque)));
    }

    #[test]
    fn constructor_validates() {
        use pidash_services::assistant::mcp::build_toolset_spec;
        let good = build_toolset_spec("http://h/mcp", None, Some("mcp_h"), false, 10.0, 300.0, "h");
        assert!(ResilientSession::new(good).is_ok());
        let bad_url = build_toolset_spec("not a url", None, Some("mcp_h"), false, 10.0, 300.0, "h");
        assert!(ResilientSession::new(bad_url).is_err());
        let bad_scheme =
            build_toolset_spec("ftp://h/mcp", None, Some("mcp_h"), false, 10.0, 300.0, "h");
        assert!(ResilientSession::new(bad_scheme).is_err());
        let bad_header = build_toolset_spec(
            "http://h/mcp",
            Some("a\nb"),
            Some("mcp_h"),
            false,
            10.0,
            300.0,
            "h",
        );
        assert!(ResilientSession::new(bad_header).is_err());
    }
}
