#![forbid(unsafe_code)]

//! Request and API-token logging, mirroring
//! `pi_dash.middleware.logger.RequestLoggerMiddleware` and
//! `APITokenLogMiddleware`.
//!
//! Python reference: `apps/api/pi_dash/middleware/logger.py` and
//! `apps/api/pi_dash/bgtasks/logger_task.py` (`safe_decode_body`,
//! `process_logs`). Rules reproduced exactly:
//!
//! - `RequestLoggerMiddleware` skips only `GET /` (the health check); every
//!   other request emits `"{method} {full_path} {status}"` on the
//!   `pi_dash.api.request` logger with `path`, `method`, `status_code`,
//!   `duration_ms` (`int(duration * 1000)`, truncating), `remote_addr`,
//!   `user_agent` and `user_id` attached. `remote_addr` is the first
//!   `X-Forwarded-For` entry when present — the split takes index 0 with
//!   no stripping, verbatim — else the connection peer. `user_id` is the
//!   authenticated user's id, or `None` for anonymous requests.
//! - `APITokenLogMiddleware` runs only when the `X-Api-Key` header is
//!   present and non-empty. It records the `log_data` payload (token
//!   identifier, path, method, raw query string, headers rendered as
//!   Python's `str(request.headers)` dict repr, safely-decoded request and
//!   response bodies, response code, client IP, user agent) plus the
//!   MongoDB extension (`created_at`, `updated_at`, `created_by`,
//!   `updated_by`). Body decoding follows `safe_decode_body`: `None` for
//!   `None`/empty, `"[Binary Content]"` for PNG/JPEG/PDF magic,
//!   `"[Could not decode content]"` on UTF-8 failure.
//!
//! The Python middleware hands the payload to the `process_logs` Celery
//! task (`.delay()`), whose Mongo/Postgres writes belong to the bgtasks
//! domain port. This layer computes the exact payloads and delivers them to
//! a [`LogSink`]; the shipped [`TracingSink`] emits them as structured
//! tracing events (the `RequestLogger` half is the `pi_dash.api.request`
//! logger by another name). The bgtasks port consumes [`ApiTokenLogRecord`]
//! to enqueue `process_logs` without recomputing anything. Sinks are
//! infallible by construction — Python swallows logging exceptions via
//! `log_exception`, and there is nothing to swallow here.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use axum::body::Body;
use axum::http::{header, Request, Response};
use tower::{Layer, Service};

/// Logger name for request lines, matching `api_logger`.
pub const REQUEST_LOGGER_TARGET: &str = "pi_dash.api.request";
/// The header gating token logging, verbatim (`request.headers.get` is
/// case-insensitive; so is `HeaderMap`).
pub const API_KEY_HEADER: &str = "x-api-key";

/// Extension key carrying the authenticated user's id for the loggers.
/// Auth layers insert `Some(id)` once sessions land; absence means
/// anonymous, exactly like `request.user.is_authenticated == False`.
#[derive(Debug, Clone)]
pub struct LoggerUserId(pub Option<String>);

/// One `RequestLoggerMiddleware` line, before rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLogRecord {
    pub method: String,
    pub path: String,
    pub full_path: String,
    pub status_code: u16,
    pub duration_ms: u64,
    pub remote_addr: Option<String>,
    pub user_agent: String,
    pub user_id: Option<String>,
}

impl RequestLogRecord {
    /// The log message, verbatim:
    /// `f"{method} {full_path} {status_code}"`.
    pub fn message(&self) -> String {
        format!("{} {} {}", self.method, self.full_path, self.status_code)
    }
}

/// One `APITokenLogMiddleware` payload: the Celery `log_data` dict plus the
/// MongoDB extension fields (`created_at`, `updated_at`, `created_by`,
/// `updated_by` as `Option<String>`s; timestamps render as UTC ISO-8601).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiTokenLogRecord {
    pub token_identifier: String,
    pub path: String,
    pub method: String,
    pub query_params: String,
    pub headers: String,
    pub body: Option<String>,
    pub response_body: Option<String>,
    pub response_code: u16,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
}

