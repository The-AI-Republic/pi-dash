//! Deletes + machine-command endpoints (D-13 handlers-D, PIDASHCONV-593).
//!
//! Ports `runner/views/runners.py:249-286` (`DevMachineDeleteEndpoint`),
//! `runners.py:427-452` (`RunnerDetailEndpoint.delete`) and
//! `runner/views/machine_commands.py:63-257` (`MachineCreateRunnerEndpoint`,
//! `MachineCreateRunnerStatusEndpoint`, `MachineCommandResultEndpoint`).
//!
//! # Execution model
//!
//! * Web handlers (`DELETE /api/runners/<id>/`,
//!   `DELETE /api/runners/dev-machines/<id>/`,
//!   `POST /api/runners/dev-machines/<id>/create-runner/`,
//!   `GET .../create-runner/<request_id>/`) run the manage.rs preamble
//!   (session → 401, workspace gate → 400/403, scope checks) and mirror
//!   its body/preamble helpers — the predicates are twinned, not
//!   imported, since they are private to the sibling file (the
//!   manage.rs `py_strip` twin precedent).
//! * The daemon result handler
//!   (`POST /api/v1/runner/dev-machines/<mid>/commands/<rid>/result/`)
//!   authenticates via D-13 [`crate::runner_enroll::auth`] and binds the
//!   token to the URL machine exactly like the D-14 machine handlers
//!   (the `machine.rs` `authed_machine_id` precedent); denials pass
//!   through untouched — `auth.rs` renders the byte-correct
//!   lowercase-`detail` DRF 401 (PIDASHCONV-718, per DRF 3.15.2
//!   `exception_handler`, `views.py:96`, and the live wire; the 589
//!   probe's capital-`D` reading does not survive byte-ordinal
//!   checks — the `exc.detail` attribute beside the key is the easy
//!   misread).
//! * SQL text comes from the merged builders
//!   ([`manage_reads`](pidash_services::runner_enroll::queries::manage_reads)
//!   M4/R2 plus the `_scoped_machine` read); the two create-endpoint
//!   reads no provider owns (workspace-by-id, project-exists) are
//!   spelled here from their verified twins (E6a/E6c + M4 shapes).
//! * Deletes execute through the services-C drivers
//!   ([`delete`](pidash_services::runner_enroll::delete)) over the first
//!   live [`DeleteStore`] — the pool-store executor services-C left to
//!   handlers 592/593. Frames route through the D-14 pubsub drivers
//!   over a pool-backed [`PoolPubsub`]; post-commit effects drain after
//!   commit with Python's per-site isolation (the `run_endpoints`
//!   `drain_lifecycle_effects` precedent).
//! * Redis verbs call the D-14 providers, never inlined:
//!   `set_command_result` / `get_command_result` / `enqueue_for_machine`
//!   (`machine_outbox`), `send_to_machine` (pubsub driver), the runner
//!   outbox verbs for frames/close/cleanup.
//!
//! # Ported bugs and quirks (translate, don't redesign; also in the PR)
//!
//! * QUIRK-offline-None (`machine_commands.py:169-184`): a generic
//!   enqueue exception is logged and treated exactly like a `None`
//!   return — the marker is overwritten to `delivery_failed` and the
//!   view answers 503, not 202.
//! * QUIRK-delete-no-revoked-gate (`runners.py:281-286`): unlike revoke
//!   and rotate, the machine delete has no `revoked_at` guard — a
//!   revoked machine deletes cleanly (204).
//! * QUIRK-workspace-or-chain (`runners.py:128-129`): body wins over
//!   query on raw truthiness — a whitespace-only body value beats a
//!   valid query value and 400s ([`manage_reads::request_workspace_id`]).
//! * QUIRK-binding-blank (`machine_commands.py:211,241`): a marker whose
//!   `dev_machine_id` is missing, null, or `""` passes the machine
//!   binding — only a mismatched non-empty id 404s.
//! * QUIRK-pod-passthrough (`machine_commands.py:134` + F7): `pod` is
//!   NOT validated — passed to the daemon verbatim.
//! * QUIRK-close-noop: `close_runner_session` after a fresh nested
//!   revoke finds no active sessions (S2 revoked them all in-tx) and
//!   no-ops; the store skips the pool close outright for S2'd runners
//!   (a pool re-select would re-see the uncommitted rows and
//!   self-deadlock on S2's locks). Already-revoked runners keep the
//!   real close.
//!
//! # Documented approximations
//!
//! * Unhandled failures answer the JSON 500 (`SERVER_ERROR_BODY`): Django
//!   renders its HTML error page here, so only the status is
//!   contract-pinned (the `runner_runs` precedent).
//! * Non-UUID path segments on the web routes answer the view's JSON 404
//!   (`{"error":"not found"}`); on the daemon route they answer the
//!   resolver page (`{"error": "Page not found."}`) — the manage.rs and
//!   machine.rs precedents respectively.
//! * Lone-surrogate strings in Redis-bound positions store lossy
//!   (`U+FFFD`): Python's `json.dumps(ensure_ascii)` escapes them, but
//!   `serde_json::Value` cannot hold a surrogate at all. In DB-bound
//!   positions they 500, matching Django's Postgres encode failure.
//! * Stored result payloads re-render through `serde_json` (compact):
//!   byte-exact for the string payloads both writers emit; exotic
//!   daemon-written floats could respell.
//! * Post-commit drains/handoffs warn-skip via [`LivePorts`] exactly like
//!   every merged Rust path — no live drain_pod/handoff executor exists
//!   in the tree yet (the `LivePorts` "provider pending" precedent).
//! * `close_runner_session`'s session-row revokes run on pool checkouts,
//!   not inside the delete transaction: the merged `PubsubStore` seam
//!   takes `&self`, so only pool (autocommit) statements fit. The
//!   close runs only for already-revoked runners (fresh revokes skip
//!   it — QUIRK-close-noop); only a mid-tail rollback after such a
//!   close would leave its revokes committed (Python rolls those back
//!   too).

// Every handler returns a fully-rendered `Response` by design (the
// manage.rs precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use axum::extract::{Extension, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use http_body_util::BodyExt as _;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::membership;
use pidash_auth::permissions::runner as runner_perm;
use pidash_auth::scope::TenantScope;
use pidash_db::runner_sessions::machine_outbox;
use pidash_db::runner_sessions::machine_outbox::MachineOutboxError;
use pidash_db::runner_sessions::models::runner_session;
use pidash_db::runner_sessions::outbox as runner_outbox;
use pidash_db::runner_sessions::outbox::OutboxError;
use pidash_db::runner_sessions::RunnerSession;
use pidash_services::runner_enroll::delete as delete_svc;
use pidash_services::runner_enroll::queries::{enroll_reads, manage_reads};
use pidash_services::runner_enroll::revoke::{self, RevokeError, RevokeStore};
use pidash_services::runner_enroll::serializers::shapes;
use pidash_services::runner_runs::finalization as finalize_kernel;
use pidash_services::runner_sessions::pubsub as pubsub_svc;
use pidash_services::runner_sessions::pubsub::PubsubStore;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{AgentRunStatus, TERMINAL_RUN_STATUSES};
use pidash_types::runner_sessions::responses::dev_machine_mismatch_drf;
use pidash_types::{UserId, WorkspaceId};

use crate::middleware::SessionHandle;
use crate::runner_enroll::auth::{authenticate_machine_token, MachineAuth, ALLOW_POST};
use crate::runner_runs::run_endpoints::execute_terminal_effects;
use crate::runner_runs::{json_response, pool_of, server_error, LivePorts, RunnerPorts};
use crate::state::AppState;
use crate::v1_cycles_modules::json_cpython::{self, JObject, JVal, JsonFail};

// ---------------------------------------------------------------------------
// Error bodies (D13-F7 `handlers/endpoints.golden.json`, key order verbatim
// from the Python dict literals)
// ---------------------------------------------------------------------------

/// `{"error": "workspace is required"}` — 400 (every web gate).
pub const WORKSPACE_REQUIRED_BODY: &str = r#"{"error":"workspace is required"}"#;
/// `{"error": "forbidden"}` — 403 (non-member / non-manager).
pub const FORBIDDEN_BODY: &str = r#"{"error":"forbidden"}"#;
/// `{"error": "not found"}` — 404 (miss, or present-but-unviewable).
pub const NOT_FOUND_BODY: &str = r#"{"error":"not found"}"#;
/// `{"error": "project is required"}` — 400 (`machine_commands.py:101-104`).
pub const PROJECT_REQUIRED_BODY: &str = r#"{"error":"project is required"}"#;
/// `{"error": "workspace_not_found"}` — 404 (`machine_commands.py:107`).
pub const WORKSPACE_NOT_FOUND_BODY: &str = r#"{"error":"workspace_not_found"}"#;
/// `{"error": "project_not_found"}` — 404 (`machine_commands.py:109`).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"error":"project_not_found"}"#;
/// `{"error": "invalid_agent"}` — 400 (`machine_commands.py:126`).
pub const INVALID_AGENT_BODY: &str = r#"{"error":"invalid_agent"}"#;
/// `{"error": "machine_offline"}` — 409 (`machine_commands.py:166`).
pub const MACHINE_OFFLINE_BODY: &str = r#"{"error":"machine_offline"}"#;
/// `{"error": "delivery_failed"}` — 503 (`machine_commands.py:182`).
pub const DELIVERY_FAILED_BODY: &str = r#"{"error":"delivery_failed"}"#;
/// `{"error": "dev_machine_revoked"}` — 409 (`machine_commands.py:72`).
pub const DEV_MACHINE_REVOKED_BODY: &str = r#"{"error":"dev_machine_revoked"}"#;
/// `{"error": "unknown_request"}` — 404 (`machine_commands.py:212,242`).
pub const UNKNOWN_REQUEST_BODY: &str = r#"{"error":"unknown_request"}"#;
/// `{"error": "invalid_status"}` — 400 (`machine_commands.py:246`).
pub const INVALID_STATUS_BODY: &str = r#"{"error":"invalid_status"}"#;
/// Django's `custom_404_view` bytes for a non-UUID daemon path segment
/// (the machine.rs precedent).
const PAGE_NOT_FOUND_BODY: &str = r#"{"error": "Page not found."}"#;

/// `{"error": "invalid_runner_name", "error_description": ...}` — 400
/// (`machine_commands.py:113-122`; the description kernel is
/// [`shapes::RUNNER_NAME_VIEW_MESSAGE`]).
pub fn invalid_runner_name_body() -> String {
    format!(
        "{{\"error\":\"invalid_runner_name\",\"error_description\":\"{}\"}}",
        shapes::RUNNER_NAME_VIEW_MESSAGE
    )
}

/// `{"error": str(exc)}` — 400 for a bad `purge_local` flag
/// (`runners.py:279,450`).
pub fn purge_flag_body() -> String {
    format!(
        "{{\"error\":\"{}\"}}",
        pidash_services::runner_enroll::purge::PURGE_LOCAL_ERROR
    )
}

fn bad_request(body: &str) -> Response {
    json_response(StatusCode::BAD_REQUEST, body.to_owned())
}

fn forbidden() -> Response {
    json_response(StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned())
}

fn not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned())
}

fn conflict(body: &str) -> Response {
    json_response(StatusCode::CONFLICT, body.to_owned())
}

