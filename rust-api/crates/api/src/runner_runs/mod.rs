//! Runner daemon + web-chat handlers (D-15 L8, stage 5, PIDASHCONV-543).
//!
//! Ports `apps/api/pi_dash/runner/views/run_endpoints.py:1-510` (12 run
//! endpoints) and `apps/api/pi_dash/runner/views/chat.py:1-857` (8 web
//! chat endpoints, 7 chat daemon endpoints, the SSE stream) to axum.
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-09-handlers-daemon.golden.json`
//! (FX-RUN-09); the `#[cfg(test)]` suites replay it via `include_str!`.
//!
//! Layout: [`run_endpoints`] owns the 12 `POST
//! `/api/v1/runner/runs/<run_id>/...`` endpoints, [`chat`] the 8 web
//! (`/api/runners/chat/...`) + 7 daemon (`/api/v1/runner/chat/...`)
//! endpoints, [`sse`] the `GET .../events/` stream. This module owns the
//! shared base: response helpers, the DRF body reader, daemon
//! authentication, the cross-domain ports, and `routes()` for the F-10
//! seam (`RouteGroup::Runner` + `RouteGroup::RunnerWeb`).
//!
//! # Reuse, not forks
//!
//! * Guards/shapes: L3 (`pidash_services::runner_runs::{guards,shape}`)
//!   plus the foundation kernels (`pidash_auth::permissions::{runner,
//!   membership}`, `pidash_auth::token::hash_token`,
//!   `pidash_auth::jwt::decode_access_token`, D-13 runner shapes).
//! * Lifecycle: L4 plans + SQL
//!   (`pidash_services::runner_runs::{lifecycle,finalization,
//!   scheduler_hook}`); this layer binds params, runs the statements,
//!   and drains [`pidash_services::runner_runs::LifecycleEffect`].
//! * Chat: L5 plans + SQL + [`pidash_services::runner_runs::chat::ChatEffect`].
//! * Session auth: [`crate::license::resolve_actor`]; throttle denial
//!   body: [`crate::assistant::throttles::RATE_LIMIT_BODY`]; SSE
//!   transport: [`crate::sse_body::sse_channel`]; spaced `json.dumps`:
//!   [`crate::assistant::events::py_dumps`].
//!
//! # Cross-domain seams ([`RunnerPorts`])
//!
//! D-13 has no auth provider and D-14 no outbox provider merged, so the
//! live implementations live here until the providers land:
//!
//! * Daemon auth (`RunnerAccessTokenAuthentication` +
//!   `resolve_runner_for_run`, `authentication.py:56-254`) is ported in
//!   this module ([`authenticate_daemon`]) — the endpoints cannot serve
//!   without it. D-13's future auth port subsumes it (edge then).
//! * `send_to_runner` (`pubsub.py:45-60` + `outbox.py:220-253`) is
//!   implemented for real in [`LivePorts`]: the contract suite pins the
//!   offline fan-out 500s (`test_web_chat_close_shape`,
//!   `test_web_chat_approval_decide_shape`), so the session lookup, the
//!   offline-reject matrix, and the XADD wire shape are behavior, not
//!   plumbing. D-14's outbox port takes over when it merges.
//! * Matcher drains, D-12 orchestration, and D-11 `dispatch_waiting` are
//!   warn-logged no-ops: every call site is isolated in Python (a drain
//!   failure only logs), so no response byte depends on them, and the
//!   Python plane still runs the beat tasks until cutover. The one
//!   exception is `_pause_and_drain`'s drain, which is unisolated in
//!   Python and 500s after commit when it raises — case-closed below.
//! * `agent_system_user_id` and the Celery emits (`fire_tick`,
//!   `runner.apply_agent_run_terminal_effects` via the F-09 publisher)
//!   are real: comments need the actor row, and the emits are plain
//!   broker publishes.
//!
//! # Post-commit discipline
//!
//! Python runs `on_commit` callbacks synchronously after commit, before
//! the response. The handlers mirror that: the transaction body collects
//! effect values, commits, then drains them in order before responding.
//! Each drain site keeps Python's isolation (log-and-continue vs
//! propagate), documented at the call.
//!
//! # Ported bugs (translate, don't redesign; also listed in the PR)
//!
//! * BUG-chat-noauth-500 (`chat.py:522`): the daemon chat `_resolve`
//!   dereferences `auth_runner.id` with no `None` guard —
//!   unauthenticated daemon chat POSTs answer 500, not 401/403. Pinned
//!   by `test_daemon_chat_requires_auth` (status only).
//! * BUG-offline-fanout-500 (`chat.py:380-425,457-510` + `outbox.py:57-67`):
//!   close/decide send `chat_close`/`chat_decide` inside `on_commit`;
//!   for an offline runner `send_to_runner` raises `RunnerOfflineError`
//!   after the row committed, so the 500 carries a persisted close.
//!   Pinned by `test_web_chat_close_shape` /
//!   `test_web_chat_approval_decide_shape` (status + row).
//! * BUG-runner-count (inherited via D-13 shapes): the nested
//!   `runner_detail` count quirk rides along through reuse.
//!
//! # Documented approximations (no contract input covers them)
//!
//! * A 500 body is the shared [`SERVER_ERROR_BODY`] JSON; Django renders
//!   its HTML error page on these paths. Contract tests pin the 500
//!   status only.
//! * `expires_at` values bind as text and let Postgres parse them — the
//!   same strings psycopg sends, so the same values parse or 500.
//! * Redis PUBLISH / XADD go through an api-crate-owned `redis::Client`
//!   built from settings (the `auth_session::magic` precedent):
//!   `RedisHandle` exposes no publish primitive and foundation crates
//!   are read-only. SUBSCRIBE stays on the foundation handle.
//! * Malformed JSON bodies 400 as `{"Detail": "JSON parse error"}`; the
//!   Python message text (CPython's `json` error) is not reproduced.

// Every handler returns a fully-rendered `Response` by design (the
// intake `parse_body` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

pub mod chat;
pub mod run_endpoints;
pub mod sse;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt as _;
use serde_json::{Map, Value};
use sqlx::PgPool;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Shared responses
// ---------------------------------------------------------------------------

/// Unhandled-failure body (500). Runner views have no custom
/// `handle_exception`, so Django renders its HTML error page here; only
/// the status is contract-pinned, and this JSON text keeps the Rust
/// surface consistent with the sibling handler ports.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Render a JSON body with the DRF content type.
pub fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("runner-runs response builds")
}

