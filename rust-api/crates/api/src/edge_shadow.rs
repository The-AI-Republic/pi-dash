//! Shadow-read mode for the cutover edge (PIDASHCONV-822, stage 8 validation).
//!
//! While a prefix flag is OFF, traffic on that prefix proxies to Django.
//! With shadow mode on, the proxy additionally computes the Rust handler's
//! response for the same safe read in the background and compares the two,
//! so real traffic validates the port without users ever seeing Rust's
//! answers.
//!
//! - [`ShadowConfig`]: `PIDASH_RUST_SHADOW` (default off),
//!   `PIDASH_RUST_SHADOW_MAX_INFLIGHT` (default 8),
//!   `PIDASH_RUST_SHADOW_SAMPLE` (0.0–1.0, default 1.0).
//! - Eligibility (checked in [`precheck`], before proxying): GET/HEAD only,
//!   every matched prefix flag OFF, no `X-Api-Key` (both backends stamp
//!   `last_used` on every such request, so a shadow re-dispatch would
//!   double-write), path not on [`DENYLIST`], sampled in, inflight permit
//!   available. Anything else counts `skipped`.
//! - The client response is built by the same [`crate::edge`] funnel with
//!   or without shadow: the Django body streams to the client untouched
//!   while a tee ([`TeeStream`]) captures a bounded copy. Shadow work runs
//!   in a spawned task after the response is sent and never blocks it.
//! - The Rust side re-dispatches the same request (method, path+query,
//!   client headers, body) through a shadow router: the same groups and
//!   additive routes, a shadow [`EdgeHandle`](crate::edge::EdgeHandle)
//!   with all flags on, a read-only session layer (session reads stay
//!   faithful, session saves are dropped), and a marker fallback. A shadow
//!   dispatch that reaches the proxy or the fallback means "no Rust
//!   handler" and counts `skipped` — it never hits Django twice.
//! - Comparison ([`compare`]): status, content-type, body. Byte-identical
//!   bodies match immediately; otherwise JSON bodies are normalized over
//!   the fixed volatile allowlist ([`VOLATILE_JSON_KEYS`],
//!   [`VOLATILE_QUERY_PARAMS`]) and compared semantically. Everything else
//!   must match byte for byte.
//! - A mismatch logs one structured `shadow mismatch` line (route
//!   template, method, both statuses, redacted body excerpt capped at
//!   [`DIFF_CAP_BYTES`], never the full body) and is counted; counters
//!   live on [`METRICS_PATH`].

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Version};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use bytes::Bytes;
use futures_core::Stream;
use serde::Serialize;
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};
use tower::ServiceExt as _;

use crate::middleware::{PgSessionStore, SessionConfig, SessionLayer, SessionStore};
use crate::state::AppState;

/// Env var enabling shadow reads. Same truthy rule as the prefix flags
/// (`1/true/yes/on`, case-insensitive); absent or anything else is off.
pub const SHADOW_ENV: &str = "PIDASH_RUST_SHADOW";
/// Env var for the shadow concurrency cap. Over the cap, requests skip
/// shadow and count `skipped`.
pub const SHADOW_MAX_INFLIGHT_ENV: &str = "PIDASH_RUST_SHADOW_MAX_INFLIGHT";
/// Env var for the shadow sampling rate, clamped to 0.0–1.0.
pub const SHADOW_SAMPLE_ENV: &str = "PIDASH_RUST_SHADOW_SAMPLE";
/// Default concurrency cap: at most this many shadow comparisons in flight.
pub const MAX_INFLIGHT_DEFAULT: usize = 8;
/// Default sampling rate: shadow every eligible read.
pub const SAMPLE_DEFAULT: f64 = 1.0;
/// Internal metrics endpoint. Always served by Rust (like `/healthz`);
/// Django has no `/internal/*` route, so there is nothing to collide with.
pub const METRICS_PATH: &str = "/internal/shadow-metrics";
/// Mismatch body excerpts are capped at 2 KB total, redacted, never the
/// full body.
pub const DIFF_CAP_BYTES: usize = 2048;
/// Django/Rust bodies larger than this are never buffered for comparison;
/// the request counts `skipped` and the client still streams untouched.
const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;
/// Bounds each shadow phase separately: waiting for the Django bytes, then
/// the Rust dispatch plus comparison. A slow client skips (never errors);
/// a slow Rust handler errors.
const SHADOW_TIMEOUT: Duration = Duration::from_secs(10);
/// Counter map cap: distinct route templates beyond this collapse into
/// [`OVERFLOW_TEMPLATE`] so a pathological path space cannot grow memory.
const MAX_TEMPLATES: usize = 4096;
/// Overflow bucket for templates past [`MAX_TEMPLATES`].
const OVERFLOW_TEMPLATE: &str = "_other";
/// Recent-mismatch ring kept for the metrics endpoint (redacted excerpts).
const RECENT_CAP: usize = 20;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Shadow-read configuration. Default is fully off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowConfig {
    /// [`SHADOW_ENV`] truthy.
    pub enabled: bool,
    /// [`SHADOW_MAX_INFLIGHT_ENV`], default [`MAX_INFLIGHT_DEFAULT`].
    /// Zero is a valid kill switch: every shadow attempt skips.
    pub max_inflight: usize,
    /// [`SHADOW_SAMPLE_ENV`], clamped to 0.0–1.0.
    pub sample_rate: f64,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_inflight: MAX_INFLIGHT_DEFAULT,
            sample_rate: SAMPLE_DEFAULT,
        }
    }
}

impl ShadowConfig {
    /// Read from the environment. Unparseable numbers fall back to their
    /// defaults (the operator's intent was shadowing when they enabled it).
    pub fn from_env() -> Self {
        let enabled = crate::edge::env_is_truthy(SHADOW_ENV);
        let max_inflight = std::env::var(SHADOW_MAX_INFLIGHT_ENV)
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(MAX_INFLIGHT_DEFAULT);
        let sample_rate = std::env::var(SHADOW_SAMPLE_ENV)
            .ok()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .map(|v| v.clamp(0.0, 1.0))
            .unwrap_or(SAMPLE_DEFAULT);
        Self {
            enabled,
            max_inflight,
            sample_rate,
        }
    }