fn unavailable(body: &str) -> Response {
    json_response(StatusCode::SERVICE_UNAVAILABLE, body.to_owned())
}

fn page_not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY.to_owned())
}

// ---------------------------------------------------------------------------
// Preamble (the manage.rs `web_actor` + `workspace_role` precedent,
// twinned — the sibling's helpers are private)
// ---------------------------------------------------------------------------

/// Session → `(pool, user_id)`: anonymous answers the DRF
/// `NotAuthenticated` 401, pool/db failures the JSON 500.
async fn web_actor(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(PgPool, Uuid), Response> {
    let pool = pool_of(state)?.clone();
    let secret = state.settings().secret_key.clone();
    let actor = crate::license::resolve_actor(&pool, secret.as_bytes(), extension)
        .await
        .map_err(|_| server_error())?;
    let Some(actor) = actor else {
        return Err(json_response(
            StatusCode::UNAUTHORIZED,
            crate::license::UNAUTHENTICATED_BODY.to_owned(),
        ));
    };
    Ok((pool, actor.id))
}

/// `workspace_role` (`core/permissions.py:37-45`): the caller's active
/// role, or `None`. Tombstones do not count; `-created_at` with
/// `.first()` → `LIMIT 1`.
async fn workspace_role(
    executor: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i32>, Response> {
    let role: Option<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "workspace_members"
           WHERE ("workspace_id" = $1 AND "member_id" = $2
             AND "is_active" AND "deleted_at" IS NULL)
           ORDER BY "created_at" DESC LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(executor)
    .await
    .map_err(|_| server_error())?;
    Ok(role.map(i32::from))
}

/// UUID-typed query/body strings: Django's `UUIDField.get_prep_value`
/// raises `ValidationError` (an unhandled 500) on garbage — the
/// manage.rs precedent answers [`server_error`].
fn parse_uuid(raw: &str) -> Result<Uuid, Response> {
    raw.parse().map_err(|_| server_error())
}

/// `timezone.now()` truncated to microseconds (Django datetimes are
/// microsecond-exact; Postgres would round stored nanos).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

/// `timezone.now().isoformat()`: `+00:00` suffix, microseconds iff
/// nonzero (the machine.rs precedent).
fn django_isoformat(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
}

/// Api-crate-owned Redis client (the machine.rs precedent): `None`
/// when `REDIS_URL` is unset, empty, or unparsable, mirroring
/// `redis_instance()` returning `None`.
fn redis_client(state: &AppState) -> Option<redis::Client> {
    state
        .settings()
        .redis
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .and_then(|url| redis::Client::open(url).ok())
}

// ---------------------------------------------------------------------------
// Request bodies (the manage.rs envelope over `json_cpython`, twinned)
// ---------------------------------------------------------------------------

/// Read `request.data`: content-length 0 validates as `{}` with the
/// body ignored; a non-JSON content type proxies to Django (form posts
/// stay on the Python plane); unparsable JSON 400s with DRF's
/// `ParseError` detail; past the depth cap is Django's JSON 500.
async fn read_request_data(state: &AppState, req: Request) -> Result<JVal, Response> {
    let content_length = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length == 0 {
        return Ok(JVal::Object(JObject::new()));
    }
    let raw_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
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
    match json_cpython::parse_request_data(&bytes) {
        Ok(value) => Ok(value),
        Err(JsonFail::Message(detail)) => Err(json_response(
            StatusCode::BAD_REQUEST,
            parse_error_body(&detail),
        )),
        Err(JsonFail::Recursion) => Err(server_error()),
    }
}

/// DRF `ParseError` body: lowercase `detail`, the
/// `JSON parse error - ` prefix, CPython's message.
fn parse_error_body(detail: &str) -> String {
    format!(
        "{{\"detail\":{}}}",
        serde_json::to_string(&format!("{}{detail}", json_cpython::JSON_PARSE_PREFIX))
            .expect("json string")
    )
}

/// Python truthiness over a parsed value (`None`/`False`/`0`/`""`/`[]`/`{}`
/// are falsy; everything else is truthy).
fn j_truthy(value: &JVal) -> bool {
    match value {
        JVal::Null => false,
        JVal::Bool(flag) => *flag,
        JVal::Num(number) => !number.is_zero(),
        JVal::Str(text) => !text.is_empty(),
        JVal::Array(items) => !items.is_empty(),
        JVal::Object(map) => !map.is_empty(),
    }
}

/// `request.data.get(key)`: dict lookup — `None` for a missing key;
/// any non-object is Python's `AttributeError` → 500.
fn data_get<'a>(data: &'a JVal, key: &str) -> Result<Option<&'a JVal>, Response> {
    match data {
        JVal::Object(map) => Ok(map.get(key)),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Str(_) | JVal::Array(_) => {
            Err(server_error())
        }
    }
}

/// Python `str.strip()` parity (the manage.rs twin): Python strips
/// `str.isspace()` — Unicode `White_Space` plus U+001C-U+001F and
/// U+0085 — while Rust `trim()` strips `White_Space` only.
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| {
        c.is_whitespace() || c == '\u{85}' || ('\u{1c}'..='\u{1f}').contains(&c)
    })
}

/// `(request.data.get(key) or "").strip()`: falsy maps to `""`,
/// strings strip (lossy — every caller stores or compares, never
/// binds to Postgres), truthy non-strings are the source's
/// `AttributeError` → 500.
fn or_empty_stripped(value: Option<&JVal>) -> Result<String, Response> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    if !j_truthy(value) {
        return Ok(String::new());
    }
    match value {
        JVal::Str(text) => Ok(py_strip(&text.to_lossy_string()).to_owned()),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Array(_) | JVal::Object(_) => {
            Err(server_error())
        }
    }
}

/// `str(request.data.get(key) or "")` for the Redis-bound result
/// fields: falsy maps to `""`, truthy values render via Python `str()`
/// (lossy for surrogate-carrying strings — see the module docs); a
/// `str()` recursion failure is Django's 500.
fn py_string(value: Option<&JVal>) -> Result<String, Response> {
    let Some(value) = value else {
        return Ok(String::new());
    };
    if !j_truthy(value) {
        return Ok(String::new());
    }
    json_cpython::py_str(value)
        .map(|rendered| rendered.to_lossy_string())
        .map_err(|_| server_error())
}

/// The raw (unstripped) body `workspace` for
/// [`manage_reads::request_workspace_id`]: falsy maps to `None` (the
/// or-chain falls through to the query value), strings pass through,
/// truthy non-strings are the source's `AttributeError` → 500.
fn body_opt_raw(value: Option<&JVal>) -> Result<Option<String>, Response> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !j_truthy(value) {
        return Ok(None);
    }
    match value {
        JVal::Str(text) => Ok(Some(text.to_lossy_string())),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Array(_) | JVal::Object(_) => {
            Err(server_error())
        }
    }
}

// ---------------------------------------------------------------------------
// Gates (rows + scope checks over the merged builders)
// ---------------------------------------------------------------------------

/// The runner facts the delete gate needs (`r_cols::COLUMNS`
/// positions: id 0, owner 1, workspace 2, visibility 8, revoked 28).
struct RunnerGate {
    id: Uuid,
    owner_id: Uuid,
    workspace_id: Uuid,
    visibility: i16,
    revoked_at: Option<DateTime<Utc>>,
}

fn decode_runner_gate(row: &sqlx::postgres::PgRow) -> Result<RunnerGate, Response> {
    Ok(RunnerGate {
        id: row.try_get(0).map_err(|_| server_error())?,
        owner_id: row.try_get(1).map_err(|_| server_error())?,
        workspace_id: row.try_get(2).map_err(|_| server_error())?,
        visibility: row.try_get(8).map_err(|_| server_error())?,
        revoked_at: row.try_get(28).map_err(|_| server_error())?,
    })
}

/// The R2 read plus the view gate (`runners.py:442-445`): missing
/// row (or present-but-unviewable) → 404 `not found`. Unlike the
/// manage.rs `get_runner` twin there is NO membership arm — Python's
/// `delete` never calls `_get_runner`, whose `is_workspace_member` →
/// 403 arm is get/patch-only, so a non-member owner still deletes.
/// The member query runs only for a present row; its role feeds the
/// manage gate's admin arm.
async fn get_runner(
    pool: &PgPool,
    user_id: Uuid,
    runner_id: Uuid,
) -> Result<(RunnerGate, Option<i32>), Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&manage_reads::runner_detail_sql())
        .bind(runner_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(not_found());
    };
    let runner = decode_runner_gate(&row)?;
    let role = workspace_role(pool, runner.workspace_id, user_id).await?;
    let can_view = runner_perm::can_view_runner(&runner_perm::RunnerFacts {
        workspace: WorkspaceId::from(runner.workspace_id.to_string()),
        authenticated: true,
        visibility: i32::from(runner.visibility),
        owned_by_requester: runner.owner_id == user_id,
    });
    if !can_view {
        return Err(not_found());
    }
    Ok((runner, role))
}

/// `can_manage_runner` (`permissions.py:126-137`) over the resolved
/// row: private runners are owner-managed only (the only reachable
/// arm — non-private rows already fail the view gate).
fn can_manage_gate(runner: &RunnerGate, user_id: Uuid, role: Option<i32>) -> bool {
    let workspace = WorkspaceId::from(runner.workspace_id.to_string());
    let scope = TenantScope::new(workspace.clone());
    runner_perm::can_manage_runner(
        &scope,
        &runner_perm::ManageFacts {
            workspace,
            requester: Some(UserId::from(user_id.to_string())),
            visibility: i32::from(runner.visibility),
            owned_by_requester: runner.owner_id == user_id,
            is_workspace_admin: membership::is_workspace_admin(role),
        },
    )
}

/// The machine facts the scope gates need (`dm_cols::COLUMNS`
/// positions: id 0, owner 1, visibility 4, revoked 7).
struct MachineGate {
    id: Uuid,
    owner_id: Uuid,
    visibility: i16,
    revoked_at: Option<DateTime<Utc>>,
}

fn decode_machine_gate(row: &sqlx::postgres::PgRow) -> Result<MachineGate, Response> {
    Ok(MachineGate {
        id: row.try_get(0).map_err(|_| server_error())?,
        owner_id: row.try_get(1).map_err(|_| server_error())?,
        visibility: row.try_get(4).map_err(|_| server_error())?,
        revoked_at: row.try_get(7).map_err(|_| server_error())?,
    })
}

/// The unlocked machine read plus the M4 scope probes
/// (`machine_commands.py:63-73` minus the revoked arm): miss or
/// out-of-scope → 404 `not found`. The delete endpoint
/// (`runners.py:281-283`) runs this same read with no revoked gate
/// (QUIRK-delete-no-revoked-gate).
async fn scope_machine(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    machine_id: Uuid,
) -> Result<MachineGate, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&manage_reads::scoped_machine_read_sql())
        .bind(machine_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(not_found());
    };
    let machine = decode_machine_gate(&row)?;
    // `can_view_dev_machine` reduces to the same private-and-owned
    // kernel as the runner check (the kernel covers both).
    let can_view = runner_perm::can_view_runner(&runner_perm::RunnerFacts {
        workspace: WorkspaceId::from(workspace_id.to_string()),
        authenticated: true,
        visibility: i32::from(machine.visibility),
        owned_by_requester: machine.owner_id == user_id,
    });
    if !can_view {
        return Err(not_found());
    }
    let runner_hit: Option<i32> =
        sqlx::query_scalar(&manage_reads::machine_scope_runner_probe_sql())
            .bind(1i32)
            .bind(machine_id)
            .bind(user_id)
            .bind(0i16)
            .bind(workspace_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
    let token_hit: Option<i32> = sqlx::query_scalar(&manage_reads::machine_scope_token_probe_sql())
        .bind(1i32)
        .bind(machine_id)
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    if runner_hit.is_none() && token_hit.is_none() {
        return Err(not_found());
    }
    Ok(machine)
}

