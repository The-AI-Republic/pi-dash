//! Refresh + revoke teardown endpoints (D-13 handlers-C, PIDASHCONV-592).
//!
//! Ports `runner/views/enrollment.py:380-476` (`RunnerRefreshEndpoint`),
//! `:503-534` (`RunnerSelfRevokeEndpoint`), `runner/views/runners.py:173-208`
//! (`DevMachineRevokeEndpoint`), `:211-246` (`DevMachineRotateEndpoint`),
//! and `:455-492` (`RunnerRevokeEndpoint`):
//!
//! * `POST /api/v1/runner/runners/<rid>/refresh/` — refresh-token parse
//!   auth, current-vs-previous match (previous ⇒ revoke +
//!   `refresh_token_replayed`), membership-or-revoke, atomic rotation,
//!   access mint, force-refresh clear.
//! * `DELETE /api/v1/runner/runners/<rid>/` — runner self-deletion:
//!   id match, revoke + revoke frame + session close + row delete.
//! * `POST /api/runners/dev-machines/<mid>/revoke/` — machine + token
//!   revoke, per-runner revoke frames + `Runner.revoke` + session close.
//! * `POST /api/runners/dev-machines/<mid>/rotate/` — token revoke +
//!   revoke frames + session closes; runner rows untouched (port as-is).
//! * `POST /api/runners/<rid>/revoke/` — row-locked read, view/manage
//!   gates, idempotent 200 (no re-emit when already revoked).
//!
//! Fixture ids D13-F3 (`tokens/tokens.golden.json`: rotation mints),
//! D13-F5 (`queries/*.sql`: refresh/revoke/cascade SQL), D13-F6
//! (`services/flows.golden.json`: cascade order, reasons, tx boundaries),
//! D13-F7 (`handlers/endpoints.golden.json`: endpoint bodies), D13-F8
//! (`external/wire_pins.json`: provider call shapes — called, never
//! inlined).
//!
//! # Execution model
//!
//! * Daemon routes authenticate through the merged [`auth`](super::auth)
//!   resolvers (refresh-token parse, access-token `mt_` + JWT); web routes
//!   through the session preamble (the `manage.rs` `web_actor` shape,
//!   restated — sibling handler modules are not imported).
//! * SQL text comes from the merged builders (`enroll_reads` E3/E4/E5,
//!   `manage_reads` M4-M7, `revoke` S1-S4, `delete` collector,
//!   `finalization`, `catalog_reads` pod/project); the five statements
//!   with no builder (membership `EXISTS`, locked runner read,
//!   refresh re-read, lazy pod/machine loads) are handler-owned SQL,
//!   Django-probed (see the `SQL` section).
//! * This module is the first pool-backed consumer of the
//!   [`RevokeStore`](pidash_services::runner_enroll::revoke::RevokeStore)
//!   and [`PubsubStore`](pidash_services::runner_sessions::pubsub::PubsubStore)
//!   seams: one `TxStores` implementation runs the drivers on a sqlx
//!   transaction, commits, then fires the collected post-commit effects
//!   in Python's registration order (per-run publish pairs → handoffs →
//!   drains → cleanup).
//! * Post-commit providers are all called, never inlined: D-15
//!   finalization builders + `drain_lifecycle_effects`, the D-14 drain
//!   executor recipe (`drain.rs` docs), the D-12 handoff driver over a
//!   pool-backed twin of `jobs::LiveCreationStore`, and the D-14 outbox
//!   cleanup call.
//!
//! # Ported bugs and quirks (translate, don't redesign; also in the PR)
//!
//! * QUIRK-rotate-keeps-runners (`runners.py:237-244`): rotate closes
//!   sessions and emits revoke frames but never revokes the runner rows.
//! * QUIRK-replay-commits (`enrollment.py:429-434,441-446`): the replay
//!   and membership 401s return from inside the outer `atomic`, so the
//!   nested revoke COMMITS and its post-commit effects fire.
//! * QUIRK-unreachable-idempotency (`enrollment.py:525-527` + F7):
//!   `revoke()` no-op tolerance for already-revoked rows is unreachable —
//!   auth 401s (`runner_revoked`) before the view runs. Ported as written.
//! * QUIRK-workspace-or-chain (`runners.py:128-129`): body/query `or`
//!   tests the RAW value, then strips — a whitespace-only body value
//!   yields `""` WITHOUT consulting the query param.
//! * QUIRK-m5-fallback (`runners.py:170`): a vanished serialize row falls
//!   back to the unannotated instance (counts omitted, `control_online`
//!   false).
//!
//! # Documented approximations
//!
//! * Unhandled failures answer the JSON 500 (`SERVER_ERROR_BODY`): Django
//!   renders its HTML error page here, so only the status is
//!   contract-pinned (the `manage.rs` position).
//! * Non-UUID daemon path segments answer `{"error": "Page not found."}`
//!   404 (the `machine.rs` `PAGE_NOT_FOUND_BODY` position — Django's
//!   `<uuid:…>` resolver 404s before auth); non-UUID web path segments
//!   answer the view's JSON 404 (`manage.rs` position).
//! * `.get()`/lazy single-row reads emit `LIMIT 1`, omitting Django's
//!   `LIMIT 21` multi-row probe (the queries-A precedent —
//!   behaviorally identical for PK lookups).
//! * The member gate collapses Django's membership `EXISTS` + role
//!   `SELECT` into one `SELECT` where both facts are needed (the
//!   `manage.rs` `workspace_role` position); the refresh path keeps the
//!   exact `EXISTS` (only membership is read there).
//! * `updated_at`/`created_at` binds are this request's microsecond
//!   `now()`s, like Django's view-`now()` + `auto_now` calls (two
//!   separate samples where the source makes two).

// Every handler returns a fully-rendered `Response` by design (the
// `manage.rs` precedent, which carries the same allow).
#![allow(clippy::result_large_err)]

use axum::extract::{Extension, Path, Request, State};
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
use pidash_db::runner_enroll::columns::{
    dev_machine as dm_cols, pod as pod_cols, runner as r_cols,
};
use pidash_services::runner_enroll::queries::{catalog_reads, enroll_reads, manage_reads};
use pidash_services::runner_enroll::revoke as revoke_kernel;
use pidash_services::runner_enroll::serializers::shapes;
use pidash_services::runner_enroll::tokens as enroll_tokens;
use pidash_services::runner_runs::finalization as finalize_kernel;
use pidash_services::runner_sessions::{drain as drain_kernel, pubsub as pubsub_kernel};
use pidash_types::{UserId, WorkspaceId};

use super::auth as enroll_auth;
use crate::middleware::SessionHandle;
use crate::runner_runs::{json_response, pool_of, server_error};
use crate::state::AppState;
use crate::v1_cycles_modules::json_cpython::{self, JObject, JVal, JsonFail};

// ---------------------------------------------------------------------------
// Error bodies (D13-F7 `handlers/endpoints.golden.json`, key order verbatim)
// ---------------------------------------------------------------------------

/// 401 `missing_refresh_token` (`enrollment.py:397-400`): no Bearer
/// credential — answered by the VIEW, not the auth class (F7).
pub const MISSING_REFRESH_TOKEN_BODY: &str = r#"{"error":"missing_refresh_token"}"#;
/// 401 `invalid_refresh_token` (`:406-409,436-439`): runner miss OR hash
/// matches neither current nor previous — same body, no leak (F7).
pub const INVALID_REFRESH_TOKEN_BODY: &str = r#"{"error":"invalid_refresh_token"}"#;
/// 401 `runner_revoked` (`:410-414`): revoked runner on the refresh path
/// (401 here, not 409 — F7).
pub const RUNNER_REVOKED_BODY: &str = r#"{"error":"runner_revoked"}"#;
/// 401/409 `dev_machine_revoked` (`:421-425`, `runners.py:231-235`): one
/// body, two statuses — 401 on refresh, 409 on rotate (F7).
pub const DEV_MACHINE_REVOKED_BODY: &str = r#"{"error":"dev_machine_revoked"}"#;
/// 401 `refresh_token_replayed` (`:429-434`): presented == previous hash —
/// `revoke("refresh_token_replayed")` runs first, then this body (F7).
pub const REFRESH_TOKEN_REPLAYED_BODY: &str = r#"{"error":"refresh_token_replayed"}"#;
/// 401 `membership_revoked` (`:441-446`): owner no longer a member —
/// `revoke("membership_revoked")` runs first, then this body (F7).
pub const MEMBERSHIP_REVOKED_BODY: &str = r#"{"error":"membership_revoked"}"#;
/// 403 `runner_id_mismatch` (`:519-523`): `auth_runner` missing (including
/// the anonymous case — `permission_classes` is empty so no DRF gate runs)
/// or different from the URL id (F7).
pub const RUNNER_ID_MISMATCH_BODY: &str = r#"{"error":"runner_id_mismatch"}"#;

// ---------------------------------------------------------------------------
// Revoke + frame reasons (F6 `flows.golden.json` / F8 `wire_pins.json`)
// ---------------------------------------------------------------------------

/// `Runner.revoke` reasons, one per call site (`models.py:542-596` +
/// F6 `revoke_cascade_steps.s1.reasons`).
pub const REVOKE_REASON_REFRESH_REPLAYED: &str = "refresh_token_replayed";
pub const REVOKE_REASON_MEMBERSHIP: &str = "membership_revoked";
pub const REVOKE_REASON_SELF: &str = "self_revoked";
pub const REVOKE_REASON_DEV_MACHINE: &str = "dev_machine_revoked";
pub const REVOKE_REASON_MANUAL: &str = "manual_revoke";

/// Revoke-frame reasons, one per emit site (`pubsub.py` + F8).
pub const FRAME_REASON_SELF: &str = "self_revoked";
pub const FRAME_REASON_DEV_MACHINE: &str = "dev machine revoked";
pub const FRAME_REASON_ROTATE: &str = "machine token rotated";
pub const FRAME_REASON_RUNNER_REVOKE: &str = "revoked by user";

/// The self-revoke frame input (`enrollment.py:528`): exactly
/// `{type: revoke, reason}` — the driver envelopes it (F8).
fn revoke_frame(reason: &str) -> Map<String, Value> {
    let mut frame = Map::new();
    frame.insert("type".to_owned(), Value::String("revoke".to_owned()));
    frame.insert("reason".to_owned(), Value::String(reason.to_owned()));
    frame
}
/// Resolver-404 for a non-UUID daemon path id (the `machine.rs`
/// `PAGE_NOT_FOUND_BODY` position, restated — that module's const is
/// private): Django's `<uuid:…>` converter 404s before auth.
const DAEMON_NOT_FOUND_BODY: &str = r#"{"error": "Page not found."}"#;

// The web bodies live on the sibling `manage` module (already `pub` —
// same file family, same F7 source): workspace-required 400, forbidden
// 403, not-found 404.
use super::manage::{FORBIDDEN_BODY, NOT_FOUND_BODY, WORKSPACE_REQUIRED_BODY};

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

fn unauthorized(body: &str) -> Response {
    json_response(StatusCode::UNAUTHORIZED, body.to_owned())
}

fn daemon_not_found() -> Response {
    json_response(StatusCode::NOT_FOUND, DAEMON_NOT_FOUND_BODY.to_owned())
}

// ---------------------------------------------------------------------------
// Clocks (the `manage.rs` / `enroll.rs` positions)
// ---------------------------------------------------------------------------

/// `timezone.now()` truncated to microseconds (Django datetimes are
/// microsecond-exact; Postgres would round stored nanos).
fn now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    DateTime::from_timestamp_micros(now.timestamp_micros()).expect("micros in range")
}