    /// Enabled config for tests: on, default cap, full sampling.
    pub fn enabled_for_tests() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

/// Pure sampling rule, drawn against a uniform [0, 1) `draw`.
pub fn should_sample(rate: f64, draw: f64) -> bool {
    draw < rate.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Denylist: GETs the router marks as having side effects
// ---------------------------------------------------------------------------

/// GET/HEAD paths that must never be shadowed, with the reason each one is
/// unsafe to re-dispatch. `{param}` segments match one path segment.
/// Enforced twice: [`precheck`] skips these before proxying, and the
/// shadow router serves a marker for them, so a matcher bug still cannot
/// re-invoke one.
///
/// The list is explicit by design (PIDASHCONV-822: "when unsure, do not
/// shadow"). Adding a route here needs the same evidence each row cites:
/// the Django view or Rust handler that writes, mints, or consumes.
pub const DENYLIST: &[(&str, &str)] = &[
    (
        "/auth/get-csrf-token/",
        "mints a fresh CSRF token and cookie on every call \
         (authentication/views/common.py CSRFTokenEndpoint)",
    ),
    (
        "/auth/github/",
        "OAuth initiate: fresh state/nonce plus a session write, 302 varies",
    ),
    (
        "/auth/github/callback/",
        "OAuth callback: consumes the single-use authorization code",
    ),
    (
        "/auth/spaces/github/",
        "OAuth initiate: fresh state/nonce plus a session write, 302 varies",
    ),
    (
        "/auth/spaces/github/callback/",
        "OAuth callback: consumes the single-use authorization code",
    ),
    (
        "/auth/google/",
        "OAuth initiate: fresh state/nonce plus a session write, 302 varies",
    ),
    (
        "/auth/google/callback/",
        "OAuth callback: consumes the single-use authorization code",
    ),
    (
        "/auth/spaces/google/",
        "OAuth initiate: fresh state/nonce plus a session write, 302 varies",
    ),
    (
        "/auth/spaces/google/callback/",
        "OAuth callback: consumes the single-use authorization code",
    ),
    (
        "/auth/gitlab/",
        "OAuth initiate: fresh state/nonce plus a session write, 302 varies",
    ),
    (
        "/auth/gitlab/callback/",
        "OAuth callback: consumes the single-use authorization code",
    ),
    (
        "/auth/spaces/gitlab/",
        "OAuth initiate: fresh state/nonce plus a session write, 302 varies",
    ),
    (
        "/auth/spaces/gitlab/callback/",
        "OAuth callback: consumes the single-use authorization code",
    ),
    (
        "/auth/gitea/",
        "OAuth initiate: fresh state/nonce plus a session write, 302 varies",
    ),
    (
        "/auth/gitea/callback/",
        "OAuth callback: consumes the single-use authorization code",
    ),
    (
        "/auth/spaces/gitea/",
        "OAuth initiate: fresh state/nonce plus a session write, 302 varies",
    ),
    (
        "/auth/spaces/gitea/callback/",
        "OAuth callback: consumes the single-use authorization code",
    ),
    (
        "/api/v1/workspaces/{slug}/assets/{asset_id}/",
        "GET mints a fresh presigned download URL per call \
         (v1_assets/asset_generic.rs generic_get)",
    ),
    (
        "/api/public/assets/v2/anchor/{anchor}/{pk}/",
        "AllowAny presigned-URL 302 redirect, fresh per call \
         (space/assets.rs get_asset)",
    ),
];

/// Match a concrete request path (no query) against one denylist template.
/// `{name}` matches any single non-empty segment.
fn template_matches(template: &str, path: &str) -> bool {
    let mut template_segments = template.split('/').peekable();
    let mut path_segments = path.split('/').peekable();
    loop {
        match (template_segments.next(), path_segments.next()) {
            (None, None) => return true,
            (Some(t), Some(p)) => {
                let param = t.starts_with('{') && t.ends_with('}') && t.len() > 2;
                if param {
                    if p.is_empty() {
                        return false;
                    }
                } else if t != p {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// The denylist reason when `path` must never be shadowed.
pub fn denylist_reason(path: &str) -> Option<&'static str> {
    DENYLIST
        .iter()
        .find(|(template, _)| template_matches(template, path))
        .map(|(_, reason)| *reason)
}

// ---------------------------------------------------------------------------
// Volatile allowlist: the only normalization shadow performs
// ---------------------------------------------------------------------------

/// JSON object keys whose values are generated per request and legitimately
/// differ between the Django and Rust responses. Exact, case-sensitive
/// match at any nesting depth. Everything else must match byte for byte.
///
/// Deliberately small: no Django GET serializer emits request-time scalars
/// outside presigned URLs (verified by grep at implementation time), so
/// request ids are the only key-level volatiles. Extend only with cited
/// evidence, never speculatively — each entry weakens every comparison.
pub const VOLATILE_JSON_KEYS: &[&str] = &["request_id", "requestId"];

/// Query parameters redacted inside URL strings found in JSON bodies.
/// Presigned URLs embed the signing time (`X-Amz-Date` / `Expires`) and a
/// signature over it, so two mints of the same object never share bytes;
/// the parameters are matched case-insensitively, values replaced.
pub const VOLATILE_QUERY_PARAMS: &[&str] = &[
    "x-amz-date",
    "x-amz-expires",
    "x-amz-signature",
    "expires",
    "signature",
    "sig",
];

/// Header/field names whose values are secrets. Redaction requires the name
/// to be followed (after optional whitespace/quotes) by `:` or `=`, so prose
/// mentioning a secret ("the secret is out") is left alone. Longest first:
/// `client_secret` must win over `secret`. Case-insensitive.
const SENSITIVE_KEYS: &[&str] = &[
    "proxy-authorization",
    "authorization",
    "refresh_token",
    "refresh-token",
    "access_token",
    "access-token",
    "client_secret",
    "id_token",
    "set-cookie",
    "csrftoken",
    "csrf_token",
    "session-id",
    "session_id",
    "sessionid",
    "x-api-key",
    "api_key",
    "apikey",
    "password",
    "passwd",
    "cookie",
    "token",
    "secret",
    "signature",
    "bearer",
    "pwd",
];

// ---------------------------------------------------------------------------
// Counters
// ---------------------------------------------------------------------------

/// Per-template shadow counters. `compared` always equals
/// `matched + mismatched`: every completed comparison lands in exactly one
/// of those two buckets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RouteCounters {
    pub compared: u64,
    pub matched: u64,
    pub mismatched: u64,
    pub skipped: u64,
    pub errored: u64,
}

/// How one proxied request resolved under shadow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Matched,
    Mismatched,
    Skipped,
    Errored,
}

/// One ring entry: a redacted mismatch summary for [`METRICS_PATH`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MismatchRecord {
    pub route: String,
    pub method: String,
    pub django_status: u16,
    pub rust_status: u16,
    /// Redacted body excerpt, already capped at [`DIFF_CAP_BYTES`].
    pub diff: String,
}

#[derive(Debug, Default)]
struct MetricsInner {
    routes: HashMap<String, RouteCounters>,
    totals: RouteCounters,
    recent: VecDeque<MismatchRecord>,
}

#[derive(Debug, Default)]
struct ShadowMetrics {
    inner: Mutex<MetricsInner>,
}

impl ShadowMetrics {
    fn count(&self, template: &str, outcome: Outcome) {
        let mut inner = self.inner.lock().expect("shadow metrics lock");
        let key = if inner.routes.contains_key(template) || inner.routes.len() < MAX_TEMPLATES {
            template.to_owned()
        } else {
            OVERFLOW_TEMPLATE.to_owned()
        };
        Self::bump(inner.routes.entry(key).or_default(), outcome);
        Self::bump(&mut inner.totals, outcome);
    }

    fn bump(counters: &mut RouteCounters, outcome: Outcome) {
        match outcome {
            Outcome::Matched => {
                counters.compared += 1;
                counters.matched += 1;
            }
            Outcome::Mismatched => {
                counters.compared += 1;
                counters.mismatched += 1;
            }
            Outcome::Skipped => {
                counters.skipped += 1;
            }
            Outcome::Errored => {
                counters.errored += 1;
            }
        }
    }

    fn record_mismatch(&self, record: MismatchRecord) {
        let mut inner = self.inner.lock().expect("shadow metrics lock");
        if inner.recent.len() >= RECENT_CAP {
            inner.recent.pop_front();
        }
        inner.recent.push_back(record);
    }

    fn snapshot(&self) -> MetricsSnapshot {
        let inner = self.inner.lock().expect("shadow metrics lock");
        MetricsSnapshot {
            totals: inner.totals,
            routes: inner.routes.clone(),
            recent_mismatches: inner.recent.iter().cloned().collect(),
        }
    }
}

/// The JSON body of [`METRICS_PATH`], minus the `enabled` flag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
struct MetricsSnapshot {
    totals: RouteCounters,
    routes: HashMap<String, RouteCounters>,
    recent_mismatches: Vec<MismatchRecord>,
}

// ---------------------------------------------------------------------------
// Gate
// ---------------------------------------------------------------------------

/// Live shadow state: config, counters, inflight semaphore, and the shadow
/// router (built once per app). Held behind an `Arc` in [`AppState`].
pub struct ShadowGate {
    config: ShadowConfig,
    metrics: ShadowMetrics,
    semaphore: std::sync::Arc<Semaphore>,
    shadow_app: Router,
}

impl fmt::Debug for ShadowGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShadowGate")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ShadowGate {
    pub(crate) fn new(config: ShadowConfig, shadow_app: Router) -> Self {
        Self {
            semaphore: std::sync::Arc::new(Semaphore::new(config.max_inflight)),
            metrics: ShadowMetrics::default(),
            config,
            shadow_app,
        }
    }

    pub(crate) fn count(&self, template: &str, outcome: Outcome) {
        self.metrics.count(template, outcome);
    }
}

// ---------------------------------------------------------------------------
// Route templates
// ---------------------------------------------------------------------------

/// Collapse a concrete path to its counter key: integer and UUID segments
/// become `{id}`, everything else stays. Keeps counter cardinality bounded
/// without a route table; the map cap plus [`OVERFLOW_TEMPLATE`] bounds the
/// rest (slugs).
pub fn route_template(path: &str) -> String {
    let collapsed: Vec<&str> = path.split('/').collect();
    let mut out = String::with_capacity(path.len());
    for (index, segment) in collapsed.iter().enumerate() {
        if index > 0 {
            out.push('/');
        }
        if index > 0 && is_id_segment(segment) {
            out.push_str("{id}");
        } else {
            out.push_str(segment);
        }
    }
    out
}

/// An all-digit segment, a dashed UUID, or a 32-hex UUID.
fn is_id_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    if segment.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    let hex = segment.strip_prefix("urn:uuid:").unwrap_or(segment);
    let dashed = hex.len() == 36
        && hex.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        });
    let plain = hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit());
    dashed || plain
}