/// Where log records go. Infallible by construction (see module docs).
pub trait LogSink: Clone + Send + Sync + 'static {
    fn request(&self, record: &RequestLogRecord);
    fn token(&self, record: &ApiTokenLogRecord);
}

/// The shipped sink: structured tracing events. The request half targets
/// `pi_dash.api.request`, the Python logger name.
#[derive(Debug, Clone, Default)]
pub struct TracingSink;

impl LogSink for TracingSink {
    fn request(&self, record: &RequestLogRecord) {
        tracing::info!(
            target: REQUEST_LOGGER_TARGET,
            path = %record.path,
            method = %record.method,
            status_code = record.status_code,
            duration_ms = record.duration_ms,
            remote_addr = ?record.remote_addr,
            user_agent = %record.user_agent,
            user_id = ?record.user_id,
            "{}",
            record.message(),
        );
    }

    fn token(&self, record: &ApiTokenLogRecord) {
        tracing::info!(
            target: REQUEST_LOGGER_TARGET,
            token_identifier = %record.token_identifier,
            path = %record.path,
            method = %record.method,
            query_params = %record.query_params,
            headers = %record.headers,
            body = ?record.body,
            response_body = ?record.response_body,
            response_code = record.response_code,
            ip_address = ?record.ip_address,
            user_agent = ?record.user_agent,
            created_by = ?record.created_by,
            updated_by = ?record.updated_by,
            "api token {} {} {}",
            record.method,
            record.path,
            record.response_code,
        );
    }
}

/// Shared knobs for both logging layers.
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Emit request lines at all. Always true in Django; the flag exists
    /// so tests can disable one layer at a time.
    pub enabled: bool,
}

impl LoggingConfig {
    pub fn enabled() -> Self {
        Self { enabled: true }
    }
}

impl Default for LoggingConfig {
    /// Django always logs; the default matches.
    fn default() -> Self {
        Self::enabled()
    }
}

/// Render headers as Python's `str(request.headers)`: a `{Name: 'value'}`
/// dict repr in iteration order, values single-quote-escaped like `repr`.
/// Names go through Django's `parse_header_name` normalization
/// (`HTTP_X_API_KEY` -> `X-Api-Key`, i.e. Python `str.title()` on the
/// hyphenated name); `http::HeaderName` only keeps the lowercase form, so
/// the title-casing is reapplied here (see `django_title`).
pub fn render_headers(headers: &header::HeaderMap) -> String {
    let mut out = String::from("{");
    for (i, (name, value)) in headers.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        // Values are latin-1 in WSGI, UTF-8-lossy here.
        out.push('\'');
        out.push_str(&django_title(name.as_str()));
        out.push_str("': '");
        out.push_str(&python_repr(&String::from_utf8_lossy(value.as_bytes())));
        out.push('\'');
    }
    out.push('}');
    out
}