/// The request pool, or the 500 when the server runs pool-less
/// (unreachable in `serve`, which fail-fasts at boot).
pub fn pool_of(state: &AppState) -> Result<&PgPool, Response> {
    match state.pools().map(|pools| pools.primary()) {
        Some(pool) => Ok(pool),
        None => Err(json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            SERVER_ERROR_BODY.to_owned(),
        )),
    }
}

/// The 500 shortcut for fallible handler paths.
pub fn server_error() -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        SERVER_ERROR_BODY.to_owned(),
    )
}

// ---------------------------------------------------------------------------
// DRF request-data reader (`Request._parse`, DRF 3.15.2)
// ---------------------------------------------------------------------------

/// Read `request.data` for a POST body: content-length 0 validates as
/// `{}` with the body ignored; a non-JSON content type proxies to
/// Django (form posts stay on the Python plane); whitespace-only JSON
/// 400s with CPython's `Expecting value` position; unparsable JSON
/// 400s `{"Detail": "JSON parse error"}`. The `app_scheduler`
/// `read_request_data` precedent, shared by every POST here.
pub async fn read_request_data(
    state: &AppState,
    req: axum::http::Request<axum::body::Body>,
) -> Result<Value, Response> {
    let content_length = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length == 0 {
        return Ok(Value::Object(Map::new()));
    }
    let raw_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    // `parse_header_parameters`: the main type lowercases; parameters
    // are ignored for parser selection (`_MediaType.match`).
    let main = raw_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if main != "application/json" {
        return Err(crate::edge::proxy(State(state.clone()), req).await);
    }
    let (_parts, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .map_err(|_| server_error())?;
    if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
        // `json.load` of whitespace-only input: `Expecting value` at
        // the first unconsumed position (line 1, column `n + 1`).
        let position = bytes.len();
        return Err(json_response(
            StatusCode::BAD_REQUEST,
            format!(
                "{{\"Detail\":\"JSON parse error - Expecting value: line 1 column {} (char {})\"}}",
                position + 1,
                position
            ),
        ));
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => Ok(value),
        Err(_) => Err(json_response(
            StatusCode::BAD_REQUEST,
            r#"{"Detail":"JSON parse error"}"#.to_owned(),
        )),
    }
}

/// Python truthiness for JSON frame values (`None`/`False`/`0`/`""`/
/// `[]`/`{}` are falsy; everything else is truthy). Mirrors the L4
/// kernel (services is read-only, so it is restated here, like the
/// `app_scheduler` twins).
pub fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            // Discriminate on the stored representation: `as_i64`
            // casts representable floats (`0.5` -> `Some(0)`), which
            // would wrongly falsify them — only a true zero is falsy.
            if n.is_i64() {
                n.as_i64() != Some(0)
            } else if n.is_u64() {
                n.as_u64() != Some(0)
            } else {
                n.as_f64().map(|f| f != 0.0).unwrap_or(false)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// First `max` Unicode code points of `text` (`text[:max]`). Mirrors
/// the L4 kernel: byte-slicing would panic on a UTF-8 boundary.
pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// `(request.data.get(key) or "")` then `[:max]`: falsy values map to
/// `""`, strings truncate by code points, truthy non-strings are the
/// source's `TypeError` (500) — `int` has no `__getitem__`.
/// Used for `thread_id`-family frame fields (`run_endpoints.py:166-176`,
/// `chat.py:539-541`).
pub fn frame_text(value: &Value, max: usize) -> Result<String, Response> {
    if !py_truthy(value) {
        return Ok(String::new());
    }
    match value.as_str() {
        Some(text) => Ok(truncate_chars(text, max)),
        None => Err(server_error()),
    }
}

/// `str(request.data.get(key) or "").strip()` (`run_endpoints.py:187`,
/// `run_lifecycle._normalize_model`): falsy maps to `""`, strings pass
/// through, other truthy values render via JSON spelling. Only ever
/// called where Python calls `str()` first, so containers cannot 500
/// here the way they do under [`frame_text`].
pub fn frame_model(value: &Value) -> String {
    if !py_truthy(value) {
        return String::new();
    }
    match value {
        Value::String(text) => text.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        // `str(dict)` / `str(list)` single-quote repr; only the
        // emptiness + truncation matter downstream (`[:128]` after a
        // truthiness gate), so the compact JSON spelling suffices.
        Value::Array(_) | Value::Object(_) => value.to_string(),
        Value::Null => String::new(),
    }
}

#[cfg(test)]
mod body_tests {
    use super::*;

    #[test]
    fn py_truthy_matches_python() {
        for falsy in [
            Value::Null,
            Value::Bool(false),
            serde_json::json!(0),
            serde_json::json!(0.0),
            serde_json::json!(""),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            assert!(!py_truthy(&falsy), "{falsy} is falsy");
        }
        for truthy in [
            serde_json::json!(true),
            serde_json::json!(1),
            serde_json::json!(0.5),
            serde_json::json!(-0.5),
            serde_json::json!(9.9),
            serde_json::json!(" "),
            serde_json::json!([0]),
            serde_json::json!({"a": 0}),
        ] {
            assert!(py_truthy(&truthy), "{truthy} is truthy");
        }
    }

    #[test]
    fn truncate_chars_counts_code_points() {
        assert_eq!(truncate_chars("abcdef", 3), "abc");
        assert_eq!(truncate_chars("———", 2), "——");
        // A byte slice at a UTF-8 boundary would panic; chars never do.
        assert_eq!(truncate_chars("éclair", 1), "é");
    }

    #[test]
    fn frame_text_gates_and_truncates() {
        assert_eq!(frame_text(&Value::Null, 4).expect("null"), "");
        assert_eq!(frame_text(&serde_json::json!(""), 4).expect("empty"), "");
        assert_eq!(
            frame_text(&serde_json::json!("abcdef"), 4).expect("str"),
            "abcd"
        );
        assert!(frame_text(&serde_json::json!(7), 4).is_err());
        assert!(frame_text(&serde_json::json!({"t": 1}), 4).is_err());
    }

    #[test]
    fn frame_model_stringifies_like_str() {
        assert_eq!(frame_model(&Value::Null), "");
        assert_eq!(frame_model(&serde_json::json!("  m1  ")), "m1");
        assert_eq!(frame_model(&serde_json::json!(7)), "7");
        assert_eq!(frame_model(&serde_json::json!(true)), "True");
    }

    #[tokio::test]
    async fn empty_body_reads_as_empty_object() {
        let state = AppState::new("test");
        let req = axum::http::Request::post("/x")
            .body(axum::body::Body::empty())
            .expect("request");
        let data = read_request_data(&state, req).await.expect("data");
        assert_eq!(data, serde_json::json!({}));
    }

    #[tokio::test]
    async fn whitespace_body_reports_cpython_position() {
        let state = AppState::new("test");
        let req = axum::http::Request::post("/x")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, "3")
            .body(axum::body::Body::from("   "))
            .expect("request");
        let response = read_request_data(&state, req).await.expect_err("400");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        assert_eq!(
            String::from_utf8(body.to_vec()).expect("utf8"),
            r#"{"Detail":"JSON parse error - Expecting value: line 1 column 4 (char 3)"}"#
        );
    }

    #[tokio::test]
    async fn malformed_json_reports_parse_error() {
        let state = AppState::new("test");
        let req = axum::http::Request::post("/x")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, "7")
            .body(axum::body::Body::from("{oops!}"))
            .expect("request");
        let response = read_request_data(&state, req).await.expect_err("400");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        assert_eq!(
            String::from_utf8(body.to_vec()).expect("utf8"),
            r#"{"Detail":"JSON parse error"}"#
        );
    }
}