// ---------------------------------------------------------------------------
// Eligibility
// ---------------------------------------------------------------------------

/// An approved shadow run: the counter template, the held inflight permit,
/// and the gate to count against.
pub(crate) struct ShadowPlan {
    template: String,
    gate: std::sync::Arc<ShadowGate>,
    permit: OwnedSemaphorePermit,
}

/// Decide, before proxying, whether this request shadows. Rejections count
/// `skipped` when shadow is enabled; when disabled this returns `None`
/// without touching anything, so the proxy path is exactly today's.
pub(crate) fn precheck(state: &AppState, req: &Request) -> Option<ShadowPlan> {
    let gate = state.shadow_gate()?.clone();
    if !gate.config.enabled {
        return None;
    }
    let path = req.uri().path().to_owned();
    let template = route_template(&path);
    let reject = |outcome| {
        gate.count(&template, outcome);
        None
    };

    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        return reject(Outcome::Skipped);
    }
    // Shadow only reads Django currently serves: any flipped prefix means
    // Rust may already own the response.
    if state
        .edge()
        .flags()
        .any_rust(&crate::edge::match_prefixes(&path))
    {
        return reject(Outcome::Skipped);
    }
    // Both backends stamp last_used on every X-Api-Key request (and the
    // machine-token path can revoke); a shadow re-dispatch would write.
    if req
        .headers()
        .contains_key(pidash_auth::token::API_KEY_HEADER)
    {
        return reject(Outcome::Skipped);
    }
    if denylist_reason(&path).is_some() {
        return reject(Outcome::Skipped);
    }
    if !should_sample(gate.config.sample_rate, rand::random::<f64>()) {
        return reject(Outcome::Skipped);
    }
    match gate.semaphore.clone().try_acquire_owned() {
        Ok(permit) => Some(ShadowPlan {
            template,
            gate,
            permit,
        }),
        Err(_) => reject(Outcome::Skipped),
    }
}

// ---------------------------------------------------------------------------
// No-handler marker
// ---------------------------------------------------------------------------

/// Why a shadow dispatch produced no Rust response. Carried in a private
/// response extension, so it can never collide with a real body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoHandlerReason {
    /// The shadow router has no route: nothing ported there yet.
    Fallback,
    /// The shadow dispatch reached the proxy (an unowned method): the Rust
    /// handler for this method does not exist, so there is nothing to
    /// compare against — and no second Django hit was made.
    Proxied,
    /// A [`DENYLIST`] route: refused even as defense in depth.
    Denylisted,
}

/// Marker extension; presence means "no Rust response, count skipped".
#[derive(Debug, Clone, Copy)]
pub(crate) struct ShadowNoHandler(pub NoHandlerReason);

/// A marker response: status and body are never compared (the extension
/// short-circuits first), so their values only matter for debuggability.
pub(crate) fn no_handler_response(reason: NoHandlerReason) -> Response {
    let mut response = (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": {
                "code": "shadow_no_handler",
                "message": "shadow dispatch has no Rust handler",
            }
        })),
    )
        .into_response();
    response.extensions_mut().insert(ShadowNoHandler(reason));
    response
}

async fn no_handler_fallback() -> Response {
    no_handler_response(NoHandlerReason::Fallback)
}

async fn denylisted_marker() -> Response {
    no_handler_response(NoHandlerReason::Denylisted)
}

/// Outer shadow layer: every [`DENYLIST`] template served as a marker.
/// Backup for the [`precheck`] denylist skip — even a matcher bug cannot
/// re-invoke a side-effect route through the shadow router.
pub(crate) fn denylist_router() -> Router<AppState> {
    let mut router = Router::new();
    for (template, _) in DENYLIST {
        router = router.route(template, get(denylisted_marker));
    }
    router
}