/// Python `str.title()`, restricted to ASCII header names: a cased letter
/// following a non-cased character uppercases, a cased letter following a
/// cased character lowercases, everything else passes through and resets
/// the state. Digits and `-` are not cased, so `x-2fa-trace` becomes
/// `X-2Fa-Trace` exactly like Django's `parse_header_name`.
fn django_title(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_cased = false;
    for c in name.chars() {
        let cased = c.is_ascii_alphabetic();
        if cased && !prev_cased {
            out.extend(c.to_uppercase());
        } else if cased && prev_cased {
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
        prev_cased = cased;
    }
    out
}

/// Python `repr` for a `str`, restricted to what header values need:
/// backslash, single-quote, and control escapes.
fn python_repr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// `safe_decode_body`, verbatim: `None` for `None`/empty, `"[Binary
/// Content]"` for PNG/JPEG/PDF magic, UTF-8 or `"[Could not decode
/// content]"`.
pub fn safe_decode_body(content: Option<&[u8]>) -> Option<String> {
    let content = content?;
    if content.is_empty() {
        return None;
    }
    if content.starts_with(b"\x89PNG")
        || content.starts_with(b"\xff\xd8\xff")
        || content.starts_with(b"%PDF")
    {
        return Some("[Binary Content]".to_string());
    }
    match std::str::from_utf8(content) {
        Ok(text) => Some(text.to_string()),
        Err(_) => Some("[Could not decode content]".to_string()),
    }
}

/// `get_client_ip`, verbatim: first `X-Forwarded-For` entry (no strip),
/// else the connection peer.
pub fn client_ip(headers: &header::HeaderMap, peer: Option<String>) -> Option<String> {
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        return Some(forwarded.split(',').next().unwrap_or("").to_string());
    }
    peer
}

fn user_agent(headers: &header::HeaderMap, default: &str) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or(default)
        .to_string()
}

fn logger_user_id(req: &Request<Body>) -> Option<String> {
    req.extensions()
        .get::<LoggerUserId>()
        .and_then(|u| u.0.clone())
}

fn should_log_request(method: &str, path: &str) -> bool {
    // `_should_log_route`: health checks (`GET /`) are not logged.
    !(method == "GET" && path == "/")
}

// ---------------------------------------------------------------------------
// RequestLogger
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RequestLoggerLayer<K> {
    config: LoggingConfig,
    sink: K,
}

impl<K> RequestLoggerLayer<K> {
    pub fn new(config: LoggingConfig, sink: K) -> Self {
        Self { config, sink }
    }
}

impl<S, K> Layer<S> for RequestLoggerLayer<K>
where
    K: Clone,
{
    type Service = RequestLoggerService<S, K>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestLoggerService {
            inner,
            config: self.config.clone(),
            sink: self.sink.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestLoggerService<S, K> {
    inner: S,
    config: LoggingConfig,
    sink: K,
}

impl<S, K> Service<Request<Body>> for RequestLoggerService<S, K>
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    K: LogSink,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        // Stash every log input before the request moves: the logger reads
        // the request, never the response, except for the status code.
        let method = req.method().to_string();
        let path = req.uri().path().to_string();
        let full_path = req
            .uri()
            .path_and_query()
            .map(|pq| pq.to_string())
            .unwrap_or_else(|| path.clone());
        let remote_addr = client_ip(req.headers(), peer_addr(&req));
        let agent = user_agent(req.headers(), "");
        let user_id = logger_user_id(&req);
        let log = should_log_request(&method, &path);
        let config = self.config.clone();
        let sink = self.sink.clone();
        let start = Instant::now();
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        Box::pin(async move {
            let response = inner.call(req).await?;
            if config.enabled && log {
                sink.request(&RequestLogRecord {
                    method,
                    path,
                    full_path,
                    status_code: response.status().as_u16(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    remote_addr,
                    user_agent: agent,
                    user_id,
                });
            }
            Ok(response)
        })
    }
}

/// Connection peer from axum's `ConnectInfo`, when the server installs it
/// (`into_make_service_with_connect_info`). Absent under test oneshot,
/// which is the `REMOTE_ADDR`-missing case.
fn peer_addr(req: &Request<Body>) -> Option<String> {
    req.extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string())
}

// ---------------------------------------------------------------------------
// APITokenLog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TokenLogLayer<K> {
    config: LoggingConfig,
    sink: K,
}

impl<K> TokenLogLayer<K> {
    pub fn new(config: LoggingConfig, sink: K) -> Self {
        Self { config, sink }
    }
}

