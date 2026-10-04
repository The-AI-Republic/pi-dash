//! api-v1 OpenAPI schema surface (D-23, stage 5).
//!
//! Ports the served `/api/schema/*` trio (`pi_dash/urls.py:32-44`, live only
//! when `ENABLE_DRF_SPECTACULAR=1`):
//!
//! * [`schema`] — the document handler: YAML by default, JSON via
//!   `?format=json`, 404 on unknown formats (PIDASHCONV-535).
//! * [`ui`] — the swagger-ui and redoc page handlers (PIDASHCONV-535).
//! * [`throttle`] — the global `AnonRateThrottle` (30/minute) decision plus
//!   the 429 denial rendering per renderer (PIDASHCONV-531).
//!
//! Cutover mapping (the `ENABLE_DRF_SPECTACULAR` decision, recorded here
//! per the issue): no Django-settings flag is read. Rust serves the trio
//! iff any tied `api/` prefix row is flipped ([`rust_serves`] — the
//! shared-prefix rule from `edge.rs`: all four of App/Assistant/Loop/
//! Prompting must be off for the prefix to stay on Django). Unflipped
//! requests proxy to Django, which serves (flag on) or 404s (flag off),
//! so Rust never invents the flag-off verdict. Operator flip:
//! `PIDASH_RUST_APP=1` (the trio is registered on the App group).
//!
//! Throttle store: the DRF loop runs over a process-local history map
//! ([`check_throttle`]), mirroring the assistant [`governor`](crate::assistant::governor)
//! precedent — Django's cache values are pickled Python lists, so the
//! shared Redis cache cannot be reused. Single-process deployments (and
//! the proxy contract gate) observe exactly Django-with-local-memory-cache
//! behaviour.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod schema;
pub mod throttle;
pub mod ui;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, Request};
use axum::handler::Handler;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, MethodRouter};
use axum::Router;

use crate::edge;
use crate::state::AppState;

use throttle::{DenialRenderer, SchemaThrottled};

/// `Allow` on every response from these views (DRF
/// `default_response_headers`, `views.py:154-162`; verified against the
/// pinned 3.15.2 source).
pub const ALLOW: &str = "GET, HEAD, OPTIONS";

/// Served paths (`pi_dash/urls.py:32-44`).
pub const SCHEMA_PATH: &str = "/api/schema/";
/// Served paths (`pi_dash/urls.py:32-44`).
pub const SWAGGER_UI_PATH: &str = "/api/schema/swagger-ui/";
/// Served paths (`pi_dash/urls.py:32-44`).
pub const REDOC_PATH: &str = "/api/schema/redoc/";
/// Slashless doc URL: `CommonMiddleware` APPEND_SLASH redirects it to
/// [`SCHEMA_PATH`] (FX-OPENAPI-05 `slashless_redirect`).
pub const SCHEMA_SLASHLESS_PATH: &str = "/api/schema";

/// Register the trio plus the slashless redirect. Merged into the App
/// group in `overlay.rs` (the first tied `api/` row).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            SCHEMA_PATH,
            owned(get(schema::serve), schema::method_not_allowed),
        )
        .route(
            SWAGGER_UI_PATH,
            owned(get(ui::serve_swagger_ui), ui::method_not_allowed),
        )
        .route(
            REDOC_PATH,
            owned(get(ui::serve_redoc), ui::method_not_allowed),
        )
        .route(SCHEMA_SLASHLESS_PATH, any(schema::slashless_redirect))
}

/// An owned schema path: GET serves from Rust (gated on the cutover flip,
/// throttled); every unsafe method answers the endpoint's pinned 405
/// locally — never proxies, because Django's own verdict on these
/// `AllowAny` read-only views is that same 405; OPTIONS proxies so DRF's
/// metadata response is preserved (pilot precedent); HEAD rides axum's
/// `get` handling like Django's `GET`-backed `HEAD`.
pub fn owned<H, T>(get_handler: MethodRouter<AppState>, deny: H) -> MethodRouter<AppState>
where
    H: Handler<T, AppState>,
    T: 'static,
{
    get_handler
        .post(deny.clone())
        .put(deny.clone())
        .patch(deny.clone())
        .delete(deny)
        .options(edge::proxy)
}