/// `_scoped_machine` (`machine_commands.py:63-73`) in full: the scope
/// read above, plus the 409 `dev_machine_revoked` arm the
/// machine-command endpoints carry.
async fn scoped_machine(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    machine_id: Uuid,
) -> Result<MachineGate, Response> {
    let machine = scope_machine(pool, user_id, workspace_id, machine_id).await?;
    if machine.revoked_at.is_some() {
        return Err(conflict(DEV_MACHINE_REVOKED_BODY));
    }
    Ok(machine)
}

/// The shared web workspace gate: body-or-query `workspace` (stripped,
/// empty → 400), UUID parse (garbage → 500), membership (non-member →
/// 403).
async fn web_workspace(
    pool: &PgPool,
    user_id: Uuid,
    data: &JVal,
    params: &crate::license::QueryMap,
) -> Result<Uuid, Response> {
    let body_raw = body_opt_raw(data_get(data, "workspace")?)?;
    let query_raw = crate::license::query_last(params, "workspace");
    let picked = manage_reads::request_workspace_id(body_raw.as_deref(), query_raw.as_deref());
    if picked.is_empty() {
        return Err(bad_request(WORKSPACE_REQUIRED_BODY));
    }
    let workspace_id = parse_uuid(&picked)?;
    if !membership::is_workspace_member(workspace_role(pool, workspace_id, user_id).await?) {
        return Err(forbidden());
    }
    Ok(workspace_id)
}

// ---------------------------------------------------------------------------
// Handler-owned SQL (reads no provider owns, spelled from verified twins)
// ---------------------------------------------------------------------------

/// Workspace by id (`machine_commands.py:105`):
/// `Workspace.objects.filter(pk=ws).first()` — the E6a
/// [`enroll_reads::workspace_by_slug_sql`] shape (default-manager
/// scope + `-created_at` + `LIMIT 1`) with the `slug` conjunct swapped
/// for the pk. `$1` = workspace id. Miss → 404 `workspace_not_found`.
pub fn workspace_by_id_sql() -> String {
    format!(
        "SELECT {} FROM \"workspaces\" \
         WHERE (\"workspaces\".\"deleted_at\" IS NULL AND \"workspaces\".\"id\" = $1) \
         ORDER BY \"workspaces\".\"created_at\" DESC LIMIT 1",
        enroll_reads::WORKSPACES_SELECT_COLUMNS
            .iter()
            .map(|col| format!("\"workspaces\".\"{col}\""))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Project-exists probe (`machine_commands.py:108`):
/// `Project.objects.filter(workspace_id=ws,
/// identifier=id).exists()` — the M4 exists shape (`SELECT $1 AS "a"
/// … LIMIT 1`, `$1` = const 1) over the E6c
/// [`enroll_reads::project_by_workspace_identifier_sql`] WHERE
/// (scope + Q-sorted conjuncts). `$2` = identifier, `$3` = workspace.
/// False → 404 `project_not_found`.
pub const PROJECT_EXISTS_SQL: &str = "SELECT $1 AS \"a\" FROM \"projects\" \
     WHERE (\"projects\".\"deleted_at\" IS NULL \
     AND \"projects\".\"identifier\" = $2 AND \"projects\".\"workspace_id\" = $3) LIMIT 1";

/// The workspace slug for the create message: the by-id read above,
/// `slug` at its [`enroll_reads::WORKSPACES_SELECT_COLUMNS`] position.
async fn fetch_workspace_slug(pool: &PgPool, workspace_id: Uuid) -> Result<String, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&workspace_by_id_sql())
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(json_response(
            StatusCode::NOT_FOUND,
            WORKSPACE_NOT_FOUND_BODY.to_owned(),
        ));
    };
    let position = enroll_reads::WORKSPACES_SELECT_COLUMNS
        .iter()
        .position(|column| *column == "slug")
        .expect("slug in select columns");
    row.try_get(position).map_err(|_| server_error())
}

/// The project-exists probe above.
async fn project_exists(
    pool: &PgPool,
    workspace_id: Uuid,
    identifier: &str,
) -> Result<bool, Response> {
    let hit: Option<i32> = sqlx::query_scalar(PROJECT_EXISTS_SQL)
        .bind(1i32)
        .bind(identifier)
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    Ok(hit.is_some())
}

// ---------------------------------------------------------------------------
// Command shapes (pure builders; D13-F6 `machine_commands_consts`)
// ---------------------------------------------------------------------------

/// `_VALID_AGENTS` (`machine_commands.py:56-58`), source order
/// (membership only — Python holds a frozenset).
pub const VALID_AGENTS: &[&str] = &[
    "claude-code",
    "codex",
    "cursor-agent",
    "open-claw",
    "grok",
    "muse-code",
];

/// The `agent` default (`machine_commands.py:124`).
pub const DEFAULT_AGENT: &str = "claude-code";

/// `_RESULT_STATUSES` (`machine_commands.py:60`), membership only.
pub const RESULT_STATUSES: &[&str] = &["ok", "error"];

/// The `create_runner` control message (`machine_commands.py:129-140`),
/// keys in source order. The `mid` envelope is added by the D-14
/// `send_to_machine` driver, not here.
#[allow(clippy::too_many_arguments)]
pub fn create_runner_message(
    request_id: &str,
    workspace_slug: &str,
    project: &str,
    pod: &str,
    name: &str,
    working_dir: &str,
    agent: &str,
    model: &str,
    reasoning_effort: &str,
) -> Map<String, Value> {
    let mut message = Map::with_capacity(10);
    message.insert(
        "type".to_string(),
        Value::String("create_runner".to_string()),
    );
    message.insert(
        "request_id".to_string(),
        Value::String(request_id.to_string()),
    );
    message.insert(
        "workspace_slug".to_string(),
        Value::String(workspace_slug.to_string()),
    );
    message.insert("project".to_string(), Value::String(project.to_string()));
    message.insert("pod".to_string(), Value::String(pod.to_string()));
    message.insert("name".to_string(), Value::String(name.to_string()));
    message.insert(
        "working_dir".to_string(),
        Value::String(working_dir.to_string()),
    );
    message.insert("agent".to_string(), Value::String(agent.to_string()));
    message.insert("model".to_string(), Value::String(model.to_string()));
    message.insert(
        "reasoning_effort".to_string(),
        Value::String(reasoning_effort.to_string()),
    );
    message
}

/// The pending marker (`machine_commands.py:150-157`), keys in source
/// order.
pub fn pending_marker(machine_id: &str, requested_at: &str) -> Map<String, Value> {
    let mut marker = Map::with_capacity(3);
    marker.insert("status".to_string(), Value::String("pending".to_string()));
    marker.insert(
        "dev_machine_id".to_string(),
        Value::String(machine_id.to_string()),
    );
    marker.insert(
        "requested_at".to_string(),
        Value::String(requested_at.to_string()),
    );
    marker
}

/// The offline/failed marker overwrites (`machine_commands.py:161-164,
/// 177-180`), keys in source order.
pub fn error_marker(kind: &str, machine_id: &str) -> Map<String, Value> {
    let mut marker = Map::with_capacity(3);
    marker.insert("status".to_string(), Value::String("error".to_string()));
    marker.insert("error".to_string(), Value::String(kind.to_string()));
    marker.insert(
        "dev_machine_id".to_string(),
        Value::String(machine_id.to_string()),
    );
    marker
}

/// The daemon result payload (`machine_commands.py:248-255`), keys in
/// source order. Truncation stays the caller's job (char-based
/// `[:128]` / `[:512]`, the porting-guide trap).
pub fn result_payload(
    status: &str,
    machine_id: &str,
    runner_id: &str,
    runner_name: &str,
    error: &str,
    reported_at: &str,
) -> Map<String, Value> {
    let mut payload = Map::with_capacity(6);
    payload.insert("status".to_string(), Value::String(status.to_string()));
    payload.insert(
        "dev_machine_id".to_string(),
        Value::String(machine_id.to_string()),
    );
    payload.insert(
        "runner_id".to_string(),
        Value::String(runner_id.to_string()),
    );
    payload.insert(
        "runner_name".to_string(),
        Value::String(runner_name.to_string()),
    );
    payload.insert("error".to_string(), Value::String(error.to_string()));
    payload.insert(
        "reported_at".to_string(),
        Value::String(reported_at.to_string()),
    );
    payload
}

/// The marker→machine binding (`machine_commands.py:211,241`):
/// `result.get("dev_machine_id") not in ("", None, str(machine))`
/// 404s. Missing/null/blank passes (QUIRK-binding-blank); any other
/// non-matching value — including non-strings — fails.
pub fn marker_bound_to(marker: &Map<String, Value>, machine_id: &str) -> bool {
    match marker.get("dev_machine_id") {
        None => true,
        Some(Value::Null) => true,
        Some(Value::String(bound)) => bound.is_empty() || bound == machine_id,
        Some(_) => false,
    }
}

/// A stored command result as an object: Python's `.get` on a
/// well-formed non-dict payload is `AttributeError` → 500 (only
/// `None`/corrupt read as unknown, and those never reach here).
pub fn marker_map(result: Value) -> Result<Map<String, Value>, Response> {
    match result {
        Value::Object(map) => Ok(map),
        _ => Err(server_error()),
    }
}

/// The status 200 body (`machine_commands.py:214`):
/// `{"request_id": ..., **result}` with `dev_machine_id` popped —
/// `request_id` first, then the stored keys in stored order, compact.
pub fn status_body(request_id: &Uuid, result: Map<String, Value>) -> String {
    let mut body = Map::with_capacity(result.len() + 1);
    body.insert(
        "request_id".to_string(),
        Value::String(request_id.to_string()),
    );
    for (key, value) in result {
        if key != "dev_machine_id" {
            body.insert(key, value);
        }
    }
    serde_json::to_string(&Value::Object(body)).expect("status body renders")
}

// ---------------------------------------------------------------------------
// Delete executor (the first live `RunnerDeleteStore`: services-C drivers
// over a pool checkout + transaction + Redis client)
// ---------------------------------------------------------------------------

/// A pool-backed [`PubsubStore`](PubsubStore): SQL on pool checkouts,
/// Redis on the shared client. Serves both the delete frames/close
/// (via [`DeleteStore`]) and the create endpoint's `send_to_machine`
/// (directly) — one implementation, no forks.
struct PoolPubsub<'a> {
    pool: &'a PgPool,
    redis: Option<&'a redis::Client>,
    runner: &'a pidash_db::config::RunnerSettings,
}

