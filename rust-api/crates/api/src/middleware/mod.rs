#![forbid(unsafe_code)]

//! Tower equivalents for Django's `MIDDLEWARE` list.
//!
//! Python reference: `MIDDLEWARE` in `apps/api/pi_dash/settings/common.py`.
//! Only the six mechanisms this issue names are ported here:
//!
//! | Module | Django middleware |
//! |---|---|
//! | [`cors`] | `corsheaders.middleware.CorsMiddleware` (django-cors-headers 4.3.1) |
//! | [`security`] | `django.middleware.security.SecurityMiddleware` + `django.middleware.clickjacking.XFrameOptionsMiddleware` |
//! | [`gzip`] | `django.middleware.gzip.GZipMiddleware` |
//! | [`body_limit`] | `pi_dash.middleware.request_body_size.RequestBodySizeLimitMiddleware` + `DATA_UPLOAD_MAX_MEMORY_SIZE` |
//! | [`logging`] | `pi_dash.middleware.logger.RequestLoggerMiddleware` + `APITokenLogMiddleware` |
//! | [`session`] | `pi_dash.authentication.middleware.session.SessionMiddleware` (response half; the pure request half lives in [`pidash_auth::session`]) |
//!
//! Deliberately not ported here: `WhiteNoiseMiddleware` (static files stay
//! on the Django proxy), `CommonMiddleware`'s append-slash redirect (needs
//! the route table; a job for the F-10 extension seams), `CsrfViewMiddleware`
//! enforcement and `AuthenticationMiddleware`/`crum` (F-05/F-06 territory),
//! and the conditional `ReadReplicaRoutingMiddleware` (F-04's
//! `RequestContext::use_read_replica` already mirrors its safe default).
//!
//! Layer order mirrors `MIDDLEWARE` from the outside in: [`cors`] is the
//! outermost wrapper, [`logging::RequestLoggerLayer`] the innermost. Apply
//! with [`stack`], which layers them innermost-first so CORS ends up
//! outermost.

pub mod body_limit;
pub mod cors;
pub mod gzip;
pub mod logging;
pub mod security;
pub mod session;

use axum::Router;

pub use body_limit::{BodyLimitLayer, BODY_TOO_LARGE_JSON, DEFAULT_BODY_LIMIT_BYTES};
pub use cors::{CorsConfig, CorsLayer, DEFAULT_HEADERS, DEFAULT_METHODS};
pub use gzip::{GzipLayer, MIN_COMPRESS_BYTES};
pub use logging::{
    ApiTokenLogRecord, LogSink, LoggerUserId, LoggingConfig, RequestLogRecord, RequestLoggerLayer,
    TokenLogLayer, TracingSink, REQUEST_LOGGER_TARGET,
};
pub use security::{SecurityConfig, SecurityLayer};
pub use session::{
    MemorySessionStore, PgSessionStore, RequestSession, SessionConfig, SessionExpiry,
    SessionHandle, SessionLayer, SessionRow, SessionStore, StoreError, StoredSession,
};

/// Fail-closed 502 for truncated body reads (upstream died mid-stream).
/// Same shape as the edge proxy's unreachable-upstream answer: Django
/// would raise out of `response.content`, never serve a half body.
pub(crate) fn truncated_body() -> axum::response::Response {
    crate::edge::bad_gateway()
}

/// Merge one field into a `Vary` header without duplicating it.
///
/// Mirrors `django.utils.cache.patch_vary_headers`: existing values are
/// kept, the new field is appended only when absent (case-insensitive), and
/// a pre-existing `*` is left alone.
pub(crate) fn append_vary(headers: &mut http::HeaderMap, field: &'static str) {
    let existing: Vec<String> = headers
        .get_all(http::header::VARY)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect();
    if existing
        .iter()
        .any(|v| v == "*" || v.eq_ignore_ascii_case(field))
    {
        return;
    }
    let mut merged = existing.join(", ");
    if !merged.is_empty() {
        merged.push_str(", ");
    }
    merged.push_str(field);
    if let Ok(value) = http::HeaderValue::from_str(&merged) {
        headers.insert(http::header::VARY, value);
    }
}

/// Wrap every route — including the Django fallback proxy — in the
/// middleware stack, innermost layer first so [`CorsLayer`] ends up
/// outermost, exactly like `MIDDLEWARE` order. One sink serves both logging
/// layers; the binary passes [`TracingSink`]. Takes the finalized router
/// (after `with_state`), so `build_app` is the single wiring point.
pub fn stack<S, K>(
    router: Router,
    cors: &CorsConfig,
    security: &SecurityConfig,
    body_limit_bytes: u64,
    logging: &LoggingConfig,
    session: SessionConfig<S>,
    sink: K,
) -> Router
where
    S: SessionStore,
    K: LogSink,
{
    router
        .layer(RequestLoggerLayer::new(logging.clone(), sink.clone()))
        .layer(TokenLogLayer::new(logging.clone(), sink))
        .layer(BodyLimitLayer::new(body_limit_bytes))
        .layer(GzipLayer::new())
        .layer(SessionLayer::new(session))
        .layer(SecurityLayer::new(security.clone()))
        .layer(CorsLayer::new(cors.clone()))
}