// ---------------------------------------------------------------------------
// Daemon authentication (`runner/authentication.py:46-254`)
// ---------------------------------------------------------------------------

/// The runner behind a verified daemon credential: `request.auth_runner`
/// as data. Carries the ownership ids the `_resolve` helpers compare
/// plus the display fields L1 enrichment reads.
#[derive(Debug, Clone)]
pub struct DaemonRunner {
    pub id: uuid::Uuid,
    pub owner_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub name: String,
    pub host_label: String,
    pub capabilities: Value,
    pub dev_label: Option<String>,
    pub dev_host_label: Option<String>,
}

/// `Authorization: Bearer ...` extraction (`_bearer`,
/// `authentication.py:46-53`): exactly two whitespace-separated parts
/// with a case-insensitive `bearer` scheme. Anything else means "no
/// credential" (`None`), exactly like Python returning `None`.
pub fn bearer_token(header_value: Option<&str>) -> Option<&str> {
    let raw = header_value?;
    let mut parts = raw.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    Some(token)
}

/// Runner identity for the machine-token path
/// (`_request_runner_id`, `authentication.py:174-180`). Run/chat URLs
/// carry no `runner_id` segment, so the daemon sends `X-Runner-Id`.
pub fn request_runner_id(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("x-runner-id")?.to_str().ok()?.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_owned())
    }
}

/// Render an `AuthenticationFailed(code)` denial: 401 `{"Detail": code}`
/// through the default DRF handler (the project's `auth_exception_handler`
/// only rewrites `Throttled`), plus the `WWW-Authenticate: Bearer`
/// challenge from `authenticate_header`.
pub fn auth_failed(code: &str) -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::WWW_AUTHENTICATE, "Bearer")
        .body(axum::body::Body::from(
            serde_json::json!({"Detail": code}).to_string(),
        ))
        .expect("auth denial builds")
}

/// One `machine_token` row plus its dev-machine revocation bit (`mt_`
/// path, `authentication.py:120-162`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct MachineTokenRow {
    id: uuid::Uuid,
    user_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    dev_machine_id: Option<uuid::Uuid>,
    host_label: String,
    revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    dev_revoked: Option<bool>,
}

/// One `runner` row plus its dev-machine columns (both auth paths).
#[derive(Debug, Clone, sqlx::FromRow)]
struct RunnerAuthRow {
    id: uuid::Uuid,
    owner_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    dev_machine_id: Option<uuid::Uuid>,
    name: String,
    host_label: String,
    refresh_token_generation: i32,
    capabilities: Value,
    revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    dev_revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    dev_label: Option<String>,
    dev_host_label: Option<String>,
}