/// Cutover gate: Rust serves the trio iff any tied `api/` row is flipped
/// (see the module docs for the `ENABLE_DRF_SPECTACULAR` mapping).
pub(crate) fn rust_serves(state: &AppState) -> bool {
    state
        .edge()
        .flags()
        .any_rust(&edge::match_prefixes(SCHEMA_PATH))
}

/// Outcome of one throttle check: allowed requests record, denials carry
/// the DRF `wait()` driving `Retry-After`.
pub(crate) struct ThrottleVerdict {
    pub allowed: bool,
    pub wait: Option<f64>,
}

/// The handler-shared throttle store: one timestamp history (newest first,
/// like DRF's cached list) per `throttle_anon_<ident>` key.
fn store() -> &'static Mutex<HashMap<String, Vec<f64>>> {
    static STORE: OnceLock<Mutex<HashMap<String, Vec<f64>>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Run the DRF loop for one anonymous ident at `now` (epoch seconds, like
/// DRF's `timer()`): trim entries `<= now - duration`, allow iff fewer
/// than quota remain, recording `now` on success. Denials do not record.
pub(crate) fn check_throttle(ident: &str, now: f64) -> ThrottleVerdict {
    let spec = throttle::ANON_THROTTLE;
    let key = throttle::cache_key(ident);
    let mut histories = store().lock().expect("throttle store lock");
    let history = histories.entry(key).or_default();
    let horizon = now - spec.window_secs as f64;
    history.retain(|&at| at > horizon);
    if throttle::allow_request(history, now, spec.requests, spec.window_secs) {
        history.insert(0, now);
        ThrottleVerdict {
            allowed: true,
            wait: None,
        }
    } else {
        let wait = throttle::throttle_wait(history, now, spec.requests, spec.window_secs);
        ThrottleVerdict {
            allowed: false,
            wait,
        }
    }
}

/// Now as epoch seconds for [`check_throttle`].
pub(crate) fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

/// DRF ident for one request: `X-Forwarded-For` (whitespace-stripped) when
/// present, else the TCP peer. Unit-test requests carry no `ConnectInfo`,
/// so loopback stands in for the peer there.
pub(crate) fn peer_ident(req: &Request) -> String {
    let forwarded = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok());
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string());
    let remote = peer.as_deref().unwrap_or("127.0.0.1");
    throttle::get_ident(forwarded, remote)
}

/// Mirror of DRF `initial()`'s `check_throttles` for these views
/// (`SessionAuthentication` only, `AnonRateThrottle`): session-authenticated
/// callers are exempt, everyone else — API keys do not identify here —
/// spends the anonymous budget. Returns the denial when throttled.
///
/// Takes owned request data, never `&Request`: `axum::body::Body` is
/// `!Sync`, so a borrowed request cannot be held across the session
/// lookup's `.await`.
pub(crate) async fn throttle_verdict(
    state: &AppState,
    ident: String,
    session: Option<crate::middleware::SessionHandle>,
) -> Option<ThrottleVerdict> {
    if is_session_authenticated(state, session).await {
        return None;
    }
    let verdict = check_throttle(&ident, now_secs());
    if verdict.allowed {
        None
    } else {
        Some(verdict)
    }
}

/// Snapshot the request's session handle, if the session layer attached
/// one. Synchronous: callers extract this (and [`peer_ident`]) before any
/// `.await` so no `&Request` borrow crosses an await (`Body: !Sync`).
pub(crate) fn session_handle(req: &Request) -> Option<crate::middleware::SessionHandle> {
    req.extensions()
        .get::<crate::middleware::SessionHandle>()
        .cloned()
}