// ---------------------------------------------------------------------------
// Django capture
// ---------------------------------------------------------------------------

/// What the tee captured (or why it did not).
#[derive(Debug)]
pub(crate) enum CaptureOutcome {
    Captured(Vec<u8>),
    /// Body exceeded [`MAX_CAPTURE_BYTES`]: forwarding continued, the copy
    /// was abandoned.
    Abandoned,
    /// The upstream stream errored: the client saw today's truncation
    /// behavior, and there are no Django bytes to compare.
    UpstreamError,
}

/// A stream adapter that forwards every upstream chunk untouched while
/// buffering a bounded copy.
///
/// Completion has two triggers because hyper releases a content-length
/// body once its framed bytes are sent, without polling the stream to
/// `None`: when the framed length is known the capture completes the
/// moment the buffer reaches it; otherwise the terminal `None` completes
/// it. Dropping the tee before either (client disconnect) abandons the
/// capture — comparing against partial bytes would be noise.
pub(crate) struct TeeStream<S> {
    inner: Pin<Box<S>>,
    buf: Vec<u8>,
    abandoned: bool,
    expected_len: Option<usize>,
    tx: Option<oneshot::Sender<CaptureOutcome>>,
}

impl<S> TeeStream<S> {
    pub(crate) fn new(
        inner: S,
        tx: oneshot::Sender<CaptureOutcome>,
        expected_len: Option<usize>,
    ) -> Self {
        Self {
            inner: Box::pin(inner),
            buf: Vec::new(),
            abandoned: false,
            expected_len,
            tx: Some(tx),
        }
    }

    fn finish(&mut self, outcome: impl FnOnce(Vec<u8>, bool) -> CaptureOutcome) {
        if let Some(tx) = self.tx.take() {
            let buf = std::mem::take(&mut self.buf);
            let _ = tx.send(outcome(buf, self.abandoned));
        }
    }

    /// Complete early once the framed length is buffered: hyper may never
    /// poll this stream again.
    fn complete_if_expected(&mut self) {
        if self.tx.is_some() && self.expected_len.is_some_and(|n| self.buf.len() >= n) {
            self.finish(|buf, _| CaptureOutcome::Captured(buf));
        }
    }
}

impl<S> Drop for TeeStream<S> {
    fn drop(&mut self) {
        // Completed captures already took the sender; anything left here
        // is an incomplete observation (disconnect, truncation).
        self.finish(|_, _| CaptureOutcome::Abandoned);
    }
}

impl<S> Stream for TeeStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>>,
{
    type Item = Result<Bytes, reqwest::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                if self.tx.is_some() && !self.abandoned {
                    if self.buf.len() + bytes.len() > MAX_CAPTURE_BYTES {
                        self.abandoned = true;
                        self.buf.clear();
                    } else {
                        self.buf.extend_from_slice(&bytes);
                        self.complete_if_expected();
                    }
                }
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.finish(|_, _| CaptureOutcome::UpstreamError);
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                // Without a framed length this is the only completion
                // signal; with one, fewer bytes than framed is a truncation
                // and the partial observation is abandoned.
                let complete =
                    !self.abandoned && self.expected_len.is_none_or(|n| self.buf.len() == n);
                self.finish(|buf, _| {
                    if complete {
                        CaptureOutcome::Captured(buf)
                    } else {
                        CaptureOutcome::Abandoned
                    }
                });
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// The Django side of one comparison: status and content-type from the
/// upstream headers, body awaited from the tee.
struct DjangoCapture {
    status: u16,
    content_type: String,
    rx: oneshot::Receiver<CaptureOutcome>,
}

/// The request parts the shadow re-dispatch rebuilds: the client's method,
/// path+query, version, headers, and body. Deliberately not the
/// proxy-forwarded headers: Rust-as-origin would see the client envelope,
/// not the proxy's `x-forwarded-*` additions.
pub(crate) struct ShadowRequestParts {
    pub(crate) method: Method,
    pub(crate) path_and_query: String,
    pub(crate) version: Version,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
}

impl ShadowRequestParts {
    fn into_request(self) -> Request {
        let mut request = Request::builder()
            .method(self.method)
            .uri(self.path_and_query)
            .version(self.version)
            .body(axum::body::Body::from(self.body))
            .expect("shadow request parts are request-shaped");
        *request.headers_mut() = self.headers;
        request
    }
}

/// Answer the client from the upstream response and spawn the shadow
/// comparison. Event streams and bodies already known to exceed the capture
/// cap skip shadow without teeing: the client streams exactly as today.
/// Bodies that are definitionally empty (HEAD, 204/304, zero
/// content-length) also skip the tee: the client path is today's exact
/// code, and the shadow side starts complete.
pub(crate) async fn respond_with_shadow(
    plan: ShadowPlan,
    upstream: reqwest::Response,
    parts: ShadowRequestParts,
) -> Response {
    let status = upstream.status();
    let headers = upstream.headers().clone();
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let expected_len = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    if is_event_stream(&content_type) || expected_len.is_some_and(|len| len > MAX_CAPTURE_BYTES) {
        plan.gate.count(&plan.template, Outcome::Skipped);
        return crate::edge::forward_upstream_response(
            status,
            &headers,
            axum::body::Body::from_stream(upstream.bytes_stream()),
        );
    }
    if !expects_body(&parts.method, status.as_u16()) || expected_len == Some(0) {
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(CaptureOutcome::Captured(Vec::new()));
        let django = DjangoCapture {
            status: status.as_u16(),
            content_type,
            rx,
        };
        tokio::spawn(run_shadow(plan, django, parts));
        return crate::edge::forward_upstream_response(
            status,
            &headers,
            axum::body::Body::from_stream(upstream.bytes_stream()),
        );
    }
    let (tx, rx) = oneshot::channel();
    let stream = TeeStream::new(upstream.bytes_stream(), tx, expected_len);
    let django = DjangoCapture {
        status: status.as_u16(),
        content_type,
        rx,
    };
    tokio::spawn(run_shadow(plan, django, parts));
    crate::edge::forward_upstream_response(status, &headers, axum::body::Body::from_stream(stream))
}

/// Whether the response can carry a body: HEAD answers and 204/304 never
/// do, whatever the headers claim.
fn expects_body(method: &Method, status: u16) -> bool {
    *method != Method::HEAD && status != 204 && status != 304
}

fn is_event_stream(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
}

/// The background comparison. The permit is held until the outcome is
/// counted. Phase 1 waits for the Django bytes (bounded; a slow client
/// skips, never errors). Phase 2 dispatches Rust and compares under the
/// same budget; timeouts and panics there count `errored`.
async fn run_shadow(plan: ShadowPlan, django: DjangoCapture, parts: ShadowRequestParts) {
    let _permit = plan.permit;
    let gate = plan.gate;
    let template = plan.template;
    let method = parts.method.to_string();

    let django_body = match tokio::time::timeout(SHADOW_TIMEOUT, django.rx).await {
        Ok(Ok(CaptureOutcome::Captured(body))) => body,
        _ => {
            gate.count(&template, Outcome::Skipped);
            return;
        }
    };
    let django_response = CapturedResponse {
        status: django.status,
        content_type: django.content_type,
        body: Bytes::from(django_body),
    };
    let django_status = django_response.status;

    let app = gate.shadow_app.clone();
    let dispatch = tokio::spawn(async move {
        let rust_http = app
            .oneshot(parts.into_request())
            .await
            .unwrap_or_else(|never| match never {});
        if let Some(marker) = rust_http.extensions().get::<ShadowNoHandler>() {
            tracing::debug!(
                reason = ?marker.0,
                "shadow dispatch found no Rust handler"
            );
            return DispatchOutcome::NoHandler;
        }
        let (mut head, body) = rust_http.into_parts();
        let rust_status = head.status.as_u16();
        let rust_content_type = head
            .headers
            .remove(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok().map(str::to_owned))
            .unwrap_or_default();
        let body = match axum::body::to_bytes(body, MAX_CAPTURE_BYTES + 1).await {
            Ok(body) if body.len() <= MAX_CAPTURE_BYTES => body,
            _ => return DispatchOutcome::RustTooLarge,
        };
        let rust_response = CapturedResponse {
            status: rust_status,
            content_type: rust_content_type,
            body,
        };
        DispatchOutcome::Compared(compare(&django_response, &rust_response), rust_status)
    });

    match tokio::time::timeout(SHADOW_TIMEOUT, dispatch).await {
        Ok(Ok(DispatchOutcome::Compared(Verdict::Match, _))) => {
            gate.count(&template, Outcome::Matched);
        }
        Ok(Ok(DispatchOutcome::Compared(Verdict::Mismatch { diff }, rust_status))) => {
            tracing::warn!(
                route_template = template.as_str(),
                method = method.as_str(),
                django_status = django_status,
                rust_status = rust_status,
                diff = diff.as_str(),
                "shadow mismatch"
            );
            gate.metrics.record_mismatch(MismatchRecord {
                route: template.clone(),
                method,
                django_status,
                rust_status,
                diff,
            });
            gate.count(&template, Outcome::Mismatched);
        }
        Ok(Ok(DispatchOutcome::NoHandler | DispatchOutcome::RustTooLarge)) => {
            gate.count(&template, Outcome::Skipped);
        }
        Ok(Err(join_error)) => {
            tracing::debug!(
                route_template = template.as_str(),
                panic = join_error.is_panic(),
                "shadow dispatch task failed"
            );
            gate.count(&template, Outcome::Errored);
        }
        Err(_) => {
            gate.count(&template, Outcome::Errored);
        }
    }
}

/// What the Rust dispatch produced.
enum DispatchOutcome {
    Compared(Verdict, u16),
    NoHandler,
    RustTooLarge,
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// One side of a comparison: status, content-type, full body.
pub(crate) struct CapturedResponse {
    pub(crate) status: u16,
    pub(crate) content_type: String,
    pub(crate) body: Bytes,
}

/// The comparison result. [`Verdict::Match`] needs no payload; a mismatch
/// carries the redacted excerpt for the log line and the ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    Match,
    Mismatch { diff: String },
}

/// Compare status, content-type, and body. Byte-identical bodies match on
/// the fast path without parsing; otherwise JSON bodies (both sides, since
/// content-types already matched) are normalized over the volatile
/// allowlist and compared semantically. Anything else that differs —
/// status, content-type, non-JSON bytes, JSON beyond the allowlist — is a
/// mismatch with a redacted excerpt.
pub(crate) fn compare(django: &CapturedResponse, rust: &CapturedResponse) -> Verdict {
    if django.status != rust.status
        || django.content_type != rust.content_type
        || django.body != rust.body
    {
        if django.content_type == rust.content_type
            && django.body != rust.body
            && content_type_is_json(&django.content_type)
            && normalized_json_equal(&django.body, &rust.body)
        {
            return Verdict::Match;
        }
        return Verdict::Mismatch {
            diff: mismatch_diff(django, rust),
        };
    }
    Verdict::Match
}

fn content_type_is_json(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|mime| mime.trim().to_ascii_lowercase().contains("json"))
}