#[allow(async_fn_in_trait)]
impl PubsubStore for PoolPubsub<'_> {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, OutboxError> {
        runner_outbox::enqueue_for_runner(self.redis, self.pool, self.runner, runner_id, message)
            .await
    }

    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, MachineOutboxError> {
        machine_outbox::enqueue_for_machine(
            self.redis,
            self.pool,
            self.runner,
            dev_machine_id,
            message,
        )
        .await
    }

    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<RunnerSession>, OutboxError> {
        let rows = sqlx::query(pubsub_svc::CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(self.pool)
            .await?;
        rows.iter()
            .map(runner_session::runner_session_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(OutboxError::from)
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), OutboxError> {
        sqlx::query(runner_session::REVOKE_SQL)
            .bind(Utc::now())
            .bind(reason)
            .bind(session_id)
            .execute(self.pool)
            .await
            .map(|_| ())
            .map_err(OutboxError::from)
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), OutboxError> {
        runner_outbox::clear_session_marker(self.redis, &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), OutboxError> {
        runner_outbox::publish_session_eviction(
            self.redis,
            &runner_id.to_string(),
            Some(&old_session_id.to_string()),
            new_session_id,
        )
        .await
    }
}

/// One collected post-commit effect, in driver registration order
/// (finalize publishes first, then per-revoke handoffs → drains →
/// cleanup — the `on_commit` order in `models.py:653-685`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeleteEffect {
    PublishTerminalEffects { run_id: Uuid },
    ApplyTerminalEffectsInline { run_id: Uuid },
    CompleteProjectMoveHandoff { runner_id: Uuid, run_id: Uuid },
    DrainPod { pod_id: Uuid },
    ScheduleStreamCleanup { runner_id: Uuid },
}

/// The live delete store: revoke/delete SQL on the caller's
/// transaction, frames/close/session SQL through [`PoolPubsub`] on the
/// pool (the seam takes `&self` — see the module docs), post-commit
/// effects collected for the drain after commit.
struct DeleteStore<'t, 'c> {
    tx: &'t mut sqlx::Transaction<'c, sqlx::Postgres>,
    pool: &'t PgPool,
    redis: Option<&'t redis::Client>,
    runner: &'t pidash_db::config::RunnerSettings,
    effects: Vec<DeleteEffect>,
    /// The runner whose nested revoke is running (set by S1, read by
    /// the handoff collector for its log line).
    current_runner: Option<Uuid>,
    /// Runners whose S2 ran on this transaction (set by
    /// [`RevokeStore::revoke_active_sessions`], read by
    /// [`delete_svc::RunnerDeleteStore::close_runner_session`]): their
    /// pool close is skipped — Python's in-tx re-select provably finds
    /// nothing there, while a pool re-select would re-see the
    /// uncommitted rows and self-deadlock on S2's row locks.
    s2_revoked: Vec<Uuid>,
    ports: LivePorts,
}

fn store_err(error: impl std::fmt::Display) -> RevokeError {
    RevokeError::Store(error.to_string())
}

/// Bind one finalize `SET` clause (the run_endpoints `bind_finalize_value`
/// precedent): `Now` takes the call's instant, `Null` binds typed
/// by column.
fn bind_finalize_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    column: &str,
    value: &pidash_services::runner_runs::SetValue,
    now: &DateTime<Utc>,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    use pidash_services::runner_runs::SetValue;
    match (column, value) {
        (_, SetValue::Now) => query.bind(*now),
        (_, SetValue::Text(text)) => query.bind(text.clone()),
        (_, SetValue::Json(payload)) => query.bind(payload.clone()),
        ("queue_position", SetValue::Null) => query.bind(None::<i16>),
        (_, SetValue::Null) => query.bind(None::<DateTime<Utc>>),
    }
}