/// `request.user.is_authenticated` for `SessionAuthentication`
/// (`django.contrib.auth.get_user` + active + session-hash check, the
/// license-domain `resolve_actor` decision without the `Actor` build — a
/// corrupt user timezone must not break this static surface, and Django's
/// auth check never parses it). No session, or no session-auth keys,
/// short-circuits without touching the database; without pools (unit-test
/// states) or on a database error the caller counts as anonymous, so the
/// public document stays servable behind the anonymous throttle.
pub(crate) async fn is_session_authenticated(
    state: &AppState,
    handle: Option<crate::middleware::SessionHandle>,
) -> bool {
    let Some(handle) = handle else {
        return false;
    };
    let mut session = handle.snapshot();
    if session.is_empty() {
        return false;
    }
    let user_id_raw = session
        .get("_auth_user_id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();
    let backend = session
        .get("_auth_user_backend")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();
    let session_hash = session
        .get("_auth_user_hash")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_owned();
    if backend != crate::license::MODEL_BACKEND {
        return false;
    }
    let user_id: uuid::Uuid = match user_id_raw.parse() {
        Ok(id) => id,
        Err(_) => return false,
    };
    let Some(pools) = state.pools() else {
        return false;
    };
    let row: Option<(String, bool)> =
        sqlx::query_as("SELECT password, is_active FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pools.primary())
            .await
            .ok()
            .flatten();
    let Some((password, is_active)) = row else {
        return false;
    };
    if !is_active {
        return false;
    }
    if session_hash.is_empty() {
        return false;
    }
    verify_session_hash(
        &session_hash,
        &password,
        state.settings().secret_key.as_bytes(),
    )
}

/// `user.get_session_auth_hash()`: `salted_hmac(...get_session_auth_hash,
/// password).hexdigest()`, compared constant-time. Same construction as
/// the license-domain verifier (whose copy is private to its module;
/// per-domain duplication of this exact function is the established
/// pattern — see the `auth_oauth`/`auth_session` copies).
fn verify_session_hash(session_hash: &str, password_field: &str, secret_key: &[u8]) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    let key = Sha256::digest(
        [
            crate::license::SESSION_AUTH_HASH_SALT.as_bytes(),
            secret_key,
        ]
        .concat(),
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC accepts any key length");
    mac.update(password_field.as_bytes());
    let expected = hex_encode(&mac.finalize().into_bytes());
    constant_time_eq(expected.as_bytes(), session_hash.as_bytes())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Stamp the DRF view shell on a response: `Allow` on every response, plus
/// `Vary: Accept` on the doc routes (two renderer classes).
pub(crate) fn view_shell(mut response: Response, vary_accept: bool) -> Response {
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static(ALLOW));
    if vary_accept {
        response
            .headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Accept"));
    }
    response
}

/// Build one response with content type and the view shell.
pub(crate) fn view_response(
    status: StatusCode,
    content_type: &'static str,
    body: Vec<u8>,
    vary_accept: bool,
) -> Response {
    let response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body))
        .expect("static view response");
    view_shell(response, vary_accept)
}