/// Parse both bodies as JSON, scrub volatiles on both sides, compare
/// semantically. Unparseable on either side is not a match (the bytes
/// already differ — that is the signal).
fn normalized_json_equal(django_body: &[u8], rust_body: &[u8]) -> bool {
    let (Ok(mut django), Ok(mut rust)) = (
        serde_json::from_slice::<serde_json::Value>(django_body),
        serde_json::from_slice::<serde_json::Value>(rust_body),
    ) else {
        return false;
    };
    scrub_volatile(&mut django);
    scrub_volatile(&mut rust);
    django == rust
}

/// Replace volatile values in place: allowlisted keys become `"***"` at any
/// depth, volatile query parameters inside URL-ish strings are redacted.
fn scrub_volatile(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if VOLATILE_JSON_KEYS.contains(&key.as_str()) {
                    *val = serde_json::Value::String("***".to_owned());
                } else {
                    scrub_volatile(val);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                scrub_volatile(item);
            }
        }
        serde_json::Value::String(text) if text.contains('?') => {
            *text = scrub_url_query_params(text);
        }
        _ => {}
    }
}

/// Redact volatile query parameters in a URL-ish string. Non-URL strings
/// pass through: only `name=value` pairs whose name exactly (ASCII
/// case-insensitively) matches the allowlist are touched.
fn scrub_url_query_params(text: &str) -> String {
    let Some(query_start) = text.find('?') else {
        return text.to_owned();
    };
    let (head, query) = text.split_at(query_start + 1);
    if !query.contains('=') {
        return text.to_owned();
    }
    let mut scrubbed = String::with_capacity(text.len());
    scrubbed.push_str(head);
    for (index, pair) in query.split('&').enumerate() {
        if index > 0 {
            scrubbed.push('&');
        }
        match pair.split_once('=') {
            Some((name, _)) if is_volatile_param(name) => {
                scrubbed.push_str(name);
                scrubbed.push_str("=[REDACTED]");
            }
            _ => scrubbed.push_str(pair),
        }
    }
    scrubbed
}

fn is_volatile_param(name: &str) -> bool {
    VOLATILE_QUERY_PARAMS
        .iter()
        .any(|param| param.eq_ignore_ascii_case(name))
}