#[allow(async_fn_in_trait)]
impl RevokeStore for DeleteStore<'_, '_> {
    async fn mark_runner_revoked(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        stored_reason: &str,
    ) -> Result<(), RevokeError> {
        self.current_runner = Some(runner_id);
        sqlx::query(revoke::MARK_RUNNER_REVOKED_SQL)
            .bind(now)
            .bind(stored_reason)
            .bind(runner_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn revoke_active_sessions(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        stored_reason: &str,
    ) -> Result<(), RevokeError> {
        sqlx::query(revoke::REVOKE_ACTIVE_SESSIONS_SQL)
            .bind(now)
            .bind(stored_reason)
            .bind(runner_id)
            .execute(&mut **self.tx)
            .await
            .map_err(store_err)?;
        // S2 ran: every active session for this runner is revoked on
        // the tx, so the driver's later `close_runner_session` must
        // skip the pool close (see its comment).
        if !self.s2_revoked.contains(&runner_id) {
            self.s2_revoked.push(runner_id);
        }
        Ok(())
    }

    async fn lock_active_runs(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<(Uuid, Option<Uuid>)>, RevokeError> {
        let rows = sqlx::query(&revoke::active_runs_lock_sql())
            .bind(runner_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(store_err)?;
        rows.iter()
            .map(|row| {
                let run_id: Uuid = row.try_get(0).map_err(store_err)?;
                let pod_id: Option<Uuid> = row.try_get(1).map_err(store_err)?;
                Ok((run_id, pod_id))
            })
            .collect()
    }

    async fn finalize_cancelled_run(
        &mut self,
        run_id: Uuid,
        runner_id: Uuid,
        stored_reason: &str,
    ) -> Result<(), RevokeError> {
        use pidash_services::runner_runs::SetValue;
        // First-writer-wins lock with the expected-runner predicate; a
        // miss is `Ok(())` — Python ignores the `False`.
        let locked: Option<sqlx::postgres::PgRow> =
            sqlx::query(&finalize_kernel::lock_run_for_finalize_sql(true, false))
                .bind(run_id)
                .bind(TERMINAL_RUN_STATUSES[0].value())
                .bind(TERMINAL_RUN_STATUSES[1].value())
                .bind(TERMINAL_RUN_STATUSES[2].value())
                .bind(TERMINAL_RUN_STATUSES[3].value())
                .bind(TERMINAL_RUN_STATUSES[4].value())
                .bind(runner_id)
                .fetch_optional(&mut **self.tx)
                .await
                .map_err(store_err)?;
        let Some(locked) = locked else {
            return Ok(());
        };
        let executor_at = pidash_db::runner_runs::agent_run::COLUMNS
            .iter()
            .position(|column| *column == "executor_kind")
            .expect("executor_kind in agent_run columns");
        let executor_kind: String = locked.try_get(executor_at).map_err(store_err)?;
        let values = finalize_kernel::plan_finalize_values(
            AgentRunStatus::Cancelled,
            &[
                ("error", SetValue::Text(revoke::FINALIZE_ERROR.to_string())),
                (
                    "error_code",
                    SetValue::Text(revoke::FINALIZE_ERROR_CODE.to_string()),
                ),
                ("cancel_reason", SetValue::Text(stored_reason.to_string())),
            ],
        )
        .map_err(store_err)?;
        // No `done_payload` merge: these updates carry none.
        debug_assert!(
            !values
                .clauses
                .iter()
                .any(|clause| clause.column == "done_payload"),
            "revoke finalize carries no done_payload"
        );
        let now = Utc::now();
        let update_sql = finalize_kernel::finalize_update_sql(&values);
        let mut update = sqlx::query(&update_sql);
        for clause in &values.clauses {
            update = bind_finalize_value(update, clause.column, &clause.value, &now);
        }
        update
            .bind(run_id)
            .execute(&mut **self.tx)
            .await
            .map_err(store_err)?;
        if executor_kind == AgentExecutorKind::CloudAgent.value() {
            let exists: Option<i32> =
                sqlx::query_scalar(finalize_kernel::terminal_event_exists_sql())
                    .bind(run_id)
                    .bind("terminal")
                    .fetch_optional(&mut **self.tx)
                    .await
                    .map_err(store_err)?;
            if exists.is_none() {
                let max_seq: Option<i32> =
                    sqlx::query_scalar(finalize_kernel::terminal_event_max_seq_sql())
                        .bind(run_id)
                        .fetch_optional(&mut **self.tx)
                        .await
                        .map_err(store_err)?
                        .flatten();
                let event = finalize_kernel::plan_terminal_event(
                    max_seq,
                    AgentRunStatus::Cancelled,
                    &finalize_kernel::finalize_error_code(&values),
                );
                sqlx::query(&finalize_kernel::terminal_event_insert_sql())
                    .bind(run_id)
                    .bind(event.seq)
                    .bind("terminal")
                    .bind(&event.payload)
                    .bind(Utc::now())
                    .execute(&mut **self.tx)
                    .await
                    .map_err(store_err)?;
            }
        }
        // On success only — the miss above returns before this.
        for effect in finalize_kernel::plan_publish_effects(run_id) {
            match effect {
                pidash_services::runner_runs::LifecycleEffect::PublishTerminalEffects {
                    run_id,
                } => self
                    .effects
                    .push(DeleteEffect::PublishTerminalEffects { run_id }),
                pidash_services::runner_runs::LifecycleEffect::ApplyTerminalEffectsInline {
                    run_id,
                } => self
                    .effects
                    .push(DeleteEffect::ApplyTerminalEffectsInline { run_id }),
                _ => {}
            }
        }
        Ok(())
    }

    async fn pinned_queued_pod_ids(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<Option<Uuid>>, RevokeError> {
        sqlx::query_scalar(revoke::PINNED_QUEUED_PODS_SQL)
            .bind(runner_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(store_err)
    }

    async fn unpin_queued_runs(&mut self, runner_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(revoke::UNPIN_QUEUED_RUNS_SQL)
            .bind(runner_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    fn complete_handoff_after_commit(&mut self, run_id: Uuid) {
        if let Some(runner_id) = self.current_runner {
            self.effects
                .push(DeleteEffect::CompleteProjectMoveHandoff { runner_id, run_id });
        }
    }

    fn drain_pod_after_commit(&mut self, pod_id: Uuid) {
        self.effects.push(DeleteEffect::DrainPod { pod_id });
    }

    fn schedule_stream_cleanup_after_commit(&mut self, runner_id: Uuid) {
        self.effects
            .push(DeleteEffect::ScheduleStreamCleanup { runner_id });
    }
}

#[allow(async_fn_in_trait)]
impl delete_svc::RunnerDeleteStore for DeleteStore<'_, '_> {
    async fn send_revoke_frame(
        &mut self,
        runner_id: Uuid,
        reason: &str,
    ) -> Result<Vec<String>, RevokeError> {
        let shared = PoolPubsub {
            pool: self.pool,
            redis: self.redis,
            runner: self.runner,
        };
        Ok(pubsub_svc::send_runner_revoke(&shared, runner_id, reason)
            .await
            .warnings)
    }

    async fn send_remove_frame(
        &mut self,
        runner_id: Uuid,
        reason: &str,
    ) -> Result<Vec<String>, RevokeError> {
        let shared = PoolPubsub {
            pool: self.pool,
            redis: self.redis,
            runner: self.runner,
        };
        Ok(pubsub_svc::send_runner_remove(&shared, runner_id, reason)
            .await
            .warnings)
    }

    async fn close_runner_session(&mut self, runner_id: Uuid) -> Result<(), RevokeError> {
        // S2 already revoked every active session for this runner on
        // the tx: Python's in-tx re-select (`runner_delete.py:91-94`
        // tail) finds none and no-ops — reproduced here by skipping.
        // A pool close would be wrong twice over: its SELECT cannot
        // see S2's uncommitted revokes, and its per-session UPDATE
        // would block on S2's row locks while the tx awaits close
        // (self-deadlock; no lock timeout is configured). Runners
        // whose nested revoke early-returned (already revoked, no S2)
        // keep the real close — the tx holds no locks on their
        // session rows, and Python genuinely processes any actives.
        if self.s2_revoked.contains(&runner_id) {
            return Ok(());
        }
        let shared = PoolPubsub {
            pool: self.pool,
            redis: self.redis,
            runner: self.runner,
        };
        pubsub_svc::close_runner_session(
            &shared,
            runner_id,
            pubsub_svc::CLOSE_RUNNER_SESSION_DEFAULT_CODE,
        )
        .await
        .map_err(store_err)
    }

    async fn revoke_machine_tokens(
        &mut self,
        machine_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), RevokeError> {
        sqlx::query(delete_svc::REVOKE_MACHINE_TOKENS_SQL)
            .bind(now)
            .bind(machine_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn lock_machine_runners(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<delete_svc::LockedRunner>, RevokeError> {
        let rows = sqlx::query(delete_svc::LOCK_MACHINE_RUNNERS_SQL)
            .bind(machine_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(store_err)?;
        rows.iter()
            .map(|row| {
                Ok(delete_svc::LockedRunner {
                    id: row.try_get(0).map_err(store_err)?,
                    revoked_at: row.try_get(28).map_err(store_err)?,
                })
            })
            .collect()
    }

    async fn fetch_runner_for_delete(&mut self, runner_id: Uuid) -> Result<bool, RevokeError> {
        sqlx::query(delete_svc::COLLECT_RUNNER_BY_ID_SQL)
            .bind(runner_id)
            .fetch_optional(&mut **self.tx)
            .await
            .map(|row| row.is_some())
            .map_err(store_err)
    }

    async fn fetch_machine_runner_ids_for_delete(
        &mut self,
        machine_id: Uuid,
    ) -> Result<Vec<Uuid>, RevokeError> {
        let rows = sqlx::query(delete_svc::COLLECT_RUNNERS_BY_MACHINE_SQL)
            .bind(machine_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(store_err)?;
        rows.iter()
            .map(|row| row.try_get(0).map_err(store_err))
            .collect()
    }

    async fn fetch_chat_ids(&mut self, runner_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError> {
        let sql = delete_svc::collect_chat_ids_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        let rows = query.fetch_all(&mut **self.tx).await.map_err(store_err)?;
        rows.iter()
            .map(|row| row.try_get(0).map_err(store_err))
            .collect()
    }

    async fn fetch_message_ids(&mut self, chat_ids: &[Uuid]) -> Result<Vec<Uuid>, RevokeError> {
        let sql = delete_svc::collect_message_ids_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        let rows = query.fetch_all(&mut **self.tx).await.map_err(store_err)?;
        rows.iter()
            .map(|row| row.try_get(0).map_err(store_err))
            .collect()
    }

    async fn delete_chat_events(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_events_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_chat_approvals(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_approvals_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_chat_dedupes(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_dedupes_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_runner_sessions(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_runner_sessions_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_force_refresh(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_force_refresh_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_live_state(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_live_state_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_run_runners(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::null_run_runners_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_run_pins(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::null_run_pins_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_event_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::null_event_messages_sql(message_ids.len());
        let mut query = sqlx::query(&sql);
        for id in message_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_chat_messages(&mut self, message_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_messages_sql(message_ids.len());
        let mut query = sqlx::query(&sql);
        for id in message_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_chat_sessions(&mut self, chat_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_chat_sessions_sql(chat_ids.len());
        let mut query = sqlx::query(&sql);
        for id in chat_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_runner_rows(&mut self, runner_ids: &[Uuid]) -> Result<(), RevokeError> {
        let sql = delete_svc::delete_runner_rows_sql(runner_ids.len());
        let mut query = sqlx::query(&sql);
        for id in runner_ids {
            query = query.bind(*id);
        }
        query
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn fetch_machine_for_delete(&mut self, machine_id: Uuid) -> Result<bool, RevokeError> {
        sqlx::query(delete_svc::COLLECT_MACHINE_BY_ID_SQL)
            .bind(machine_id)
            .fetch_optional(&mut **self.tx)
            .await
            .map(|row| row.is_some())
            .map_err(store_err)
    }

    async fn delete_machine_sessions(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(delete_svc::DELETE_MACHINE_SESSIONS_SQL)
            .bind(machine_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_runner_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(delete_svc::NULL_RUNNER_MACHINES_SQL)
            .bind(machine_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn null_token_machines(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(delete_svc::NULL_TOKEN_MACHINES_SQL)
            .bind(machine_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }

    async fn delete_machine_row(&mut self, machine_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(delete_svc::DELETE_MACHINE_ROW_SQL)
            .bind(machine_id)
            .execute(&mut **self.tx)
            .await
            .map(|_| ())
            .map_err(store_err)
    }
}

// ---------------------------------------------------------------------------
// Post-commit drain (Python's per-site `on_commit` isolation)
// ---------------------------------------------------------------------------

/// Publish one Celery message through the F-09 AMQP publisher (the
/// run_endpoints `emit_celery` precedent: connect, publish, close per
/// message). Broker errors propagate to the caller, which logs them —
/// every emit site is isolated in Python.
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

/// Drain collected [`DeleteEffect`]s after commit, in registration
/// order, with Python's per-site isolation (`models.py:653-685`,
/// `agent_run_finalization.py:89-103`):
/// * publish / apply / handoff are isolated (log + continue);
/// * drain / cleanup propagate (a failure aborts the rest and 500s —
///   rows already committed).
async fn drain_delete_effects(
    pool: &PgPool,
    ports: &LivePorts,
    redis: Option<&redis::Client>,
    runner: &pidash_db::config::RunnerSettings,
    effects: Vec<DeleteEffect>,
) -> Result<(), Response> {
    for effect in effects {
        match effect {
            DeleteEffect::PublishTerminalEffects { run_id } => {
                if let Err(error) = emit_celery(
                    finalize_kernel::TERMINAL_EFFECTS_TASK,
                    vec![Value::String(run_id.to_string())],
                )
                .await
                {
                    tracing::error!(
                        run_id = %run_id,
                        error = %error,
                        "failed to publish terminal effects for run",
                    );
                }
            }
            DeleteEffect::ApplyTerminalEffectsInline { run_id } => {
                if let Err(error) = execute_terminal_effects(pool, ports, run_id).await {
                    tracing::error!(
                        run_id = %run_id,
                        error = ?error,
                        "failed to apply terminal effects for run",
                    );
                }
            }
            DeleteEffect::CompleteProjectMoveHandoff { runner_id, run_id } => {
                if let Err(error) = ports.complete_project_move_handoff(run_id).await {
                    tracing::error!(
                        error = ?error,
                        "{}",
                        revoke::handoff_failure_log(&runner_id, &run_id),
                    );
                }
            }
            DeleteEffect::DrainPod { pod_id } => {
                ports
                    .drain_pod_by_id(pod_id)
                    .await
                    .map_err(|_| server_error())?;
            }
            DeleteEffect::ScheduleStreamCleanup { runner_id } => {
                runner_outbox::schedule_stream_cleanup_for_runner(
                    redis,
                    runner,
                    &runner_id.to_string(),
                )
                .await
                .map_err(|_| server_error())?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `DELETE /api/runners/<runner_id>/` (`runners.py:427-452`): view /
/// manage gates, `purge_local` parse, the `delete_runner` service, 204.
/// `request.data` is never touched — only the query flag is parsed.
pub async fn runner_delete(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
    Query(params): Query<crate::license::QueryMap>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let runner_id: Uuid = match raw_id.parse() {
        Ok(runner_id) => runner_id,
        Err(_) => return not_found(),
    };
    let (runner, role) = match get_runner(&pool, user_id, runner_id).await {
        Ok(hit) => hit,
        Err(response) => return response,
    };
    if !can_manage_gate(&runner, user_id, role) {
        return forbidden();
    }
    let purge = match pidash_services::runner_enroll::purge::parse_purge_local(
        crate::license::query_last(&params, "purge_local").as_deref(),
        true,
    ) {
        Ok(purge) => purge,
        Err(_) => return bad_request(&purge_flag_body()),
    };
    let redis = redis_client(&state);
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let (effects, ports) = {
        let mut store = DeleteStore {
            tx: &mut tx,
            pool: &pool,
            redis: redis.as_ref(),
            runner: &state.settings().runner,
            effects: Vec::new(),
            current_runner: None,
            s2_revoked: Vec::new(),
            ports: LivePorts::new(pool.clone(), &state),
        };
        match delete_svc::delete_runner(
            &mut store,
            runner.id,
            runner.revoked_at,
            purge,
            now_micros(),
        )
        .await
        {
            // Frame/enqueue warnings Python logs (`pubsub.py`
            // `logger.warning`/`logger.exception`, `models.py`
            // unknown-reason warning).
            Ok(outcome) => {
                for warning in &outcome.warnings {
                    tracing::warn!(warning = %warning, "delete_runner driver warning");
                }
            }
            Err(_) => return server_error(),
        }
        (std::mem::take(&mut store.effects), store.ports)
    };
    if tx.commit().await.is_err() {
        return server_error();
    }
    if let Err(response) = drain_delete_effects(
        &pool,
        &ports,
        redis.as_ref(),
        &state.settings().runner,
        effects,
    )
    .await
    {
        return response;
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `DELETE /api/runners/dev-machines/<machine_id>/`
/// (`runners.py:249-286`): workspace gate, scope check, `purge_local`
/// parse, the `delete_dev_machine` service, 204. The workspace id
/// reads body-first (so the body parses even on this DELETE).
pub async fn machine_delete(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
    Query(params): Query<crate::license::QueryMap>,
    req: Request,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let machine_id: Uuid = match raw_id.parse() {
        Ok(machine_id) => machine_id,
        Err(_) => return not_found(),
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let workspace_id = match web_workspace(&pool, user_id, &data, &params).await {
        Ok(workspace_id) => workspace_id,
        Err(response) => return response,
    };
    let purge = match pidash_services::runner_enroll::purge::parse_purge_local(
        crate::license::query_last(&params, "purge_local").as_deref(),
        true,
    ) {
        Ok(purge) => purge,
        Err(_) => return bad_request(&purge_flag_body()),
    };
    // No revoked gate here (QUIRK-delete-no-revoked-gate).
    let machine = match scope_machine(&pool, user_id, workspace_id, machine_id).await {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let redis = redis_client(&state);
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let (effects, ports) = {
        let mut store = DeleteStore {
            tx: &mut tx,
            pool: &pool,
            redis: redis.as_ref(),
            runner: &state.settings().runner,
            effects: Vec::new(),
            current_runner: None,
            s2_revoked: Vec::new(),
            ports: LivePorts::new(pool.clone(), &state),
        };
        // Python evaluates `timezone.now()` per nested `runner.revoke()`.
        let mut revoke_clock = || now_micros();
        match delete_svc::delete_dev_machine(
            &mut store,
            machine.id,
            purge,
            now_micros(),
            &mut revoke_clock,
        )
        .await
        {
            // Frame/enqueue warnings Python logs (same provenance as
            // the runner-delete arm above).
            Ok(outcome) => {
                for warning in &outcome.warnings {
                    tracing::warn!(warning = %warning, "delete_dev_machine driver warning");
                }
            }
            Err(_) => return server_error(),
        }
        (std::mem::take(&mut store.effects), store.ports)
    };
    if tx.commit().await.is_err() {
        return server_error();
    }
    if let Err(response) = drain_delete_effects(
        &pool,
        &ports,
        redis.as_ref(),
        &state.settings().runner,
        effects,
    )
    .await
    {
        return response;
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /api/runners/dev-machines/<machine_id>/create-runner/`
/// (`machine_commands.py:76-186`): workspace gate, scope check,
/// project/agent/name validation, pending marker, `send_to_machine`
/// with the 409/503 arms, 202 `request_id`.
pub async fn machine_create_runner(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
    Query(params): Query<crate::license::QueryMap>,
    req: Request,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let machine_id: Uuid = match raw_id.parse() {
        Ok(machine_id) => machine_id,
        Err(_) => return not_found(),
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let workspace_id = match web_workspace(&pool, user_id, &data, &params).await {
        Ok(workspace_id) => workspace_id,
        Err(response) => return response,
    };
    let machine = match scoped_machine(&pool, user_id, workspace_id, machine_id).await {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    let project = match or_empty_stripped(match data_get(&data, "project") {
        Ok(project) => project,
        Err(response) => return response,
    }) {
        Ok(project) => project,
        Err(response) => return response,
    };
    if project.is_empty() {
        return bad_request(PROJECT_REQUIRED_BODY);
    }
    let workspace_slug = match fetch_workspace_slug(&pool, workspace_id).await {
        Ok(slug) => slug,
        Err(response) => return response,
    };
    match project_exists(&pool, workspace_id, &project).await {
        Ok(true) => {}
        Ok(false) => {
            return json_response(StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned());
        }
        Err(response) => return response,
    }
    let name_raw = match or_empty_stripped(match data_get(&data, "name") {
        Ok(name) => name,
        Err(response) => return response,
    }) {
        Ok(name) => name,
        Err(response) => return response,
    };
    let name =
        pidash_services::runner_enroll::queries::catalog_reads::truncate_chars(&name_raw, 128);
    if !name.is_empty() && !shapes::runner_name_is_valid(&name) {
        return bad_request(&invalid_runner_name_body());
    }
    let agent_raw = match or_empty_stripped(match data_get(&data, "agent") {
        Ok(agent) => agent,
        Err(response) => return response,
    }) {
        Ok(agent) => agent,
        Err(response) => return response,
    };
    let agent = if agent_raw.is_empty() {
        DEFAULT_AGENT.to_string()
    } else {
        agent_raw
    };
    if !VALID_AGENTS.contains(&agent.as_str()) {
        return bad_request(INVALID_AGENT_BODY);
    }
    // `pod` is NOT validated (QUIRK-pod-passthrough).
    let pod = match or_empty_stripped(match data_get(&data, "pod") {
        Ok(pod) => pod,
        Err(response) => return response,
    }) {
        Ok(pod) => pod,
        Err(response) => return response,
    };
    let working_dir = match or_empty_stripped(match data_get(&data, "working_dir") {
        Ok(working_dir) => working_dir,
        Err(response) => return response,
    }) {
        Ok(working_dir) => working_dir,
        Err(response) => return response,
    };
    let model = match or_empty_stripped(match data_get(&data, "model") {
        Ok(model) => model,
        Err(response) => return response,
    }) {
        Ok(model) => model,
        Err(response) => return response,
    };
    let reasoning_effort = match or_empty_stripped(match data_get(&data, "reasoning_effort") {
        Ok(reasoning_effort) => reasoning_effort,
        Err(response) => return response,
    }) {
        Ok(reasoning_effort) => reasoning_effort,
        Err(response) => return response,
    };
    let request_id = Uuid::new_v4();
    let machine_id_str = machine.id.to_string();
    let message = create_runner_message(
        &request_id.to_string(),
        &workspace_slug,
        &project,
        &pod,
        &name,
        &working_dir,
        &agent,
        &model,
        &reasoning_effort,
    );
    let redis = redis_client(&state);
    let pending = pending_marker(&machine_id_str, &django_isoformat(now_micros()));
    if machine_outbox::set_command_result(redis.as_ref(), &request_id.to_string(), &pending)
        .await
        .is_err()
    {
        return server_error();
    }
    let shared = PoolPubsub {
        pool: &pool,
        redis: redis.as_ref(),
        runner: &state.settings().runner,
    };
    let delivered = match pubsub_svc::send_to_machine(&shared, machine.id, &message).await {
        Err(MachineOutboxError::MachineOffline { .. }) => {
            let marker = error_marker("machine_offline", &machine_id_str);
            if machine_outbox::set_command_result(redis.as_ref(), &request_id.to_string(), &marker)
                .await
                .is_err()
            {
                return server_error();
            }
            return conflict(MACHINE_OFFLINE_BODY);
        }
        Ok(delivered) => delivered,
        Err(other) => {
            // QUIRK-offline-None: a generic failure logs and falls into
            // the `None` arm below (503), exactly like the source's
            // `except Exception: delivered = None`.
            tracing::error!(
                machine_id = %machine_id_str,
                error = %other,
                "create_runner enqueue failed",
            );
            None
        }
    };
    if delivered.is_none() {
        let marker = error_marker("delivery_failed", &machine_id_str);
        if machine_outbox::set_command_result(redis.as_ref(), &request_id.to_string(), &marker)
            .await
            .is_err()
        {
            return server_error();
        }
        return unavailable(DELIVERY_FAILED_BODY);
    }
    json_response(
        StatusCode::ACCEPTED,
        format!("{{\"request_id\":\"{request_id}\"}}"),
    )
}

/// `GET /api/runners/dev-machines/<machine_id>/create-runner/<request_id>/`
/// (`machine_commands.py:189-214`): workspace gate, scope check, the
/// bound status read, 200. The workspace id reads body-first, so the
/// (usually absent) GET body parses too.
pub async fn machine_create_runner_status(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path((raw_mid, raw_rid)): Path<(String, String)>,
    Query(params): Query<crate::license::QueryMap>,
    req: Request,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let machine_id: Uuid = match raw_mid.parse() {
        Ok(machine_id) => machine_id,
        Err(_) => return not_found(),
    };
    // A non-UUID request id never reaches the view in Django (the
    // `<uuid:>` converter 404s); the resolver analog here is the same
    // view 404 as a bad machine id.
    let request_id: Uuid = match raw_rid.parse() {
        Ok(request_id) => request_id,
        Err(_) => return not_found(),
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let workspace_id = match web_workspace(&pool, user_id, &data, &params).await {
        Ok(workspace_id) => workspace_id,
        Err(response) => return response,
    };
    match scoped_machine(&pool, user_id, workspace_id, machine_id).await {
        Ok(_) => {}
        Err(response) => return response,
    }
    let redis = redis_client(&state);
    let stored =
        match machine_outbox::get_command_result(redis.as_ref(), &request_id.to_string()).await {
            Ok(stored) => stored,
            Err(_) => return server_error(),
        };
    let Some(stored) = stored else {
        return json_response(StatusCode::NOT_FOUND, UNKNOWN_REQUEST_BODY.to_owned());
    };
    let marker = match marker_map(stored) {
        Ok(marker) => marker,
        Err(response) => return response,
    };
    // Bound against the URL machine id (the source compares
    // `str(machine_id)` — the kwarg, not the row).
    if !marker_bound_to(&marker, &machine_id.to_string()) {
        return json_response(StatusCode::NOT_FOUND, UNKNOWN_REQUEST_BODY.to_owned());
    }
    json_response(StatusCode::OK, status_body(&request_id, marker))
}

/// `_auth_dev_machine` (`machine_sessions.py:54-66`) as data (the
/// machine.rs precedent): the URL machine id iff the presented token
/// is bound to it, else `None`.
fn authed_machine_id(auth: Option<&MachineAuth>, url_id: Uuid) -> Option<Uuid> {
    match auth {
        Some(auth) if auth.token.dev_machine_id == Some(url_id) => Some(url_id),
        _ => None,
    }
}

fn dev_machine_mismatch() -> Response {
    let denial = dev_machine_mismatch_drf();
    let status = StatusCode::from_u16(denial.status).unwrap_or(StatusCode::FORBIDDEN);
    json_response(status, denial.body)
}

/// `POST /api/v1/runner/dev-machines/<mid>/commands/<rid>/result/`
/// (`machine_commands.py:217-257`): machine-token auth plus the
/// `_auth_dev_machine` binding, the pending-marker binding, the status
/// allow-list, the result write, 204. The body parses only after both
/// guards — an unknown marker beats a malformed body.
pub async fn machine_command_result(
    State(state): State<AppState>,
    Path((raw_mid, raw_rid)): Path<(String, String)>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let dev_machine_id: Uuid = match raw_mid.parse() {
        Ok(dev_machine_id) => dev_machine_id,
        Err(_) => return page_not_found(),
    };
    let request_id: Uuid = match raw_rid.parse() {
        Ok(request_id) => request_id,
        Err(_) => return page_not_found(),
    };
    let secret = state.settings().secret_key.clone();
    let auth =
        match authenticate_machine_token(&pool, secret.as_bytes(), &headers, ALLOW_POST).await {
            Ok(auth) => auth,
            // The D-13 denial already renders the byte-correct DRF 401
            // (lowercase `detail` + `Bearer` challenge + this view's `Allow`);
            // pass it through.
            Err(response) => return response,
        };
    if authed_machine_id(auth.as_ref(), dev_machine_id).is_none() {
        return dev_machine_mismatch();
    }
    let redis = redis_client(&state);
    let stored =
        match machine_outbox::get_command_result(redis.as_ref(), &request_id.to_string()).await {
            Ok(stored) => stored,
            Err(_) => return server_error(),
        };
    let Some(stored) = stored else {
        return json_response(StatusCode::NOT_FOUND, UNKNOWN_REQUEST_BODY.to_owned());
    };
    let marker = match marker_map(stored) {
        Ok(marker) => marker,
        Err(response) => return response,
    };
    if !marker_bound_to(&marker, &dev_machine_id.to_string()) {
        return json_response(StatusCode::NOT_FOUND, UNKNOWN_REQUEST_BODY.to_owned());
    }
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let status = match py_string(match data_get(&data, "status") {
        Ok(status) => status,
        Err(response) => return response,
    }) {
        Ok(status) => status,
        Err(response) => return response,
    };
    if !RESULT_STATUSES.contains(&status.as_str()) {
        return bad_request(INVALID_STATUS_BODY);
    }
    let runner_id = match py_string(match data_get(&data, "runner_id") {
        Ok(runner_id) => runner_id,
        Err(response) => return response,
    }) {
        Ok(runner_id) => runner_id,
        Err(response) => return response,
    };
    let runner_name_raw = match py_string(match data_get(&data, "runner_name") {
        Ok(runner_name) => runner_name,
        Err(response) => return response,
    }) {
        Ok(runner_name) => runner_name,
        Err(response) => return response,
    };
    let error_raw = match py_string(match data_get(&data, "error") {
        Ok(error) => error,
        Err(response) => return response,
    }) {
        Ok(error) => error,
        Err(response) => return response,
    };
    let runner_name = pidash_services::runner_enroll::queries::catalog_reads::truncate_chars(
        &runner_name_raw,
        128,
    );
    let error =
        pidash_services::runner_enroll::queries::catalog_reads::truncate_chars(&error_raw, 512);
    let payload = result_payload(
        &status,
        &dev_machine_id.to_string(),
        &runner_id,
        &runner_name,
        &error,
        &django_isoformat(now_micros()),
    );
    if machine_outbox::set_command_result(redis.as_ref(), &request_id.to_string(), &payload)
        .await
        .is_err()
    {
        return server_error();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// An owned path: the listed methods serve from Rust, every other
/// method falls through to Django (the manage.rs precedent, twinned —
/// `HEAD` routes explicitly so Django's 405 survives byte for byte).
fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the web deletes + machine-command routes
/// (`runner/web_urls.py`: dev-machine delete, create-runner post, the
/// create-runner status get). Merged under `RouteGroup::RunnerWeb` at
/// the F-10 seam; the runner-detail DELETE joins the sibling
/// `manage::routes` registration for its path (axum merges
/// same-path `MethodRouter`s, so a second registration would collide
/// on the doubly-defined DELETE).
pub fn web_routes() -> Router<AppState> {
    use axum::routing::{delete, get, post};
    const DELETE_ONLY: &[&str] = &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"];
    const POST_ONLY: &[&str] = &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    const GET_ONLY: &[&str] = &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    Router::new()
        .route(
            "/api/runners/dev-machines/{machine_id}/",
            owned(delete(machine_delete), DELETE_ONLY),
        )
        .route(
            "/api/runners/dev-machines/{machine_id}/create-runner/",
            owned(post(machine_create_runner), POST_ONLY),
        )
        .route(
            "/api/runners/dev-machines/{machine_id}/create-runner/{request_id}/",
            owned(get(machine_create_runner_status), GET_ONLY),
        )
}

/// Register the daemon result route (`runner/urls.py`: the
/// machine-command result post). Merged under `RouteGroup::Runner` at
/// the F-10 seam.
pub fn daemon_routes() -> Router<AppState> {
    use axum::routing::post;
    const POST_ONLY: &[&str] = &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    Router::new().route(
        "/api/v1/runner/dev-machines/{dev_machine_id}/commands/{request_id}/result/",
        owned(post(machine_command_result), POST_ONLY),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt as _;

    use crate::edge::EdgeHandle;

    const FIXTURE_ENDPOINTS: &str =
        include_str!("../../../../fixtures/runner_enroll/handlers/endpoints.golden.json");
    const FIXTURE_FLOWS: &str =
        include_str!("../../../../fixtures/runner_enroll/services/flows.golden.json");
    const FIXTURE_WIRE: &str =
        include_str!("../../../../fixtures/runner_enroll/external/wire_pins.json");

    fn endpoints() -> serde_json::Value {
        serde_json::from_str(FIXTURE_ENDPOINTS).expect("endpoints.golden.json parses")
    }

    fn flows() -> serde_json::Value {
        serde_json::from_str(FIXTURE_FLOWS).expect("flows.golden.json parses")
    }

    fn wire() -> serde_json::Value {
        serde_json::from_str(FIXTURE_WIRE).expect("wire_pins.json parses")
    }

    fn web_fixture(key: &str) -> serde_json::Value {
        endpoints()["web"][key].clone()
    }

    /// The `errors` array of a golden entry as `(status, compact body)` in
    /// fixture order. `...` bodies are prefix-pinned by the caller.
    fn golden_errors(entry: &serde_json::Value) -> Vec<(u16, String)> {
        entry["errors"]
            .as_array()
            .expect("errors array")
            .iter()
            .map(|case| {
                (
                    case["status"].as_u64().expect("status") as u16,
                    serde_json::to_string(&case["body"]).expect("body renders"),
                )
            })
            .collect()
    }

    // -- D13-F7 endpoint bodies ----------------------------------------------

    #[test]
    fn delete_runner_bodies_match_f7() {
        let entry = web_fixture("DELETE_runners_rid");
        assert_eq!(entry["route"], "DELETE /api/runners/<uuid:runner_id>/");
        assert_eq!(entry["source"], "runners.py:427-452");
        let errors = golden_errors(&entry);
        assert_eq!(
            errors,
            vec![
                (404, NOT_FOUND_BODY.to_string()),
                (403, FORBIDDEN_BODY.to_string()),
                (
                    400,
                    r#"{"error":"purge_local must be one of: ..."}"#.to_string()
                ),
            ]
        );
        assert!(purge_flag_body().starts_with(r#"{"error":"purge_local must be one of: "#));
    }

    #[test]
    fn delete_machine_bodies_match_f7() {
        let entry = web_fixture("DELETE_dev_machines_mid");
        assert_eq!(
            entry["route"],
            "DELETE /api/runners/dev-machines/<uuid:machine_id>/"
        );
        assert_eq!(entry["source"], "runners.py:249-286");
        let errors = golden_errors(&entry);
        assert_eq!(errors.len(), 4, "gate order pinned: {errors:?}");
        assert_eq!(errors[0], (400, WORKSPACE_REQUIRED_BODY.to_string()));
        assert_eq!(errors[1], (403, FORBIDDEN_BODY.to_string()));
        assert_eq!(errors[2].0, 400);
        assert!(errors[2].1.contains("purge_local must be one of: "));
        assert_eq!(errors[3], (404, NOT_FOUND_BODY.to_string()));
    }

    #[test]
    fn create_runner_bodies_match_f7() {
        let entry = web_fixture("POST_dev_machines_create_runner");
        assert_eq!(
            entry["route"],
            "POST /api/runners/dev-machines/<uuid:machine_id>/create-runner/"
        );
        assert_eq!(entry["source"], "machine_commands.py:76-186");
        let errors = golden_errors(&entry);
        let expected: Vec<(u16, String)> = vec![
            (400, WORKSPACE_REQUIRED_BODY.to_string()),
            (403, FORBIDDEN_BODY.to_string()),
            (404, NOT_FOUND_BODY.to_string()),
            (409, DEV_MACHINE_REVOKED_BODY.to_string()),
            (400, PROJECT_REQUIRED_BODY.to_string()),
            (404, WORKSPACE_NOT_FOUND_BODY.to_string()),
            (404, PROJECT_NOT_FOUND_BODY.to_string()),
            (
                400,
                r#"{"error":"invalid_runner_name","error_description":"name must start with ..."}"#
                    .to_string(),
            ),
            (400, INVALID_AGENT_BODY.to_string()),
            (409, MACHINE_OFFLINE_BODY.to_string()),
            (503, DELIVERY_FAILED_BODY.to_string()),
        ];
        assert_eq!(errors, expected);
        // The golden truncates the description; the full text pins here.
        assert_eq!(
            invalid_runner_name_body(),
            r#"{"error":"invalid_runner_name","error_description":"name must start with a letter, digit, or underscore and contain only letters, digits, underscore, dot, or dash"}"#
        );
        assert_eq!(
            serde_json::to_string(&flows()["invalid_runner_name_body"]["body"]).unwrap(),
            invalid_runner_name_body()
        );
    }

    #[test]
    fn create_status_bodies_match_f7() {
        let entry = web_fixture("GET_machine_create_runner_status");
        assert_eq!(
            entry["route"],
            "GET /api/runners/dev-machines/<uuid:machine_id>/create-runner/<uuid:request_id>/"
        );
        assert_eq!(entry["source"], "machine_commands.py:189-214");
        assert_eq!(
            golden_errors(&entry),
            vec![
                (400, WORKSPACE_REQUIRED_BODY.to_string()),
                (403, FORBIDDEN_BODY.to_string()),
                (404, NOT_FOUND_BODY.to_string()),
                (409, DEV_MACHINE_REVOKED_BODY.to_string()),
                (404, UNKNOWN_REQUEST_BODY.to_string()),
            ]
        );
    }

    #[test]
    fn command_result_bodies_match_f7() {
        let entry = endpoints()["daemon"]["POST_machine_command_result"].clone();
        assert_eq!(
            entry["route"],
            "POST /api/v1/runner/dev-machines/<uuid:dev_machine_id>/commands/<uuid:request_id>/result/"
        );
        assert_eq!(entry["source"], "machine_commands.py:217-257");
        assert_eq!(
            golden_errors(&entry),
            vec![
                (403, r#"{"error":"dev_machine_mismatch"}"#.to_string()),
                (404, UNKNOWN_REQUEST_BODY.to_string()),
                (400, INVALID_STATUS_BODY.to_string()),
            ]
        );
        let denial = dev_machine_mismatch_drf();
        assert_eq!(denial.status, 403);
        assert_eq!(denial.body, r#"{"error":"dev_machine_mismatch"}"#);
    }

    // -- D13-F6 service goldens ------------------------------------------------

    #[test]
    fn agent_allow_list_matches_f6() {
        let fixture_doc = flows();
        let fixture = fixture_doc["machine_commands_consts"]["_VALID_AGENTS"]
            .as_array()
            .expect("agents array");
        let mut ours = VALID_AGENTS.to_vec();
        ours.sort_unstable();
        let mut theirs: Vec<&str> = fixture
            .iter()
            .map(|agent| agent.as_str().expect("agent str"))
            .collect();
        theirs.sort_unstable();
        assert_eq!(ours, theirs);
        assert_eq!(DEFAULT_AGENT, "claude-code");
        let mut statuses = RESULT_STATUSES.to_vec();
        statuses.sort_unstable();
        assert_eq!(
            statuses,
            vec!["error", "ok"],
            "fixture RESULT_STATUSES order"
        );
        assert_eq!(
            flows()["machine_commands_consts"]["RESULT_STATUSES"],
            serde_json::json!(["error", "ok"])
        );
    }

    #[test]
    fn runner_name_cases_replay_f6() {
        let fixture_doc = flows();
        let cases = fixture_doc["runner_name_re"]["cases"]
            .as_array()
            .expect("cases array");
        assert_eq!(cases.len(), 13, "fixture case count pinned");
        for case in cases {
            let raw = case["name"].as_str().expect("name str");
            let name = match raw {
                "<128 x's>" => "x".repeat(128),
                "<129 x's>" => "x".repeat(129),
                other => other.to_string(),
            };
            assert_eq!(
                shapes::runner_name_is_valid(&name),
                case["match"].as_bool().expect("match bool"),
                "case {raw:?}"
            );
        }
    }

    #[test]
    fn message_and_marker_shapes_match_f6() {
        let message = create_runner_message("r", "s", "p", "po", "n", "w", "a", "m", "e");
        let keys: Vec<&str> = message.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "type",
                "request_id",
                "workspace_slug",
                "project",
                "pod",
                "name",
                "working_dir",
                "agent",
                "model",
                "reasoning_effort"
            ]
        );
        assert!(
            !message.contains_key("mid"),
            "the D-14 driver envelopes, not this builder"
        );
        let fixture_doc = flows();
        let shape = fixture_doc["machine_commands_consts"]["create_message_shape"]
            .as_object()
            .expect("shape object");
        for key in &keys {
            assert!(shape.contains_key(*key), "fixture covers {key}");
        }
        let marker = pending_marker("m", "t");
        let marker_keys: Vec<&str> = marker.keys().map(String::as_str).collect();
        assert_eq!(
            marker_keys,
            vec!["status", "dev_machine_id", "requested_at"]
        );
        let payload = result_payload("ok", "m", "r", "n", "e", "t");
        let payload_keys: Vec<&str> = payload.keys().map(String::as_str).collect();
        assert_eq!(
            payload_keys,
            vec![
                "status",
                "dev_machine_id",
                "runner_id",
                "runner_name",
                "error",
                "reported_at"
            ]
        );
        let failed = error_marker("delivery_failed", "m");
        assert_eq!(
            failed,
            serde_json::json!({"status": "error", "error": "delivery_failed", "dev_machine_id": "m"})
                .as_object()
                .unwrap()
                .clone()
        );
    }

    #[test]
    fn purge_flag_body_matches_source() {
        assert_eq!(
            purge_flag_body(),
            r#"{"error":"purge_local must be one of: true, false, 1, 0, yes, no"}"#
        );
    }

    // -- D13-F5 handler-owned SQL -----------------------------------------------

    #[test]
    fn workspace_by_id_sql_matches_e6a_shape() {
        let sql = workspace_by_id_sql();
        assert_eq!(
            sql,
            format!(
                "SELECT {} FROM \"workspaces\" \
                 WHERE (\"workspaces\".\"deleted_at\" IS NULL AND \"workspaces\".\"id\" = $1) \
                 ORDER BY \"workspaces\".\"created_at\" DESC LIMIT 1",
                enroll_reads::WORKSPACES_SELECT_COLUMNS
                    .iter()
                    .map(|col| format!("\"workspaces\".\"{col}\""))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        );
        // `slug` decodes positionally from this select list.
        assert_eq!(
            enroll_reads::WORKSPACES_SELECT_COLUMNS
                .iter()
                .position(|column| *column == "slug"),
            Some(10)
        );
        // Same scope + ordering as the verified E6a twin.
        assert!(enroll_reads::workspace_by_slug_sql().contains("\"deleted_at\" IS NULL"));
        assert!(sql.contains("\"deleted_at\" IS NULL"));
    }

    #[test]
    fn project_exists_sql_matches_e6c_where() {
        assert_eq!(
            PROJECT_EXISTS_SQL,
            "SELECT $1 AS \"a\" FROM \"projects\" \
             WHERE (\"projects\".\"deleted_at\" IS NULL \
             AND \"projects\".\"identifier\" = $2 AND \"projects\".\"workspace_id\" = $3) LIMIT 1"
        );
        // The E6c twin binds identifier-first; the exists shape prepends
        // the M4 `$1` const without reordering the conjuncts.
        let twin = enroll_reads::project_by_workspace_identifier_sql();
        assert!(twin
            .contains("\"projects\".\"identifier\" = $1 AND \"projects\".\"workspace_id\" = $2"));
    }

    // -- D13-F8 wire pins (called, never inlined) ---------------------------------

    #[test]
    fn command_result_wire_matches_f8() {
        use pidash_types::runner_sessions::keys::machine as keys;
        assert_eq!(
            wire()["machine_cmd_result"]["key"],
            "f'machine_cmd_result:{request_id}' (command_result_key)"
        );
        assert_eq!(wire()["machine_cmd_result"]["ttl_secs"], 900);
        assert_eq!(keys::COMMAND_RESULT_TTL_SECS, 900);
        assert_eq!(keys::command_result_key("abc"), "machine_cmd_result:abc");
    }

    #[test]
    fn offline_reject_set_matches_f8() {
        use pidash_types::runner_sessions::is_offline_reject_machine_type;
        assert!(is_offline_reject_machine_type("create_runner"));
        assert!(is_offline_reject_machine_type("config_push"));
        assert!(!is_offline_reject_machine_type("welcome"));
    }

    #[test]
    fn auth_binding_matrix_matches_f8() {
        use super::MachineAuth;
        use crate::runner_enroll::auth::MachineTokenAuthRow;
        let machine = Uuid::new_v4();
        let other = Uuid::new_v4();
        let row = |dev_machine_id| MachineTokenAuthRow {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            dev_machine_id,
            workspace_id: Uuid::new_v4(),
            host_label: String::new(),
            token_hash: String::new(),
            token_fingerprint: String::new(),
            label: String::new(),
            is_service: false,
            created_at: Utc::now(),
            last_used_at: None,
            revoked_at: None,
            workspace_slug: None,
            dev_machine_revoked: false,
        };
        let auth = |dev_machine_id| {
            Some(MachineAuth {
                user_id: Uuid::new_v4(),
                token: row(dev_machine_id),
            })
        };
        assert_eq!(authed_machine_id(None, machine), None);
        assert_eq!(authed_machine_id(auth(None).as_ref(), machine), None);
        assert_eq!(authed_machine_id(auth(Some(other)).as_ref(), machine), None);
        assert_eq!(
            authed_machine_id(auth(Some(machine)).as_ref(), machine),
            Some(machine)
        );
    }

    // -- Pure gates -----------------------------------------------------------------

    #[test]
    fn marker_binding_matrix() {
        let mid = "m1";
        let case = |value: Option<Value>| {
            let mut marker = Map::new();
            if let Some(value) = value {
                marker.insert("dev_machine_id".to_string(), value);
            }
            marker_bound_to(&marker, mid)
        };
        assert!(case(None), "missing passes");
        assert!(case(Some(Value::Null)), "null passes");
        assert!(case(Some(Value::String(String::new()))), "blank passes");
        assert!(case(Some(Value::String(mid.to_string()))), "match passes");
        assert!(!case(Some(Value::String("other".to_string()))));
        assert!(!case(Some(Value::from(123))), "non-strings fail");
        assert!(!case(Some(Value::Bool(true))));
        assert!(!case(Some(Value::Array(vec![]))));
    }

    #[test]
    fn marker_map_rejects_non_objects() {
        assert!(marker_map(serde_json::json!({"a": 1})).is_ok());
        for value in [
            serde_json::json!([1]),
            serde_json::json!("x"),
            serde_json::json!(4),
            serde_json::json!(true),
        ] {
            let response = marker_map(value).expect_err("non-object 500s");
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    #[test]
    fn status_body_orders_request_id_first_and_strips_binding() {
        let rid = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let mut stored = Map::new();
        stored.insert("status".to_string(), Value::String("ok".to_string()));
        stored.insert("dev_machine_id".to_string(), Value::String("m".to_string()));
        stored.insert("runner_name".to_string(), Value::String("r".to_string()));
        assert_eq!(
            status_body(&rid, stored),
            r#"{"request_id":"11111111-2222-3333-4444-555555555555","status":"ok","runner_name":"r"}"#
        );
    }

    #[test]
    fn body_value_semantics() {
        let data = json_cpython::parse_request_data(
            br#"{"s": "  x  ", "n": 123, "b": true, "nil": null, "e": "", "arr": [1]}"#,
        )
        .expect("parses");
        assert_eq!(
            or_empty_stripped(data_get(&data, "s").unwrap()).unwrap(),
            "x"
        );
        assert_eq!(
            or_empty_stripped(data_get(&data, "missing").unwrap()).unwrap(),
            ""
        );
        assert_eq!(
            or_empty_stripped(data_get(&data, "nil").unwrap()).unwrap(),
            ""
        );
        assert!(or_empty_stripped(data_get(&data, "n").unwrap()).is_err());
        assert!(or_empty_stripped(data_get(&data, "b").unwrap()).is_err());
        assert!(or_empty_stripped(data_get(&data, "arr").unwrap()).is_err());
        assert_eq!(py_string(data_get(&data, "n").unwrap()).unwrap(), "123");
        assert_eq!(py_string(data_get(&data, "b").unwrap()).unwrap(), "True");
        assert_eq!(py_string(data_get(&data, "nil").unwrap()).unwrap(), "");
        assert_eq!(
            body_opt_raw(data_get(&data, "s").unwrap()).unwrap(),
            Some("  x  ".to_string()),
            "raw for the or-chain (unstripped)"
        );
        assert_eq!(body_opt_raw(data_get(&data, "e").unwrap()).unwrap(), None);
        assert!(body_opt_raw(data_get(&data, "n").unwrap()).is_err());
        // Non-object bodies have no `.get` (AttributeError → 500).
        let array = json_cpython::parse_request_data(b"[1]").expect("parses");
        assert!(data_get(&array, "s").is_err());
    }

    #[test]
    fn truncations_count_chars_not_bytes() {
        use pidash_services::runner_enroll::queries::catalog_reads::truncate_chars;
        let wide = "é".repeat(200);
        assert_eq!(truncate_chars(&wide, 128).chars().count(), 128);
        assert_eq!(truncate_chars(&wide, 512).chars().count(), 200);
        assert_eq!(truncate_chars("ascii", 128), "ascii");
    }

    #[test]
    fn timestamps_are_microsecond_django_isoformat() {
        let stamped = now_micros();
        assert_eq!(stamped.timestamp_subsec_nanos() % 1000, 0);
        let frozen = chrono::DateTime::from_timestamp(1_700_000_000, 123_456_000).unwrap();
        assert_eq!(django_isoformat(frozen), "2023-11-14T22:13:20.123456+00:00");
        let whole = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        assert_eq!(django_isoformat(whole), "2023-11-14T22:13:20+00:00");
    }

    // -- Routes ------------------------------------------------------------------------

    fn test_state() -> AppState {
        AppState::with_edge("0.1.0", EdgeHandle::for_tests("http://127.0.0.1:1"))
    }

    async fn status_for(app: Router<AppState>, method: &str, path: &str) -> StatusCode {
        let app = app.with_state(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("oneshot serves");
        response.status()
    }

    const MID: &str = "e985a543-b79a-48a8-bdfe-7e0aaa937914";
    const RID: &str = "399779fa-1d90-4aa0-8a64-501c6ba7ae0b";

    /// Owned methods reach Rust (pool-less state 500s inside the
    /// handler), unowned methods proxy to Django (502 against the dead
    /// test upstream), anything else 404s.
    #[tokio::test]
    async fn web_routes_register_all_three_paths() {
        let delete = format!("/api/runners/dev-machines/{MID}/");
        let create = format!("/api/runners/dev-machines/{MID}/create-runner/");
        let status = format!("/api/runners/dev-machines/{MID}/create-runner/{RID}/");
        assert_eq!(
            status_for(web_routes(), "DELETE", &delete).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            status_for(web_routes(), "POST", &delete).await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status_for(web_routes(), "POST", &create).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            status_for(web_routes(), "GET", &create).await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status_for(web_routes(), "GET", &status).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            status_for(web_routes(), "POST", &status).await,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status_for(web_routes(), "DELETE", "/api/runners/dev-machines/nope/").await,
            StatusCode::INTERNAL_SERVER_ERROR,
            "bad UUID reaches the view 404 only past the pool gate"
        );
    }

    #[tokio::test]
    async fn daemon_route_registers() {
        let result = format!("/api/v1/runner/dev-machines/{MID}/commands/{RID}/result/");
        assert_eq!(
            status_for(daemon_routes(), "POST", &result).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            status_for(daemon_routes(), "GET", &result).await,
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn overlay_groups_merge_without_conflict() {
        // Building the group routers panics on same-path collisions:
        // this pins that the new registrations (and the DELETE joined
        // onto the sibling route) coexist with every merged router.
        use crate::overlay::{oss_group_routes, RouteGroup};
        let _ = oss_group_routes(RouteGroup::RunnerWeb);
        let _ = oss_group_routes(RouteGroup::Runner);
    }

    #[tokio::test]
    async fn runner_detail_delete_joins_the_sibling_route() {
        // The DELETE serves from Rust (500 pool-less), not Django (502):
        // the method joined manage.rs's existing registration.
        let path = format!("/api/runners/{RID}/");
        assert_eq!(
            status_for(super::super::manage::routes(), "DELETE", &path).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