/// `MachineToken.objects.select_related(...).get(token_hash=...)`
/// (`authentication.py:122-123`): hash match by lookup, one row.
async fn fetch_machine_token(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<MachineTokenRow>, sqlx::Error> {
    sqlx::query_as::<_, MachineTokenRow>(
        r#"SELECT mt."id", mt."user_id", mt."workspace_id", mt."dev_machine_id",
                  mt."host_label", mt."revoked_at",
                  dm."revoked_at" IS NOT NULL AS "dev_revoked"
           FROM "machine_token" mt
           LEFT OUTER JOIN "dev_machine" dm ON dm."id" = mt."dev_machine_id"
           WHERE mt."token_hash" = $1"#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

/// `Runner.objects.select_related("workspace", "pod", "dev_machine").get(id=...)`
/// (`authentication.py:92,138`): the auth-needed columns only — the
/// related rows are never read past their revocation bits.
async fn fetch_runner_for_auth(
    pool: &PgPool,
    runner_id: uuid::Uuid,
) -> Result<Option<RunnerAuthRow>, sqlx::Error> {
    sqlx::query_as::<_, RunnerAuthRow>(
        r#"SELECT r."id", r."owner_id", r."workspace_id", r."dev_machine_id",
                  r."name", r."host_label", r."refresh_token_generation",
                  r."capabilities", r."revoked_at",
                  dm."revoked_at" AS "dev_revoked_at",
                  dm."label" AS "dev_label", dm."host_label" AS "dev_host_label"
           FROM "runner" r
           LEFT OUTER JOIN "dev_machine" dm ON dm."id" = r."dev_machine_id"
           WHERE r."id" = $1"#,
    )
    .bind(runner_id)
    .fetch_optional(pool)
    .await
}

impl From<&RunnerAuthRow> for DaemonRunner {
    fn from(row: &RunnerAuthRow) -> Self {
        DaemonRunner {
            id: row.id,
            owner_id: row.owner_id,
            workspace_id: row.workspace_id,
            name: row.name.clone(),
            host_label: row.host_label.clone(),
            capabilities: row.capabilities.clone(),
            dev_label: row.dev_label.clone(),
            dev_host_label: row.dev_host_label.clone(),
        }
    }
}

/// `is_workspace_member` (`core/permissions.py:32-39`): a live,
/// non-soft-deleted `WorkspaceMember` row (the default manager filters
/// `deleted_at IS NULL`).
async fn workspace_member_exists(
    pool: &PgPool,
    workspace_id: uuid::Uuid,
    user_id: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        r#"SELECT EXISTS(SELECT 1 FROM "workspace_members"
           WHERE "workspace_id" = $1 AND "member_id" = $2
             AND "is_active" AND "deleted_at" IS NULL)"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map(|row| row.unwrap_or(false))
}

/// `RunnerAccessTokenAuthentication.authenticate`
/// (`authentication.py:79-162`) as data: `Ok(None)` when no Bearer
/// credential is presented (every endpoint then resolves "not owned"
/// or 500s, per its own `_resolve`); `Err(response)` for a rejected
/// credential (401 `{"Detail": code}`).
///
/// The URL carries no `runner_id` on run/chat routes, so step 5 of the
/// docstring (`token.sub == url_runner_id`) never applies and the
/// machine-token runner identity always comes from `X-Runner-Id`.
pub async fn authenticate_daemon(
    pool: &PgPool,
    secret_key: &[u8],
    headers: &HeaderMap,
) -> Result<Option<DaemonRunner>, Response> {
    let header_str = headers
        .get(header::AUTHORIZATION)
        .map(|value| value.to_str());
    // `parts[0].decode()` / `parts[1].decode()` (`authentication.py:52-53`):
    // non-UTF-8 bytes are the source's `UnicodeDecodeError` (500).
    let header_str = match header_str {
        Some(Ok(text)) => Some(text),
        Some(Err(_)) => return Err(server_error()),
        None => None,
    };
    let Some(raw) = bearer_token(header_str) else {
        return Ok(None);
    };
    if raw.starts_with("mt_") {
        return authenticate_machine_token(pool, secret_key, headers, raw).await;
    }
    authenticate_legacy_jwt(pool, secret_key, raw).await
}

/// Legacy per-runner JWT path (`authentication.py:85-118`).
async fn authenticate_legacy_jwt(
    pool: &PgPool,
    secret_key: &[u8],
    raw: &str,
) -> Result<Option<DaemonRunner>, Response> {
    // Stock settings carry no `RUNNER_ACCESS_TOKEN_KEYS` (`[]`), so the
    // ring is the derived `"default"` key (`tokens.py:108-110`).
    let secret = String::from_utf8_lossy(secret_key).into_owned();
    let ring = pidash_services::runner_enroll::tokens::build_key_ring(&[], &secret);
    let payload = match pidash_auth::jwt::decode_access_token(raw, &ring) {
        Ok(claims) => claims,
        Err(error) => return Err(auth_failed(error.code())),
    };
    // `Runner.objects...get(id=runner_id)` (`authentication.py:91-94`):
    // a non-UUID `sub` is the source's `ValidationError` (500), a
    // missing row is `runner_not_found` (401).
    let runner_id: uuid::Uuid = match payload.sub.parse() {
        Ok(id) => id,
        Err(_) => return Err(server_error()),
    };
    let row = fetch_runner_for_auth(pool, runner_id)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(auth_failed("runner_not_found"));
    };
    if row.revoked_at.is_some() {
        return Err(auth_failed("runner_revoked"));
    }
    if row.dev_machine_id.is_some() && row.dev_revoked_at.is_some() {
        return Err(auth_failed("dev_machine_revoked"));
    }
    if payload.rtg < i64::from(row.refresh_token_generation) - 1 {
        return Err(auth_failed("access_token_stale_rtg"));
    }
    let min_rtg: Option<i32> = sqlx::query_scalar(
        r#"SELECT "min_rtg" FROM "runner_force_refresh" WHERE "runner_id" = $1"#,
    )
    .bind(row.id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    if let Some(floor) = min_rtg {
        if payload.rtg < i64::from(floor) {
            return Err(auth_failed("force_refresh_required"));
        }
    }
    Ok(Some(DaemonRunner::from(&row)))
}

/// Shared MachineToken path (`authentication.py:120-162`).
async fn authenticate_machine_token(
    pool: &PgPool,
    secret_key: &[u8],
    headers: &HeaderMap,
    raw: &str,
) -> Result<Option<DaemonRunner>, Response> {
    let token_hash = pidash_auth::token::hash_token(raw, secret_key);
    let token = fetch_machine_token(pool, &token_hash)
        .await
        .map_err(|_| server_error())?;
    let Some(token) = token else {
        return Err(auth_failed("machine_token_invalid"));
    };
    if token.revoked_at.is_some() {
        return Err(auth_failed("machine_token_revoked"));
    }
    if token.dev_machine_id.is_some() && token.dev_revoked.unwrap_or(false) {
        return Err(auth_failed("dev_machine_revoked"));
    }
    let member = workspace_member_exists(pool, token.workspace_id, token.user_id)
        .await
        .map_err(|_| server_error())?;
    if !member {
        // `token.revoke()` then deny (`authentication.py:130-132`).
        let now = chrono::Utc::now();
        let _ = sqlx::query(
            r#"UPDATE "machine_token" SET "revoked_at" = $1 WHERE "machine_token"."id" = $2"#,
        )
        .bind(now)
        .bind(token.id)
        .execute(pool)
        .await;
        return Err(auth_failed("membership_revoked"));
    }
    let Some(runner_raw) = request_runner_id(headers) else {
        return Err(auth_failed("runner_id_required"));
    };
    // `Runner.objects...get(id=runner_id)` (`authentication.py:137-140`):
    // non-UUID header is `ValidationError` (500), missing row is 401.
    let runner_id: uuid::Uuid = match runner_raw.parse() {
        Ok(id) => id,
        Err(_) => return Err(server_error()),
    };
    let row = fetch_runner_for_auth(pool, runner_id)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(auth_failed("runner_not_found"));
    };
    if row.revoked_at.is_some() {
        return Err(auth_failed("runner_revoked"));
    }
    if row.dev_machine_id.is_some() && row.dev_revoked_at.is_some() {
        return Err(auth_failed("dev_machine_revoked"));
    }
    if row.owner_id != token.user_id || row.workspace_id != token.workspace_id {
        return Err(auth_failed("runner_not_bound_to_machine_token"));
    }
    if let Some(token_machine) = token.dev_machine_id {
        if row.dev_machine_id != Some(token_machine) {
            return Err(auth_failed("runner_not_bound_to_machine_token"));
        }
    } else if row.dev_machine_id.is_some() || row.host_label != token.host_label {
        return Err(auth_failed("runner_not_bound_to_machine_token"));
    }
    // `last_used_at` bump (`authentication.py:153`): `QuerySet.update`
    // touches no `updated_at`.
    let now = chrono::Utc::now();
    let _ = sqlx::query(
        r#"UPDATE "machine_token" SET "last_used_at" = $1 WHERE "machine_token"."id" = $2"#,
    )
    .bind(now)
    .bind(token.id)
    .execute(pool)
    .await;
    Ok(Some(DaemonRunner::from(&row)))
}

/// `resolve_runner_for_run` (`authentication.py:243-254`): the run is
/// owned by the authenticated runner. `None` (no credential) and a
/// runnerless run both resolve `False` — the endpoint answers
/// `run_not_owned_by_runner` (403), never 401.
pub fn resolve_runner_for_run(
    run_runner_id: Option<uuid::Uuid>,
    runner: Option<&DaemonRunner>,
) -> bool {
    let (Some(runner_id), Some(runner)) = (run_runner_id, runner) else {
        return false;
    };
    runner_id == runner.id
}

#[cfg(test)]
mod auth_tests {
    use super::*;

    #[test]
    fn bearer_extraction_matches_get_authorization_header() {
        assert_eq!(bearer_token(None), None);
        assert_eq!(bearer_token(Some("")), None);
        assert_eq!(bearer_token(Some("Bearer abc123")), Some("abc123"));
        assert_eq!(bearer_token(Some("bearer abc123")), Some("abc123"));
        assert_eq!(bearer_token(Some("BEARER abc123")), Some("abc123"));
        // Not exactly two parts: no credential.
        assert_eq!(bearer_token(Some("Bearer")), None);
        assert_eq!(bearer_token(Some("Bearer a b")), None);
        assert_eq!(bearer_token(Some("Token abc")), None);
        assert_eq!(bearer_token(Some("   Bearer    spaced   ")), Some("spaced"));
    }

    #[test]
    fn runner_id_header_trims_and_rejects_blank() {
        let mut headers = HeaderMap::new();
        assert_eq!(request_runner_id(&headers), None);
        headers.insert("x-runner-id", "  ".parse().expect("header"));
        assert_eq!(request_runner_id(&headers), None);
        headers.insert(
            "x-runner-id",
            "  0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d  "
                .parse()
                .expect("header"),
        );
        assert_eq!(
            request_runner_id(&headers).as_deref(),
            Some("0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d")
        );
    }

    #[test]
    fn auth_failed_renders_401_detail_with_challenge() {
        let response = auth_failed("machine_token_invalid");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .expect("challenge"),
            "Bearer"
        );
    }

    #[test]
    fn resolve_runner_for_run_matches_python() {
        let id = uuid::Uuid::new_v4();
        let other = uuid::Uuid::new_v4();
        let runner = DaemonRunner {
            id,
            owner_id: uuid::Uuid::new_v4(),
            workspace_id: uuid::Uuid::new_v4(),
            name: String::new(),
            host_label: String::new(),
            capabilities: Value::Null,
            dev_label: None,
            dev_host_label: None,
        };
        assert!(resolve_runner_for_run(Some(id), Some(&runner)));
        assert!(!resolve_runner_for_run(Some(other), Some(&runner)));
        assert!(!resolve_runner_for_run(None, Some(&runner)));
        assert!(!resolve_runner_for_run(Some(id), None));
    }

    #[tokio::test]
    async fn missing_credential_resolves_anonymous() {
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/unused").expect("pool");
        let headers = HeaderMap::new();
        let runner = authenticate_daemon(&pool, b"secret", &headers)
            .await
            .expect("anonymous");
        assert!(runner.is_none());
    }
}