/// The mismatch excerpt: both statuses, both content-types, and the
/// differing body region with common prefix/suffix trimmed — secrets
/// redacted, total capped at [`DIFF_CAP_BYTES`].
///
/// Redaction runs on the full bodies BEFORE trimming: trimming first would
/// split secrets across the cut and leak fragments (a common `"token":
/// "sk-…` prefix with differing tails leaves a live credential fragment in
/// the excerpt). Volatiles are normalized too, mirroring [`compare`], so
/// the excerpt explains the mismatch instead of showing timestamp noise.
/// Every emitted byte has passed through [`redact_secrets`].
fn mismatch_diff(django: &CapturedResponse, rust: &CapturedResponse) -> String {
    let mut diff = format!(
        "status django={} rust={}; content-type django={:?} rust={:?}; ",
        django.status, rust.status, django.content_type, rust.content_type
    );
    if django.body == rust.body {
        // Status or content-type carried the mismatch; the bodies match.
        diff.push_str("body identical");
        return truncate_to_bytes(diff, DIFF_CAP_BYTES);
    }
    let budget = DIFF_CAP_BYTES.saturating_sub(diff.len()).max(64) / 2;
    let django_text = diff_text(&django.body, &django.content_type);
    let rust_text = diff_text(&rust.body, &rust.content_type);
    let (prefix, suffix) = common_affixes(django_text.as_bytes(), rust_text.as_bytes());
    let django_middle = &django_text.as_bytes()[prefix..django_text.len() - suffix];
    let rust_middle = &rust_text.as_bytes()[prefix..rust_text.len() - suffix];
    if django_middle.is_empty() && rust_middle.is_empty() {
        // Normalized-equal but raw-different: the bodies differ only inside
        // secret spans or volatile values. Say so instead of showing empty
        // excerpts.
        diff.push_str("body differs only in redacted or volatile values");
    } else {
        let django_excerpt = truncate_lossy(django_middle, budget);
        let rust_excerpt = truncate_lossy(rust_middle, budget);
        diff.push_str(&format!(
            "body django[{prefix}..{}]={django_excerpt:?}; rust[{prefix}..{}]={rust_excerpt:?}",
            django_text.len() - suffix,
            rust_text.len() - suffix,
        ));
    }
    truncate_to_bytes(diff, DIFF_CAP_BYTES)
}

/// Diff-ready body text: JSON bodies are normalized over the volatile
/// allowlist and re-serialized, volatile query parameters are scrubbed
/// everywhere (they also appear outside JSON), then secrets are redacted.
fn diff_text(body: &[u8], content_type: &str) -> String {
    let mut text = String::from_utf8_lossy(body).into_owned();
    if content_type_is_json(content_type) {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) {
            scrub_volatile(&mut value);
            if let Ok(normalized) = serde_json::to_string(&value) {
                text = normalized;
            }
        }
    }
    if text.contains('?') {
        text = scrub_url_query_params(&text);
    }
    redact_secrets(&text)
}

/// Lengths of the common byte prefix and suffix (non-overlapping).
fn common_affixes(a: &[u8], b: &[u8]) -> (usize, usize) {
    let prefix = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
    let max_suffix = (a.len() - prefix).min(b.len() - prefix);
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take(max_suffix)
        .take_while(|(x, y)| x == y)
        .count();
    (prefix, suffix)
}

/// Lossy text of at most `max_chars` characters, with a truncation marker
/// that keeps the "never the full body" promise honest for large middles.
fn truncate_lossy(bytes: &[u8], max_chars: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.chars().count() <= max_chars {
        return text.into_owned();
    }
    let kept: String = text.chars().take(max_chars).collect();
    format!(
        "{kept}…[truncated {} chars]",
        text.chars().count() - max_chars
    )
}

fn truncate_to_bytes(text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[truncated]", &text[..end])
}

// ---------------------------------------------------------------------------
// Secret redaction
// ---------------------------------------------------------------------------

/// Redact secret values in diff text. A [`SENSITIVE_KEYS`] name redacts the
/// value that follows it when the name is followed (after optional
/// whitespace/quotes) by `:` or `=`; quoted values keep their quotes.
/// `Bearer <token>` is redacted as a unit. Matching is ASCII
/// case-insensitive; prose that merely mentions a key is left alone.
pub fn redact_secrets(text: &str) -> String {
    // Bearer first: the `authorization` key pass would otherwise redact the
    // scheme word and leave the token behind.
    let mut out = redact_bearer(text);
    for key in SENSITIVE_KEYS {
        out = redact_key(&out, key);
    }
    out
}

fn redact_key(text: &str, key: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(found) = lower[cursor..].find(key) {
        let key_start = cursor + found;
        let after_key = key_start + key.len();
        match value_span(text, after_key) {
            Some((value_start, value_end, quoted)) => {
                out.push_str(&text[cursor..value_start]);
                out.push_str("[REDACTED]");
                if quoted {
                    // Keep the closing quote; the opening quote is already out.
                    out.push_str(&text[value_end..value_end + 1]);
                    cursor = value_end + 1;
                } else {
                    cursor = value_end;
                }
            }
            None => {
                // Not a key context (prose, or no value): keep the key text
                // and continue after it.
                out.push_str(&text[cursor..after_key]);
                cursor = after_key;
            }
        }
    }
    out.push_str(&text[cursor..]);
    out
}

/// The value span after a sensitive key: `(start, end, quoted)`. `None`
/// when the key is not followed by `:` or `=` (prose, not a pair).
fn value_span(text: &str, after_key: usize) -> Option<(usize, usize, bool)> {
    let bytes = text.as_bytes();
    let mut index = after_key;
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t' | b'"' | b'\'') {
        index += 1;
    }
    if index < bytes.len() && matches!(bytes[index], b':' | b'=') {
        index += 1;
    } else {
        return None;
    }
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }
    if index < bytes.len() && matches!(bytes[index], b'"' | b'\'') {
        let quote = bytes[index];
        let value_start = index + 1;
        let mut end = value_start;
        while end < bytes.len() && bytes[end] != quote {
            end += 1;
        }
        return Some((value_start, end.min(bytes.len()), true));
    }
    let value_start = index;
    let mut end = index;
    while end < bytes.len()
        && !matches!(
            bytes[end],
            b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b',' | b';' | b'&' | b'}' | b')'
        )
    {
        end += 1;
    }
    if end == value_start {
        return None;
    }
    Some((value_start, end, false))
}