/// `int(time.time())` for JWT `iat` (the `enroll.rs` `unix_now_secs`
/// position).
fn unix_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Handler-owned SQL (Django-probed, repo-pinned Django 4.2.30 — no merged
// builder covers these five shapes, and the services layer is read-only
// from here, so the text lives with the handlers like `manage.rs`
// `workspace_role_in`)
// ---------------------------------------------------------------------------

/// `"table"."col", …` projection (the services-builders `qualified`
/// shape, restated).
fn qualified(table: &str, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| format!("\"{table}\".\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Locked runner read (`runners.py:476`).
///
/// `Runner.objects.select_for_update().filter(pk).first()` inside
/// `transaction.atomic()`: full row (plain `Manager` — no scope),
/// `Meta.ordering` kept, `LIMIT 1 FOR UPDATE`. `$1` = runner id.
///
/// Probe: `/tmp/592_probe.py` P2 (column order + `ORDER BY` + `LIMIT 1`;
/// the `FOR UPDATE` tail is the E3/M6 merged-builder shape).
fn runner_locked_read_sql() -> String {
    format!(
        "SELECT {} FROM \"runner\" WHERE \"runner\".\"id\" = $1 \
         ORDER BY \"runner\".\"last_heartbeat_at\" DESC, \"runner\".\"created_at\" DESC \
         LIMIT 1 FOR UPDATE",
        qualified(r_cols::TABLE, r_cols::COLUMNS),
    )
}

/// Workspace-membership probe (`core/permissions.py:28-34`, via the
/// refresh view `:441`).
///
/// `WorkspaceMember.objects.filter(workspace_id, member,
/// is_active=True).exists()`: `SELECT 1 AS "a" … LIMIT 1`, Q-sorted
/// (manager scope, bare `is_active`, member, workspace). `$1` = member
/// (user) id, `$2` = workspace id.
///
/// Probe: `/tmp/592_probe5.py` (literal `1`, conjunct order).
const MEMBERSHIP_EXISTS_SQL: &str = "SELECT 1 AS \"a\" FROM \"workspace_members\" \
     WHERE (\"workspace_members\".\"deleted_at\" IS NULL \
     AND \"workspace_members\".\"is_active\" \
     AND \"workspace_members\".\"member_id\" = $1 \
     AND \"workspace_members\".\"workspace_id\" = $2) LIMIT 1";

/// Post-revoke re-read (`runners.py:491`).
///
/// `runner.refresh_from_db()`: the `.get()` single-row shape (full row,
/// no scope, no ordering) with `LIMIT 1` (the queries-A `LIMIT 21`-probe
/// omission). `$1` = runner id.
///
/// Probe: `/tmp/592_probe.py` P3 (column order, no `ORDER BY`).
fn runner_refresh_sql() -> String {
    format!(
        "SELECT {} FROM \"runner\" WHERE \"runner\".\"id\" = $1 LIMIT 1",
        qualified(r_cols::TABLE, r_cols::COLUMNS),
    )
}

/// Lazy `runner.pod` load for the revoke response (`serializers.py`
/// `pod_detail`).
///
/// `Pod.objects.get(pk)`: `PodManager` scope (`deleted_at IS NULL`),
/// no ordering, `LIMIT 1`. `$1` = pod id.
///
/// Probe: `/tmp/592_probe.py` P4a.
fn pod_lazy_sql() -> String {
    format!(
        "SELECT {} FROM \"pod\" WHERE (\"pod\".\"deleted_at\" IS NULL AND \"pod\".\"id\" = $1) LIMIT 1",
        qualified(pod_cols::TABLE, pod_cols::COLUMNS),
    )
}

/// Lazy `runner.dev_machine` load for the revoke response
/// (`dev_machine_detail`).
///
/// `DevMachine.objects.get(pk)`: plain manager (no scope), no ordering,
/// `LIMIT 1`. `$1` = machine id. Runs only when `dev_machine_id` is set.
///
/// Probe: `/tmp/592_probe.py` P4b.
fn machine_lazy_sql() -> String {
    format!(
        "SELECT {} FROM \"dev_machine\" WHERE \"dev_machine\".\"id\" = $1 LIMIT 1",
        qualified(dm_cols::TABLE, dm_cols::COLUMNS),
    )
}

/// The reverse one-to-one `runner.live_state` read (the `manage.rs`
/// `fetch_live_state` text, restated — that module's helper is private).
///
/// `RunnerLiveState` columns in `_meta` order over `runner_id`, `LIMIT 1`.
/// `$1` = runner id.
const LIVE_STATE_SQL: &str =
    "SELECT \"runner_id\", \"observed_run_id\", \"last_event_at\", \"last_event_kind\", \
     \"last_event_summary\", \"agent_pid\", \"agent_subprocess_alive\", \
     \"approvals_pending\", \"usage\", \"llm_model\", \"turn_count\", \"updated_at\" \
     FROM \"runner_live_state\" WHERE \"runner_live_state\".\"runner_id\" = $1 LIMIT 1";

// ---------------------------------------------------------------------------
// Preamble (the `manage.rs` web / `machine.rs` daemon positions, restated)
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
/// role, or `None`. `WorkspaceMember.objects` is the
/// `SoftDeletionManager`, so tombstones do not count; `Meta.ordering`
/// is `-created_at` with `.first()` → `LIMIT 1`.
async fn workspace_role(
    executor: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i32>, Response> {
    workspace_role_in(executor, workspace_id, user_id).await
}

async fn workspace_role_in(
    executor: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
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
/// raises `ValidationError` (an unhandled 500) on garbage.
fn parse_uuid(raw: &str) -> Result<Uuid, Response> {
    raw.parse().map_err(|_| server_error())
}

/// UUID-typed web path segments: the view's JSON 404 on garbage
/// (Django's `<uuid:…>` converter 404s at URL-resolve).
fn path_uuid_web(raw: &str) -> Result<Uuid, Response> {
    raw.parse().map_err(|_| not_found())
}

/// UUID-typed daemon path segments: the resolver-style 404 on garbage.
fn path_uuid_daemon(raw: &str) -> Result<Uuid, Response> {
    raw.parse().map_err(|_| daemon_not_found())
}

/// Read `request.data` for a POST body: content-length 0 validates as
/// `{}` with the body ignored; a non-JSON content type proxies to Django
/// (form posts stay on the Python plane); unparsable JSON 400s with
/// DRF's `ParseError` Detail; past the depth cap is Django's JSON 500.
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
    match json_cpython::parse_request_data(&bytes) {
        Ok(value) => Ok(value),
        Err(JsonFail::Message(detail)) => Err(json_response(
            StatusCode::BAD_REQUEST,
            parse_error_body(&detail),
        )),
        Err(JsonFail::Recursion) => Err(server_error()),
    }
}

/// DRF `ParseError` body (`rest_framework/parsers.py`): lowercase
/// `detail`, the `JSON parse error - ` prefix, CPython's message.
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

/// `request.data.get(key)`: dict lookup — `None` for a missing key; any
/// non-object is Python's `AttributeError` → 500.
fn data_get<'a>(data: &'a JVal, key: &str) -> Result<Option<&'a JVal>, Response> {
    match data {
        JVal::Object(map) => Ok(map.get(key)),
        JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Str(_) | JVal::Array(_) => {
            Err(server_error())
        }
    }
}

/// The body's `workspace` leg of `_request_workspace_id`
/// (`runners.py:128-129`), as the raw string: falsy/missing maps to
/// `None` (the kernel falls through to the query leg), truthy strings
/// pass through UNstripped (QUIRK-workspace-or-chain — the `or` tests
/// the raw value, the strip runs after), truthy non-strings are the
/// source's `AttributeError` → 500.
fn body_workspace(data: &JVal) -> Result<Option<String>, Response> {
    let value = data_get(data, "workspace")?;
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

/// Api-crate-owned Redis client (the `machine.rs` `redis_client`
/// position): `None` when `REDIS_URL` is unset, empty, or unparsable,
/// mirroring `redis_instance()` returning `None`.
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
// `POST /api/v1/runner/runners/<rid>/refresh/` (`enrollment.py:380-476`)
// ---------------------------------------------------------------------------

/// The E3 row facts the refresh view reads (runner block at 0; the
/// joined workspace row is never read past the join itself; position 0
/// is the URL id already in hand).
struct RefreshRunner {
    owner_id: Uuid,
    workspace_id: Uuid,
    dev_machine_id: Option<Uuid>,
    refresh_token_hash: String,
    refresh_token_generation: i32,
    previous_refresh_token_hash: String,
    revoked_at: Option<DateTime<Utc>>,
}

/// Decode the E3 row positionally (`r_cols::COLUMNS` order — indices
/// pinned by `e3_decode_indices_follow_columns`).
fn decode_refresh_runner(row: &sqlx::postgres::PgRow) -> Result<RefreshRunner, Response> {
    Ok(RefreshRunner {
        owner_id: row.try_get(1).map_err(|_| server_error())?,
        workspace_id: row.try_get(2).map_err(|_| server_error())?,
        dev_machine_id: row.try_get(3).map_err(|_| server_error())?,
        refresh_token_hash: row.try_get(9).map_err(|_| server_error())?,
        refresh_token_generation: row.try_get(11).map_err(|_| server_error())?,
        previous_refresh_token_hash: row.try_get(12).map_err(|_| server_error())?,
        revoked_at: row.try_get(28).map_err(|_| server_error())?,
    })
}

/// `access.expires_at.isoformat()` (`:473`): whole-seconds UTC renders
/// `+00:00` with no fraction (the `machine.rs` `django_isoformat`
/// position — `expires_at` is always whole seconds).
fn render_expires_at(expires_at: &DateTime<Utc>) -> String {
    expires_at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
}

/// The 200 body (`:469-476`), in dict order.
fn refresh_ok_body(
    refresh_token: &str,
    access_token: &str,
    access_token_expires_at: &str,
    refresh_token_generation: i32,
) -> String {
    serde_json::json!({
        "refresh_token": refresh_token,
        "access_token": access_token,
        "access_token_expires_at": access_token_expires_at,
        "refresh_token_generation": refresh_token_generation,
    })
    .to_string()
}

/// `POST /api/v1/runner/runners/<runner_id>/refresh/`
/// (`enrollment.py:394-476`): parse-only Bearer [REDACTED] the row-locked
/// rotation. QUIRK-replay-commits: the replay and membership 401s commit
/// their nested revoke (the `return` exits the outer `atomic` normally)
/// and fire its post-commit effects before answering.
pub async fn runner_refresh(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let runner_id = match path_uuid_daemon(&raw_id) {
        Ok(runner_id) => runner_id,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    let raw = match enroll_auth::parse_refresh_token(&headers) {
        Ok(raw) => raw,
        Err(response) => return response,
    };
    let Some(raw) = raw else {
        return unauthorized(MISSING_REFRESH_TOKEN_BODY);
    };
    let presented_hash = pidash_auth::token::hash_token(&raw, secret.as_bytes());

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    // E3 — locked runner + workspace (`:403-404`).
    let row: Option<sqlx::postgres::PgRow> =
        match sqlx::query(&enroll_reads::refresh_locked_read_sql())
            .bind(runner_id)
            .fetch_optional(&mut *tx)
            .await
        {
            Ok(row) => row,
            Err(_) => return server_error(),
        };
    let Some(row) = row else {
        return unauthorized(INVALID_REFRESH_TOKEN_BODY);
    };
    let runner = match decode_refresh_runner(&row) {
        Ok(runner) => runner,
        Err(response) => return response,
    };
    if runner.revoked_at.is_some() {
        return unauthorized(RUNNER_REVOKED_BODY);
    }
    // E4 — dev-machine revoked probe, only when linked (`:415-425`).
    if let Some(dev_machine_id) = runner.dev_machine_id {
        let hit: Option<i32> =
            match sqlx::query_scalar(&enroll_reads::dev_machine_revoked_probe_sql())
                .bind(dev_machine_id)
                .fetch_optional(&mut *tx)
                .await
            {
                Ok(hit) => hit,
                Err(_) => return server_error(),
            };
        if hit.is_some() {
            return unauthorized(DEV_MACHINE_REVOKED_BODY);
        }
    }

    // Current-vs-previous match (`:427-439`).
    if presented_hash == runner.refresh_token_hash {
        // Happy path — fall through to the membership check.
    } else if !runner.previous_refresh_token_hash.is_empty()
        && presented_hash == runner.previous_refresh_token_hash
    {
        let now = now_micros();
        let mut stores = TxStores::new(&mut tx, runner_id);
        let outcome = match revoke_kernel::revoke_runner(
            &mut stores,
            runner_id,
            runner.revoked_at,
            REVOKE_REASON_REFRESH_REPLAYED,
            now,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => return server_error(),
        };
        let bundle = stores.into_bundle(outcome);
        if tx.commit().await.is_err() {
            return server_error();
        }
        if fire_bundle(&pool, &state, bundle).await.is_err() {
            return server_error();
        }
        return unauthorized(REFRESH_TOKEN_REPLAYED_BODY);
    } else {
        return unauthorized(INVALID_REFRESH_TOKEN_BODY);
    }

    // Live membership re-check (`:441-446`).
    let member: Option<i32> = match sqlx::query_scalar(MEMBERSHIP_EXISTS_SQL)
        .bind(runner.owner_id)
        .bind(runner.workspace_id)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(member) => member,
        Err(_) => return server_error(),
    };
    if member.is_none() {
        let now = now_micros();
        let mut stores = TxStores::new(&mut tx, runner_id);
        let outcome = match revoke_kernel::revoke_runner(
            &mut stores,
            runner_id,
            runner.revoked_at,
            REVOKE_REASON_MEMBERSHIP,
            now,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => return server_error(),
        };
        let bundle = stores.into_bundle(outcome);
        if tx.commit().await.is_err() {
            return server_error();
        }
        if fire_bundle(&pool, &state, bundle).await.is_err() {
            return server_error();
        }
        return unauthorized(MEMBERSHIP_REVOKED_BODY);
    }

    // E5 — atomic rotation (`:448-460`): `$1` new hash, `$2` new
    // fingerprint, `$3` new generation (old + 1), `$4` previous hash (the
    // OLD current hash — a Python-computed bind, not a column ref), `$5`
    // runner.
    let new_refresh = enroll_tokens::mint_refresh_token(&secret);
    let new_generation = runner.refresh_token_generation + 1;
    if sqlx::query(&enroll_reads::refresh_rotate_sql())
        .bind(&new_refresh.hashed)
        .bind(&new_refresh.fingerprint)
        .bind(new_generation)
        .bind(&runner.refresh_token_hash)
        .bind(runner_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    let runner_id_str = runner_id.to_string();
    let owner_id_str = runner.owner_id.to_string();
    let workspace_id_str = runner.workspace_id.to_string();
    let new_access = match enroll_tokens::mint_access_token_with_keys(
        &enroll_tokens::MintParams {
            runner_id: &runner_id_str,
            user_id: &owner_id_str,
            workspace_id: &workspace_id_str,
            rtg: i64::from(new_generation),
            ttl_secs: None,
            default_ttl_secs: state.settings().runner.access_token_ttl_secs,
            now_unix: unix_now_secs(),
        },
        &[],
        &secret,
    ) {
        Ok(access) => access,
        Err(_) => return server_error(),
    };
    // Force-refresh clear, still inside the tx (`:467`).
    if sqlx::query(&enroll_reads::force_refresh_clear_sql())
        .bind(runner_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    if tx.commit().await.is_err() {
        return server_error();
    }
    let expires_at = render_expires_at(&new_access.expires_at);
    json_response(
        StatusCode::OK,
        refresh_ok_body(
            &new_refresh.raw,
            &new_access.raw,
            &expires_at,
            new_generation,
        ),
    )
}

// ---------------------------------------------------------------------------
// `DELETE /api/v1/runner/runners/<rid>/` (`enrollment.py:503-534`)
// ---------------------------------------------------------------------------

/// `DELETE /api/v1/runner/runners/<runner_id>/` (`:517-534`): runner
/// self-deletion. Anonymous (or a mismatched id) answers the view's 403 —
/// `permission_classes` is empty, so no DRF gate runs first. The revoke
/// runs on its own transaction (its post-commit effects fire before the
/// frame); the frame, close, and collector delete run outside any tx.
pub async fn runner_self_revoke(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool.clone(),
        Err(response) => return response,
    };
    let runner_id = match path_uuid_daemon(&raw_id) {
        Ok(runner_id) => runner_id,
        Err(response) => return response,
    };
    let secret = state.settings().secret_key.clone();
    // Stock settings carry no `RUNNER_ACCESS_TOKEN_KEYS` (`[]`, both
    // planes), so the ring is always the derived dev key (the `enroll.rs`
    // `&[]` position).
    let ring = enroll_tokens::build_key_ring(&[], &secret);
    let auth = match enroll_auth::authenticate_access_token(
        &pool,
        secret.as_bytes(),
        &ring,
        &headers,
        Some(&raw_id),
    )
    .await
    {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    let Some(auth) = auth else {
        return json_response(StatusCode::FORBIDDEN, RUNNER_ID_MISMATCH_BODY.to_owned());
    };
    if auth.runner.id != runner_id {
        return json_response(StatusCode::FORBIDDEN, RUNNER_ID_MISMATCH_BODY.to_owned());
    }

    let runner_pk = auth.runner.id;
    let revoked_at = auth.runner.revoked_at;
    // `revoke()` opens its own atomic; the post-commit bundle fires on
    // its commit, before the frame below (`:527` + `models.py:597-685`).
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let bundle = {
        let mut stores = TxStores::new(&mut tx, runner_pk);
        let outcome = match revoke_kernel::revoke_runner(
            &mut stores,
            runner_pk,
            revoked_at,
            REVOKE_REASON_SELF,
            now_micros(),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => return server_error(),
        };
        stores.into_bundle(outcome)
    };
    if tx.commit().await.is_err() {
        return server_error();
    }
    if fire_bundle(&pool, &state, bundle).await.is_err() {
        return server_error();
    }

    // Revoke frame + session close + row delete, outside any tx (`:528-533`).
    let pubsub = PoolPubsubStore::new(&pool, &state);
    let frame = revoke_frame(FRAME_REASON_SELF);
    match pubsub_kernel::send_to_runner(&pubsub, runner_pk, &frame).await {
        Ok(outcome) => {
            for warning in &outcome.warnings {
                tracing::warn!("{warning}");
            }
        }
        // `revoke` is offline-allowed, so this arm cannot fire in
        // practice; Python has no `try` here, so a failure 500s.
        Err(_) => return server_error(),
    }
    if pubsub_kernel::close_runner_session(
        &pubsub,
        runner_pk,
        pubsub_kernel::CLOSE_RUNNER_SESSION_DEFAULT_CODE,
    )
    .await
    .is_err()
    {
        return server_error();
    }
    if delete_collected_runner(&pool, runner_pk).await.is_err() {
        return server_error();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// `POST /api/runners/dev-machines/<mid>/revoke/` (`runners.py:173-208`)
// ---------------------------------------------------------------------------

/// The M6 locked row facts the revoke/rotate views read, plus the
/// render fields for the QUIRK-m5-fallback path (the unannotated
/// in-memory instance).
struct LockedMachine {
    id: Uuid,
    owner_id: Uuid,
    host_label: String,
    label: String,
    visibility: i16,
    last_seen_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Decode the M6 row positionally (`dm_cols::COLUMNS` order — indices
/// pinned by `m6_decode_indices_follow_columns`).
fn decode_locked_machine(row: &sqlx::postgres::PgRow) -> Result<LockedMachine, Response> {
    Ok(LockedMachine {
        id: row.try_get(0).map_err(|_| server_error())?,
        owner_id: row.try_get(1).map_err(|_| server_error())?,
        host_label: row.try_get(2).map_err(|_| server_error())?,
        label: row.try_get(3).map_err(|_| server_error())?,
        visibility: row.try_get(4).map_err(|_| server_error())?,
        last_seen_at: row.try_get(6).map_err(|_| server_error())?,
        revoked_at: row.try_get(7).map_err(|_| server_error())?,
        created_at: row.try_get(8).map_err(|_| server_error())?,
        updated_at: row.try_get(9).map_err(|_| server_error())?,
    })
}

/// `_machine_is_in_workspace_scope` (`runners.py:132-147`): the kernel
/// view check plus the M4a/M4b `EXISTS` probes (runner-or-token link).
async fn machine_in_workspace_scope(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    machine: &LockedMachine,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, Response> {
    // `can_view_dev_machine` (`permissions.py:57-64`): identical body to
    // `can_view_runner` (private + owner) — the same kernel answers it.
    let can_view = runner_perm::can_view_runner(&runner_perm::RunnerFacts {
        workspace: WorkspaceId::from(workspace_id.to_string()),
        authenticated: true,
        visibility: i32::from(machine.visibility),
        owned_by_requester: machine.owner_id == user_id,
    });
    if !can_view {
        return Ok(false);
    }
    let visibility: i16 = runner_perm::VISIBILITY_PRIVATE as i16;
    let runner_hit: Option<i32> =
        sqlx::query_scalar(&manage_reads::machine_scope_runner_probe_sql())
            .bind(1i32)
            .bind(machine.id)
            .bind(user_id)
            .bind(visibility)
            .bind(workspace_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| server_error())?;
    if runner_hit.is_some() {
        return Ok(true);
    }
    let token_hit: Option<i32> = sqlx::query_scalar(&manage_reads::machine_scope_token_probe_sql())
        .bind(1i32)
        .bind(machine.id)
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| server_error())?;
    Ok(token_hit.is_some())
}

/// Resolve the `workspace` gate shared by the machine endpoints
/// (`runners.py:180-187,218-225`): body-or-query id (400 when empty),
/// UUID parse (500 on garbage), member gate (403).
async fn machine_workspace_gate(
    pool: &PgPool,
    user_id: Uuid,
    data: &JVal,
    params: &crate::license::QueryMap,
) -> Result<Uuid, Response> {
    let body_ws = body_workspace(data)?;
    let query_ws = crate::license::query_last(params, "workspace");
    let workspace_raw = manage_reads::request_workspace_id(body_ws.as_deref(), query_ws.as_deref());
    if workspace_raw.is_empty() {
        return Err(bad_request(WORKSPACE_REQUIRED_BODY));
    }
    let workspace_id = parse_uuid(&workspace_raw)?;
    let role = workspace_role(pool, workspace_id, user_id).await?;
    if !membership::is_workspace_member(role) {
        return Err(forbidden());
    }
    Ok(workspace_id)
}

/// `POST /api/runners/dev-machines/<machine_id>/revoke/`
/// (`runners.py:179-208`): machine + token revoke, then per-runner
/// revoke frames (all frames first) + `Runner.revoke` + session close —
/// everything inside one atomic (QUIRK-machine-tx). Answers the M5
/// machine serialization.
pub async fn dev_machine_revoke(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<crate::license::QueryMap>,
    req: Request,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let workspace_id = match machine_workspace_gate(&pool, user_id, &data, &params).await {
        Ok(workspace_id) => workspace_id,
        Err(response) => return response,
    };
    let machine_id = match path_uuid_web(&raw_id) {
        Ok(machine_id) => machine_id,
        Err(response) => return response,
    };

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    // M6 — locked machine read (`:190`).
    let row: Option<sqlx::postgres::PgRow> =
        match sqlx::query(&manage_reads::machine_locked_read_sql())
            .bind(machine_id)
            .fetch_optional(&mut *tx)
            .await
        {
            Ok(row) => row,
            Err(_) => return server_error(),
        };
    let Some(row) = row else {
        return not_found();
    };
    let machine = match decode_locked_machine(&row) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    match machine_in_workspace_scope(&mut tx, &machine, user_id, workspace_id).await {
        Ok(true) => {}
        Ok(false) => return not_found(),
        Err(response) => return response,
    };

    // Conditional machine write (`:194-197`): `$1` is the view's `now()`,
    // `$2` the `auto_now` `pre_save` value (a fresh sample at save time).
    // The stamped pair feeds the QUIRK-m5-fallback render.
    let now = now_micros();
    let mut stamped: Option<(DateTime<Utc>, DateTime<Utc>)> = None;
    if machine.revoked_at.is_none() {
        let auto_now = now_micros();
        if sqlx::query(&manage_reads::machine_revoke_sql())
            .bind(now)
            .bind(auto_now)
            .bind(machine_id)
            .execute(&mut *tx)
            .await
            .is_err()
        {
            return server_error();
        }
        stamped = Some((now, auto_now));
    }
    // Token revoke-all (`:198`).
    if sqlx::query(&manage_reads::machine_tokens_revoke_sql())
        .bind(now)
        .bind(machine_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    // Locked active-runner list (`:199`).
    let runner_rows: Vec<sqlx::postgres::PgRow> =
        match sqlx::query(&manage_reads::machine_runners_locked_list_sql())
            .bind(machine_id)
            .fetch_all(&mut *tx)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return server_error(),
        };
    let mut runner_ids = Vec::with_capacity(runner_rows.len());
    let mut runner_revoked: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(runner_rows.len());
    for row in &runner_rows {
        let id: Uuid = match row.try_get(0) {
            Ok(id) => id,
            Err(_) => return server_error(),
        };
        let revoked_at: Option<DateTime<Utc>> = match row.try_get(28) {
            Ok(revoked_at) => revoked_at,
            Err(_) => return server_error(),
        };
        runner_ids.push(id);
        runner_revoked.push(revoked_at);
    }

    // Frames for every runner BEFORE any revoke (`:201-203` — the
    // enqueue-vs-evict rationale, same as `delete_runner`).
    {
        let pubsub = TxPubsub::new(&mut tx, &state);
        for runner_id in &runner_ids {
            let outcome =
                pubsub_kernel::send_runner_revoke(&pubsub, *runner_id, FRAME_REASON_DEV_MACHINE)
                    .await;
            for warning in &outcome.warnings {
                tracing::warn!("{warning}");
            }
        }
    }
    // Per-runner revoke + close (`:204-206`), collecting one
    // post-commit bundle per runner, in loop order. Each `revoke()`
    // samples its own `now()` (`models.py:598`).
    let mut bundles = Vec::with_capacity(runner_ids.len());
    for (index, runner_id) in runner_ids.iter().enumerate() {
        let mut stores = TxStores::new(&mut tx, *runner_id);
        let outcome = match revoke_kernel::revoke_runner(
            &mut stores,
            *runner_id,
            runner_revoked[index],
            REVOKE_REASON_DEV_MACHINE,
            now_micros(),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => return server_error(),
        };
        bundles.push(stores.into_bundle(outcome));
        let pubsub = TxPubsub::new(&mut tx, &state);
        if pubsub_kernel::close_runner_session(
            &pubsub,
            *runner_id,
            pubsub_kernel::CLOSE_RUNNER_SESSION_DEFAULT_CODE,
        )
        .await
        .is_err()
        {
            return server_error();
        }
    }
    if tx.commit().await.is_err() {
        return server_error();
    }
    for bundle in bundles {
        if fire_bundle(&pool, &state, bundle).await.is_err() {
            return server_error();
        }
    }
    match serialize_machine(&pool, &machine, user_id, workspace_id, stamped).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// `POST /api/runners/dev-machines/<mid>/rotate/` (`runners.py:211-246`)
// ---------------------------------------------------------------------------

/// `POST /api/runners/dev-machines/<machine_id>/rotate/`
/// (`runners.py:217-246`): token revoke + per-runner revoke frames +
/// session closes inside one atomic. QUIRK-rotate-keeps-runners: the
/// runner ROWS are untouched (no `Runner.revoke`, no bundles). Answers
/// the M5 machine serialization.
pub async fn dev_machine_rotate(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<crate::license::QueryMap>,
    req: Request,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let data = match read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    let workspace_id = match machine_workspace_gate(&pool, user_id, &data, &params).await {
        Ok(workspace_id) => workspace_id,
        Err(response) => return response,
    };
    let machine_id = match path_uuid_web(&raw_id) {
        Ok(machine_id) => machine_id,
        Err(response) => return response,
    };

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    // M6 — locked machine read (`:228`).
    let row: Option<sqlx::postgres::PgRow> =
        match sqlx::query(&manage_reads::machine_locked_read_sql())
            .bind(machine_id)
            .fetch_optional(&mut *tx)
            .await
        {
            Ok(row) => row,
            Err(_) => return server_error(),
        };
    let Some(row) = row else {
        return not_found();
    };
    let machine = match decode_locked_machine(&row) {
        Ok(machine) => machine,
        Err(response) => return response,
    };
    match machine_in_workspace_scope(&mut tx, &machine, user_id, workspace_id).await {
        Ok(true) => {}
        Ok(false) => return not_found(),
        Err(response) => return response,
    };
    // Revoke-after-revoke guard (`:231-235`), before any write.
    if machine.revoked_at.is_some() {
        return conflict(DEV_MACHINE_REVOKED_BODY);
    }

    // Token revoke-all (`:238`).
    let now = now_micros();
    if sqlx::query(&manage_reads::machine_tokens_revoke_sql())
        .bind(now)
        .bind(machine_id)
        .execute(&mut *tx)
        .await
        .is_err()
    {
        return server_error();
    }
    // Active-runner ids (`:239-241`).
    let runner_ids: Vec<Uuid> = match sqlx::query_scalar(&manage_reads::machine_runner_ids_sql())
        .bind(machine_id)
        .fetch_all(&mut *tx)
        .await
    {
        Ok(ids) => ids,
        Err(_) => return server_error(),
    };
    // Per id: revoke frame + session close (`:242-244`). No row writes.
    {
        let pubsub = TxPubsub::new(&mut tx, &state);
        for runner_id in &runner_ids {
            let outcome =
                pubsub_kernel::send_runner_revoke(&pubsub, *runner_id, FRAME_REASON_ROTATE).await;
            for warning in &outcome.warnings {
                tracing::warn!("{warning}");
            }
            if pubsub_kernel::close_runner_session(
                &pubsub,
                *runner_id,
                pubsub_kernel::CLOSE_RUNNER_SESSION_DEFAULT_CODE,
            )
            .await
            .is_err()
            {
                return server_error();
            }
        }
    }
    if tx.commit().await.is_err() {
        return server_error();
    }
    match serialize_machine(&pool, &machine, user_id, workspace_id, None).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// `POST /api/runners/<rid>/revoke/` (`runners.py:455-492`)
// ---------------------------------------------------------------------------

/// `POST /api/runners/<runner_id>/revoke/` (`:471-492`): the row lock
/// spans the read-then-revoke window; view/manage gates; idempotent 200
/// (an already-revoked row returns its current state with no frame, no
/// close, no bundle). The frame + close + re-read run OUTSIDE the tx.
pub async fn runner_revoke(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    Path(raw_id): Path<String>,
) -> Response {
    let (pool, user_id) = match web_actor(&state, extension).await {
        Ok(preamble) => preamble,
        Err(response) => return response,
    };
    let runner_id = match path_uuid_web(&raw_id) {
        Ok(runner_id) => runner_id,
        Err(response) => return response,
    };

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return server_error(),
    };
    let row: Option<sqlx::postgres::PgRow> = match sqlx::query(&runner_locked_read_sql())
        .bind(runner_id)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(row) => row,
        Err(_) => return server_error(),
    };
    let Some(row) = row else {
        return not_found();
    };
    let runner = match decode_full_runner(&row) {
        Ok(runner) => runner,
        Err(response) => return response,
    };
    // View gate → 404 (`:479-480`).
    let can_view = runner_perm::can_view_runner(&runner_perm::RunnerFacts {
        workspace: WorkspaceId::from(runner.workspace_id.to_string()),
        authenticated: true,
        visibility: i32::from(runner.visibility),
        owned_by_requester: runner.owner_id == user_id,
    });
    if !can_view {
        return not_found();
    }
    // Manage gate → 403 (`:481-482`): the kernel takes the admin fact
    // precomputed, so the role read stays lazy exactly like Python
    // (private non-owners never query).
    if runner.visibility == runner_perm::VISIBILITY_PRIVATE as i16 {
        if runner.owner_id != user_id {
            return forbidden();
        }
    } else {
        let role = match workspace_role_in(&mut *tx, runner.workspace_id, user_id).await {
            Ok(role) => role,
            Err(response) => return response,
        };
        let workspace = WorkspaceId::from(runner.workspace_id.to_string());
        let scope = TenantScope::new(workspace.clone());
        let manageable = runner_perm::can_manage_runner(
            &scope,
            &runner_perm::ManageFacts {
                workspace,
                requester: Some(UserId::from(user_id.to_string())),
                visibility: i32::from(runner.visibility),
                owned_by_requester: runner.owner_id == user_id,
                is_workspace_admin: membership::is_workspace_admin(role),
            },
        );
        if !manageable {
            return forbidden();
        }
    }

    let already_revoked = runner.revoked_at.is_some();
    let mut bundle = None;
    if !already_revoked {
        let mut stores = TxStores::new(&mut tx, runner_id);
        let outcome = match revoke_kernel::revoke_runner(
            &mut stores,
            runner_id,
            runner.revoked_at,
            REVOKE_REASON_MANUAL,
            now_micros(),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => return server_error(),
        };
        bundle = Some(stores.into_bundle(outcome));
    }
    if tx.commit().await.is_err() {
        return server_error();
    }
    // The frame + close run OUTSIDE the tx, only on a fresh revoke
    // (`:488-490`); the re-read + serialize run in both arms (`:491-492`).
    if let Some(bundle) = bundle {
        if fire_bundle(&pool, &state, bundle).await.is_err() {
            return server_error();
        }
        let pubsub = PoolPubsubStore::new(&pool, &state);
        let outcome =
            pubsub_kernel::send_runner_revoke(&pubsub, runner_id, FRAME_REASON_RUNNER_REVOKE).await;
        for warning in &outcome.warnings {
            tracing::warn!("{warning}");
        }
        if pubsub_kernel::close_runner_session(
            &pubsub,
            runner_id,
            pubsub_kernel::CLOSE_RUNNER_SESSION_DEFAULT_CODE,
        )
        .await
        .is_err()
        {
            return server_error();
        }
    }
    match render_revoked_runner(&pool, runner_id).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(response) => response,
    }
}

// ---------------------------------------------------------------------------
// Renders (owned strings borrowed into the `shapes` kernels)
// ---------------------------------------------------------------------------

/// Owned render buffer for one dev machine + its M5 annotations.
struct MachineRender {
    id: String,
    host_label: String,
    label: String,
    visibility: i64,
    runner_count: Option<i64>,
    online_runner_count: Option<i64>,
    control_online: Option<bool>,
    last_seen_at: Option<String>,
    last_heartbeat_at: Option<Option<String>>,
    revoked_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl MachineRender {
    fn to_json(&self) -> Result<String, Response> {
        let row = shapes::DevMachineRow {
            id: &self.id,
            host_label: &self.host_label,
            label: &self.label,
            visibility: self.visibility,
            runner_count: self.runner_count,
            online_runner_count: self.online_runner_count,
            control_online: self.control_online,
            last_seen_at: self.last_seen_at.as_deref(),
            last_heartbeat_at: self
                .last_heartbeat_at
                .as_ref()
                .map(|inner| inner.as_deref()),
            revoked_at: self.revoked_at.as_deref(),
            created_at: &self.created_at,
            updated_at: &self.updated_at,
        };
        serde_json::to_string(&shapes::dev_machine_to_representation(&row))
            .map_err(|_| server_error())
    }
}

/// `_serialize_dev_machine` (`runners.py:150-170`): the M5 annotated
/// re-read (post-commit, with a fresh presence cutoff), or the
/// unannotated in-memory instance when the row vanished
/// (QUIRK-m5-fallback — `stamped` carries the revoke write's
/// `(revoked_at, updated_at)` when it ran).
async fn serialize_machine(
    pool: &PgPool,
    machine: &LockedMachine,
    user_id: Uuid,
    workspace_id: Uuid,
    stamped: Option<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<String, Response> {
    let visibility: i16 = runner_perm::VISIBILITY_PRIVATE as i16;
    let cutoff =
        now_micros() - chrono::Duration::seconds(manage_reads::CONTROL_PRESENCE_WINDOW_SECS);
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&manage_reads::machine_serialize_sql())
        .bind(user_id)
        .bind(visibility)
        .bind(workspace_id)
        .bind(user_id)
        .bind(visibility)
        .bind(workspace_id)
        .bind("online")
        .bind("busy")
        .bind(user_id)
        .bind(visibility)
        .bind(workspace_id)
        .bind(1i32)
        .bind(cutoff)
        .bind(machine.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        // QUIRK-m5-fallback: counts omitted, `control_online` false,
        // model fields from the in-memory instance (stamped when the
        // revoke write ran).
        let (revoked_at, updated_at) = match stamped {
            Some((revoked_at, updated_at)) => (Some(revoked_at), updated_at),
            None => (machine.revoked_at, machine.updated_at),
        };
        return MachineRender {
            id: machine.id.to_string(),
            host_label: machine.host_label.clone(),
            label: machine.label.clone(),
            visibility: i64::from(machine.visibility),
            runner_count: None,
            online_runner_count: None,
            control_online: None,
            last_seen_at: machine
                .last_seen_at
                .as_ref()
                .map(crate::serializer::render_datetime),
            last_heartbeat_at: None,
            revoked_at: revoked_at.as_ref().map(crate::serializer::render_datetime),
            created_at: crate::serializer::render_datetime(&machine.created_at),
            updated_at: crate::serializer::render_datetime(&updated_at),
        }
        .to_json();
    };
    let last_seen_at: Option<DateTime<Utc>> = row.try_get(6).map_err(|_| server_error())?;
    let revoked_at: Option<DateTime<Utc>> = row.try_get(7).map_err(|_| server_error())?;
    let created_at: DateTime<Utc> = row.try_get(8).map_err(|_| server_error())?;
    let updated_at: DateTime<Utc> = row.try_get(9).map_err(|_| server_error())?;
    let last_heartbeat_at: Option<DateTime<Utc>> = row.try_get(12).map_err(|_| server_error())?;
    MachineRender {
        id: machine.id.to_string(),
        host_label: row.try_get(2).map_err(|_| server_error())?,
        label: row.try_get(3).map_err(|_| server_error())?,
        visibility: i64::from(machine.visibility),
        runner_count: Some(row.try_get(10).map_err(|_| server_error())?),
        online_runner_count: Some(row.try_get(11).map_err(|_| server_error())?),
        control_online: Some(row.try_get(13).map_err(|_| server_error())?),
        last_seen_at: last_seen_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        last_heartbeat_at: Some(
            last_heartbeat_at
                .as_ref()
                .map(crate::serializer::render_datetime),
        ),
        revoked_at: revoked_at.as_ref().map(crate::serializer::render_datetime),
        created_at: crate::serializer::render_datetime(&created_at),
        updated_at: crate::serializer::render_datetime(&updated_at),
    }
    .to_json()
}

/// Owned `runner` row for gates + full rendering (token internals and
/// `free_worktrees` skipped — never serialized here).
struct FullRunner {
    id: Uuid,
    owner_id: Uuid,
    workspace_id: Uuid,
    dev_machine_id: Option<Uuid>,
    pod_id: Uuid,
    name: String,
    host_label: String,
    provisioning: String,
    visibility: i16,
    enrolled_at: Option<DateTime<Utc>>,
    capabilities: Value,
    status: String,
    os: String,
    arch: String,
    runner_version: String,
    dev_metadata: Value,
    protocol_version: i32,
    last_heartbeat_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    revoked_reason: String,
}

/// Decode a full runner row positionally (`r_cols::COLUMNS` order —
/// indices pinned by `runner_decode_indices_follow_columns`).
fn decode_full_runner(row: &sqlx::postgres::PgRow) -> Result<FullRunner, Response> {
    Ok(FullRunner {
        id: row.try_get(0).map_err(|_| server_error())?,
        owner_id: row.try_get(1).map_err(|_| server_error())?,
        workspace_id: row.try_get(2).map_err(|_| server_error())?,
        dev_machine_id: row.try_get(3).map_err(|_| server_error())?,
        pod_id: row.try_get(4).map_err(|_| server_error())?,
        name: row.try_get(5).map_err(|_| server_error())?,
        host_label: row.try_get(6).map_err(|_| server_error())?,
        provisioning: row.try_get(7).map_err(|_| server_error())?,
        visibility: row.try_get(8).map_err(|_| server_error())?,
        enrolled_at: row.try_get(16).map_err(|_| server_error())?,
        capabilities: row.try_get(17).map_err(|_| server_error())?,
        status: row.try_get(18).map_err(|_| server_error())?,
        os: row.try_get(19).map_err(|_| server_error())?,
        arch: row.try_get(20).map_err(|_| server_error())?,
        runner_version: row.try_get(21).map_err(|_| server_error())?,
        dev_metadata: row.try_get(22).map_err(|_| server_error())?,
        protocol_version: row.try_get(23).map_err(|_| server_error())?,
        last_heartbeat_at: row.try_get(24).map_err(|_| server_error())?,
        created_at: row.try_get(26).map_err(|_| server_error())?,
        updated_at: row.try_get(27).map_err(|_| server_error())?,
        revoked_at: row.try_get(28).map_err(|_| server_error())?,
        revoked_reason: row.try_get(29).map_err(|_| server_error())?,
    })
}

/// Owned pod mini (`serializers.py:133-142`).
struct PodMiniOwned {
    id: String,
    name: String,
    is_default: bool,
    project_id: Uuid,
}

/// Owned `dev_machine` mini (`serializers.py:145-153`).
struct DevMachineMiniOwned {
    id: String,
    host_label: String,
    label: String,
}

/// Owned live-state snapshot (`RunnerLiveState`, `models.py:1454-1500`).
struct LiveStateOwned {
    observed_run_id: Option<String>,
    last_event_at: Option<String>,
    last_event_kind: Option<String>,
    last_event_summary: Option<String>,
    agent_pid: Option<i64>,
    agent_subprocess_alive: Option<bool>,
    approvals_pending: Option<i64>,
    usage: Value,
    llm_model: Option<String>,
    turn_count: Option<i64>,
    updated_at: String,
}

/// The reverse one-to-one `runner.live_state` read: `None` when the row
/// is missing (pre-flag runners render `live_state: null`).
async fn fetch_live_state(
    pool: &PgPool,
    runner_id: Uuid,
) -> Result<Option<LiveStateOwned>, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(LIVE_STATE_SQL)
        .bind(runner_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let observed: Option<Uuid> = row.try_get(1).map_err(|_| server_error())?;
    let last_event_at: Option<DateTime<Utc>> = row.try_get(2).map_err(|_| server_error())?;
    let updated_at: DateTime<Utc> = row.try_get(11).map_err(|_| server_error())?;
    let agent_pid: Option<i32> = row.try_get(5).map_err(|_| server_error())?;
    let approvals_pending: Option<i32> = row.try_get(7).map_err(|_| server_error())?;
    let turn_count: Option<i32> = row.try_get(10).map_err(|_| server_error())?;
    Ok(Some(LiveStateOwned {
        observed_run_id: observed.map(|id| id.to_string()),
        last_event_at: last_event_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        last_event_kind: row.try_get(3).map_err(|_| server_error())?,
        last_event_summary: row.try_get(4).map_err(|_| server_error())?,
        agent_pid: agent_pid.map(i64::from),
        agent_subprocess_alive: row.try_get(6).map_err(|_| server_error())?,
        approvals_pending: approvals_pending.map(i64::from),
        usage: row.try_get(8).map_err(|_| server_error())?,
        llm_model: row.try_get(9).map_err(|_| server_error())?,
        turn_count: turn_count.map(i64::from),
        updated_at: crate::serializer::render_datetime(&updated_at),
    }))
}

/// Owned render buffer for one runner + its pod/dev-machine minis.
struct RunnerRender {
    id: String,
    name: String,
    status: String,
    host_label: String,
    provisioning: String,
    os: String,
    arch: String,
    runner_version: String,
    dev_metadata: Value,
    protocol_version: i64,
    capabilities: Value,
    last_heartbeat_at: Option<String>,
    owner: String,
    dev_machine: Option<String>,
    dev_machine_detail: Option<DevMachineMiniOwned>,
    visibility: i64,
    pod: String,
    pod_detail: PodMiniOwned,
    pod_identifier: String,
    live_state: Option<LiveStateOwned>,
    enrolled_at: Option<String>,
    revoked_at: Option<String>,
    revoked_reason: String,
    created_at: String,
    updated_at: String,
}

impl RunnerRender {
    fn to_json(&self) -> Result<String, Response> {
        let project_id = self.pod_detail.project_id.to_string();
        let pod = shapes::PodMiniRow {
            id: &self.pod_detail.id,
            name: &self.pod_detail.name,
            is_default: self.pod_detail.is_default,
            project: &project_id,
            project_identifier: &self.pod_identifier,
        };
        let dev_machine =
            self.dev_machine_detail
                .as_ref()
                .map(|detail| shapes::DevMachineMiniRow {
                    id: &detail.id,
                    host_label: &detail.host_label,
                    label: &detail.label,
                });
        let live_state = self.live_state.as_ref().map(|live| shapes::LiveStateRow {
            observed_run_id: live.observed_run_id.as_deref(),
            last_event_at: live.last_event_at.as_deref(),
            last_event_kind: live.last_event_kind.as_deref(),
            last_event_summary: live.last_event_summary.as_deref(),
            agent_pid: live.agent_pid,
            agent_subprocess_alive: live.agent_subprocess_alive,
            approvals_pending: live.approvals_pending,
            usage: &live.usage,
            llm_model: live.llm_model.as_deref(),
            turn_count: live.turn_count,
            updated_at: &live.updated_at,
        });
        let row = shapes::RunnerRow {
            id: &self.id,
            name: &self.name,
            status: &self.status,
            host_label: &self.host_label,
            provisioning: &self.provisioning,
            os: &self.os,
            arch: &self.arch,
            runner_version: &self.runner_version,
            dev_metadata: &self.dev_metadata,
            protocol_version: self.protocol_version,
            capabilities: &self.capabilities,
            last_heartbeat_at: self.last_heartbeat_at.as_deref(),
            owner: &self.owner,
            dev_machine: self.dev_machine.as_deref(),
            dev_machine_detail: dev_machine,
            visibility: self.visibility,
            pod: &self.pod,
            pod_detail: pod,
            live_state,
            enrolled_at: self.enrolled_at.as_deref(),
            revoked_at: self.revoked_at.as_deref(),
            revoked_reason: &self.revoked_reason,
            created_at: &self.created_at,
            updated_at: &self.updated_at,
        };
        serde_json::to_string(&shapes::runner_to_representation(&row)).map_err(|_| server_error())
    }
}

/// Serialize the re-read runner for the revoke 200 (`:491-492`): the
/// refresh row plus the serializer's lazy loads (pod, dev-machine mini,
/// project identifier, live state). A vanished row is the source's
/// `DoesNotExist` → 500.
async fn render_revoked_runner(pool: &PgPool, runner_id: Uuid) -> Result<String, Response> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&runner_refresh_sql())
        .bind(runner_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(server_error());
    };
    let runner = decode_full_runner(&row)?;
    // Lazy `runner.pod` (`Pod.objects.get` — vanished row 500s).
    let pod_row: Option<sqlx::postgres::PgRow> = sqlx::query(&pod_lazy_sql())
        .bind(runner.pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| server_error())?;
    let Some(pod_row) = pod_row else {
        return Err(server_error());
    };
    let pod_id: Uuid = pod_row.try_get(0).map_err(|_| server_error())?;
    let project_id: Uuid = pod_row.try_get(2).map_err(|_| server_error())?;
    let pod = PodMiniOwned {
        id: pod_id.to_string(),
        name: pod_row.try_get(3).map_err(|_| server_error())?,
        is_default: pod_row.try_get(6).map_err(|_| server_error())?,
        project_id,
    };
    // Lazy `runner.dev_machine` (only when linked; vanished row 500s).
    let mut dev_machine_detail = None;
    if let Some(dev_machine_id) = runner.dev_machine_id {
        let machine_row: Option<sqlx::postgres::PgRow> = sqlx::query(&machine_lazy_sql())
            .bind(dev_machine_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
        let Some(machine_row) = machine_row else {
            return Err(server_error());
        };
        let mini_id: Uuid = machine_row.try_get(0).map_err(|_| server_error())?;
        dev_machine_detail = Some(DevMachineMiniOwned {
            id: mini_id.to_string(),
            host_label: machine_row.try_get(2).map_err(|_| server_error())?,
            label: machine_row.try_get(3).map_err(|_| server_error())?,
        });
    }
    // Lazy `pod.project.identifier` (the `catalog_reads` full-row read —
    // the builder's `ORDER BY` is unobservable on a PK lookup).
    let project_row: Option<sqlx::postgres::PgRow> =
        sqlx::query(&catalog_reads::project_by_id_sql())
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| server_error())?;
    let Some(project_row) = project_row else {
        return Err(server_error());
    };
    let pod_identifier: String = project_row
        .try_get("identifier")
        .map_err(|_| server_error())?;
    let live_state = fetch_live_state(pool, runner_id).await?;
    RunnerRender {
        id: runner.id.to_string(),
        name: runner.name.clone(),
        status: runner.status.clone(),
        host_label: runner.host_label.clone(),
        provisioning: runner.provisioning.clone(),
        os: runner.os.clone(),
        arch: runner.arch.clone(),
        runner_version: runner.runner_version.clone(),
        dev_metadata: runner.dev_metadata.clone(),
        protocol_version: i64::from(runner.protocol_version),
        capabilities: runner.capabilities.clone(),
        last_heartbeat_at: runner
            .last_heartbeat_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        owner: runner.owner_id.to_string(),
        dev_machine: runner.dev_machine_id.map(|id| id.to_string()),
        dev_machine_detail,
        visibility: i64::from(runner.visibility),
        pod: runner.pod_id.to_string(),
        pod_detail: pod,
        pod_identifier,
        live_state,
        enrolled_at: runner
            .enrolled_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        revoked_at: runner
            .revoked_at
            .as_ref()
            .map(crate::serializer::render_datetime),
        revoked_reason: runner.revoked_reason.clone(),
        created_at: crate::serializer::render_datetime(&runner.created_at),
        updated_at: crate::serializer::render_datetime(&runner.updated_at),
    }
    .to_json()
}

// ---------------------------------------------------------------------------
// Executors: the first pool-backed `RevokeStore` (F6: called, never inlined)
// ---------------------------------------------------------------------------

use pidash_db::runner_sessions::models::runner_session as db_runner_session;
use pidash_db::runner_sessions::outbox as session_outbox;
use pidash_db::runner_sessions::outbox::OutboxError as SessionOutboxError;
use pidash_db::runner_sessions::RunnerSession as DbRunnerSession;
use pidash_services::runner_enroll::delete as delete_kernel;
use pidash_services::runner_enroll::revoke::{RevokeError, RevokeOutcome, RevokeStore};
use pidash_services::runner_runs::LifecycleEffect;
use pidash_services::runner_sessions::pubsub::PubsubStore;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::runner_runs::{AgentRunStatus, TERMINAL_RUN_STATUSES};

/// One revoke's post-commit effects, in Python's registration order:
/// the per-run publish pairs (registered during S3, in run order) fire
/// first, then the handoffs, drains, and stream cleanup (`models.py`
/// `finalize_agent_run:125` + `Runner.revoke:673-685`).
struct EffectBundle {
    runner_id: Uuid,
    publish: Vec<LifecycleEffect>,
    handoffs: Vec<Uuid>,
    drains: Vec<Uuid>,
    cleanup: Option<Uuid>,
}

/// Pool-backed [`RevokeStore`]: S1-S4 SQL runs on the caller's
/// transaction; the finalize's publish pairs and the revoke collectors
/// accumulate for post-commit firing.
struct TxStores<'t, 'p> {
    tx: &'t mut sqlx::Transaction<'p, sqlx::Postgres>,
    runner_id: Uuid,
    publish: Vec<LifecycleEffect>,
    handoffs: Vec<Uuid>,
    drains: Vec<Uuid>,
    cleanup: Option<Uuid>,
}

impl<'t, 'p> TxStores<'t, 'p> {
    fn new(tx: &'t mut sqlx::Transaction<'p, sqlx::Postgres>, runner_id: Uuid) -> Self {
        Self {
            tx,
            runner_id,
            publish: Vec::new(),
            handoffs: Vec::new(),
            drains: Vec::new(),
            cleanup: None,
        }
    }

    /// Consume the stores into the post-commit bundle, logging the
    /// driver's warnings (the executor owns them per `revoke.rs`).
    fn into_bundle(self, outcome: RevokeOutcome) -> EffectBundle {
        for warning in &outcome.warnings {
            tracing::warn!("{warning}");
        }
        EffectBundle {
            runner_id: self.runner_id,
            publish: self.publish,
            handoffs: self.handoffs,
            drains: self.drains,
            cleanup: self.cleanup,
        }
    }
}

/// The revoke-finalize SET order (planned `status`, `ended_at`,
/// `queue_position`, the two markers, then our extras in dict order):
/// pinned so a kernel drift fails loudly instead of mis-binding.
const REVOKE_FINALIZE_COLUMNS: [&str; 8] = [
    "status",
    "ended_at",
    "queue_position",
    "terminal_hooks_applied_at",
    "terminal_capacity_released_at",
    "error",
    "error_code",
    "cancel_reason",
];

impl RevokeStore for TxStores<'_, '_> {
    async fn mark_runner_revoked(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        reason: &str,
    ) -> Result<(), RevokeError> {
        sqlx::query(revoke_kernel::MARK_RUNNER_REVOKED_SQL)
            .bind(now)
            .bind(reason)
            .bind(runner_id)
            .execute(&mut **self.tx)
            .await
            .map_err(|error| RevokeError::Store(error.to_string()))?;
        Ok(())
    }

    async fn revoke_active_sessions(
        &mut self,
        runner_id: Uuid,
        now: DateTime<Utc>,
        stored_reason: &str,
    ) -> Result<(), RevokeError> {
        sqlx::query(revoke_kernel::REVOKE_ACTIVE_SESSIONS_SQL)
            .bind(now)
            .bind(stored_reason)
            .bind(runner_id)
            .execute(&mut **self.tx)
            .await
            .map_err(|error| RevokeError::Store(error.to_string()))?;
        Ok(())
    }

    async fn lock_active_runs(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<(Uuid, Option<Uuid>)>, RevokeError> {
        sqlx::query_as::<_, (Uuid, Option<Uuid>)>(&revoke_kernel::active_runs_lock_sql())
            .bind(runner_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(|error| RevokeError::Store(error.to_string()))
    }

    async fn finalize_cancelled_run(
        &mut self,
        run_id: Uuid,
        runner_id: Uuid,
        stored_reason: &str,
    ) -> Result<(), RevokeError> {
        use pidash_services::runner_runs::SetValue;
        // `finalize_agent_run(run, "cancelled", error="runner revoked",
        // error_code="runner_revoked", cancel_reason=<stored>)`: no
        // `done_payload` merge (these updates carry none).
        let values = finalize_kernel::plan_finalize_values(
            AgentRunStatus::Cancelled,
            &[
                ("error", SetValue::Text("runner revoked".to_owned())),
                ("error_code", SetValue::Text("runner_revoked".to_owned())),
                ("cancel_reason", SetValue::Text(stored_reason.to_owned())),
            ],
        )
        .map_err(|error| RevokeError::Store(error.to_string()))?;
        let columns: Vec<&str> = values.clauses.iter().map(|clause| clause.column).collect();
        if columns.as_slice() != REVOKE_FINALIZE_COLUMNS {
            return Err(RevokeError::Store(format!(
                "revoke finalize columns drifted: {columns:?}"
            )));
        }
        // First-writer-wins lock (`with_runner=true`): a lost race is a
        // silent `Ok` (Python ignores `False`).
        let lock_sql = finalize_kernel::lock_run_for_finalize_sql(true, false);
        let mut locked = sqlx::query(&lock_sql).bind(run_id);
        for status in TERMINAL_RUN_STATUSES {
            locked = locked.bind(status.value());
        }
        locked = locked.bind(runner_id);
        let row: Option<sqlx::postgres::PgRow> = locked
            .fetch_optional(&mut **self.tx)
            .await
            .map_err(|error| RevokeError::Store(error.to_string()))?;
        let Some(row) = row else {
            return Ok(());
        };
        // Clause-order binds (`finalize_update_sql`).
        sqlx::query(&finalize_kernel::finalize_update_sql(&values))
            .bind(AgentRunStatus::Cancelled.value())
            .bind(chrono::Utc::now())
            .bind(None::<i16>)
            .bind(None::<DateTime<Utc>>)
            .bind(None::<DateTime<Utc>>)
            .bind("runner revoked")
            .bind("runner_revoked")
            .bind(stored_reason)
            .bind(run_id)
            .execute(&mut **self.tx)
            .await
            .map_err(|error| RevokeError::Store(error.to_string()))?;
        // The cloud-only terminal event (the `runs.rs` position).
        let executor_kind: String = row
            .try_get("executor_kind")
            .map_err(|error| RevokeError::Store(error.to_string()))?;
        if AgentExecutorKind::from_value(&executor_kind) == Some(AgentExecutorKind::CloudAgent) {
            let exists = sqlx::query_scalar::<_, i32>(finalize_kernel::terminal_event_exists_sql())
                .bind(run_id)
                .bind("terminal")
                .fetch_optional(&mut **self.tx)
                .await
                .map_err(|error| RevokeError::Store(error.to_string()))?;
            if exists.is_none() {
                let max_seq =
                    sqlx::query_scalar::<_, i32>(finalize_kernel::terminal_event_max_seq_sql())
                        .bind(run_id)
                        .fetch_optional(&mut **self.tx)
                        .await
                        .map_err(|error| RevokeError::Store(error.to_string()))?;
                let plan = finalize_kernel::plan_terminal_event(
                    max_seq,
                    AgentRunStatus::Cancelled,
                    &finalize_kernel::finalize_error_code(&values),
                );
                sqlx::query(&finalize_kernel::terminal_event_insert_sql())
                    .bind(run_id)
                    .bind(plan.seq)
                    .bind("terminal")
                    .bind(plan.payload)
                    .bind(chrono::Utc::now())
                    .execute(&mut **self.tx)
                    .await
                    .map_err(|error| RevokeError::Store(error.to_string()))?;
            }
        }
        // D-15's terminal-effects `on_commit` (registered in-tx, fires
        // first post-commit).
        self.publish
            .extend(finalize_kernel::plan_publish_effects(run_id));
        Ok(())
    }

    async fn pinned_queued_pod_ids(
        &mut self,
        runner_id: Uuid,
    ) -> Result<Vec<Option<Uuid>>, RevokeError> {
        sqlx::query_scalar(revoke_kernel::PINNED_QUEUED_PODS_SQL)
            .bind(runner_id)
            .fetch_all(&mut **self.tx)
            .await
            .map_err(|error| RevokeError::Store(error.to_string()))
    }

    async fn unpin_queued_runs(&mut self, runner_id: Uuid) -> Result<(), RevokeError> {
        sqlx::query(revoke_kernel::UNPIN_QUEUED_RUNS_SQL)
            .bind(runner_id)
            .execute(&mut **self.tx)
            .await
            .map_err(|error| RevokeError::Store(error.to_string()))?;
        Ok(())
    }

    fn complete_handoff_after_commit(&mut self, run_id: Uuid) {
        self.handoffs.push(run_id);
    }

    fn drain_pod_after_commit(&mut self, pod_id: Uuid) {
        self.drains.push(pod_id);
    }

    fn schedule_stream_cleanup_after_commit(&mut self, runner_id: Uuid) {
        self.cleanup = Some(runner_id);
    }
}

// ---------------------------------------------------------------------------
// Executors: the first pool-backed `PubsubStore`s (F8: called, never inlined)
// ---------------------------------------------------------------------------

/// Pool-backed [`PubsubStore`] for the out-of-tx frames and closes
/// (post-commit paths): each statement autocommits, like the source
/// paths that run outside any `atomic`.
struct PoolPubsubStore<'p, 's> {
    pool: &'p PgPool,
    state: &'s AppState,
}

impl<'p, 's> PoolPubsubStore<'p, 's> {
    fn new(pool: &'p PgPool, state: &'s AppState) -> Self {
        Self { pool, state }
    }
}

impl PubsubStore for PoolPubsubStore<'_, '_> {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, SessionOutboxError> {
        let redis = redis_client(self.state);
        session_outbox::enqueue_for_runner(
            redis.as_ref(),
            self.pool,
            &self.state.settings().runner,
            runner_id,
            message,
        )
        .await
    }

    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, pidash_db::runner_sessions::machine_outbox::MachineOutboxError>
    {
        let redis = redis_client(self.state);
        pidash_db::runner_sessions::machine_outbox::enqueue_for_machine(
            redis.as_ref(),
            self.pool,
            &self.state.settings().runner,
            dev_machine_id,
            message,
        )
        .await
    }

    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<DbRunnerSession>, SessionOutboxError> {
        let rows = sqlx::query(pubsub_kernel::CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(self.pool)
            .await
            .map_err(SessionOutboxError::Db)?;
        rows.iter()
            .map(db_runner_session::runner_session_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(SessionOutboxError::Db)
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), SessionOutboxError> {
        sqlx::query(db_runner_session::REVOKE_SQL)
            .bind(now_micros())
            .bind(reason)
            .bind(session_id)
            .execute(self.pool)
            .await
            .map_err(SessionOutboxError::Db)?;
        Ok(())
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), SessionOutboxError> {
        let redis = redis_client(self.state);
        session_outbox::clear_session_marker(redis.as_ref(), &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), SessionOutboxError> {
        let redis = redis_client(self.state);
        let runner_id = runner_id.to_string();
        let old = old_session_id.to_string();
        session_outbox::publish_session_eviction(
            redis.as_ref(),
            &runner_id,
            Some(&old),
            new_session_id,
        )
        .await
    }
}

/// Tx-backed [`PubsubStore`] for the in-tx frames and closes (machine
/// revoke/rotate run inside one atomic — QUIRK-machine-tx). The
/// transaction sits behind an async mutex: the `&self` driver methods
/// lock it per call (the drivers never re-enter the store, so the
/// lock is uncontended), keeping the handler future `Send` with no
/// `unsafe`.
struct TxPubsub<'t, 'p, 's> {
    tx: tokio::sync::Mutex<&'t mut sqlx::Transaction<'p, sqlx::Postgres>>,
    state: &'s AppState,
}

impl<'t, 'p, 's> TxPubsub<'t, 'p, 's> {
    fn new(tx: &'t mut sqlx::Transaction<'p, sqlx::Postgres>, state: &'s AppState) -> Self {
        Self {
            tx: tokio::sync::Mutex::new(tx),
            state,
        }
    }
}

impl PubsubStore for TxPubsub<'_, '_, '_> {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, SessionOutboxError> {
        let mut guard = self.tx.lock().await;
        let tx: &mut sqlx::Transaction<'_, sqlx::Postgres> = &mut guard;
        let redis = redis_client(self.state);
        session_outbox::enqueue_for_runner(
            redis.as_ref(),
            &mut **tx,
            &self.state.settings().runner,
            runner_id,
            message,
        )
        .await
    }

    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, pidash_db::runner_sessions::machine_outbox::MachineOutboxError>
    {
        let mut guard = self.tx.lock().await;
        let tx: &mut sqlx::Transaction<'_, sqlx::Postgres> = &mut guard;
        let redis = redis_client(self.state);
        pidash_db::runner_sessions::machine_outbox::enqueue_for_machine(
            redis.as_ref(),
            &mut **tx,
            &self.state.settings().runner,
            dev_machine_id,
            message,
        )
        .await
    }

    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<DbRunnerSession>, SessionOutboxError> {
        let mut guard = self.tx.lock().await;
        let tx: &mut sqlx::Transaction<'_, sqlx::Postgres> = &mut guard;
        let rows = sqlx::query(pubsub_kernel::CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(&mut **tx)
            .await
            .map_err(SessionOutboxError::Db)?;
        rows.iter()
            .map(db_runner_session::runner_session_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(SessionOutboxError::Db)
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), SessionOutboxError> {
        let mut guard = self.tx.lock().await;
        let tx: &mut sqlx::Transaction<'_, sqlx::Postgres> = &mut guard;
        sqlx::query(db_runner_session::REVOKE_SQL)
            .bind(now_micros())
            .bind(reason)
            .bind(session_id)
            .execute(&mut **tx)
            .await
            .map_err(SessionOutboxError::Db)?;
        Ok(())
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), SessionOutboxError> {
        let redis = redis_client(self.state);
        session_outbox::clear_session_marker(redis.as_ref(), &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), SessionOutboxError> {
        let redis = redis_client(self.state);
        let runner_id = runner_id.to_string();
        let old = old_session_id.to_string();
        session_outbox::publish_session_eviction(
            redis.as_ref(),
            &runner_id,
            Some(&old),
            new_session_id,
        )
        .await
    }
}

/// One post-commit step, in fire order.
enum FireOp {
    Publish(LifecycleEffect),
    Handoff(Uuid),
    Drain(Uuid),
    Cleanup(Uuid),
}

/// Flatten one revoke's bundle into fire order: the publish pairs
/// first (registered during S3, in run order), then handoffs, drains,
/// and the stream cleanup (`models.py` `finalize_agent_run:125` +
/// `Runner.revoke:673-685`). Pure, so the order is unit-pinned.
fn fire_plan(bundle: EffectBundle) -> Vec<FireOp> {
    let mut plan =
        Vec::with_capacity(bundle.publish.len() + bundle.handoffs.len() + bundle.drains.len() + 1);
    plan.extend(bundle.publish.into_iter().map(FireOp::Publish));
    plan.extend(bundle.handoffs.into_iter().map(FireOp::Handoff));
    plan.extend(bundle.drains.into_iter().map(FireOp::Drain));
    plan.extend(bundle.cleanup.into_iter().map(FireOp::Cleanup));
    plan
}

/// Drain one publish effect, isolated with its failure log
/// (`agent_run_finalization.py` `_publish_effects`: the emit and the
/// inline apply sit in separate `try` blocks).
async fn drain_one_publish(
    pool: &PgPool,
    ports: &crate::runner_runs::LivePorts,
    effect: LifecycleEffect,
) {
    let label = match &effect {
        LifecycleEffect::PublishTerminalEffects { run_id } => {
            format!("failed to publish terminal effects for run {run_id}")
        }
        LifecycleEffect::ApplyTerminalEffectsInline { run_id } => {
            format!("failed to apply terminal effects for run {run_id}")
        }
        unexpected => format!("failed to drain unexpected terminal effect: {unexpected:?}"),
    };
    if crate::runner_runs::run_endpoints::drain_lifecycle_effects(pool, ports, vec![effect])
        .await
        .is_err()
    {
        tracing::error!("{label}");
    }
}

/// Drain finalized runs' publish pairs in order.
async fn drain_publish_effects(pool: &PgPool, state: &AppState, effects: Vec<LifecycleEffect>) {
    if effects.is_empty() {
        return;
    }
    let ports = crate::runner_runs::LivePorts::new(pool.clone(), state);
    for effect in effects {
        drain_one_publish(pool, &ports, effect).await;
    }
}

/// Fire one revoke's post-commit bundle (`models.py:597-685`): publish
/// effects isolated each, handoffs isolated each (swallowed after the
/// failure log), drains and cleanup propagating (the source registers
/// them bare, and a raise stops Django's `on_commit` chain).
async fn fire_bundle(pool: &PgPool, state: &AppState, bundle: EffectBundle) -> Result<(), ()> {
    let runner_id = bundle.runner_id;
    let ports = crate::runner_runs::LivePorts::new(pool.clone(), state);
    for op in fire_plan(bundle) {
        match op {
            FireOp::Publish(effect) => drain_one_publish(pool, &ports, effect).await,
            FireOp::Handoff(run_id) => {
                if fire_handoff(pool, state, run_id).await.is_err() {
                    tracing::warn!(
                        "{}",
                        revoke_kernel::handoff_failure_log(&runner_id, &run_id)
                    );
                }
            }
            FireOp::Drain(pod_id) => drain_pod_by_id(pool, state, pod_id).await?,
            FireOp::Cleanup(runner_id) => {
                let redis = redis_client(state);
                session_outbox::schedule_stream_cleanup_for_runner(
                    redis.as_ref(),
                    &state.settings().runner,
                    &runner_id.to_string(),
                )
                .await
                .map_err(|_| ())?;
            }
        }
    }
    Ok(())
}

/// The self-revoke row delete (`enrollment.py:533`):
/// `Runner.objects.filter(pk).delete()` runs Django's collector over the
/// one row — same sequence as `delete_collected_runner_rows` (restated
/// order, shared builders), on the pool (no tx on this path).
async fn delete_collected_runner(pool: &PgPool, runner_id: Uuid) -> Result<(), ()> {
    let root: Option<sqlx::postgres::PgRow> = sqlx::query(delete_kernel::COLLECT_RUNNER_BY_ID_SQL)
        .bind(runner_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ())?;
    if root.is_none() {
        return Ok(());
    }
    let chat_ids: Vec<Uuid> = sqlx::query_scalar(&delete_kernel::collect_chat_ids_sql(1))
        .bind(runner_id)
        .fetch_all(pool)
        .await
        .map_err(|_| ())?;
    let mut message_ids: Vec<Uuid> = Vec::new();
    if !chat_ids.is_empty() {
        let fetch_sql = delete_kernel::collect_message_ids_sql(chat_ids.len());
        let mut fetched = sqlx::query_scalar(&fetch_sql);
        for chat_id in &chat_ids {
            fetched = fetched.bind(*chat_id);
        }
        message_ids = fetched.fetch_all(pool).await.map_err(|_| ())?;
        let events_sql = delete_kernel::delete_chat_events_sql(chat_ids.len());
        let approvals_sql = delete_kernel::delete_chat_approvals_sql(chat_ids.len());
        let dedupes_sql = delete_kernel::delete_chat_dedupes_sql(chat_ids.len());
        let mut events = sqlx::query(&events_sql);
        let mut approvals = sqlx::query(&approvals_sql);
        let mut dedupes = sqlx::query(&dedupes_sql);
        for chat_id in &chat_ids {
            events = events.bind(*chat_id);
            approvals = approvals.bind(*chat_id);
            dedupes = dedupes.bind(*chat_id);
        }
        events.execute(pool).await.map_err(|_| ())?;
        approvals.execute(pool).await.map_err(|_| ())?;
        dedupes.execute(pool).await.map_err(|_| ())?;
    }
    sqlx::query(&delete_kernel::delete_runner_sessions_sql(1))
        .bind(runner_id)
        .execute(pool)
        .await
        .map_err(|_| ())?;
    sqlx::query(&delete_kernel::delete_force_refresh_sql(1))
        .bind(runner_id)
        .execute(pool)
        .await
        .map_err(|_| ())?;
    sqlx::query(&delete_kernel::delete_live_state_sql(1))
        .bind(runner_id)
        .execute(pool)
        .await
        .map_err(|_| ())?;
    sqlx::query(&delete_kernel::null_run_runners_sql(1))
        .bind(runner_id)
        .execute(pool)
        .await
        .map_err(|_| ())?;
    sqlx::query(&delete_kernel::null_run_pins_sql(1))
        .bind(runner_id)
        .execute(pool)
        .await
        .map_err(|_| ())?;
    if !message_ids.is_empty() {
        let null_sql = delete_kernel::null_event_messages_sql(message_ids.len());
        let mut nulls = sqlx::query(&null_sql);
        for message_id in &message_ids {
            nulls = nulls.bind(*message_id);
        }
        nulls.execute(pool).await.map_err(|_| ())?;
    }
    if !message_ids.is_empty() {
        let deletes_sql = delete_kernel::delete_chat_messages_sql(message_ids.len());
        let mut deletes = sqlx::query(&deletes_sql);
        for message_id in &message_ids {
            deletes = deletes.bind(*message_id);
        }
        deletes.execute(pool).await.map_err(|_| ())?;
    }
    if !chat_ids.is_empty() {
        let deletes_sql = delete_kernel::delete_chat_sessions_sql(chat_ids.len());
        let mut deletes = sqlx::query(&deletes_sql);
        for chat_id in &chat_ids {
            deletes = deletes.bind(*chat_id);
        }
        deletes.execute(pool).await.map_err(|_| ())?;
    }
    sqlx::query(&delete_kernel::delete_runner_rows_sql(1))
        .bind(runner_id)
        .execute(pool)
        .await
        .map_err(|_| ())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Executors: the `drain_pod_by_id` loop (the `drain.rs` recipe, verbatim)
// ---------------------------------------------------------------------------

use pidash_services::runner_sessions::drain::{AssignmentFacts, AssignmentPlan};
use pidash_services::runner_sessions::guards::alive_threshold;

/// `drain_pod_by_id` (`matcher.py:242-251`): pod lookup (miss ⇒ `Ok`,
/// no tx), else the drain loop. A dispatch failure propagates — the row
/// stays `ASSIGNED` while the caller observes the failure.
async fn drain_pod_by_id(pool: &PgPool, state: &AppState, pod_id: Uuid) -> Result<(), ()> {
    let pod: Option<sqlx::postgres::PgRow> = sqlx::query(drain_kernel::DRAIN_POD_BY_ID_LOOKUP_SQL)
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ())?;
    if pod.is_none() {
        return Ok(());
    }
    drain_pod(pool, state, pod_id).await
}

/// `drain_pod` (`matcher.py:194-239`): lock the idle list, then per
/// runner in list order take the next queued run, write the assignment,
/// and plan the frame; commit once; then dispatch each frame in
/// assignment order through the merged `send_to_runner` driver.
async fn drain_pod(pool: &PgPool, state: &AppState, pod_id: Uuid) -> Result<(), ()> {
    let mut tx = pool.begin().await.map_err(|_| ())?;
    let threshold = alive_threshold(now_micros());
    let runners = sqlx::query(drain_kernel::DRAIN_POD_IDLE_RUNNERS_SQL)
        .bind(threshold)
        .bind(pod_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| ())?;
    let mut plans: Vec<AssignmentPlan> = Vec::new();
    for runner in &runners {
        let runner_id: Uuid = runner.try_get(0).map_err(|_| ())?;
        let owner_id: Uuid = runner.try_get(1).map_err(|_| ())?;
        let provisioning: String = runner.try_get(7).map_err(|_| ())?;
        let visibility: i16 = runner.try_get(8).map_err(|_| ())?;
        // Non-private runners issue no query (the `qs.none()` arm).
        let Some(next_sql) =
            drain_kernel::next_for_runner_sql(&provisioning, i32::from(visibility))
        else {
            continue;
        };
        let run: Option<sqlx::postgres::PgRow> = sqlx::query(&next_sql)
            .bind(pod_id)
            .bind(runner_id)
            .bind(owner_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ())?;
        let Some(run) = run else {
            continue;
        };
        let run_id: Uuid = run.try_get(0).map_err(|_| ())?;
        let work_item_id: Option<Uuid> = run.try_get(7).map_err(|_| ())?;
        let prompt: String = run.try_get(19).map_err(|_| ())?;
        let run_config: Value = run.try_get(23).map_err(|_| ())?;
        let Value::Object(run_config) = run_config else {
            return Err(());
        };
        // `$3` is a fresh `timezone.now()` per assignment (`:233`).
        sqlx::query(drain_kernel::ASSIGN_RUN_UPDATE_SQL)
            .bind(owner_id)
            .bind(runner_id)
            .bind(now_micros())
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ())?;
        plans.push(drain_kernel::plan_assignment(&AssignmentFacts {
            run_id,
            runner_id,
            owner_id,
            work_item_id,
            prompt,
            run_config,
        }));
    }
    tx.commit().await.map_err(|_| ())?;
    if !plans.is_empty() {
        tracing::info!("{}", drain_kernel::drain_pod_log(&pod_id, plans.len()));
    }
    let pubsub = PoolPubsubStore::new(pool, state);
    for plan in &plans {
        let outcome = match plan.after_commit {
            drain_kernel::DrainEffect::SendAssign {
                runner_id,
                ref message,
            } => pubsub_kernel::send_to_runner(&pubsub, runner_id, message)
                .await
                .map_err(|_| ())?,
        };
        for warning in &outcome.warnings {
            tracing::warn!("{warning}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Executors: the D-12 handoff twin seam (F6: the driver is called, never
// inlined)
// ---------------------------------------------------------------------------

use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
use pidash_db::dispatch::status::AgentRunStatus as DbAgentRunStatus;
use pidash_db::tx::Transaction as DbTransaction;
use pidash_jobs::dispatch::{
    dispatch_after_commit as collect_dispatch, dispatch_agent_run,
    execution_fields as resolve_execution_fields, lock_cloud_creation_capacity as lock_capacity,
    ActorScope, DeferredAdmissionError, ExecutionFieldsError, ExecutionInputs, ExecutionSeams,
    ProjectScope,
};
use pidash_services::assistant::seams as assistant_seams;
use pidash_services::dispatch::tools as dispatch_tools;
use pidash_services::dispatch::{
    consume_admission_token, AdmissionCache, DeferredConsume, LlmProfile, UserFlags as SvcUserFlags,
};
use pidash_services::extensions::{CloudAgentToolsetsSeam, NoExtraToolsets};
use pidash_services::orchestration::creation::{
    self as creation_kernel, active_run_sql, latest_prior_run_sql, run_insert_returning_sql,
    run_lock_sql, run_select_sql, user_id_for_run, ASSIGNED_POD_SELECT_SQL,
    BUNDLE_ANCESTOR_HOP_SQL, BUNDLE_ASSIGNEES_SQL, BUNDLE_CHILDREN_SQL, BUNDLE_CODE_REVIEWS_SQL,
    BUNDLE_COMMENTS_SQL, BUNDLE_DONE_PAYLOAD_SQL, BUNDLE_LABELS_SQL, BUNDLE_OVERRIDES_SQL,
    BUNDLE_PARENT_COLS_SQL, BUNDLE_PARENT_DESCRIPTION_SQL, BUNDLE_PRIOR_RUN_COUNT_SQL,
    BUNDLE_PROJECT_IDENTIFIER_SQL, BUNDLE_PROJECT_STATES_SQL, BUNDLE_RELATIONS_SQL,
    BUNDLE_RELATION_TARGETS_SQL, BUNDLE_REMOTE_SQL, BUNDLE_SEQUENCE_SQL, BUNDLE_TICKER_SQL,
    BUNDLE_WORKSPACE_SQL, DEFAULT_POD_SELECT_SQL, FINALIZE_UPDATE_SQL, ISSUE_LOCK_SQL,
    ISSUE_SELECT_SQL, PROJECT_SELECT_SQL, PROMPT_UPDATE_SQL, RUNNER_SELECT_SQL,
    RUN_CONFIG_UPDATE_SQL, STATE_SELECT_SQL, TERMINAL_EVENT_EXISTS_SQL, TERMINAL_EVENT_INSERT_SQL,
    TERMINAL_EVENT_SEQ_SQL, TICKER_RESUME_SELECT_SQL, USER_FLAGS_SELECT_SQL,
    WORK_ITEM_ID_SELECT_SQL,
};
use pidash_services::orchestration::creation::{
    finalize_lock_sql, AdmissionError as ServiceAdmissionError, CreationError, CreationSeam,
    ExecutionError, ExecutionFields as ServiceExecutionFields, ExecutionRequest,
    FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView, ProjectView, RenderBundle,
    RunView, RunnerView, StateView, TickerBudget,
};
use pidash_services::prompting::{composer, context};

/// Redis-backed [`AdmissionCache`]: the first production implementation
/// of the sync trait. Each call opens a short sync connection (the
/// trait is sync; only cloud admission paths touch it). An unset or
/// unusable `REDIS_URL` fails closed like Python's backend failure.
#[derive(Clone)]
struct RedisAdmissionCache {
    client: Option<redis::Client>,
}

fn cache_unavailable<T>(what: &'static str) -> Result<T, redis::RedisError> {
    Err(redis::RedisError::from((
        redis::ErrorKind::Io,
        what,
        "redis unavailable".to_owned(),
    )))
}

impl AdmissionCache for RedisAdmissionCache {
    type Error = redis::RedisError;

    fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error> {
        let Some(client) = &self.client else {
            return cache_unavailable("bucket_count");
        };
        let mut connection = client.get_connection()?;
        let value: Option<i64> = redis::cmd("GET").arg(key).query(&mut connection)?;
        Ok(value)
    }

    fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), Self::Error> {
        let Some(client) = &self.client else {
            return cache_unavailable("add_or_incr");
        };
        let mut connection = client.get_connection()?;
        // `cache.add(key, 1, timeout) or cache.incr(key)`: set-if-absent
        // with TTL, else increment.
        let set: Option<String> = redis::cmd("SET")
            .arg(key)
            .arg(1)
            .arg("EX")
            .arg(timeout_secs)
            .arg("NX")
            .query(&mut connection)?;
        if set.is_none() {
            let _: i64 = redis::cmd("INCR").arg(key).query(&mut connection)?;
        }
        Ok(())
    }
}

/// Pool-backed [`CreationSeam`] + [`FinalizeAgentRunSeam`]: the api-side
/// twin of `jobs::LiveCreationStore` (same builders, same order, same
/// decodes — cited per method). The jobs store cannot serve api
/// requests (its `StoreDeps` closures are sync and have no production
/// factory), and the quarantine keeps the twin in this file; the
/// D-12 decisions stay in the services driver in both.
struct HandoffStore<'t, 'p> {
    tx: DbTransaction<'t>,
    pool: &'p PgPool,
    cloud: CloudAgentSettings,
    managed: ManagedRunnerSettings,
    cache: RedisAdmissionCache,
    now_unix_secs: i64,
    dispatches: Vec<Uuid>,
    deferred: Vec<DeferredConsume>,
    terminal_effects: Vec<Uuid>,
}

impl<'t, 'p> HandoffStore<'t, 'p> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        tx: DbTransaction<'t>,
        pool: &'p PgPool,
        cloud: CloudAgentSettings,
        managed: ManagedRunnerSettings,
        cache: RedisAdmissionCache,
        now_unix_secs: i64,
    ) -> Self {
        Self {
            tx,
            pool,
            cloud,
            managed,
            cache,
            now_unix_secs,
            dispatches: Vec::new(),
            deferred: Vec::new(),
            terminal_effects: Vec::new(),
        }
    }

    fn db(error: sqlx::Error) -> CreationError {
        CreationError::Db(error.to_string())
    }

    fn into_parts(
        self,
    ) -> (
        DbTransaction<'t>,
        Vec<Uuid>,
        Vec<DeferredConsume>,
        Vec<Uuid>,
        RedisAdmissionCache,
    ) {
        (
            self.tx,
            self.dispatches,
            self.deferred,
            self.terminal_effects,
            self.cache,
        )
    }
}

fn twin_decode_error(column: &str, value: &str) -> sqlx::Error {
    sqlx::Error::Decode(format!("unknown agent_run.{column} {value:?}").into())
}

fn twin_map_status(value: String) -> Result<DbAgentRunStatus, sqlx::Error> {
    DbAgentRunStatus::from_value(&value).ok_or_else(|| twin_decode_error("status", &value))
}

fn twin_map_executor(value: String) -> Result<AgentExecutorKind, sqlx::Error> {
    // Strictness is safe here (unlike the trigger): the
    // `agent_run_cloud_has_no_local_assignment` check constraint
    // (`runner/models.py:1065-1075`, migration 0024) admits only the
    // three members, so an unknown stored value is unreachable.
    AgentExecutorKind::from_value(&value).ok_or_else(|| twin_decode_error("executor_kind", &value))
}

fn twin_map_run_view(row: &sqlx::postgres::PgRow) -> Result<RunView, sqlx::Error> {
    let status: String = row.try_get("status")?;
    let trigger: String = row.try_get("trigger")?;
    let executor_kind: String = row.try_get("executor_kind")?;
    Ok(RunView {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        created_by_id: row.try_get("created_by_id")?,
        pod_id: row.try_get("pod_id")?,
        runner_id: row.try_get("runner_id")?,
        pinned_runner_id: row.try_get("pinned_runner_id")?,
        parent_run_id: row.try_get("parent_run_id")?,
        work_item_id: row.try_get("work_item_id")?,
        status: twin_map_status(status)?,
        trigger,
        executor_kind: twin_map_executor(executor_kind)?,
        phase_kind: row.try_get("phase_kind")?,
        run_config: row.try_get("run_config")?,
        tool_plan: row.try_get("tool_plan")?,
        error_code: row.try_get("error_code")?,
        error: row.try_get("error")?,
        prompt: row.try_get("prompt")?,
        prompt_manifest: row.try_get("prompt_manifest")?,
        ended_at: row.try_get("ended_at")?,
    })
}

fn twin_map_issue(row: &sqlx::postgres::PgRow) -> Result<IssueView, sqlx::Error> {
    Ok(IssueView {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        project_id: row.try_get("project_id")?,
        state_id: row.try_get("state_id")?,
        parent_id: row.try_get("parent_id")?,
        created_by_id: row.try_get("created_by_id")?,
        assigned_pod_id: row.try_get("assigned_pod_id")?,
        agent_executor: row.try_get("agent_executor")?,
        git_work_branch: row.try_get("git_work_branch")?,
        workpad: row.try_get("workpad")?,
        name: row.try_get("name")?,
        description_stripped: row.try_get("description_stripped")?,
        priority: row.try_get("priority")?,
        sequence_id: row.try_get("sequence_id")?,
        target_date: row.try_get("target_date")?,
    })
}

fn twin_map_project(row: &sqlx::postgres::PgRow) -> Result<ProjectView, sqlx::Error> {
    let pool: i32 = row.try_get("agent_default_max_ticks")?;
    let interval_impl: i32 = row.try_get("agent_default_interval_seconds")?;
    let interval_review: i32 = row.try_get("agent_review_default_interval_seconds")?;
    let interval_test: i32 = row.try_get("agent_test_default_interval_seconds")?;
    Ok(ProjectView {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        identifier: row.try_get("identifier")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        repo_url: row.try_get("repo_url")?,
        base_branch: row.try_get("base_branch")?,
        default_agent_executor: row.try_get("default_agent_executor")?,
        project_lead_id: row.try_get("project_lead_id")?,
        default_assignee_id: row.try_get("default_assignee_id")?,
        pool: i64::from(pool),
        interval_impl: i64::from(interval_impl),
        interval_review: i64::from(interval_review),
        interval_test: i64::from(interval_test),
    })
}

impl CreationSeam for HandoffStore<'_, '_> {
    async fn issue(&mut self, issue_id: Uuid) -> Result<IssueView, CreationError> {
        let row = sqlx::query(ISSUE_SELECT_SQL)
            .bind(issue_id)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        twin_map_issue(&row).map_err(Self::db)
    }

    async fn project(&mut self, project_id: Uuid) -> Result<ProjectView, CreationError> {
        let row = sqlx::query(PROJECT_SELECT_SQL)
            .bind(project_id)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        twin_map_project(&row).map_err(Self::db)
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let row: Option<(Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
            .bind(state_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        match row {
            Some((id, name, group)) => Ok(Some(StateView { id, name, group })),
            None => Err(CreationError::MissingRow(format!("no state {state_id}"))),
        }
    }

    async fn latest_prior_run(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = sqlx::query(&latest_prior_run_sql())
            .bind(issue_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        row.map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(Self::db)
    }

    async fn active_run_for(&mut self, issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = sqlx::query(&active_run_sql())
            .bind(issue_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        row.map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(Self::db)
    }

    async fn run(&mut self, run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        let row = sqlx::query(&run_select_sql())
            .bind(run_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        row.map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(Self::db)
    }

    async fn runner(&mut self, runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        let row: Option<(Uuid, Uuid, String)> = sqlx::query_as(RUNNER_SELECT_SQL)
            .bind(runner_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.map(|(id, pod_id, status)| RunnerView { id, pod_id, status }))
    }

    async fn assigned_pod(&mut self, pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(ASSIGNED_POD_SELECT_SQL)
            .bind(pod_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.map(|(id, project_id)| PodView { id, project_id }))
    }

    async fn default_pod_for_project(
        &mut self,
        project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        let row: Option<(Uuid, Uuid)> = sqlx::query_as(DEFAULT_POD_SELECT_SQL)
            .bind(project_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.map(|(id, project_id)| PodView { id, project_id }))
    }

    async fn resume_parent_run_id(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        let row: Option<(Option<Uuid>,)> = sqlx::query_as(TICKER_RESUME_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.and_then(|row| row.0))
    }

    async fn work_item_id_for_run(&mut self, run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let row: Option<(Option<Uuid>,)> = sqlx::query_as(WORK_ITEM_ID_SELECT_SQL)
            .bind(run_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(row.and_then(|row| row.0))
    }

    async fn lock_issue_for_handoff(
        &mut self,
        issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        let locked: Option<(Uuid,)> = sqlx::query_as(ISSUE_LOCK_SQL)
            .bind(issue_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        if locked.is_none() {
            return Ok(None);
        }
        let issue = self.issue(issue_id).await?;
        let project_id = issue
            .project_id
            .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
        let project = self.project(project_id).await?;
        let state = self.state(issue.state_id).await?;
        Ok(Some(LockedIssue {
            issue,
            project,
            state,
        }))
    }

    async fn lock_run_for_handoff(
        &mut self,
        run_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        let row = sqlx::query(&run_lock_sql())
            .bind(run_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        row.map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(Self::db)
    }

    async fn user_flags(&mut self, user_id: Uuid) -> Result<SvcUserFlags, CreationError> {
        let row: (bool, bool) = sqlx::query_as(USER_FLAGS_SELECT_SQL)
            .bind(user_id)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(SvcUserFlags {
            is_active: row.0,
            is_bot: row.1,
        })
    }

    async fn insert_run(&mut self, row: &NewAgentRun) -> Result<RunView, CreationError> {
        let inserted = sqlx::query(&run_insert_returning_sql())
            .bind(row.id)
            .bind(row.workspace_id)
            .bind(row.created_by_id)
            .bind(row.pod_id)
            .bind(row.pinned_runner_id)
            .bind(row.work_item_id)
            .bind(row.parent_run_id)
            .bind(row.executor_kind.value())
            .bind(&row.error_code)
            .bind(&row.tool_plan)
            .bind(&row.trigger)
            .bind(&row.phase_kind)
            .bind(&row.run_config)
            .bind(row.now)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        twin_map_run_view(&inserted).map_err(Self::db)
    }

    async fn save_prompt(
        &mut self,
        run_id: Uuid,
        prompt: &str,
        manifest: &Value,
    ) -> Result<(), CreationError> {
        sqlx::query(PROMPT_UPDATE_SQL)
            .bind(prompt)
            .bind(manifest)
            .bind(run_id)
            .execute(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(())
    }

    async fn save_run_config(&mut self, run_id: Uuid, config: &Value) -> Result<(), CreationError> {
        sqlx::query(RUN_CONFIG_UPDATE_SQL)
            .bind(config)
            .bind(run_id)
            .execute(&mut **self.tx.inner())
            .await
            .map_err(Self::db)?;
        Ok(())
    }

    async fn execution_fields(
        &mut self,
        req: &ExecutionRequest,
    ) -> Result<ServiceExecutionFields, ExecutionError> {
        let project = ProjectScope {
            project_id: req.project_id,
            workspace_id: req.workspace_id,
            default_agent_executor: req.default_agent_executor.clone(),
        };
        let actor = req.actor.map(|actor| ActorScope {
            id: actor.id,
            flags: actor.flags,
        });
        let actor_id = actor.as_ref().map(|actor| actor.id);
        // The jobs store's H/E/P closures have no production factory, so
        // the twin prefetches their facts asynchronously and moves the
        // values into the `FnOnce` seams (identical verdicts).
        let has_key = match actor_id {
            Some(id) => twin_has_api_key(self.pool, id)
                .await
                .map_err(|detail| ExecutionError::Store(CreationError::Db(detail)))?,
            None => false,
        };
        let profile = {
            let mapped = assistant_seams::agent_model_profile_for_user(has_key);
            LlmProfile {
                available: mapped.available,
                reason_code: mapped.reason_code,
            }
        };
        let has_llm = assistant_seams::has_usable_llm_config(has_key);
        let Self {
            pool,
            cache,
            cloud,
            managed,
            now_unix_secs,
            deferred,
            ..
        } = self;
        let mut short = DbTransaction::begin(pool)
            .await
            .map_err(|error| ExecutionError::Store(CreationError::Db(error.to_string())))?;
        let inputs = ExecutionInputs {
            project: &project,
            run_kind: &req.run_kind,
            has_issue: req.has_issue,
            required_capabilities: &[],
            actor: actor.as_ref(),
            automatic: req.automatic,
            requested: req.requested.as_deref(),
            now_unix_secs: *now_unix_secs,
        };
        let mut collect = |consume: DeferredConsume| deferred.push(consume);
        let fields = resolve_execution_fields(
            &mut short,
            &inputs,
            cloud,
            managed,
            cache,
            &mut collect,
            ExecutionSeams {
                has_usable_llm_config: move || has_llm,
                extra_toolsets_enabled: move || dispatch_tools::extra_toolsets_enabled_for(),
                llm_profile: move || profile,
            },
        )
        .await;
        short
            .commit()
            .await
            .map_err(|error| ExecutionError::Store(CreationError::Db(error.to_string())))?;
        fields
            .map(twin_map_execution_fields)
            .map_err(twin_map_execution_error)
    }

    async fn lock_cloud_creation_capacity(
        &mut self,
        workspace_id: Uuid,
        executor_kind: AgentExecutorKind,
        automatic: bool,
    ) -> Result<Option<ServiceAdmissionError>, CreationError> {
        let outcome = lock_capacity(
            &mut self.tx,
            workspace_id,
            &executor_kind,
            automatic,
            &self.cloud,
        )
        .await;
        match outcome {
            Ok(deferred) => Ok(deferred.map(twin_map_admission_error)),
            Err(ExecutionFieldsError::Admission(exc)) => {
                Err(CreationError::CapacityRefused(exc.detail().to_owned()))
            }
            Err(ExecutionFieldsError::Db(error)) => Err(Self::db(error)),
            // Unreachable: the lock path only locks, counts and
            // verdicts (no resolve/LLM/tool gates).
            Err(other) => Err(CreationError::Db(format!("unexpected lock error: {other}"))),
        }
    }

    fn dispatch_after_commit(&mut self, run_id: Uuid) {
        collect_dispatch(&mut |id| self.dispatches.push(id), run_id);
    }

    async fn render_bundle(
        &mut self,
        issue_id: Uuid,
        run_id: Uuid,
        parent_run_id: Option<Uuid>,
        trigger: &str,
        created_by_id: Uuid,
    ) -> Result<RenderBundle, CreationError> {
        twin_load_render_bundle(
            &mut self.tx,
            issue_id,
            run_id,
            parent_run_id,
            trigger,
            created_by_id,
        )
        .await
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        NoExtraToolsets.schema_tool_name().to_owned()
    }
}

fn twin_map_execution_fields(
    fields: pidash_jobs::dispatch::ExecutionFields,
) -> ServiceExecutionFields {
    let pinned_runner_entry = match fields.executor_kind {
        AgentExecutorKind::CloudAgent => Some(None),
        AgentExecutorKind::ManagedRunner => Some(fields.pinned_runner_id),
        AgentExecutorKind::LocalRunner => None,
    };
    ServiceExecutionFields {
        executor_kind: fields.executor_kind,
        tool_plan: fields.tool_plan,
        pinned_runner_entry,
        error_code: fields.error_code,
        cloud_admission_error: fields.cloud_admission_error.map(twin_map_admission_error),
    }
}

fn twin_map_execution_error(error: ExecutionFieldsError) -> ExecutionError {
    match error {
        ExecutionFieldsError::Db(error) => {
            ExecutionError::Store(CreationError::Db(error.to_string()))
        }
        ExecutionFieldsError::Admission(exc) => ExecutionError::Refused(exc.detail().to_owned()),
        other => ExecutionError::Refused(other.to_string()),
    }
}

fn twin_map_admission_error(error: DeferredAdmissionError) -> ServiceAdmissionError {
    ServiceAdmissionError {
        code: error.code,
        detail: error.detail,
    }
}

/// `get_config(user)` (`llm.py:67-68`):
/// `UserLLMConfig.objects.filter(user).first()` — full row, unordered
/// (`LIMIT 1`, no `ORDER BY`). `$1` = user id.
const TWIN_LLM_CONFIG_SQL: &str = r#"SELECT "id", "user_id", "provider_kind", "base_url", "model_name", "api_key_encrypted", "last_verified_at", "created_at", "updated_at" FROM "assistant_user_llm_config" WHERE "assistant_user_llm_config"."user_id" = $1 LIMIT 1"#;

/// `get_config(user)` + `has_api_key` (`models.py:260-262`): key present
/// and non-empty.
async fn twin_has_api_key(pool: &PgPool, user_id: Uuid) -> Result<bool, String> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(TWIN_LLM_CONFIG_SQL)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(false);
    };
    let key: Option<Vec<u8>> = row
        .try_get("api_key_encrypted")
        .map_err(|error| error.to_string())?;
    Ok(key.is_some_and(|key| !key.is_empty()))
}

fn twin_db(error: sqlx::Error) -> CreationError {
    CreationError::Db(error.to_string())
}

/// Python `datetime.isoformat()` for an aware UTC timestamp (`+00:00`,
/// microseconds only when nonzero — `context.py:322`).
fn twin_render_isoformat(dt: DateTime<Utc>) -> String {
    if dt.timestamp_subsec_micros() == 0 {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
    } else {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
    }
}

async fn twin_bundle_labels(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    sqlx::query_scalar(BUNDLE_LABELS_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)
}

async fn twin_bundle_assignees(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<String>, CreationError> {
    let rows: Vec<(Option<String>, Option<String>)> = sqlx::query_as(BUNDLE_ASSIGNEES_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(|(display, email)| context::assignee_display(display.as_deref(), email.as_deref()))
        .collect())
}

async fn twin_bundle_project_states(
    tx: &mut DbTransaction<'_>,
    project_id: Uuid,
) -> Result<Vec<context::ProjectStateView>, CreationError> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(BUNDLE_PROJECT_STATES_SQL)
        .bind(project_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(|(name, group, description)| context::ProjectStateView {
            name,
            group,
            description,
        })
        .collect())
}

type TwinChildCols = (Uuid, Option<String>, i32, String, Option<String>);
type TwinTargetCols = (
    Uuid,
    Option<String>,
    i32,
    String,
    Option<String>,
    Option<String>,
);
type TwinCommentCols = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Uuid>,
    Option<DateTime<Utc>>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<bool>,
);
type TwinReviewCols = (String, Option<String>, String, bool, bool, String, String);
type TwinParentCols = (Option<String>, Option<Uuid>, Option<String>, i64);

async fn twin_bundle_children(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::IssueRef>, CreationError> {
    let rows: Vec<TwinChildCols> = sqlx::query_as(BUNDLE_CHILDREN_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(
            |(_, name, sequence, project_identifier, state)| context::IssueRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title: name.unwrap_or_default(),
                state: state.unwrap_or_default(),
            },
        )
        .collect())
}

async fn twin_bundle_relations(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<
    (
        Vec<context::RelationRow>,
        std::collections::HashMap<String, context::IssueRef>,
        std::collections::HashMap<String, context::DirectionalRef>,
    ),
    CreationError,
> {
    let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(BUNDLE_RELATIONS_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let mut other_ids: Vec<Uuid> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for (left, right, _) in &rows {
        let other = if left == &issue_id { *right } else { *left };
        if other != issue_id && seen_ids.insert(other) {
            other_ids.push(other);
        }
    }
    let targets: Vec<TwinTargetCols> = if other_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(BUNDLE_RELATION_TARGETS_SQL)
            .bind(&other_ids)
            .fetch_all(&mut **tx.inner())
            .await
            .map_err(twin_db)?
    };
    let mut refs = std::collections::HashMap::new();
    let mut directional_refs = std::collections::HashMap::new();
    for (id, name, sequence, project_identifier, state, group) in targets {
        let key = id.to_string();
        let title = name.unwrap_or_default();
        let state_name = state.unwrap_or_default();
        refs.insert(
            key.clone(),
            context::IssueRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title: title.clone(),
                state: state_name.clone(),
            },
        );
        directional_refs.insert(
            key,
            context::DirectionalRef {
                identifier: context::issue_identifier(&project_identifier, i64::from(sequence)),
                title,
                state: state_name,
                state_group: group.unwrap_or_default(),
            },
        );
    }
    let relation_rows = rows
        .into_iter()
        .map(|(left, right, relation_type)| context::RelationRow {
            issue_id: left.to_string(),
            related_issue_id: right.to_string(),
            relation_type,
        })
        .collect();
    Ok((relation_rows, refs, directional_refs))
}

async fn twin_bundle_comments(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CommentView>, CreationError> {
    let rows: Vec<TwinCommentCols> = sqlx::query_as(BUNDLE_COMMENTS_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(
            |(
                stripped,
                speaker_type,
                speaker_label,
                run_id,
                created_at,
                display,
                email,
                username,
                is_bot,
            )| {
                let actor = match (&display, &email, &username, &is_bot) {
                    (None, None, None, None) => None,
                    _ => Some(context::ActorView {
                        display_name: display,
                        email,
                        username,
                        is_bot: is_bot.unwrap_or(false),
                    }),
                };
                context::CommentView {
                    body: stripped.unwrap_or_default(),
                    speaker_type: speaker_type.unwrap_or_default(),
                    speaker_label,
                    actor,
                    created_at_iso: created_at.map(twin_render_isoformat),
                    run_id: run_id.map(|id| id.to_string()),
                }
            },
        )
        .collect())
}

async fn twin_bundle_reviews(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
) -> Result<Vec<context::CodeReviewView>, CreationError> {
    let rows: Vec<TwinReviewCols> = sqlx::query_as(BUNDLE_CODE_REVIEWS_SQL)
        .bind(issue_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(
            |(url, title, state, merged, draft, provider, external_iid)| context::CodeReviewView {
                url,
                title,
                state,
                merged,
                draft,
                provider,
                external_iid,
            },
        )
        .collect())
}

async fn twin_bundle_remote(
    tx: &mut DbTransaction<'_>,
    project_id: Uuid,
) -> Result<(Option<context::RemoteView>, Option<context::AdapterNames>), CreationError> {
    let row: Option<(String, String, String)> = sqlx::query_as(BUNDLE_REMOTE_SQL)
        .bind(project_id)
        .fetch_optional(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let Some((provider, host_url, full_name)) = row else {
        return Ok((None, None));
    };
    let adapter = match provider.to_lowercase().as_str() {
        "github" => Some(context::AdapterNames {
            display_name: "GitHub".to_owned(),
            code_review_term: "pull request".to_owned(),
        }),
        "gitlab" => Some(context::AdapterNames {
            display_name: "GitLab".to_owned(),
            code_review_term: "merge request".to_owned(),
        }),
        _ => None,
    };
    Ok((
        Some(context::RemoteView {
            provider,
            host_url,
            full_name,
        }),
        adapter,
    ))
}

struct TwinChainNode {
    id: Uuid,
    project_identifier: String,
    title: String,
}

async fn twin_bundle_ancestors(
    tx: &mut DbTransaction<'_>,
    issue: &IssueView,
    project_identifier: &str,
) -> Result<Vec<TwinChainNode>, CreationError> {
    let mut chain = vec![TwinChainNode {
        id: issue.id,
        project_identifier: project_identifier.to_owned(),
        title: issue.name.clone().unwrap_or_default(),
    }];
    let mut seen = std::collections::HashSet::from([issue.id]);
    let mut next = issue.parent_id;
    while let Some(id) = next {
        if chain.len() >= 50 || !seen.insert(id) {
            break;
        }
        let row: Option<(Option<String>, Option<Uuid>, Option<Uuid>)> =
            sqlx::query_as(BUNDLE_ANCESTOR_HOP_SQL)
                .bind(id)
                .fetch_optional(&mut **tx.inner())
                .await
                .map_err(twin_db)?;
        let Some((name, project_id, parent_id)) = row else {
            break;
        };
        let project_identifier = match project_id {
            Some(pid) => {
                let row: Option<(String,)> = sqlx::query_as(BUNDLE_PROJECT_IDENTIFIER_SQL)
                    .bind(pid)
                    .fetch_optional(&mut **tx.inner())
                    .await
                    .map_err(twin_db)?;
                row.map(|row| row.0).unwrap_or_default()
            }
            None => String::new(),
        };
        chain.push(TwinChainNode {
            id,
            project_identifier,
            title: name.unwrap_or_default(),
        });
        next = parent_id;
    }
    Ok(chain)
}

async fn twin_bundle_parent_and_lineage(
    tx: &mut DbTransaction<'_>,
    ancestors: &[TwinChainNode],
) -> Result<(Option<context::ParentView>, Vec<context::LineageNode>), CreationError> {
    let parent = match ancestors.get(1) {
        None => None,
        Some(node) => {
            let row: Option<TwinParentCols> = sqlx::query_as(BUNDLE_PARENT_COLS_SQL)
                .bind(node.id)
                .fetch_optional(&mut **tx.inner())
                .await
                .map_err(twin_db)?;
            match row {
                None => None,
                Some((name, state_id, work_branch, comments_count)) => {
                    let state_name = match state_id {
                        Some(id) => {
                            let row: Option<(Uuid, String, String)> =
                                sqlx::query_as(STATE_SELECT_SQL)
                                    .bind(id)
                                    .fetch_optional(&mut **tx.inner())
                                    .await
                                    .map_err(twin_db)?;
                            row.map(|row| row.1)
                        }
                        None => None,
                    };
                    let description: Option<(Option<String>,)> =
                        sqlx::query_as(BUNDLE_PARENT_DESCRIPTION_SQL)
                            .bind(node.id)
                            .fetch_optional(&mut **tx.inner())
                            .await
                            .map_err(twin_db)?;
                    let sequence: Option<(i32,)> = sqlx::query_as(BUNDLE_SEQUENCE_SQL)
                        .bind(node.id)
                        .fetch_optional(&mut **tx.inner())
                        .await
                        .map_err(twin_db)?;
                    let sequence = sequence.map(|row| i64::from(row.0)).unwrap_or_default();
                    Some(context::ParentView {
                        identifier: context::issue_identifier(&node.project_identifier, sequence),
                        title: name,
                        state_name,
                        work_branch,
                        description_stripped: description.and_then(|row| row.0),
                        comments_count,
                    })
                }
            }
        }
    };
    let lineage = if ancestors.len() > 2 {
        let mut nodes = Vec::new();
        for node in ancestors {
            let sequence: Option<(i32,)> = sqlx::query_as(BUNDLE_SEQUENCE_SQL)
                .bind(node.id)
                .fetch_optional(&mut **tx.inner())
                .await
                .map_err(twin_db)?;
            let sequence = sequence.map(|row| i64::from(row.0)).unwrap_or_default();
            nodes.push(context::LineageNode {
                identifier: context::issue_identifier(&node.project_identifier, sequence),
                title: node.title.clone(),
            });
        }
        nodes
    } else {
        Vec::new()
    };
    Ok((parent, lineage))
}

async fn twin_bundle_done_payload(
    tx: &mut DbTransaction<'_>,
    run_id: Uuid,
) -> Result<Option<Value>, CreationError> {
    let row: Option<(Option<Value>,)> = sqlx::query_as(BUNDLE_DONE_PAYLOAD_SQL)
        .bind(run_id)
        .fetch_optional(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(row.and_then(|row| row.0))
}

async fn twin_bundle_overrides(
    tx: &mut DbTransaction<'_>,
    workspace_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Vec<composer::OverrideRow>, CreationError> {
    let rows: Vec<(String, String, i32, Option<Uuid>)> = sqlx::query_as(BUNDLE_OVERRIDES_SQL)
        .bind(workspace_id)
        .bind(user_id)
        .fetch_all(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    Ok(rows
        .into_iter()
        .map(
            |(section_key, body, version, user_id)| composer::OverrideRow {
                workspace_id: workspace_id.to_string(),
                section_key,
                body,
                version: i64::from(version),
                is_active: true,
                user_id: user_id.map(|id| id.to_string()),
            },
        )
        .collect())
}

async fn twin_load_render_bundle(
    tx: &mut DbTransaction<'_>,
    issue_id: Uuid,
    run_id: Uuid,
    parent_run_id: Option<Uuid>,
    trigger: &str,
    created_by_id: Uuid,
) -> Result<RenderBundle, CreationError> {
    let row = sqlx::query(ISSUE_SELECT_SQL)
        .bind(issue_id)
        .fetch_one(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let issue = twin_map_issue(&row).map_err(twin_db)?;
    let project_id = issue
        .project_id
        .ok_or_else(|| CreationError::MissingRow("issue has no project".to_owned()))?;
    let row = sqlx::query(PROJECT_SELECT_SQL)
        .bind(project_id)
        .fetch_one(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let project = twin_map_project(&row).map_err(twin_db)?;
    let state = match issue.state_id {
        Some(id) => {
            let row: Option<(Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
                .bind(id)
                .fetch_optional(&mut **tx.inner())
                .await
                .map_err(twin_db)?;
            row.map(|(id, name, group)| StateView { id, name, group })
        }
        None => None,
    };
    let workspace: (String, String) = sqlx::query_as(BUNDLE_WORKSPACE_SQL)
        .bind(issue.workspace_id)
        .fetch_one(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let labels = twin_bundle_labels(tx, issue_id).await?;
    let assignees = twin_bundle_assignees(tx, issue_id).await?;
    let project_states = twin_bundle_project_states(tx, project_id).await?;
    let children = twin_bundle_children(tx, issue_id).await?;
    let (relation_rows, refs, directional_refs) = twin_bundle_relations(tx, issue_id).await?;
    let comments = twin_bundle_comments(tx, issue_id).await?;
    let reviews = twin_bundle_reviews(tx, issue_id).await?;
    let (remote, adapter) = twin_bundle_remote(tx, project_id).await?;
    let ancestors = twin_bundle_ancestors(tx, &issue, &project.identifier).await?;
    let (parent, lineage) = twin_bundle_parent_and_lineage(tx, &ancestors).await?;
    let prior_run_count: i64 = sqlx::query_scalar(BUNDLE_PRIOR_RUN_COUNT_SQL)
        .bind(issue_id)
        .bind(run_id)
        .fetch_one(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let ticker: Option<(i32, i32, i32, bool)> = sqlx::query_as(BUNDLE_TICKER_SQL)
        .bind(issue_id)
        .fetch_optional(&mut **tx.inner())
        .await
        .map_err(twin_db)?;
    let direct_parent_payload = match parent_run_id {
        Some(id) => twin_bundle_done_payload(tx, id).await?,
        None => None,
    };
    let ticker_parent_payload = {
        let resume: Option<(Option<Uuid>,)> = sqlx::query_as(TICKER_RESUME_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(&mut **tx.inner())
            .await
            .map_err(twin_db)?;
        match resume.and_then(|row| row.0) {
            Some(id) => twin_bundle_done_payload(tx, id).await?,
            None => None,
        }
    };
    let user_id = user_id_for_run(trigger, created_by_id)
        .map(|id| id.parse::<Uuid>())
        .transpose()
        .map_err(|_| CreationError::MissingRow("bad user id".to_owned()))?;
    let override_rows = twin_bundle_overrides(tx, issue.workspace_id, user_id).await?;
    Ok(RenderBundle {
        issue,
        project,
        workspace_slug: workspace.0,
        workspace_name: workspace.1,
        state,
        labels,
        assignees,
        project_states,
        children,
        relation_rows,
        refs,
        directional_refs,
        comments,
        reviews,
        remote,
        adapter,
        parent,
        lineage,
        prior_run_count,
        ticker: ticker.map(|(used, waited, granted, enabled)| TickerBudget {
            used: i64::from(used),
            waited: i64::from(waited),
            granted: i64::from(granted),
            enabled,
        }),
        direct_parent_payload,
        ticker_parent_payload,
        override_rows,
    })
}

impl FinalizeAgentRunSeam for HandoffStore<'_, '_> {
    async fn finalize_failed_run(
        &mut self,
        run_id: Uuid,
        error_code: &str,
        error: &str,
        now: DateTime<Utc>,
    ) -> Result<RunView, CreationError> {
        let locked = sqlx::query(&finalize_lock_sql())
            .bind(run_id)
            .fetch_optional(&mut **self.tx.inner())
            .await
            .map_err(twin_db)?;
        if locked.is_none() {
            let row = sqlx::query(&run_select_sql())
                .bind(run_id)
                .fetch_one(&mut **self.tx.inner())
                .await
                .map_err(twin_db)?;
            return twin_map_run_view(&row).map_err(twin_db);
        }
        sqlx::query(FINALIZE_UPDATE_SQL)
            .bind(now)
            .bind(error_code)
            .bind(error)
            .bind(run_id)
            .execute(&mut **self.tx.inner())
            .await
            .map_err(twin_db)?;
        let locked = locked
            .map(|row| twin_map_run_view(&row))
            .transpose()
            .map_err(twin_db)?;
        if locked.is_some_and(|run| run.executor_kind == AgentExecutorKind::CloudAgent) {
            let exists: bool = sqlx::query_scalar(TERMINAL_EVENT_EXISTS_SQL)
                .bind(run_id)
                .fetch_one(&mut **self.tx.inner())
                .await
                .map_err(twin_db)?;
            if !exists {
                let seq: i32 = sqlx::query_scalar(TERMINAL_EVENT_SEQ_SQL)
                    .bind(run_id)
                    .fetch_one(&mut **self.tx.inner())
                    .await
                    .map_err(twin_db)?;
                let payload = serde_json::json!({"status": "failed", "error_code": error_code});
                sqlx::query(TERMINAL_EVENT_INSERT_SQL)
                    .bind(run_id)
                    .bind(seq)
                    .bind(&payload)
                    .bind(now)
                    .execute(&mut **self.tx.inner())
                    .await
                    .map_err(twin_db)?;
            }
        }
        // The `on_commit` lambda (`_publish_effects`): captured for the
        // handoff drain below (the jobs twin returns it pending for D-15;
        // the api twin can drain it directly).
        self.terminal_effects.push(run_id);
        let row = sqlx::query(&run_select_sql())
            .bind(run_id)
            .fetch_one(&mut **self.tx.inner())
            .await
            .map_err(twin_db)?;
        twin_map_run_view(&row).map_err(twin_db)
    }
}

/// Fire one post-commit handoff (`models.py:653-674` → D-12
/// `complete_project_move_handoff`): own transaction, commit, then the
/// handoff tx's own `on_commit` registrations in order (terminal pairs,
/// dispatches, deferred consumes). Any failure `Err`s — the caller
/// swallows it after the failure log.
async fn fire_handoff(pool: &PgPool, state: &AppState, run_id: Uuid) -> Result<(), ()> {
    let settings = state.settings();
    let cloud = settings.cloud_agent.clone();
    let managed = settings.managed_runner.clone();
    let cache = RedisAdmissionCache {
        client: redis_client(state),
    };
    let tx = DbTransaction::begin(pool).await.map_err(|_| ())?;
    let mut store = HandoffStore::new(tx, pool, cloud.clone(), managed, cache, unix_now_secs());
    let _outcome = creation_kernel::complete_project_move_handoff(&mut store, run_id, now_micros())
        .await
        .map_err(|_| ())?;
    let (tx, dispatches, deferred, terminal_effects, cache) = store.into_parts();
    tx.commit().await.map_err(|_| ())?;
    let mut pairs = Vec::with_capacity(terminal_effects.len() * 2);
    for id in terminal_effects {
        pairs.extend(finalize_kernel::plan_publish_effects(id));
    }
    drain_publish_effects(pool, state, pairs).await;
    let now = now_micros();
    for id in dispatches {
        dispatch_agent_run(pool, &cloud, id, now)
            .await
            .map_err(|_| ())?;
    }
    for consume in &deferred {
        consume_admission_token(&cache, consume);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Routes (merged at the F-10 seam in `overlay.rs`)
// ---------------------------------------------------------------------------

/// Attach the proxy passthrough for a path's unowned methods (the
/// `manage.rs`/`enroll.rs` `owned` position): unowned methods stay on
/// the Python plane.
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
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the daemon teardown routes (`runner/urls.py`:
/// `runners/<uuid>/refresh/`, `runners/<uuid>/`). Merged under
/// `RouteGroup::Runner` at the F-10 seam; sibling handler issues extend
/// the merge, keeping both sides.
pub fn daemon_routes() -> Router<AppState> {
    use axum::routing::{delete, post};
    const POST_ONLY: &[&str] = &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"];
    const DELETE_ONLY: &[&str] = &["GET", "POST", "PUT", "PATCH", "OPTIONS"];
    Router::new()
        .route(
            "/api/v1/runner/runners/{runner_id}/refresh/",
            owned(post(runner_refresh), POST_ONLY),
        )
        .route(
            "/api/v1/runner/runners/{runner_id}/",
            owned(delete(runner_self_revoke), DELETE_ONLY),
        )
}

/// Register the web teardown routes (`runner/web_urls.py`:
/// `dev-machines/<uuid>/revoke|rotate/`, `runners/<uuid>/revoke/`).
/// Merged under `RouteGroup::RunnerWeb` at the F-10 seam.
pub fn web_routes() -> Router<AppState> {
    use axum::routing::post;
    const POST_ONLY: &[&str] = &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"];
    Router::new()
        .route(
            "/api/runners/dev-machines/{machine_id}/revoke/",
            owned(post(dev_machine_revoke), POST_ONLY),
        )
        .route(
            "/api/runners/dev-machines/{machine_id}/rotate/",
            owned(post(dev_machine_rotate), POST_ONLY),
        )
        .route(
            "/api/runners/{runner_id}/revoke/",
            owned(post(runner_revoke), POST_ONLY),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner_enroll::manage::{FORBIDDEN_BODY, NOT_FOUND_BODY, WORKSPACE_REQUIRED_BODY};
    use pidash_db::runner_enroll::columns::revoke_reasons::KNOWN_REVOKE_REASONS;
    use pidash_services::runner_enroll::queries::{enroll_reads, manage_reads};
    use pidash_services::runner_enroll::revoke::check_revoke_reason;
    use pidash_services::runner_enroll::tokens::{mint_refresh_token, REFRESH_TOKEN_PREFIX};
    use pidash_services::runner_runs::LifecycleEffect;

    const FIXTURE_ENDPOINTS: &str =
        include_str!("../../../../fixtures/runner_enroll/handlers/endpoints.golden.json");
    const FIXTURE_TOKENS: &str =
        include_str!("../../../../fixtures/runner_enroll/tokens/tokens.golden.json");
    const FIXTURE_FLOWS: &str =
        include_str!("../../../../fixtures/runner_enroll/services/flows.golden.json");
    const FIXTURE_WIRE: &str =
        include_str!("../../../../fixtures/runner_enroll/external/wire_pins.json");
    const FIXTURE_ENROLL_REFRESH_SQL: &str =
        include_str!("../../../../fixtures/runner_enroll/queries/enroll_refresh.sql");
    const FIXTURE_MACHINES_RUNNERS_SQL: &str =
        include_str!("../../../../fixtures/runner_enroll/queries/machines_runners.sql");

    fn daemon_fixture(key: &str) -> serde_json::Value {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE_ENDPOINTS).unwrap();
        fixture["daemon"][key].clone()
    }

    fn web_fixture(key: &str) -> serde_json::Value {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE_ENDPOINTS).unwrap();
        fixture["web"][key].clone()
    }

    /// Every recorded `errors[]` entry must equal one `(body, status)`
    /// pair byte-for-byte (the `to_string` pins key order too), and
    /// every pair must be hit — both directions, no orphans.
    fn assert_errors_match(entry: &serde_json::Value, pairs: &[(&str, u16)]) {
        let errors = entry["errors"].as_array().unwrap();
        assert!(!errors.is_empty(), "fixture records no errors");
        assert_eq!(
            errors.len(),
            pairs.len(),
            "fixture error count drifted: {} vs {} pairs",
            errors.len(),
            pairs.len()
        );
        let mut hits = vec![false; pairs.len()];
        for error in errors {
            let rendered = serde_json::to_string(&error["body"]).unwrap();
            let status = error["status"].as_u64().unwrap() as u16;
            let slot = pairs
                .iter()
                .position(|(body, code)| *body == rendered && *code == status)
                .unwrap_or_else(|| {
                    panic!("fixture error {status} {rendered} matches no const pair")
                });
            hits[slot] = true;
        }
        assert!(hits.iter().all(|hit| *hit), "a const pair is unhit");
    }

    fn normalize_sql(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    // ---- D13-F7: endpoint bodies -------------------------------------

    #[test]
    fn f7_refresh_errors_match_consts() {
        assert_errors_match(
            &daemon_fixture("POST_runners_refresh"),
            &[
                (MISSING_REFRESH_TOKEN_BODY, 401),
                (INVALID_REFRESH_TOKEN_BODY, 401),
                (RUNNER_REVOKED_BODY, 401),
                (DEV_MACHINE_REVOKED_BODY, 401),
                (REFRESH_TOKEN_REPLAYED_BODY, 401),
                (MEMBERSHIP_REVOKED_BODY, 401),
            ],
        );
    }

    #[test]
    fn f7_self_revoke_errors_match_consts() {
        let entry = daemon_fixture("DELETE_runners_rid");
        let errors = entry["errors"].as_array().unwrap();
        assert_eq!(errors.len(), 2);
        // The 403 is view-owned (this module); the 401 `Detail` is
        // auth-class-owned (589, pinned there) — presence only here.
        assert_errors_match(
            &serde_json::json!({"errors": [errors[0].clone()]}),
            &[(RUNNER_ID_MISMATCH_BODY, 403)],
        );
        assert_eq!(errors[1]["status"], 401);
        assert!(errors[1]["body"].get("Detail").is_some());
    }

    #[test]
    fn f7_web_errors_match_consts() {
        assert_errors_match(
            &web_fixture("POST_dev_machines_revoke"),
            &[
                (WORKSPACE_REQUIRED_BODY, 400),
                (FORBIDDEN_BODY, 403),
                (NOT_FOUND_BODY, 404),
            ],
        );
        assert_errors_match(
            &web_fixture("POST_dev_machines_rotate"),
            &[
                (WORKSPACE_REQUIRED_BODY, 400),
                (FORBIDDEN_BODY, 403),
                (NOT_FOUND_BODY, 404),
                (DEV_MACHINE_REVOKED_BODY, 409),
            ],
        );
        assert_errors_match(
            &web_fixture("POST_runners_revoke"),
            &[(NOT_FOUND_BODY, 404), (FORBIDDEN_BODY, 403)],
        );
    }

    #[test]
    fn f7_refresh_ok_key_order() {
        // Fixture `ok` prose order: refresh_token, access_token,
        // access_token_expires_at, refresh_token_generation.
        assert_eq!(
            refresh_ok_body("rt_new", "jwt.at", "2026-10-04T00:00:00Z", 7),
            r#"{"refresh_token":"rt_new","access_token":"jwt.at","access_token_expires_at":"2026-10-04T00:00:00Z","refresh_token_generation":7}"#
        );
    }

    #[test]
    fn f7_revoke_reasons_are_known() {
        for reason in [
            REVOKE_REASON_REFRESH_REPLAYED,
            REVOKE_REASON_MEMBERSHIP,
            REVOKE_REASON_SELF,
            REVOKE_REASON_DEV_MACHINE,
            REVOKE_REASON_MANUAL,
        ] {
            assert!(
                KNOWN_REVOKE_REASONS.contains(&reason),
                "{reason} must stay in the canonical set"
            );
        }
    }

    #[test]
    fn f7_frame_reasons_match_fixture_prose() {
        let delete_ok = daemon_fixture("DELETE_runners_rid")["ok"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            delete_ok.contains(&format!("reason:{FRAME_REASON_SELF}")),
            "self-revoke frame reason drifted: {delete_ok}"
        );
        let revoke_ok = web_fixture("POST_runners_revoke")["ok"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            revoke_ok.contains(FRAME_REASON_RUNNER_REVOKE),
            "runner-revoke frame reason drifted: {revoke_ok}"
        );
        // Machine reasons are prose-absent in F7; pinned against
        // `runners.py:203,243` (verified this run).
        assert_eq!(FRAME_REASON_DEV_MACHINE, "dev machine revoked");
        assert_eq!(FRAME_REASON_ROTATE, "machine token rotated");
    }

    // ---- D13-F3: rotation mints --------------------------------------

    #[test]
    fn f3_hash_vectors_recompute() {
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};

        let fixture: serde_json::Value = serde_json::from_str(FIXTURE_TOKENS).unwrap();
        let secret = fixture["fixed_secret_key"].as_str().unwrap();
        assert_eq!(
            REFRESH_TOKEN_PREFIX,
            fixture["prefixes_ttls"]["REFRESH_TOKEN_PREFIX"]
                .as_str()
                .unwrap()
        );
        let mut pepper_hasher = Sha256::new();
        pepper_hasher.update(format!("runner/pepper/{secret}"));
        let pepper = pepper_hasher.finalize();
        for vector in fixture["hash_vectors"].as_array().unwrap() {
            let raw = vector["raw"].as_str().unwrap();
            let mut raw_hasher = Sha256::new();
            raw_hasher.update(raw.as_bytes());
            let fingerprint = hex(&raw_hasher.finalize())[..12].to_owned();
            assert_eq!(fingerprint, vector["fingerprint"].as_str().unwrap());
            let mut mac = Hmac::<Sha256>::new_from_slice(&pepper).unwrap();
            mac.update(raw.as_bytes());
            assert_eq!(
                hex(&mac.finalize().into_bytes()),
                vector["hash"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn f3_minted_refresh_token_shape() {
        // `minted_shapes.mint_refresh_token`: `rt_` + urlsafe(32);
        // hash + fingerprint recompute under the F3 formulas.
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};

        let secret = "d13-fixture-secret-key";
        let minted = mint_refresh_token(secret);
        assert!(minted.raw.starts_with(REFRESH_TOKEN_PREFIX));
        assert_eq!(minted.raw.len(), REFRESH_TOKEN_PREFIX.len() + 43);
        let mut raw_hasher = Sha256::new();
        raw_hasher.update(minted.raw.as_bytes());
        assert_eq!(hex(&raw_hasher.finalize())[..12], minted.fingerprint);
        let mut pepper_hasher = Sha256::new();
        pepper_hasher.update(format!("runner/pepper/{secret}"));
        let pepper = pepper_hasher.finalize();
        let mut mac = Hmac::<Sha256>::new_from_slice(&pepper).unwrap();
        mac.update(minted.raw.as_bytes());
        assert_eq!(hex(&mac.finalize().into_bytes()), minted.hashed);
    }

    // ---- D13-F5: refresh/revoke SQL ----------------------------------

    #[test]
    fn f5_refresh_path_sql_cores() {
        // E3 lock read: the row predicate + FOR UPDATE tail.
        let locked = normalize_sql(&enroll_reads::refresh_locked_read_sql());
        assert!(locked.contains(r#"WHERE "runner"."id" = $1"#), "{locked}");
        assert!(locked.contains("FOR UPDATE"), "{locked}");
        assert!(
            normalize_sql(FIXTURE_ENROLL_REFRESH_SQL).contains(r#"WHERE r."id" = $1 FOR UPDATE"#),
            "E3 fixture drifted"
        );
        // E4 dev-machine revoked probe: separate EXISTS, literal 1.
        let probe = normalize_sql(&enroll_reads::dev_machine_revoked_probe_sql());
        assert!(probe.contains(r#"FROM "dev_machine""#), "{probe}");
        assert!(probe.contains(r#""revoked_at" IS NOT NULL"#), "{probe}");
        // E5 rotation: `save(update_fields=[previous, hash,
        // fingerprint, generation])` renders client-side binds (Django
        // never emits the fixture's `generation + 1` / column-ref
        // shorthand — the E5 text abbreviates the ORM call, and the
        // call site binds old-hash-then-old-plus-1 to match).
        // updated_at untouched; force-refresh row cleared in-tx.
        let rotate = normalize_sql(&enroll_reads::refresh_rotate_sql());
        assert!(
            rotate.contains(
                r#"SET "refresh_token_hash" = $1, "refresh_token_fingerprint" = $2, "refresh_token_generation" = $3, "previous_refresh_token_hash" = $4 WHERE "runner"."id" = $5"#
            ),
            "{rotate}"
        );
        assert!(
            !rotate.contains("updated_at"),
            "E5 must not bump updated_at"
        );
        let clear = normalize_sql(&enroll_reads::force_refresh_clear_sql());
        assert!(
            clear.contains(
                r#"DELETE FROM "runner_force_refresh" WHERE "runner_force_refresh"."runner_id" = $1"#
            ),
            "{clear}"
        );
    }

    #[test]
    fn f5_handler_owned_builders() {
        // R4 row-locked read core (fixture abbreviates `SELECT *`;
        // the builder emits the probe-verified Django column list).
        let locked = normalize_sql(&runner_locked_read_sql());
        assert!(
            locked.contains(r#"FROM "runner" WHERE "runner"."id" = $1"#),
            "{locked}"
        );
        assert!(locked.contains("LIMIT 1 FOR UPDATE"), "{locked}");
        assert!(
            normalize_sql(FIXTURE_MACHINES_RUNNERS_SQL)
                .contains(r#"SELECT * FROM "runner" WHERE "id" = $1 LIMIT 1 FOR UPDATE"#),
            "R4 fixture drifted"
        );
        // Membership probe: literal 1, conjunct order pinned.
        assert_eq!(
            normalize_sql(MEMBERSHIP_EXISTS_SQL),
            normalize_sql(
                r#"SELECT 1 AS "a" FROM "workspace_members"
                WHERE ("workspace_members"."deleted_at" IS NULL
                AND "workspace_members"."is_active"
                AND "workspace_members"."member_id" = $1
                AND "workspace_members"."workspace_id" = $2) LIMIT 1"#
            )
        );
        // Re-read: full row, no scope, no ordering, LIMIT 1.
        let reread = normalize_sql(&runner_refresh_sql());
        assert!(
            reread.contains(r#"FROM "runner" WHERE "runner"."id" = $1 LIMIT 1"#),
            "{reread}"
        );
        assert!(!reread.contains("ORDER BY"), "{reread}");
        // Lazy loads carry their manager scope.
        assert!(
            normalize_sql(&pod_lazy_sql()).contains(r#""deleted_at" IS NULL"#),
            "PodManager scope"
        );
        assert!(
            normalize_sql(&machine_lazy_sql()).contains(r#"FROM "dev_machine""#),
            "machine lazy load"
        );
    }

    #[test]
    fn f5_machine_tx_sql_cores() {
        // M6 cores, executed in-tx by the machine endpoints.
        let locked = normalize_sql(&manage_reads::machine_locked_read_sql());
        assert!(locked.contains(r#"FROM "dev_machine""#), "{locked}");
        assert!(locked.contains("FOR UPDATE"), "{locked}");
        let revoke = normalize_sql(&manage_reads::machine_revoke_sql());
        assert!(revoke.contains(r#"SET "revoked_at" = "#), "{revoke}");
        let tokens = normalize_sql(&manage_reads::machine_tokens_revoke_sql());
        assert!(tokens.contains(r#"UPDATE "machine_token""#), "{tokens}");
        assert!(tokens.contains(r#""revoked_at" IS NULL"#), "{tokens}");
    }

    // ---- D13-F6: revoke cascade --------------------------------------

    #[test]
    fn f6_fire_plan_order() {
        let flows: serde_json::Value = serde_json::from_str(FIXTURE_FLOWS).unwrap();
        let order = flows["revoke_cascade_steps"]["post_commit_hooks"]
            .as_str()
            .unwrap();
        assert!(order.starts_with("handoffs"), "{order}");
        let bundle = EffectBundle {
            runner_id: Uuid::nil(),
            publish: vec![
                LifecycleEffect::PublishTerminalEffects {
                    run_id: Uuid::nil(),
                },
                LifecycleEffect::ApplyTerminalEffectsInline {
                    run_id: Uuid::nil(),
                },
            ],
            handoffs: vec![Uuid::nil()],
            drains: vec![Uuid::nil()],
            cleanup: Some(Uuid::nil()),
        };
        let plan = fire_plan(bundle);
        assert_eq!(plan.len(), 5);
        assert!(matches!(plan[0], FireOp::Publish(_)));
        assert!(matches!(plan[1], FireOp::Publish(_)));
        assert!(matches!(plan[2], FireOp::Handoff(_)));
        assert!(matches!(plan[3], FireOp::Drain(_)));
        assert!(matches!(plan[4], FireOp::Cleanup(_)));
    }

    #[test]
    fn f6_reason_handling() {
        // Known + short: as-is, silent.
        let (stored, warnings) = check_revoke_reason(REVOKE_REASON_SELF);
        assert_eq!(stored, REVOKE_REASON_SELF);
        assert!(warnings.is_empty());
        // Over 32 code points: truncate + warn (plus the
        // unknown-reason warning — the long form is not canonical).
        let long = format!("{}_with_a_long_suffix_past_32_chars", REVOKE_REASON_SELF);
        assert!(long.chars().count() > 32);
        let (stored, warnings) = check_revoke_reason(&long);
        assert_eq!(stored.chars().count(), 32);
        assert_eq!(warnings.len(), 2);
        // Unknown: warn + proceed (no raise).
        let (stored, warnings) = check_revoke_reason("not_a_real_reason");
        assert_eq!(stored, "not_a_real_reason");
        assert_eq!(warnings.len(), 1);
    }

    // ---- D13-F8: pubsub wire pins ------------------------------------

    #[test]
    fn f8_revoke_frame_bytes() {
        let wire: serde_json::Value = serde_json::from_str(FIXTURE_WIRE).unwrap();
        let frame_note = wire["pubsub_verbs"]["send_runner_revoke"]["frame"]
            .as_str()
            .unwrap();
        assert!(frame_note.contains("'type': 'revoke'"), "{frame_note}");
        assert!(frame_note.contains("NO runner_id"), "{frame_note}");
        // The self-revoke `send_to_runner` input this module builds:
        // exactly {type, reason}, insertion order, no runner_id.
        let rendered = serde_json::Value::Object(revoke_frame(FRAME_REASON_SELF)).to_string();
        assert_eq!(rendered, r#"{"type":"revoke","reason":"self_revoked"}"#);
        // The `send_runner_revoke` call-site reasons are all distinct,
        // non-empty, and prose-verified where the fixtures pin them.
        let reasons = [
            FRAME_REASON_SELF,
            FRAME_REASON_DEV_MACHINE,
            FRAME_REASON_ROTATE,
            FRAME_REASON_RUNNER_REVOKE,
        ];
        for reason in reasons {
            assert!(!reason.is_empty());
        }
        let mut deduped = reasons.to_vec();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(deduped.len(), reasons.len());
    }
}