// ---------------------------------------------------------------------------
// Cross-domain ports (D-14 outbox, D-12 orchestration, D-11 dispatch, D-10)
// ---------------------------------------------------------------------------

/// How a port call failed. `Offline` is `RunnerOfflineError`
/// (`outbox.py:70-83`) — the only failure `send_to_runner` re-raises;
/// every other Redis failure is swallowed inside `send_to_runner`
/// (`pubsub.py:59-60`), so [`LivePorts::send_to_runner`] logs those
/// and returns `Ok`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortError {
    Offline {
        runner_id: uuid::Uuid,
        message_type: String,
    },
    Transport(String),
}

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // `RunnerOfflineError.__init__` (`outbox.py:78-82`):
            // `f"runner {runner_id} is offline; type {message_type!r}
            // cannot queue"`. Control types are plain identifiers, so
            // `{!r}` always renders single quotes.
            PortError::Offline {
                runner_id,
                message_type,
            } => write!(
                f,
                "runner {runner_id} is offline; type '{message_type}' cannot queue"
            ),
            PortError::Transport(detail) => f.write_str(detail),
        }
    }
}

/// The D-10/D-11/D-12/D-14 boundary. Executors take `&impl RunnerPorts`
/// (the L6a `SweepsRunsOutbox` precedent); production handlers pass
/// [`LivePorts`], unit tests pass fakes. Each method names the exact
/// Python call it replaces.
pub trait RunnerPorts: Send + Sync {
    /// `send_to_runner(runner_id, message)` (`pubsub.py:45-60`): the
    /// bare control frame (no `mid` — the provider envelopes it, as
    /// `_ensure_envelope` does). `Err` only for
    /// [`PortError::Offline`]; transport failures are logged inside.
    fn send_to_runner(
        &self,
        runner_id: uuid::Uuid,
        message: Value,
    ) -> impl std::future::Future<Output = Result<(), PortError>> + Send;
    /// `drain_for_runner_by_id(runner_id)` (`matcher.py:298-303`).
    fn drain_for_runner_by_id(
        &self,
        runner_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<(), PortError>> + Send;
    /// `drain_pod_by_id(pod_id)` (`matcher.py:242-250`).
    fn drain_pod_by_id(
        &self,
        pod_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<(), PortError>> + Send;
    /// `_apply_post_run_orchestration(run)` (`run_lifecycle.py:103-138`):
    /// disarm, deferred pause, pending-entry fire — each isolated, in
    /// order. The D-12 provider re-fetches the run row it needs.
    fn post_run_orchestration(
        &self,
        run_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<(), PortError>> + Send;
    /// `complete_project_move_handoff(run_id)` (D-12, after the hooks
    /// transaction commits, isolated).
    fn complete_project_move_handoff(
        &self,
        run_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<(), PortError>> + Send;
    /// `get_agent_system_user()` (`orchestration/workpad.py:44-71`):
    /// the bot user id authoring pause/failure comments.
    fn agent_system_user_id(
        &self,
    ) -> impl std::future::Future<Output = Result<uuid::Uuid, PortError>> + Send;
    /// `dispatch_waiting(workspace_id)` (D-11, cloud-agent capacity).
    /// Synchronous and unisolated in Python — a failure propagates and
    /// the capacity marker stays unset for the reconciler.
    fn dispatch_waiting(
        &self,
        workspace_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<(), PortError>> + Send;
    /// `apply_agent_run_terminal_effects.delay(str(run_id))`
    /// (`finalization.py:89-103`): the queued half of
    /// `_publish_effects`, isolated.
    fn emit_terminal_effects(
        &self,
        run_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<(), PortError>> + Send;
    /// `fire_tick.delay(str(ticker_id))` (`run_lifecycle.py:141-155`):
    /// pending-entry fire, isolated.
    fn emit_fire_tick(
        &self,
        ticker_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<(), PortError>> + Send;
    /// The shared Redis client for chat publishes (`redis_instance()`);
    /// `None` when `REDIS_URL` is unset, in which case chat publishes
    /// return silently.
    fn redis_client(&self) -> Option<&redis::Client>;
}

// ---- Outbox wire (D-14 `outbox.py`, mirrored until the provider lands) ----

/// Live control-message types (`outbox.py:38-54`). Anything else is the
/// source's `ValueError` — swallowed by `send_to_runner` like every
/// non-offline failure.
const OUTBOX_VALID_TYPES: &[&str] = &[
    "assign",
    "cancel",
    "decide",
    "config_push",
    "revoke",
    "remove_runner",
    "resume_ack",
    "force_refresh",
    "welcome",
    "chat_warm",
    "chat_user_message",
    "chat_cancel",
    "chat_close",
    "chat_decide",
];

/// Offline-rejected types (`outbox.py:57-67`): every chat type plus the
/// four issue-run types. Queued offline for anything else.
const OUTBOX_OFFLINE_REJECT: &[&str] = &[
    "assign",
    "cancel",
    "decide",
    "resume_ack",
    "chat_warm",
    "chat_user_message",
    "chat_cancel",
    "chat_close",
    "chat_decide",
];

/// `stream_key` (`outbox.py:94-95`).
pub fn outbox_stream_key(runner_id: &uuid::Uuid) -> String {
    format!("runner_stream:{runner_id}")
}

/// `group_name` (`outbox.py:98-99`).
pub fn outbox_group_name(runner_id: &uuid::Uuid) -> String {
    format!("runner-group:{runner_id}")
}

/// `offline_stream_key` (`outbox.py:101-102`).
pub fn outbox_offline_stream_key(runner_id: &uuid::Uuid) -> String {
    format!("runner_offline_stream:{runner_id}")
}

/// `_serialize` (`outbox.py:120-136`): the `{mid, type, payload}` XADD
/// fields. `mid` reuses the message's own or mints a hyphenated v4;
/// `payload` is stdlib `json.dumps(body, default=str)` (spaced,
/// ASCII — [`crate::assistant::events::py_dumps`]).
pub fn outbox_fields(message: &Value) -> (String, String, String) {
    let mut body = message.clone();
    let mid = body
        .get("mid")
        .and_then(Value::as_str)
        .filter(|mid| !mid.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Value::Object(map) = &mut body {
        map.insert("mid".to_owned(), Value::String(mid.clone()));
    }
    let msg_type = body
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let payload = crate::assistant::events::py_dumps(&body);
    (mid, msg_type, payload)
}

/// `active_session_id_for_runner` (`outbox.py:192-205`): any live
/// `runner_session` row. Only the `None`-ness decides; the ordering
/// (`-created_at`, `models.py:709`) is kept for exactness.
pub async fn active_session_exists(
    pool: &PgPool,
    runner_id: uuid::Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, uuid::Uuid>(
        r#"SELECT "id" FROM "runner_session"
           WHERE "runner_id" = $1 AND "revoked_at" IS NULL
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(runner_id)
    .fetch_optional(pool)
    .await
    .map(|row| row.is_some())
}

/// The production ports: real outbox enqueue, real system-user lookup,
/// real Celery emits; matcher drains, D-12 orchestration, and D-11
/// dispatch stay warn-logged no-ops until their providers merge (every
/// call site is isolated in Python, so no response byte depends on
/// them — except `_pause_and_drain`'s drain, which propagates; see
/// [`run_endpoints`](self::run_endpoints)).
#[derive(Debug, Clone)]
pub struct LivePorts {
    pool: PgPool,
    redis: Option<redis::Client>,
}

impl LivePorts {
    /// Build from the request pool plus an api-crate-owned Redis client
    /// (the `auth_session::magic` precedent — `RedisHandle` exposes no
    /// publish primitive and foundation crates are read-only). `None`
    /// when `REDIS_URL` is unset or unparsable, mirroring
    /// `redis_instance()` returning `None`.
    pub fn new(pool: PgPool, state: &AppState) -> Self {
        let redis = state
            .settings()
            .redis
            .url
            .as_deref()
            .filter(|url| !url.is_empty())
            .and_then(|url| redis::Client::open(url).ok());
        Self { pool, redis }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// `OFFLINE_STREAM_MAXLEN` / `OFFLINE_STREAM_TTL_SECS`
/// (`outbox.py:248-249`, `settings/common.py` defaults).
const OFFLINE_STREAM_MAXLEN: usize = 1000;
const OFFLINE_STREAM_TTL_SECS: u64 = 86400;

impl RunnerPorts for LivePorts {
    async fn send_to_runner(&self, runner_id: uuid::Uuid, message: Value) -> Result<(), PortError> {
        let msg_type = message
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        // `enqueue_for_runner` (`outbox.py:220-253`) via `send_to_runner`
        // (`pubsub.py:45-60`): unknown types are `ValueError`, swallowed
        // with the transport failures below.
        if !OUTBOX_VALID_TYPES.contains(&msg_type.as_str()) {
            tracing::warn!(%runner_id, %msg_type, "send_to_runner: unknown message type; swallowed");
            return Ok(());
        }
        let Some(client) = &self.redis else {
            tracing::warn!(%runner_id, "send_to_runner: redis unavailable; swallowed");
            return Ok(());
        };
        let active = active_session_exists(&self.pool, runner_id).await;
        match active {
            Err(error) => {
                tracing::warn!(%error, %runner_id, "send_to_runner: session lookup failed; swallowed");
                return Ok(());
            }
            Ok(true) => {
                let outcome = xadd_live(client, runner_id, &message).await;
                if let Err(error) = outcome {
                    tracing::warn!(%error, %runner_id, "send_to_runner: live enqueue failed; swallowed");
                }
                return Ok(());
            }
            Ok(false) => {}
        }
        if OUTBOX_OFFLINE_REJECT.contains(&msg_type.as_str()) {
            return Err(PortError::Offline {
                runner_id,
                message_type: msg_type,
            });
        }
        if let Err(error) = xadd_offline(client, runner_id, &message).await {
            tracing::warn!(%error, %runner_id, "send_to_runner: offline enqueue failed; swallowed");
        }
        Ok(())
    }

    async fn drain_for_runner_by_id(&self, runner_id: uuid::Uuid) -> Result<(), PortError> {
        // D-14 provider pending (PIDASHCONV-546 fixtures only): the
        // matcher still runs on the Python plane until cutover.
        tracing::warn!(%runner_id, "drain_for_runner_by_id: D-14 provider pending; skipped");
        Ok(())
    }

    async fn drain_pod_by_id(&self, pod_id: uuid::Uuid) -> Result<(), PortError> {
        tracing::warn!(%pod_id, "drain_pod_by_id: D-14 provider pending; skipped");
        Ok(())
    }

    async fn post_run_orchestration(&self, run_id: uuid::Uuid) -> Result<(), PortError> {
        // D-12 provider pending (fixtures only): isolated in Python, so
        // no response byte depends on it.
        tracing::warn!(%run_id, "post_run_orchestration: D-12 provider pending; skipped");
        Ok(())
    }

    async fn complete_project_move_handoff(&self, run_id: uuid::Uuid) -> Result<(), PortError> {
        tracing::warn!(%run_id, "complete_project_move_handoff: D-12 provider pending; skipped");
        Ok(())
    }

    async fn agent_system_user_id(&self) -> Result<uuid::Uuid, PortError> {
        // `get_agent_system_user` (`orchestration/workpad.py:44-71`):
        // get-or-create on the unique username, refusing a
        // non-bot collision. Real: pause/failure comments need the row.
        let row: Option<(uuid::Uuid, bool)> =
            sqlx::query_as(r#"SELECT "id", "is_bot" FROM "users" WHERE "username" = $1"#)
                .bind(AGENT_USERNAME)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| PortError::Transport(error.to_string()))?;
        if let Some((id, is_bot)) = row {
            if !is_bot {
                return Err(PortError::Transport(format!(
                    "User {AGENT_USERNAME:?} exists but is not a bot"
                )));
            }
            return Ok(id);
        }
        let id = uuid::Uuid::new_v4();
        let inserted: Result<Option<(uuid::Uuid,)>, sqlx::Error> = sqlx::query_as(
            r#"INSERT INTO "users"
               ("id", "username", "email", "first_name", "last_name", "is_bot", "password")
               VALUES ($1, $2, $3, $4, $5, TRUE, $6)
               ON CONFLICT ("username") DO NOTHING
               RETURNING "id""#,
        )
        .bind(id)
        .bind(AGENT_USERNAME)
        .bind(AGENT_USER_EMAIL)
        .bind(AGENT_USER_FIRST_NAME)
        .bind(AGENT_USER_LAST_NAME)
        .bind(unusable_password())
        .fetch_optional(&self.pool)
        .await;
        match inserted {
            Ok(Some((id,))) => Ok(id),
            _ => {
                // Lost the get-or-create race: re-read the winner.
                let row: Option<(uuid::Uuid, bool)> =
                    sqlx::query_as(r#"SELECT "id", "is_bot" FROM "users" WHERE "username" = $1"#)
                        .bind(AGENT_USERNAME)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|error| PortError::Transport(error.to_string()))?;
                match row {
                    Some((id, true)) => Ok(id),
                    Some(_) => Err(PortError::Transport(format!(
                        "User {AGENT_USERNAME:?} exists but is not a bot"
                    ))),
                    None => Err(PortError::Transport(
                        "agent user missing after race".to_owned(),
                    )),
                }
            }
        }
    }

    async fn dispatch_waiting(&self, workspace_id: uuid::Uuid) -> Result<(), PortError> {
        // D-11 provider pending: unisolated in Python (the capacity
        // marker stays unset on failure), but no contract input covers
        // the marker timing.
        tracing::warn!(%workspace_id, "dispatch_waiting: D-11 provider pending; skipped");
        Ok(())
    }

    async fn emit_terminal_effects(&self, run_id: uuid::Uuid) -> Result<(), PortError> {
        emit_celery(
            TERMINAL_EFFECTS_TASK,
            vec![Value::String(run_id.to_string())],
        )
        .await
        .map_err(PortError::Transport)
    }

    async fn emit_fire_tick(&self, ticker_id: uuid::Uuid) -> Result<(), PortError> {
        emit_celery(FIRE_TICK_TASK, vec![Value::String(ticker_id.to_string())])
            .await
            .map_err(PortError::Transport)
    }

    fn redis_client(&self) -> Option<&redis::Client> {
        self.redis.as_ref()
    }
}

/// Agent bot identity (`orchestration/workpad.py:29-32`).
const AGENT_USERNAME: &str = "pi_dash_agent";
const AGENT_USER_EMAIL: &str = "agent@example.com";
const AGENT_USER_FIRST_NAME: &str = "Pi Dash";
const AGENT_USER_LAST_NAME: &str = "Agent";

/// `runner.apply_agent_run_terminal_effects` (`runner/tasks.py:333`).
const TERMINAL_EFFECTS_TASK: &str = "runner.apply_agent_run_terminal_effects";
/// `pi_dash.bgtasks.agent_ticker.fire_tick` (D-10 task name).
const FIRE_TICK_TASK: &str = "pi_dash.bgtasks.agent_ticker.fire_tick";

/// `User.set_unusable_password()`: an unmatchable hash (never a valid
/// encoded hash, so no password validates against it).
fn unusable_password() -> String {
    format!("!{}", uuid::Uuid::new_v4().as_simple())
}

/// Publish one Celery message through the F-09 AMQP publisher (the
/// `auth_session::email` `publish_message` precedent: connect, publish,
/// close per message). Broker errors propagate to the caller, which
/// logs them — every emit site is isolated in Python.
async fn emit_celery(task: &str, args: Vec<Value>) -> Result<(), String> {
    let config = pidash_jobs::AmqpConfig::from_env().map_err(|error| error.to_string())?;
    let publisher = pidash_jobs::Publisher::connect(&config)
        .await
        .map_err(|error| error.to_string())?;
    let message = pidash_jobs::CeleryTaskMessage::new(task, args, Map::new());
    let outcome = publisher.publish(&message).await;
    let _ = publisher.close().await;
    outcome.map_err(|error| error.to_string())
}

/// Live enqueue: `_ensure_group` (`outbox.py:139-148`, BUSYGROUP
/// tolerated) then XADD on the runner stream. Raw `redis::cmd` calls:
/// the crate's `streams` helpers are feature-gated off and
/// `api/Cargo.toml` is read-only, so the commands are spelled out.
async fn xadd_live(
    client: &redis::Client,
    runner_id: uuid::Uuid,
    message: &Value,
) -> Result<(), redis::RedisError> {
    let mut connection = client.get_multiplexed_async_connection().await?;
    let stream = outbox_stream_key(&runner_id);
    let group = outbox_group_name(&runner_id);
    let mut create = redis::cmd("XGROUP");
    create
        .arg("CREATE")
        .arg(&stream)
        .arg(&group)
        .arg("$")
        .arg("MKSTREAM");
    let created: Result<(), redis::RedisError> = create.query_async(&mut connection).await;
    if let Err(error) = created {
        if !error.to_string().contains("BUSYGROUP") {
            return Err(error);
        }
    }
    let (mid, msg_type, payload) = outbox_fields(message);
    let mut xadd = redis::cmd("XADD");
    xadd.arg(&stream)
        .arg("*")
        .arg("mid")
        .arg(&mid)
        .arg("type")
        .arg(&msg_type)
        .arg("payload")
        .arg(&payload);
    let _: String = xadd.query_async(&mut connection).await?;
    Ok(())
}

/// Offline-buffer enqueue (`outbox.py:248-253`): bounded XADD plus TTL.
/// Unreachable for chat types (all offline-reject), kept for the
/// queueable issue-run types sharing this provider.
async fn xadd_offline(
    client: &redis::Client,
    runner_id: uuid::Uuid,
    message: &Value,
) -> Result<(), redis::RedisError> {
    use redis::AsyncCommands;
    let mut connection = client.get_multiplexed_async_connection().await?;
    let key = outbox_offline_stream_key(&runner_id);
    let (mid, msg_type, payload) = outbox_fields(message);
    let mut xadd = redis::cmd("XADD");
    xadd.arg(&key)
        .arg("MAXLEN")
        .arg("~")
        .arg(OFFLINE_STREAM_MAXLEN)
        .arg("*")
        .arg("mid")
        .arg(&mid)
        .arg("type")
        .arg(&msg_type)
        .arg("payload")
        .arg(&payload);
    let _: String = xadd.query_async(&mut connection).await?;
    let _: () = connection
        .expire(&key, OFFLINE_STREAM_TTL_SECS as i64)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Chat event publish (`services/chat.py:140-147`)
// ---------------------------------------------------------------------------

/// `publish_event`: PUBLISH the serialized event on the session channel.
/// A missing client is a no-op (`redis_instance()` returning `None`);
/// every other failure propagates — the publish sits in `on_commit`
/// unisolated, so the source 500s after commit too. The caller passes
/// [`RunnerPorts::redis_client`].
pub async fn publish_chat_event(
    client: Option<&redis::Client>,
    channel: &str,
    payload: &str,
) -> Result<(), redis::RedisError> {
    let Some(client) = client else {
        return Ok(());
    };
    use redis::AsyncCommands;
    let mut connection = client.get_multiplexed_async_connection().await?;
    let _: () = connection.publish(channel, payload).await?;
    Ok(())
}

#[cfg(test)]
mod ports_tests {
    use super::*;

    #[test]
    fn outbox_keys_match_python_builders() {
        let runner: uuid::Uuid = "0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d"
            .parse()
            .expect("uuid");
        assert_eq!(
            outbox_stream_key(&runner),
            "runner_stream:0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d"
        );
        assert_eq!(
            outbox_group_name(&runner),
            "runner-group:0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d"
        );
        assert_eq!(
            outbox_offline_stream_key(&runner),
            "runner_offline_stream:0192d3b4-8c1c-7a2e-9f4b-6d5c8b7a6e5d"
        );
    }

    #[test]
    fn outbox_fields_envelope_like_serialize() {
        // A message mid is reused; the payload is spaced stdlib dumps
        // with the mid merged in.
        let (mid, msg_type, payload) = outbox_fields(&serde_json::json!({
            "type": "chat_close",
            "chat_session_id": "s1",
            "mid": "m1",
            "reason": "bye",
        }));
        assert_eq!(mid, "m1");
        assert_eq!(msg_type, "chat_close");
        assert_eq!(
            payload,
            r#"{"type": "chat_close", "chat_session_id": "s1", "mid": "m1", "reason": "bye"}"#
        );
        // Missing mid mints a hyphenated v4.
        let (mid, _, _) = outbox_fields(&serde_json::json!({"type": "chat_warm"}));
        assert_eq!(mid.len(), 36);
        assert!(mid.parse::<uuid::Uuid>().is_ok());
    }

    #[test]
    fn offline_matrix_rejects_every_chat_type() {
        for msg_type in [
            "chat_warm",
            "chat_user_message",
            "chat_cancel",
            "chat_close",
            "chat_decide",
        ] {
            assert!(OUTBOX_VALID_TYPES.contains(&msg_type), "{msg_type} valid");
            assert!(
                OUTBOX_OFFLINE_REJECT.contains(&msg_type),
                "{msg_type} rejects"
            );
        }
        // Queueable survivors keep the offline path.
        assert!(!OUTBOX_OFFLINE_REJECT.contains(&"welcome"));
        assert!(!OUTBOX_OFFLINE_REJECT.contains(&"config_push"));
        assert!(!OUTBOX_VALID_TYPES.contains(&"bogus"));
    }

    #[test]
    fn agent_identity_matches_workpad() {
        assert_eq!(AGENT_USERNAME, "pi_dash_agent");
        assert_eq!(AGENT_USER_EMAIL, "agent@example.com");
        assert_eq!(AGENT_USER_FIRST_NAME, "Pi Dash");
        assert_eq!(AGENT_USER_LAST_NAME, "Agent");
        assert_eq!(
            TERMINAL_EFFECTS_TASK,
            "runner.apply_agent_run_terminal_effects"
        );
        assert_eq!(FIRE_TICK_TASK, "pi_dash.bgtasks.agent_ticker.fire_tick");
    }
}