impl<S, K> Layer<S> for TokenLogLayer<K>
where
    K: Clone,
{
    type Service = TokenLogService<S, K>;

    fn layer(&self, inner: S) -> Self::Service {
        TokenLogService {
            inner,
            config: self.config.clone(),
            sink: self.sink.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenLogService<S, K> {
    inner: S,
    config: LoggingConfig,
    sink: K,
}

/// Current UTC time as ISO-8601 (`timezone.now()` for the mongo
/// extension fields). libc-free civil-from-days arithmetic, no date
/// dependency for one timestamp.
fn utc_now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}+00:00",
        tod / 3_600,
        (tod % 3_600) / 60,
        tod % 60,
    )
}

impl<S, K> Service<Request<Body>> for TokenLogService<S, K>
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    K: LogSink,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        // `if not api_key: return` — absent or empty means no logging, and
        // the request passes through untouched (and unbuffered).
        let api_key = req
            .headers()
            .get(API_KEY_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        let config = self.config.clone();
        let sink = self.sink.clone();
        Box::pin(async move {
            let Some(api_key) = api_key else {
                return inner.call(req).await;
            };
            // Buffer the request body so it can be logged AND forwarded —
            // Django reads `request.body` up front the same way. An
            // unreadable body fails closed (502), like the other layers.
            let (parts, body) = req.into_parts();
            let request_bytes = match http_body_util::BodyExt::collect(body).await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => return Ok(crate::middleware::truncated_body()),
            };
            let method = parts.method.to_string();
            let path = parts.uri.path().to_string();
            let query_params = parts.uri.query().unwrap_or("").to_string();
            let headers_rendered = render_headers(&parts.headers);
            let ip_address = client_ip(&parts.headers, None);
            let agent = parts
                .headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let req = Request::from_parts(parts, Body::from(request_bytes.clone()));
            let user_id = logger_user_id(&req);
            let response = inner.call(req).await?;
            let (resp_parts, resp_body) = response.into_parts();
            let response_bytes = match http_body_util::BodyExt::collect(resp_body).await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => return Ok(crate::middleware::truncated_body()),
            };
            if config.enabled {
                let now = utc_now_iso();
                sink.token(&ApiTokenLogRecord {
                    token_identifier: api_key,
                    path,
                    method,
                    query_params,
                    headers: headers_rendered,
                    body: safe_decode_body(Some(&request_bytes)),
                    response_body: safe_decode_body(Some(&response_bytes)),
                    response_code: resp_parts.status.as_u16(),
                    ip_address,
                    user_agent: agent,
                    created_at: now.clone(),
                    updated_at: now,
                    created_by: user_id.clone(),
                    updated_by: user_id,
                });
            }
            Ok(Response::from_parts(resp_parts, Body::from(response_bytes)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, StatusCode};
    use tower::{Layer, ServiceExt};

    #[derive(Debug, Clone)]
    enum Record {
        Request(RequestLogRecord),
        Token(ApiTokenLogRecord),
    }

    #[derive(Debug, Clone, Default)]
    struct VecSink {
        records: std::sync::Arc<std::sync::Mutex<Vec<Record>>>,
    }

    impl VecSink {
        fn take(&self) -> Vec<Record> {
            std::mem::take(&mut *self.records.lock().expect("sink lock"))
        }
    }

    impl LogSink for VecSink {
        fn request(&self, record: &RequestLogRecord) {
            self.records
                .lock()
                .expect("sink lock")
                .push(Record::Request(record.clone()));
        }

        fn token(&self, record: &ApiTokenLogRecord) {
            self.records
                .lock()
                .expect("sink lock")
                .push(Record::Token(record.clone()));
        }
    }

    /// Concrete inner service (see cors tests for why `Router`).
    fn ok_router() -> axum::Router {
        axum::Router::new()
            .route("/", axum::routing::any(|| async { "ok" }))
            .route("/api/x/", axum::routing::any(|| async { "done" }))
            .route("/upload/", axum::routing::any(|| async { "ok" }))
    }

    #[test]
    fn safe_decode_body_matches_python() {
        assert_eq!(safe_decode_body(None), None);
        assert_eq!(safe_decode_body(Some(b"")), None);
        assert_eq!(safe_decode_body(Some(b"hello")), Some("hello".to_string()));
        assert_eq!(
            safe_decode_body(Some(b"\x89PNG\r\n")),
            Some("[Binary Content]".to_string())
        );
        assert_eq!(
            safe_decode_body(Some(b"\xff\xd8\xff\xe0")),
            Some("[Binary Content]".to_string())
        );
        assert_eq!(
            safe_decode_body(Some(b"%PDF-1.4")),
            Some("[Binary Content]".to_string())
        );
        assert_eq!(
            safe_decode_body(Some(b"\xff\xfe\x00invalid")),
            Some("[Could not decode content]".to_string())
        );
    }

    #[test]
    fn client_ip_prefers_first_forwarded_entry_unstripped() {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.2.3.4, 5.6.7.8"),
        );
        // No strip, verbatim Python: index 0 of the split.
        assert_eq!(
            client_ip(&headers, Some("9.9.9.9".to_string())),
            Some("1.2.3.4".to_string())
        );
        assert_eq!(
            client_ip(&header::HeaderMap::new(), Some("9.9.9.9".to_string())),
            Some("9.9.9.9".to_string())
        );
        assert_eq!(client_ip(&header::HeaderMap::new(), None), None);
    }

    #[test]
    fn headers_render_like_python_str_of_headers() {
        let mut headers = header::HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("sekret"));
        headers.insert(header::USER_AGENT, HeaderValue::from_static("UA/1"));
        // Python dict repr: insertion order, Django title-cased names
        // (`parse_header_name`, verified against live Django), single quotes.
        assert_eq!(
            render_headers(&headers),
            "{'X-Api-Key': 'sekret', 'User-Agent': 'UA/1'}"
        );
    }

    #[test]
    fn header_names_follow_django_title_casing() {
        // `str.title()` on the hyphenated name, including the digit-reset
        // rule (`2x` keeps its capitals). Vectors checked against CPython.
        assert_eq!(django_title("x-api-key"), "X-Api-Key");
        assert_eq!(django_title("content-type"), "Content-Type");
        assert_eq!(django_title("x-forwarded-for"), "X-Forwarded-For");
        assert_eq!(django_title("etag"), "Etag");
        assert_eq!(django_title("x-2fa-trace"), "X-2Fa-Trace");
        assert_eq!(django_title("x-rate-limit-2x"), "X-Rate-Limit-2X");
    }

    #[test]
    fn request_record_message_matches_python_format() {
        let record = RequestLogRecord {
            method: "POST".to_string(),
            path: "/api/x/".to_string(),
            full_path: "/api/x/?a=1".to_string(),
            status_code: 200,
            duration_ms: 12,
            remote_addr: Some("1.2.3.4".to_string()),
            user_agent: "UA".to_string(),
            user_id: None,
        };
        assert_eq!(record.message(), "POST /api/x/?a=1 200");
    }

    #[tokio::test]
    async fn request_logger_skips_health_check() {
        let sink = VecSink::default();
        let response = RequestLoggerLayer::new(LoggingConfig::enabled(), sink.clone())
            .layer(ok_router())
            .oneshot(Request::get("/").body(Body::empty()).expect("request"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(sink.take().is_empty());
    }

    #[tokio::test]
    async fn request_logger_records_fields() {
        let sink = VecSink::default();
        // The auth layers (outside this innermost layer) set the user id
        // on the request before the logger snapshots it — mirroring
        // AuthenticationMiddleware running before the view-adjacent logger.
        let response = RequestLoggerLayer::new(LoggingConfig::enabled(), sink.clone())
            .layer(ok_router())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/x/?a=1")
                    .header("x-forwarded-for", "1.2.3.4, 5.6.7.8")
                    .header(header::USER_AGENT, "UA/1")
                    .extension(LoggerUserId(Some("user-7".to_string())))
                    .body(Body::from("hi"))
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let records = sink.take();
        assert_eq!(records.len(), 1);
        let Record::Request(record) = &records[0] else {
            panic!("expected a request record");
        };
        assert_eq!(record.method, "POST");
        assert_eq!(record.path, "/api/x/");
        assert_eq!(record.full_path, "/api/x/?a=1");
        assert_eq!(record.status_code, 200);
        assert_eq!(record.remote_addr.as_deref(), Some("1.2.3.4"));
        assert_eq!(record.user_agent, "UA/1");
        assert_eq!(record.user_id.as_deref(), Some("user-7"));
        assert_eq!(record.message(), format!("POST /api/x/?a=1 200"));
    }

    #[tokio::test]
    async fn token_log_skips_requests_without_api_key() {
        let sink = VecSink::default();
        let response = TokenLogLayer::new(LoggingConfig::enabled(), sink.clone())
            .layer(ok_router())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/x/?a=1")
                    .body(Body::from("hi"))
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(sink.take().is_empty());
    }

    #[tokio::test]
    async fn token_log_records_exact_payload() {
        let sink = VecSink::default();
        let response = TokenLogLayer::new(LoggingConfig::enabled(), sink.clone())
            .layer(ok_router())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/x/?a=1")
                    .header("x-api-key", "sekret")
                    .header(header::USER_AGENT, "UA/1")
                    .header("x-forwarded-for", "1.2.3.4")
                    .extension(LoggerUserId(Some("user-9".to_string())))
                    .body(Body::from("{\"k\": 1}"))
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        // Bodies survive the layer byte-identical in both directions.
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("body")
            .to_bytes();
        assert_eq!(&body[..], b"done");
        let records = sink.take();
        assert_eq!(records.len(), 1);
        let Record::Token(record) = &records[0] else {
            panic!("expected a token record");
        };
        assert_eq!(record.token_identifier, "sekret");
        assert_eq!(record.path, "/api/x/");
        assert_eq!(record.method, "POST");
        assert_eq!(record.query_params, "a=1");
        assert!(record.headers.contains("'X-Api-Key': 'sekret'"));
        assert_eq!(record.body.as_deref(), Some("{\"k\": 1}"));
        assert_eq!(record.response_body.as_deref(), Some("done"));
        assert_eq!(record.response_code, 200);
        assert_eq!(record.ip_address.as_deref(), Some("1.2.3.4"));
        assert_eq!(record.user_agent.as_deref(), Some("UA/1"));
        assert_eq!(record.created_by.as_deref(), Some("user-9"));
        assert_eq!(record.updated_by.as_deref(), Some("user-9"));
        assert!(record.created_at.ends_with("+00:00"));
        assert_eq!(record.created_at, record.updated_at);
    }

    #[tokio::test]
    async fn token_log_empty_key_is_gated_like_python_falsy() {
        let sink = VecSink::default();
        // `if not api_key: return` — an empty header value skips logging.
        let response = TokenLogLayer::new(LoggingConfig::enabled(), sink.clone())
            .layer(ok_router())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/x/")
                    .header("x-api-key", "")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(sink.take().is_empty());
    }

    #[tokio::test]
    async fn token_log_marks_binary_bodies() {
        let sink = VecSink::default();
        let png = [b"\x89PNG\r\n".as_slice(), &[0u8; 64]].concat();
        let response = TokenLogLayer::new(LoggingConfig::enabled(), sink.clone())
            .layer(ok_router())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/upload/")
                    .header("x-api-key", "sekret")
                    .body(Body::from(png.clone()))
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let records = sink.take();
        assert_eq!(records.len(), 1);
        let Record::Token(record) = &records[0] else {
            panic!("expected a token record");
        };
        assert_eq!(record.body.as_deref(), Some("[Binary Content]"));
    }
}