/// Answer a throttled request: the renderer-appropriate 429 denial plus
/// the view shell (`Allow`, and `Vary` on the doc routes).
pub(crate) fn throttled(
    renderer: DenialRenderer,
    wait: Option<f64>,
    vary_accept: bool,
) -> Response {
    let denied = SchemaThrottled::new(renderer, wait).into_response();
    view_shell(denied, vary_accept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    /// Every test burns its own throttle bucket: the store is process-wide
    /// and tests run in parallel, so each test below uses a unique ident.
    fn app(flipped: bool) -> Router {
        let state = AppState::with_edge("test", edge::EdgeHandle::for_tests("http://127.0.0.1:1"));
        if flipped {
            state.edge().set_flag(edge::Prefix::App, true);
        }
        routes().with_state(state)
    }

    fn get(path: &str, ident: &str) -> Request {
        Request::get(path)
            .header("x-forwarded-for", ident)
            .body(axum::body::Body::empty())
            .expect("request")
    }

    /// The schema trio matches the tied `api/` rows, and serving flips with
    /// any one of them — the shared-prefix rule (`edge.rs`).
    #[test]
    fn flag_gate_follows_any_tied_api_row() {
        assert_eq!(
            edge::match_prefixes(SCHEMA_PATH),
            vec![
                edge::Prefix::App,
                edge::Prefix::Assistant,
                edge::Prefix::Loop,
                edge::Prefix::Prompting,
            ]
        );
        let state = AppState::with_edge("test", edge::EdgeHandle::for_tests("http://127.0.0.1:1"));
        assert!(!rust_serves(&state));
        for prefix in [
            edge::Prefix::App,
            edge::Prefix::Assistant,
            edge::Prefix::Loop,
            edge::Prefix::Prompting,
        ] {
            state.edge().set_flag(prefix, true);
            assert!(rust_serves(&state), "{prefix:?} flips schema traffic");
            state.edge().set_flag(prefix, false);
        }
        for prefix in [edge::Prefix::Web, edge::Prefix::ApiV1, edge::Prefix::Auth] {
            state.edge().set_flag(prefix, true);
            assert!(
                !rust_serves(&state),
                "{prefix:?} must not flip schema traffic"
            );
            state.edge().set_flag(prefix, false);
        }
    }

    /// Unflipped, every schema path proxies to Django (502 against the dead
    /// test upstream proves the proxy wiring — Django serves or 404s for
    /// real through it, per its own `ENABLE_DRF_SPECTACULAR` flag).
    #[tokio::test]
    async fn unflipped_requests_proxy() {
        for path in [
            SCHEMA_PATH,
            SWAGGER_UI_PATH,
            REDOC_PATH,
            SCHEMA_SLASHLESS_PATH,
        ] {
            let response = app(false)
                .oneshot(get(path, "mod-unflipped"))
                .await
                .expect("serve");
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{path}");
        }
    }

    /// OPTIONS proxies even while flipped (pilot precedent: DRF's metadata
    /// response is preserved instead of axum's default 405).
    #[tokio::test]
    async fn options_proxies_while_flipped() {
        for path in [SCHEMA_PATH, SWAGGER_UI_PATH, REDOC_PATH] {
            let response = app(true)
                .oneshot(
                    Request::options(path)
                        .header("x-forwarded-for", "mod-options")
                        .body(axum::body::Body::empty())
                        .expect("request"),
                )
                .await
                .expect("serve");
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{path}");
        }
    }

    /// The store replays the fixture burst (FX-OPENAPI-05
    /// `burst_attempt_statuses`): 30 allows, then denials with a wait that
    /// renders `Retry-After >= 1`.
    #[test]
    fn store_allows_thirty_then_denies_with_wait() {
        let now = 1_700_000_000.0;
        for _ in 0..30 {
            assert!(check_throttle("mod-burst", now).allowed);
        }
        let denied = check_throttle("mod-burst", now);
        assert!(!denied.allowed);
        assert_eq!(denied.wait, Some(60.0));
        assert_eq!(throttle::retry_after_secs(denied.wait), Some(60));
    }

    /// A window-old history re-allows (FX-OPENAPI-05
    /// `window_reset_reallows`).
    #[test]
    fn store_window_reset_reallows() {
        let now = 1_700_000_000.0;
        for _ in 0..30 {
            assert!(check_throttle("mod-reset", now).allowed);
        }
        assert!(!check_throttle("mod-reset", now).allowed);
        assert!(check_throttle("mod-reset", now + 61.0).allowed);
    }

    /// Buckets are per ident: one caller's burst never throttles another.
    #[test]
    fn store_buckets_are_per_ident() {
        let now = 1_700_000_000.0;
        for _ in 0..30 {
            assert!(check_throttle("mod-alice", now).allowed);
        }
        assert!(!check_throttle("mod-alice", now).allowed);
        assert!(check_throttle("mod-bob", now).allowed);
    }

    /// Ident vectors: X-Forwarded-For wins (whitespace-stripped), else the
    /// TCP peer, else loopback when the server gives no peer (unit tests).
    #[test]
    fn peer_ident_prefers_xff_then_peer() {
        let req = get(SCHEMA_PATH, "  9.9.9.9 , 1.1.1.1  ");
        assert_eq!(peer_ident(&req), "9.9.9.9,1.1.1.1");
        let bare = Request::get(SCHEMA_PATH)
            .body(axum::body::Body::empty())
            .expect("request");
        assert_eq!(peer_ident(&bare), "127.0.0.1");
        let mut peered = Request::get(SCHEMA_PATH)
            .body(axum::body::Body::empty())
            .expect("request");
        peered.extensions_mut().insert(ConnectInfo(
            "192.0.2.7:1234".parse::<SocketAddr>().expect("addr"),
        ));
        assert_eq!(peer_ident(&peered), "192.0.2.7");
    }

    /// No session, an empty session, or a session without Django-auth keys
    /// counts as anonymous — all without touching the database.
    #[tokio::test]
    async fn session_check_anonymous_without_auth_keys() {
        let state = AppState::with_edge("test", edge::EdgeHandle::for_tests("http://127.0.0.1:1"));
        assert!(!is_session_authenticated(&state, None).await);

        let empty =
            crate::middleware::SessionHandle::new(crate::middleware::RequestSession::empty());
        assert!(!is_session_authenticated(&state, Some(empty)).await);

        let mut wrong_backend = crate::middleware::RequestSession::empty();
        wrong_backend.set(
            "_auth_user_id".to_owned(),
            serde_json::json!("12345678-1234-1234-1234-1234567890ab"),
        );
        wrong_backend.set(
            "_auth_user_backend".to_owned(),
            serde_json::json!("some.other.Backend"),
        );
        let handle = crate::middleware::SessionHandle::new(wrong_backend);
        assert!(!is_session_authenticated(&state, Some(handle)).await);
    }

    /// A session carrying auth keys still counts as anonymous when no pools
    /// are attached (unit-test states cannot verify the user row): the
    /// public document stays servable behind the anonymous throttle.
    #[tokio::test]
    async fn session_check_without_pools_counts_anonymous() {
        let state = AppState::with_edge("test", edge::EdgeHandle::for_tests("http://127.0.0.1:1"));
        assert!(state.pools().is_none());
        let mut session = crate::middleware::RequestSession::empty();
        session.set(
            "_auth_user_id".to_owned(),
            serde_json::json!("12345678-1234-1234-1234-1234567890ab"),
        );
        session.set(
            "_auth_user_backend".to_owned(),
            serde_json::json!(crate::license::MODEL_BACKEND),
        );
        session.set("_auth_user_hash".to_owned(), serde_json::json!("bogus"));
        let handle = crate::middleware::SessionHandle::new(session);
        assert!(!is_session_authenticated(&state, Some(handle)).await);
    }

    /// The session snapshotter reads the layer-attached handle: absent
    /// without the extension, present with it.
    #[test]
    fn session_handle_snapshotter_reads_the_extension() {
        let bare = Request::get(SCHEMA_PATH)
            .body(axum::body::Body::empty())
            .expect("request");
        assert!(session_handle(&bare).is_none());
        let mut req = Request::get(SCHEMA_PATH)
            .body(axum::body::Body::empty())
            .expect("request");
        req.extensions_mut()
            .insert(crate::middleware::SessionHandle::new(
                crate::middleware::RequestSession::empty(),
            ));
        assert!(session_handle(&req).is_some());
    }

    /// The local session-hash verifier matches Django's
    /// `salted_hmac(...get_session_auth_hash, password)` construction (same
    /// vectors as the license-domain verifier it mirrors).
    #[test]
    fn session_hash_verify_matches_django_construction() {
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};
        let secret = b"test-secret-key";
        let password = "pbkdf2_sha256$720000$salt$hash";
        let key = Sha256::digest(
            [
                crate::license::SESSION_AUTH_HASH_SALT.as_bytes(),
                secret.as_slice(),
            ]
            .concat(),
        );
        let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("key");
        mac.update(password.as_bytes());
        let expected: Vec<u8> = mac.finalize().into_bytes().to_vec();
        let hex: String = expected.iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify_session_hash(&hex, password, secret));
        assert!(!verify_session_hash(&hex, "other-password-field", secret));
        assert!(!verify_session_hash(&hex, password, b"other-secret"));
    }
}