/// `Bearer <token>` (any capitalization of the scheme) redacts the token.
fn redact_bearer(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(found) = lower[cursor..].find("bearer ") {
        let token_start = cursor + found + "bearer ".len();
        let bytes = text.as_bytes();
        let mut end = token_start;
        while end < bytes.len()
            && !matches!(
                bytes[end],
                b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b',' | b';' | b'}'
            )
        {
            end += 1;
        }
        out.push_str(&text[cursor..token_start]);
        out.push_str("[REDACTED]");
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

// ---------------------------------------------------------------------------
// Metrics endpoint
// ---------------------------------------------------------------------------

/// `GET /internal/shadow-metrics`: the gate's counters plus the recent
/// mismatch ring. Served even when shadow is disabled (all zeros,
/// `enabled: false`) so deployments can verify the wiring.
pub async fn metrics(State(state): State<AppState>) -> Response {
    #[derive(Serialize)]
    struct Body {
        enabled: bool,
        totals: RouteCounters,
        routes: HashMap<String, RouteCounters>,
        recent_mismatches: Vec<MismatchRecord>,
    }
    let body = match state.shadow_gate() {
        Some(gate) => {
            let snapshot = gate.metrics.snapshot();
            Body {
                enabled: gate.config.enabled,
                totals: snapshot.totals,
                routes: snapshot.routes,
                recent_mismatches: snapshot.recent_mismatches,
            }
        }
        None => Body {
            enabled: false,
            totals: RouteCounters::default(),
            routes: HashMap::new(),
            recent_mismatches: Vec::new(),
        },
    };
    Json(body).into_response()
}

// ---------------------------------------------------------------------------
// Shadow router
// ---------------------------------------------------------------------------

/// A session store that reads through to the wrapped store and drops every
/// save. Shadow dispatch authenticates exactly like production (same rows,
/// same expiry checks) but can never write the `sessions` table — a
/// handler that mutates the session still computes its response, the
/// mutation just never persists.
#[derive(Debug, Clone)]
pub struct ReadOnlySessionStore<S> {
    inner: S,
}

impl<S> ReadOnlySessionStore<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: SessionStore> SessionStore for ReadOnlySessionStore<S> {
    fn load(
        &self,
        key: String,
    ) -> Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Option<crate::middleware::StoredSession>,
                        crate::middleware::StoreError,
                    >,
                > + Send,
        >,
    > {
        self.inner.load(key)
    }

    fn save(
        &self,
        row: crate::middleware::SessionRow,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<String, crate::middleware::StoreError>> + Send>,
    > {
        // Dropped on purpose: shadow must not write sessions. The key is
        // echoed back so the layer's post-processing still runs.
        let key = row.key.clone().unwrap_or_default();
        Box::pin(async move { Ok(key) })
    }

    fn user_exists(
        &self,
        user_id: String,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<bool, crate::middleware::StoreError>> + Send>,
    > {
        self.inner.user_exists(user_id)
    }
}

/// Assemble the shadow app: the denylist markers outside, the additive
/// routes in the middle, the groups inside with a no-handler fallback —
/// the same fallback-dispatch shape as the real router, so same-path
/// shadowing behaves identically. The read-only session layer wraps the
/// whole app (mirroring production, where middleware sits outside
/// routing), and everything runs under the shadow state.
pub(crate) fn build_shadow_app(
    shadow_state: &AppState,
    groups: Router<AppState>,
    extras: Vec<Router<AppState>>,
) -> Router {
    let inner = groups
        .fallback(no_handler_fallback)
        .with_state(shadow_state.clone());
    let mut middle: Router<AppState> = Router::new();
    for extra in extras {
        middle = middle.merge(extra);
    }
    let middle = middle
        .fallback_service(inner)
        .with_state(shadow_state.clone());
    let outer = denylist_router().fallback_service(middle);
    apply_session_layer(outer, shadow_state).with_state(shadow_state.clone())
}

fn apply_session_layer(router: Router<AppState>, state: &AppState) -> Router<AppState> {
    match state.pools() {
        Some(pools) => {
            let store = ReadOnlySessionStore::new(PgSessionStore::new(pools.primary().clone()));
            router.layer(SessionLayer::new(SessionConfig::from_settings(
                state.settings(),
                Some(store),
            )))
        }
        None => router.layer(SessionLayer::new(SessionConfig::from_settings(
            state.settings(),
            None::<PgSessionStore>,
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(status: u16, content_type: &str, body: &[u8]) -> CapturedResponse {
        CapturedResponse {
            status,
            content_type: content_type.to_owned(),
            body: Bytes::copy_from_slice(body),
        }
    }

    #[test]
    fn config_from_env_defaults_off_parses_and_clamps() {
        // One test: the cases share process env, and parallel tests racing
        // on set_var/remove_var would flake.
        for var in [SHADOW_ENV, SHADOW_MAX_INFLIGHT_ENV, SHADOW_SAMPLE_ENV] {
            std::env::remove_var(var);
        }
        assert_eq!(ShadowConfig::from_env(), ShadowConfig::default());
        assert!(!ShadowConfig::default().enabled);
        assert_eq!(ShadowConfig::default().max_inflight, MAX_INFLIGHT_DEFAULT);
        assert_eq!(ShadowConfig::default().sample_rate, SAMPLE_DEFAULT);

        std::env::set_var(SHADOW_ENV, "1");
        std::env::set_var(SHADOW_MAX_INFLIGHT_ENV, "3");
        std::env::set_var(SHADOW_SAMPLE_ENV, "0.5");
        let config = ShadowConfig::from_env();
        assert!(config.enabled);
        assert_eq!(config.max_inflight, 3);
        assert_eq!(config.sample_rate, 0.5);

        std::env::set_var(SHADOW_ENV, "banana");
        std::env::set_var(SHADOW_MAX_INFLIGHT_ENV, "banana");
        std::env::set_var(SHADOW_SAMPLE_ENV, "banana");
        let config = ShadowConfig::from_env();
        assert!(!config.enabled);
        assert_eq!(config.max_inflight, MAX_INFLIGHT_DEFAULT);
        assert_eq!(config.sample_rate, SAMPLE_DEFAULT);

        std::env::set_var(SHADOW_SAMPLE_ENV, "9.5");
        assert_eq!(ShadowConfig::from_env().sample_rate, 1.0);
        std::env::set_var(SHADOW_SAMPLE_ENV, "-2.0");
        assert_eq!(ShadowConfig::from_env().sample_rate, 0.0);

        for var in [SHADOW_ENV, SHADOW_MAX_INFLIGHT_ENV, SHADOW_SAMPLE_ENV] {
            std::env::remove_var(var);
        }
    }

    #[test]
    fn sampler_boundaries() {
        assert!(should_sample(1.0, 0.999));
        assert!(!should_sample(1.0, 1.0));
        assert!(!should_sample(0.0, 0.0));
        assert!(should_sample(0.5, 0.499));
        assert!(!should_sample(0.5, 0.5));
    }

    #[test]
    fn templates_collapse_ids() {
        assert_eq!(route_template("/"), "/");
        assert_eq!(route_template("/robots.txt"), "/robots.txt");
        assert_eq!(
            route_template("/api/workspaces/acme/projects/123/pages/"),
            "/api/workspaces/acme/projects/{id}/pages/"
        );
        assert_eq!(
            route_template("/api/x/550e8400-e29b-41d4-a716-446655440000/"),
            "/api/x/{id}/"
        );
        assert_eq!(
            route_template("/api/x/550e8400e29b41d4a716446655440000/"),
            "/api/x/{id}/"
        );
        // Slugs survive: the map cap plus _other bounds them instead.
        assert_eq!(
            route_template("/api/workspaces/acme/"),
            "/api/workspaces/acme/"
        );
    }

    #[test]
    fn denylist_matches_exact_and_params() {
        assert!(denylist_reason("/auth/get-csrf-token/").is_some());
        assert!(denylist_reason("/auth/github/").is_some());
        assert!(denylist_reason("/auth/spaces/gitlab/callback/").is_some());
        assert!(denylist_reason("/api/v1/workspaces/acme/assets/9/").is_some());
        assert!(denylist_reason("/api/public/assets/v2/anchor/x/1/").is_some());
        assert!(denylist_reason("/").is_none());
        assert!(denylist_reason("/api/v1/workspaces/acme/assets/").is_none());
        assert!(denylist_reason("/auth/github/callback/extra/").is_none());
    }

    #[test]
    fn identical_responses_match_without_parsing() {
        let django = captured(200, "application/json", b"{\"a\": 1}");
        let rust = captured(200, "application/json", b"{\"a\": 1}");
        assert_eq!(compare(&django, &rust), Verdict::Match);
    }

    #[test]
    fn status_and_content_type_mismatches_carry_both() {
        let django = captured(200, "application/json", b"{}");
        let rust = captured(500, "application/json", b"{}");
        let Verdict::Mismatch { diff } = compare(&django, &rust) else {
            panic!("status drift must mismatch");
        };
        assert!(diff.contains("django=200"), "{diff}");
        assert!(diff.contains("rust=500"), "{diff}");

        let rust = captured(200, "text/html", b"{}");
        let Verdict::Mismatch { diff } = compare(&django, &rust) else {
            panic!("content-type drift must mismatch");
        };
        assert!(diff.contains("application/json"), "{diff}");
        assert!(diff.contains("text/html"), "{diff}");
    }

    #[test]
    fn volatile_keys_and_signatures_match() {
        let django = captured(
            200,
            "application/json",
            br#"{"request_id": "aaa", "rows": [{"url": "https://f.test/d?X-Amz-Date=t1&X-Amz-Signature=s1&key=k"}]}"#,
        );
        let rust = captured(
            200,
            "application/json",
            br#"{"request_id": "bbb", "rows": [{"url": "https://f.test/d?X-Amz-Date=t2&X-Amz-Signature=s2&key=k"}]}"#,
        );
        assert_eq!(compare(&django, &rust), Verdict::Match);
    }

    #[test]
    fn non_volatile_json_drift_mismatches() {
        let django = captured(200, "application/json", b"{\"a\": 1}");
        let rust = captured(200, "application/json", b"{\"a\": 2}");
        assert!(matches!(compare(&django, &rust), Verdict::Mismatch { .. }));
    }

    #[test]
    fn non_json_bodies_compare_by_bytes() {
        let django = captured(200, "text/plain", b"hello");
        let rust = captured(200, "text/plain", b"hello!");
        assert!(matches!(compare(&django, &rust), Verdict::Mismatch { .. }));
    }

    #[test]
    fn unparseable_json_claims_mismatch() {
        let django = captured(200, "application/json", b"{\"a\": 1}");
        let rust = captured(200, "application/json", b"not json");
        assert!(matches!(compare(&django, &rust), Verdict::Mismatch { .. }));
    }

    #[test]
    fn diff_is_capped_and_trims_common_regions() {
        let mut django_body = vec![b'x'; 4000];
        django_body.extend_from_slice(b"ALPHA1");
        django_body.extend(vec![b'y'; 4000]);
        let mut rust_body = vec![b'x'; 4000];
        rust_body.extend_from_slice(b"OMEGA2");
        rust_body.extend(vec![b'y'; 4000]);
        let django = captured(200, "text/plain", &django_body);
        let rust = captured(200, "text/plain", &rust_body);
        let Verdict::Mismatch { diff } = compare(&django, &rust) else {
            panic!("must mismatch");
        };
        assert!(diff.len() <= DIFF_CAP_BYTES, "len {}", diff.len());
        assert!(diff.contains("ALPHA1"), "{diff}");
        assert!(diff.contains("OMEGA2"), "{diff}");
        // Common regions are trimmed, not dumped.
        assert!(!diff.contains(&"x".repeat(100)), "{diff}");
    }

    #[test]
    fn redaction_covers_pairs_bearer_and_prose() {
        let dirty = r#"{"token": "sk-live-1", "password": "hunter2", "note": "x"}
            Authorization: Bearer sk-live-2
            Cookie: sessionid=abc123; theme=dark
            ?next=/&signature=topsecret"#;
        let clean = redact_secrets(dirty);
        for secret in ["sk-live-1", "hunter2", "sk-live-2", "abc123", "topsecret"] {
            assert!(!clean.contains(secret), "{clean}");
        }
        assert!(clean.contains("[REDACTED]"), "{clean}");
        // Non-secret values survive.
        assert!(
            clean.contains("theme=dark") || clean.contains("theme"),
            "{clean}"
        );

        let prose = "the secret is out and the password policy changed";
        assert_eq!(redact_secrets(prose), prose);
        assert_eq!(
            redact_secrets(r#"{"token": "v"}"#),
            r#"{"token": "[REDACTED]"}"#
        );
    }

    #[test]
    fn counters_balance_and_overflow() {
        let metrics = ShadowMetrics::default();
        metrics.count("/a", Outcome::Matched);
        metrics.count("/a", Outcome::Mismatched);
        metrics.count("/a", Outcome::Skipped);
        metrics.count("/a", Outcome::Errored);
        let snapshot = metrics.snapshot();
        assert_eq!(
            snapshot.totals,
            RouteCounters {
                compared: 2,
                matched: 1,
                mismatched: 1,
                skipped: 1,
                errored: 1,
            }
        );
        for index in 0..(MAX_TEMPLATES + 10) {
            metrics.count(&format!("/t{index}"), Outcome::Skipped);
        }
        let snapshot = metrics.snapshot();
        assert!(snapshot.routes.len() <= MAX_TEMPLATES + 1);
        assert!(snapshot.routes.contains_key(OVERFLOW_TEMPLATE));
    }

    #[test]
    fn diff_redacts_before_trimming_affixes() {
        // A common `"token": "sk-…` prefix with differing tails: trimming
        // first would leak the tails as fragments.
        let django = captured(200, "application/json", br#"{"token": "sk-only-AAA"}"#);
        let rust = captured(200, "application/json", br#"{"token": "sk-only-BBB"}"#);
        let Verdict::Mismatch { diff } = compare(&django, &rust) else {
            panic!("tokens differ, must mismatch");
        };
        for leaked in ["sk-only-AAA", "sk-only-BBB", "only-AAA", "only-BBB"] {
            assert!(!diff.contains(leaked), "{diff}");
        }
        assert!(
            diff.contains("only in redacted or volatile values"),
            "{diff}"
        );
    }

    #[test]
    fn mismatch_ring_is_capped() {
        let metrics = ShadowMetrics::default();
        for index in 0..(RECENT_CAP + 5) {
            metrics.record_mismatch(MismatchRecord {
                route: format!("/r{index}"),
                method: "GET".to_owned(),
                django_status: 200,
                rust_status: 200,
                diff: "d".to_owned(),
            });
        }
        assert_eq!(metrics.snapshot().recent_mismatches.len(), RECENT_CAP);
    }
}
