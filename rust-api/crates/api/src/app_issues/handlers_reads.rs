#![forbid(unsafe_code)]
// Handlers return `Response` errors directly (custom bodies `Denial` cannot
// express); the per-function `allow` precedent would repeat twelve times.
#![allow(clippy::result_large_err)]

//! Misc issue reads + sub-issues (D-26 handlers-B).
//!
//! Ports five units of `app/views/issue/` onto the merged layers:
//! - `GET`/`PATCH .../user-properties/` (`base.py:756-784`,
//!   `ProjectUserDisplayPropertyEndpoint`)
//! - `POST .../issue-dates/` (`base.py:1119-1196`, `IssueBulkUpdateDateEndpoint`)
//! - `GET .../issues/<id>/meta/` (`base.py:1199-1211`, `IssueMetaEndpoint`)
//! - `GET .../work-items/<ident>-<n>/` (`base.py:1214-1489`,
//!   `IssueDetailIdentifierEndpoint`, lite + full)
//! - `GET`/`POST .../issues/<id>/sub-issues/` (`sub_issue.py:33-248`,
//!   `SubIssuesEndpoint`)
//!
//! Registration is the cutover granularity: only the owned methods on these
//! five paths are served from Rust; every other method proxies to Django
//! through [`crate::edge::proxy`] (DRF authenticates before it checks the
//! method, so answering 405 in Rust would break the anonymous-vs-authed
//! split Django owns).
//!
//! Reuse (merged, called not copied):
//! - SQL builders: `super::queries_core` (sub-issue selects / scope) and
//!   `super::queries_engage` (`meta_sql`, the identifier project / member /
//!   lite / full statements).
//! - Shapes: `pidash_services::app_issues` (`issue_detail_to_representation`
//!   for the 36-key identifier-full body, `issue_detail_base_to_representation`
//!   for the sub-issue POST rows, `project_user_property_to_representation`,
//!   `issue_is_actively_synced`, `order_sql`).
//! - Row fetching / shaping: `super::{fetch_json_rows, fetch_count,
//!   order_key, shape_row, query_last}` plus the F-07 serializer kernel.
//! - The blocker fields select out of the merged D-12
//!   `pidash_services::orchestration::blockers` row form; the ticker block
//!   runs on the merged D-10 ticker struct + D-12 clock.
//! - Enqueues are best-effort deferred publishes into `rust_job_queue`
//!   (the cycles precedent): the response stands when the queue is down.
//!
//! Deliberately handler-local (small, path-specific): the gate (none of
//! these views fetches the project row except identifier, so pilot-2's
//! `resolve_gate` 404 would be wrong here), the JSON body reader, and the
//! full-row column-alias derivation.
//!
//! Fixtures: `rust-api/fixtures/app_issues/handlers/FX-ISS-15.reads.json`
//! and `rust-api/fixtures/app_issues/guards/FX-ISS-21.signals_tasks.json`.
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//! - Bulk dates: issues missing from the DB are silently skipped; issues
//!   with both dates set are appended twice (harmless under `bulk_update`);
//!   non-`%Y-%m-%d` date strings raise `ValueError` → generic 500; the 400
//!   body uses the `message` key, not `error`. `int`/`bool`/`None` ids
//!   convert in `id__in` and then miss (skipped, 200); a repeated id
//!   observes the earlier update's in-memory dates.
//! - Bulk dates and the sub-issue link write through `bulk_update`, so no
//!   signals fire (FX-ISS-21): no orchestration transition, no git-sync
//!   completion — only the `issue_activity` enqueues below.
//! - Sub-issue POST `current_instance` carries the SUB-issue id
//!   (`{"parent": sub_id}`), not the old parent.
//! - Identifier lite honors the git-only `is_synced` annotation, so
//!   Github-synced issues report `is_synced: false` there.
//! - Sub-issue POST rows render the unannotated shape (the view annotates
//!   only `state_group`, which the serializer does not read): the seven
//!   annotation-driven base fields are omitted via `SkipField`.
//! - An invalid `group_by` key raises `KeyError` → 400, and `order_by` on
//!   an unknown field raises `FieldError` → generic 500.
//! - PATCH user-properties creates the row before validating, so a failed
//!   PATCH still leaves a fresh defaults row behind.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};

use crate::state::AppState;

use super::queries_core;
use super::queries_engage;
use super::{
    fetch_count, fetch_json_rows, order_key, query_last, shape_row, Binder, Denial, QueryMap,
};
use pidash_db::tasks_ticker::IssueAgentTicker;
use pidash_services::app_issues::serializers_assoc::{
    project_user_property_to_representation, ProjectUserPropertyRow,
};
use pidash_services::app_issues::{
    issue_detail_base_to_representation, issue_detail_to_representation, issue_is_actively_synced,
    order_sql, AgentLiveStateRow, AgentRunDetailRow, AgentTickerInput, IssueDetailBaseRow,
    IssueDetailRow, ACTIVE_AGENT_RUN_SQL, AGENT_RUN_COUNT_SQL, GITHUB_ISSUE_SYNC_PROBE_SQL,
    GIT_ISSUE_SYNC_PROBE_SQL, LATEST_AGENT_RUN_SQL,
};
use pidash_services::orchestration::blockers::{
    blockers_sql, dependents_sql, has_open_blockers_sql, relations_summary, BlockerRow,
    RelationsSummary,
};
use pidash_services::orchestration::clock::ProjectClockPolicy;
use pidash_types::orchestration::StateRef;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the five owned paths. Sibling paths have no Rust route and keep
/// proxying to Django through the fallback; non-owned methods on owned paths
/// proxy per-method (DRF's auth-before-method semantics).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/user-properties/",
            axum::routing::get(user_properties_get)
                .patch(user_properties_patch)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/issue-dates/",
            axum::routing::post(bulk_dates)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/meta/",
            axum::routing::get(meta)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        // One `{tail}` segment: Django's
        // `<str:project_identifier>-<str:issue_identifier>` splits on the
        // LAST dash (greedy first group); tails without a dash, or with an
        // empty side, match no Django route and proxy so Django answers its
        // own resolver 404.
        .route(
            "/api/workspaces/{slug}/work-items/{tail}/",
            axum::routing::get(identifier)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/sub-issues/",
            axum::routing::get(sub_issues_get)
                .post(sub_issues_post)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

// ---------------------------------------------------------------------------
// Gate
// ---------------------------------------------------------------------------

/// The authenticated request context for these paths: who acts, in which
/// project, in which zone. Unlike pilot-2's `Gate` there is deliberately no
/// project-row existence check — none of these views fetches the project row
/// (except identifier, which does it explicitly), so a missing or
/// soft-deleted project must NOT 404 here.
struct ReadsGate {
    user_id: uuid::Uuid,
    timezone: Tz,
    project_id: uuid::Uuid,
}

/// `request.user` from the Django session (`_auth_user_id`), mirroring
/// pilot-2's actor lookup: no session, no key, or a non-UUID id means
/// anonymous → 401.
fn actor_user_id(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Option<uuid::Uuid> {
    let handle = extension?.0;
    let mut session = handle.snapshot();
    let raw = session.get("_auth_user_id")?.as_str()?.to_owned();
    raw.parse::<uuid::Uuid>().ok()
}

/// Python `str.strip()` membership (`db/models/project.py:210`): Rust
/// `White_Space` plus U+001C-U+001F (verified by exhaustively diffing
/// `str.strip` against `char::is_whitespace` over all code points —
/// those four are the only differences).
fn is_py_strip_ws(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}

/// Normalize a non-UUID identifier for the equality lookup
/// (`db/models/project.py:210`): `str(value).strip().upper()`.
fn normalize_resolve_identifier(raw: &str) -> String {
    raw.trim_matches(is_py_strip_ws).to_uppercase()
}

/// `Project.resolve(workspace_slug, value)`: UUIDs pass through; other
/// identifiers match the upper-cased `identifier` in the workspace; misses
/// raise `Http404("Project not found")` → 404 `{"detail": ...}`.
async fn rewrite_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = normalize_resolve_identifier(raw);
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ProjectNotFound)
}

/// `allow_permission(allowed)` at `PROJECT` level (`app/permissions/base.py`):
/// an active project membership with a listed role, or any active project
/// membership plus an active workspace ADMIN membership. Else the
/// allow-style 403. Soft-deleted memberships do not count.
async fn allow_roles(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    allowed: &[i32],
) -> Result<(), Denial> {
    let role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if role.is_some_and(|row| allowed.contains(&i32::from(row.0))) {
        return Ok(());
    }
    let member: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let admin: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.role = 20
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if member.is_some() && admin.is_some() {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// `ProjectEntityPermission` (`app/permissions/project.py:85`): reads need
/// any active project membership (no role filter, no workspace-admin
/// fallback); writes need ADMIN/MEMBER. Denials render DRF's default
/// `{"detail": ...}` body (the class sets no `message`).
async fn entity_access(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    write: bool,
) -> Result<(), Response> {
    let role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    let allowed = matches!(
        (write, role),
        (false, Some(_)) | (true, Some((20,))) | (true, Some((15,)))
    );
    if allowed {
        Ok(())
    } else {
        Err(json_response(
            StatusCode::FORBIDDEN,
            crate::permissions::DEFAULT_DENIED_BODY.to_owned(),
        ))
    }
}

/// The actor's zone (`TimezoneMixin` activates it per request; every
/// serializer datetime renders through it). Resolved on every path — even
/// the datetime-free ones — because a garbage zone 500s in `initial()`
/// before any view code runs.
async fn actor_timezone(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Session auth → project rewrite → `@allow_permission` gate → zone, in
/// Django's order.
async fn resolve_allow_gate(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    allowed: &[i32],
) -> Result<(sqlx::PgPool, ReadsGate), Denial> {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let project_id = rewrite_project_id(&pool, slug, project_raw).await?;
    allow_roles(&pool, slug, &project_id, &user_id, allowed).await?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    Ok((
        pool,
        ReadsGate {
            user_id,
            timezone,
            project_id,
        },
    ))
}

/// Session auth → project rewrite → `ProjectEntityPermission` → zone.
async fn resolve_entity_gate(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    write: bool,
) -> Result<(sqlx::PgPool, ReadsGate), Response> {
    let pool = pool_of(state).map_err(IntoResponse::into_response)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized.into_response())?;
    let project_id = rewrite_project_id(&pool, slug, project_raw)
        .await
        .map_err(IntoResponse::into_response)?;
    entity_access(&pool, slug, &project_id, &user_id, write).await?;
    let timezone = actor_timezone(&pool, &user_id)
        .await
        .map_err(IntoResponse::into_response)?;
    Ok((
        pool,
        ReadsGate {
            user_id,
            timezone,
            project_id,
        },
    ))
}

// ---------------------------------------------------------------------------
// Small responses
// ---------------------------------------------------------------------------

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static response")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// The identifier/member/guest 403 (`base.py:1262-1265`, `:1471-1474`):
/// a view-inline body, not the allow-style one.
fn not_allowed_body() -> Response {
    json_response(
        StatusCode::FORBIDDEN,
        r#"{"error":"You are not allowed to view this issue"}"#.to_owned(),
    )
}

/// The bulk-dates validation 400 (`base.py:1162-1165`): note the `message`
/// key, not `error`.
fn dates_exceeded_body() -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        r#"{"message":"Start date cannot exceed target date"}"#.to_owned(),
    )
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

/// Read the JSON body of a proxied-or-owned request (the cycles `detail_body`
/// shape): same 2 MiB default limit as axum's `Json`; an empty body reads as
/// `{}`, like DRF; a non-JSON media type is DRF's 415; a malformed document
/// answers the extractor rejection.
async fn read_body(state: &AppState, req: axum::extract::Request) -> Result<Value, Response> {
    use axum::extract::FromRequest;
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, 2 * 1024 * 1024)
        .await
        .map_err(|_| Denial::ServerError.into_response())?;
    if bytes.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let mime = content_type.split(';').next().unwrap_or("").trim();
    if !mime.eq_ignore_ascii_case("application/json") {
        let named = if content_type.is_empty() {
            "text/plain"
        } else {
            content_type
        };
        let body = format!(
            "{{\"detail\":\"Unsupported media type {} in request.\"}}",
            json_string(named)
        );
        return Err(json_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, body));
    }
    let req = axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes));
    match axum::Json::<Value>::from_request(req, state).await {
        Ok(axum::Json(body)) => Ok(body),
        Err(rejection) => Err(rejection.into_response()),
    }
}

// ---------------------------------------------------------------------------
// Row decoding
// ---------------------------------------------------------------------------

fn get_str<'a>(row: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

fn get_i64(row: &Map<String, Value>, key: &str) -> Option<i64> {
    row.get(key).and_then(Value::as_i64)
}

fn get_i32(row: &Map<String, Value>, key: &str) -> Option<i32> {
    get_i64(row, key).and_then(|value| i32::try_from(value).ok())
}

fn get_bool(row: &Map<String, Value>, key: &str) -> Option<bool> {
    row.get(key).and_then(Value::as_bool)
}

fn get_f64(row: &Map<String, Value>, key: &str) -> Option<f64> {
    row.get(key).and_then(Value::as_f64)
}

/// A `row_to_json` timestamptz string as UTC.
fn get_moment(row: &Map<String, Value>, key: &str) -> Option<DateTime<Utc>> {
    let text = get_str(row, key)?;
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|aware| aware.with_timezone(&Utc))
}

/// DRF `DateTimeField` rendering through the activated actor zone (the
/// `TimezoneMixin` rule pilot-2's serializer path follows).
fn render_actor_moment(row: &Map<String, Value>, key: &str, timezone: &Tz) -> Option<String> {
    let text = get_str(row, key)?;
    let aware = chrono::DateTime::parse_from_rfc3339(text).ok()?;
    Some(crate::serializer::render_datetime_in(&aware, timezone))
}

/// DRF plain-JSON-encoder rendering for a stored-UTC datetime (the `.values()`
/// / raw-dict paths): `isoformat` with `+00:00` rewritten to `Z`.
fn render_utc_moment(row: &Map<String, Value>, key: &str) -> Option<String> {
    let text = get_str(row, key)?;
    let aware = chrono::DateTime::parse_from_rfc3339(text).ok()?;
    Some(crate::serializer::render_datetime(
        &aware.with_timezone(&Utc),
    ))
}

/// A uuid-array column (`COALESCE(..., '{}')` — never null) as owned strings.
fn get_id_list(row: &Map<String, Value>, key: &str) -> Vec<String> {
    row.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Deferred publishes (best-effort `rust_job_queue` rows; the response stands)
// ---------------------------------------------------------------------------

/// Celery wire name for `issue_activity`
/// (`bgtasks/issue_activities_task.py:1504`).
const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";
/// Celery wire name for `recent_visited_task` (bare `@shared_task` default).
const RECENT_VISITED_TASK: &str = "pi_dash.bgtasks.recent_visited_task.recent_visited_task";

/// `base_host(request, is_app=True)` (`utils/host.py:17`): `WEB_URL` else
/// `APP_BASE_URL`; unset skips the enqueue (the cycles precedent — Django
/// would 500, but every merged handler treats the queue as best-effort).
fn request_origin(state: &AppState) -> Option<String> {
    state
        .settings()
        .urls
        .web_url
        .clone()
        .or_else(|| state.settings().urls.app_base_url.clone())
}

/// Best-effort deferred publish (the space intake precedent): without the
/// queue the response still stands.
async fn enqueue_message(pool: &sqlx::PgPool, message: pidash_jobs::celery::CeleryTaskMessage) {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// One bulk-dates `issue_activity.delay(...)` (`base.py:1168-1176`,
/// `:1181-1189`): seven kwargs exactly as the view passes them — notably NO
/// `notification`/`origin` keys, so the task defaults apply.
fn bulk_dates_activity_message(
    field: &str,
    raw_new: &Value,
    current: &str,
    issue_id: &str,
    actor_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    epoch: i64,
) -> pidash_jobs::celery::CeleryTaskMessage {
    // `json.dumps` default separators (`", "`, `": "`), not serde compact.
    let requested_data = format!(
        "{{\"{field}\": {}}}",
        serde_json::to_string(raw_new).unwrap_or("null".to_owned())
    );
    let current_instance = format!("{{\"{field}\": {}}}", json_string(current));
    let mut kwargs = Map::with_capacity(7);
    kwargs.insert(
        "type".to_owned(),
        Value::String("issue.activity.updated".to_owned()),
    );
    kwargs.insert("requested_data".to_owned(), Value::String(requested_data));
    kwargs.insert(
        "current_instance".to_owned(),
        Value::String(current_instance),
    );
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_owned()));
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert("epoch".to_owned(), Value::from(epoch));
    pidash_jobs::celery::CeleryTaskMessage::new(ISSUE_ACTIVITY_TASK, vec![], kwargs)
}

/// One sub-issue link `issue_activity.delay(...)` (`sub_issue.py:224-237`):
/// nine kwargs in call order — `requested_data` carries the PARENT path id
/// while `current_instance` carries the SUB id (ported bug: the current map
/// should hold the old parent).
fn sub_issue_activity_message(
    parent_id: &uuid::Uuid,
    sub_issue_id: &str,
    actor_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    epoch: i64,
    origin: &str,
) -> pidash_jobs::celery::CeleryTaskMessage {
    let mut kwargs = Map::with_capacity(9);
    kwargs.insert(
        "type".to_owned(),
        Value::String("issue.activity.updated".to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(format!("{{\"parent\": \"{parent_id}\"}}")),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert(
        "issue_id".to_owned(),
        Value::String(sub_issue_id.to_owned()),
    );
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        Value::String(format!("{{\"parent\": \"{sub_issue_id}\"}}")),
    );
    kwargs.insert("epoch".to_owned(), Value::from(epoch));
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    pidash_jobs::celery::CeleryTaskMessage::new(ISSUE_ACTIVITY_TASK, vec![], kwargs)
}

/// The identifier `recent_visited_task.delay(...)` (`base.py:1476-1482`):
/// kwargs in call order, every id a string.
fn identifier_visited_message(
    slug: &str,
    issue_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
) -> pidash_jobs::celery::CeleryTaskMessage {
    let mut kwargs = Map::with_capacity(5);
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    kwargs.insert("entity_name".to_owned(), Value::String("issue".to_owned()));
    kwargs.insert(
        "entity_identifier".to_owned(),
        Value::String(issue_id.to_string()),
    );
    kwargs.insert("user_id".to_owned(), Value::String(user_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    pidash_jobs::celery::CeleryTaskMessage::new(RECENT_VISITED_TASK, vec![], kwargs)
}

// ---------------------------------------------------------------------------
// User display properties (`base.py:756-784`)
// ---------------------------------------------------------------------------

/// `GET`/`PATCH .../user-properties/`: the actor's per-project display row,
/// created with model defaults on first touch. `@allow_permission([ADMIN,
/// MEMBER, GUEST])` on both methods.
async fn user_properties_get(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Response, Denial> {
    let (pool, gate) =
        resolve_allow_gate(&state, &slug, &project_raw, extension, &[20, 15, 5]).await?;
    let row = fetch_or_create_property(&pool, &gate).await?;
    Ok(render_property(&row, &gate.timezone))
}

async fn user_properties_patch(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Response> {
    let (pool, gate) = resolve_allow_gate(&state, &slug, &project_raw, extension, &[20, 15, 5])
        .await
        .map_err(IntoResponse::into_response)?;
    // Created BEFORE validation (`:759-775`): a failing PATCH still leaves
    // a fresh defaults row behind.
    fetch_or_create_property(&pool, &gate)
        .await
        .map_err(IntoResponse::into_response)?;
    let body = read_body(&state, req).await?;
    let object = match body {
        Value::Object(object) => object,
        _ => {
            let detail = format!(
                "Invalid data. Expected a dictionary, but got {}.",
                json_type_name(&body)
            );
            return Err(json_response(
                StatusCode::BAD_REQUEST,
                format!("{{\"non_field_errors\":[{}]}}", json_string(&detail)),
            ));
        }
    };
    let validated = validate_property_patch(&object)?;
    apply_property_patch(&pool, &gate, &validated)
        .await
        .map_err(IntoResponse::into_response)?;
    let row = fetch_property(&pool, &gate)
        .await
        .map_err(IntoResponse::into_response)?
        .ok_or(Denial::ServerError.into_response())?;
    Ok(render_property(&row, &gate.timezone))
}

/// Model-default JSON documents (`db/models/issue.py:50-90`,
/// `db/models/project.py:68-69`), in source key order.
fn default_filters() -> Value {
    serde_json::json!({
        "priority": null, "state": null, "state_group": null,
        "assignees": null, "created_by": null, "labels": null,
        "start_date": null, "target_date": null, "subscriber": null,
    })
}

fn default_display_filters() -> Value {
    serde_json::json!({
        "group_by": null, "order_by": "-created_at", "type": null,
        "sub_issue": true, "show_empty_groups": true, "layout": "list",
        "calendar_date_range": "",
    })
}

fn default_display_properties() -> Value {
    serde_json::json!({
        "assignee": true, "attachment_count": true, "created_on": true,
        "due_date": true, "estimate": true, "key": true, "labels": true,
        "link": true, "priority": true, "start_date": true, "state": true,
        "sub_issue_count": true, "updated_on": true,
    })
}

fn default_preferences() -> Value {
    serde_json::json!({
        "pages": {"block_display": true},
        "navigation": {"default_tab": "work_items", "hide_in_more_menu": []},
    })
}

const PROPERTY_COLUMNS: &str = "id, created_at, updated_at, deleted_at, filters, display_filters, \
    display_properties, rich_filters, preferences, sort_order, created_by_id, updated_by_id, \
    project_id, workspace_id, user_id";

async fn fetch_property(
    pool: &sqlx::PgPool,
    gate: &ReadsGate,
) -> Result<Option<Map<String, Value>>, Denial> {
    let mut binder = Binder::new();
    let user = binder.bind_uuid(gate.user_id);
    let project = binder.bind_uuid(gate.project_id);
    let inner = format!(
        "SELECT {PROPERTY_COLUMNS} FROM project_user_properties \
        WHERE user_id = {user} AND project_id = {project} AND deleted_at IS NULL \
        ORDER BY created_at DESC LIMIT 1"
    );
    let mut rows = fetch_json_rows(pool, &inner, binder.values()).await?;
    Ok(rows.pop())
}

/// `get_or_create(user, project)`: the live row, or a fresh defaults row.
/// `ProjectBaseModel.save` sets `workspace` from the project, so a missing
/// project row 404s here (`ObjectDoesNotExist`); the unique guard retries
/// the read on a lost race, like Django's `get_or_create`.
async fn fetch_or_create_property(
    pool: &sqlx::PgPool,
    gate: &ReadsGate,
) -> Result<Map<String, Value>, Denial> {
    if let Some(row) = fetch_property(pool, gate).await? {
        return Ok(row);
    }
    let workspace: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL")
            .bind(gate.project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = workspace else {
        return Err(Denial::NotFound);
    };
    let now = Utc::now();
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO project_user_properties (id, created_at, updated_at, created_by_id, \
        updated_by_id, deleted_at, project_id, workspace_id, user_id, filters, display_filters, \
        display_properties, rich_filters, preferences, sort_order) \
        VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
        ON CONFLICT (user_id, project_id) WHERE deleted_at IS NULL DO NOTHING",
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(gate.user_id)
    .bind(gate.project_id)
    .bind(workspace_id)
    .bind(gate.user_id)
    .bind(default_filters())
    .bind(default_display_filters())
    .bind(default_display_properties())
    .bind(Value::Object(Map::new()))
    .bind(default_preferences())
    .bind(65535.0f64)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    fetch_property(pool, gate).await?.ok_or(Denial::ServerError)
}

/// Validated PATCH payload: the five JSON documents plus `sort_order`, each
/// `Some` when the input carries the key. Unknown, read-only (`user`,
/// `workspace`, `project`) and auto keys are ignored, like DRF.
#[derive(Default)]
struct PropertyPatch {
    filters: Option<Value>,
    display_filters: Option<Value>,
    display_properties: Option<Value>,
    rich_filters: Option<Value>,
    preferences: Option<Value>,
    sort_order: Option<f64>,
}

/// `ProjectUserPropertySerializer(partial=True)` validation
/// (`issue.py:542-548`): JSON fields take any non-null document;
/// `sort_order` takes DRF `FloatField` input (numbers, numeric strings,
/// bools as 1.0/0.0). Errors render DRF's per-field lists, in model field
/// order.
fn validate_property_patch(object: &Map<String, Value>) -> Result<PropertyPatch, Response> {
    let mut patch = PropertyPatch::default();
    let mut errors: Vec<String> = Vec::new();
    for field in [
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
        "preferences",
    ] {
        if let Some(value) = object.get(field) {
            if value.is_null() {
                errors.push(format!(
                    "{}:[\"This field may not be null.\"]",
                    json_string(field)
                ));
            } else {
                let slot = match field {
                    "filters" => &mut patch.filters,
                    "display_filters" => &mut patch.display_filters,
                    "display_properties" => &mut patch.display_properties,
                    "rich_filters" => &mut patch.rich_filters,
                    _ => &mut patch.preferences,
                };
                *slot = Some(value.clone());
            }
        }
    }
    if let Some(value) = object.get("sort_order") {
        match parse_float_field(value) {
            Some(number) => patch.sort_order = Some(number),
            None if value.is_null() => {
                errors.push("\"sort_order\":[\"This field may not be null.\"]".to_owned())
            }
            None => errors.push("\"sort_order\":[\"A valid number is required.\"]".to_owned()),
        }
    }
    if !errors.is_empty() {
        return Err(json_response(
            StatusCode::BAD_REQUEST,
            format!("{{{}}}", errors.join(",")),
        ));
    }
    Ok(patch)
}

/// DRF `FloatField.to_internal_value`: bools coerce via `float()`, numeric
/// strings parse, anything else (including null) fails.
fn parse_float_field(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        Value::String(text) => text.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => None,
    }
}

/// `type(data).__name__` for the non-dict PATCH body message.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `serializer.save()`: the validated fields plus `updated_at` (`auto_now`)
/// and `updated_by` (the actor, via `BaseModel.save`). Runs even for an
/// empty PATCH.
async fn apply_property_patch(
    pool: &sqlx::PgPool,
    gate: &ReadsGate,
    patch: &PropertyPatch,
) -> Result<(), Denial> {
    let now = Utc::now();
    // Bind order follows the SET list built below ($1..$4 fixed, validated
    // fields from $5 on).
    let mut fields: Vec<(&str, Value)> = Vec::new();
    if let Some(value) = &patch.filters {
        fields.push(("filters", value.clone()));
    }
    if let Some(value) = &patch.display_filters {
        fields.push(("display_filters", value.clone()));
    }
    if let Some(value) = &patch.display_properties {
        fields.push(("display_properties", value.clone()));
    }
    if let Some(value) = &patch.rich_filters {
        fields.push(("rich_filters", value.clone()));
    }
    if let Some(value) = &patch.preferences {
        fields.push(("preferences", value.clone()));
    }
    let mut sql =
        String::from("UPDATE project_user_properties SET updated_at = $1, updated_by_id = $2");
    for (index, (field, _)) in fields.iter().enumerate() {
        sql.push_str(&format!(", {field} = ${}", index + 5));
    }
    if patch.sort_order.is_some() {
        sql.push_str(&format!(", sort_order = ${}", fields.len() + 5));
    }
    sql.push_str(" WHERE user_id = $3 AND project_id = $4 AND deleted_at IS NULL");
    let mut query = sqlx::query(&sql)
        .bind(now)
        .bind(gate.user_id)
        .bind(gate.user_id)
        .bind(gate.project_id);
    for (_, value) in &fields {
        query = query.bind(value);
    }
    if let Some(number) = patch.sort_order {
        query = query.bind(number);
    }
    query.execute(pool).await.map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// `ProjectUserPropertySerializer.to_representation`: the 15-key row with
/// datetimes in the actor zone.
fn render_property(row: &Map<String, Value>, timezone: &Tz) -> Response {
    let created_at = render_actor_moment(row, "created_at", timezone);
    let updated_at = render_actor_moment(row, "updated_at", timezone);
    let deleted_at = render_actor_moment(row, "deleted_at", timezone);
    let created_by = get_str(row, "created_by_id").map(str::to_owned);
    let updated_by = get_str(row, "updated_by_id").map(str::to_owned);
    let empty = Value::Object(Map::new());
    let owned = ProjectUserPropertyRow {
        id: get_str(row, "id").unwrap_or_default(),
        created_at: created_at.as_deref(),
        updated_at: updated_at.as_deref(),
        deleted_at: deleted_at.as_deref(),
        filters: row.get("filters").unwrap_or(&empty),
        display_filters: row.get("display_filters").unwrap_or(&empty),
        display_properties: row.get("display_properties").unwrap_or(&empty),
        rich_filters: row.get("rich_filters").unwrap_or(&empty),
        preferences: row.get("preferences").unwrap_or(&empty),
        sort_order: get_f64(row, "sort_order").unwrap_or(f64::NAN),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        project: get_str(row, "project_id").unwrap_or_default(),
        workspace: get_str(row, "workspace_id").unwrap_or_default(),
        user: get_str(row, "user_id").unwrap_or_default(),
    };
    let view = project_user_property_to_representation(&owned);
    let body = serde_json::to_string(&view).unwrap_or_default();
    json_response(StatusCode::OK, body)
}

// ---------------------------------------------------------------------------
// Bulk dates (`base.py:1119-1196`)
// ---------------------------------------------------------------------------

/// `POST .../issue-dates/`: set start/target dates over many issues.
/// `@allow_permission([ADMIN, MEMBER])`. Writes through `bulk_update` (no
/// signals — FX-ISS-21); one `issue_activity` enqueue per SET date.
async fn bulk_dates(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Response> {
    let (pool, gate) = resolve_allow_gate(&state, &slug, &project_raw, extension, &[20, 15])
        .await
        .map_err(IntoResponse::into_response)?;
    let body = read_body(&state, req).await?;
    // `request.data.get("updates", [])`: a non-object body has no `.get`
    // (`AttributeError` → 500); `.get` on the object defaults to `[]`.
    let object = body
        .as_object()
        .ok_or(Denial::ServerError.into_response())?;
    let updates = select_updates(object)?;
    // The id list comprehension runs FIRST (`:1143`): a non-object element
    // raises `TypeError` → 500, a missing `id` raises `KeyError` → 400.
    let mut ids: Vec<UpdateId<'_>> = Vec::with_capacity(updates.len());
    for update in updates {
        let item = update
            .as_object()
            .ok_or(Denial::ServerError.into_response())?;
        let id = item.get("id").ok_or_else(|| {
            Denial::BadError("The required key does not exist.".to_owned()).into_response()
        })?;
        ids.push(classify_update_id(id)?);
    }
    // `id__in` with a non-UUID STRING raises `ValidationError` → 400; the
    // converted ids (`int`/`bool`/`None`) join the lookup harmlessly and
    // then miss the `str(issue.id)`-keyed dict.
    let mut parsed: Vec<uuid::Uuid> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let UpdateId::Raw(text) = id {
            match text.parse::<uuid::Uuid>() {
                Ok(value) => parsed.push(value),
                Err(_) => return Err(invalid_detail_body()),
            }
        }
    }
    let epoch = Utc::now().timestamp();
    // One fetch over plain `Issue.objects` (`:1147`): triage, archived and
    // draft rows ARE updatable here.
    let rows = fetch_bulk_issues(&pool, &slug, &gate.project_id, &parsed)
        .await
        .map_err(IntoResponse::into_response)?;
    let mut by_id: std::collections::HashMap<&str, &Map<String, Value>> =
        std::collections::HashMap::with_capacity(rows.len());
    for row in &rows {
        if let Some(id) = get_str(row, "id") {
            by_id.insert(id, row);
        }
    }
    // Per-update processing in request order (`:1151-1191`): unknown ids
    // are silently SKIPPED (the dict is keyed by canonical uuid string, so
    // a non-canonical-but-valid spelling misses too — as do the converted
    // `int`/`bool`/`None` ids, which never equal a `str(issue.id)` key).
    // A repeated id observes the EARLIER update's values: the loop assigns
    // onto the shared in-memory issue (`issue.start_date = …`), so the
    // second occurrence validates against and enqueues the first one's
    // dates, not the stored ones.
    let mut writes: Vec<BulkWrite> = Vec::new();
    for (update, id) in updates.iter().zip(ids.iter()) {
        let id = match id {
            UpdateId::Raw(text) => *text,
            UpdateId::Unmatchable => continue,
        };
        let Some(row) = by_id.get(id) else {
            continue;
        };
        let item = update
            .as_object()
            .ok_or(Denial::ServerError.into_response())?;
        let (written_start, written_target) = written_current(&writes, id);
        let current_start = written_start
            .as_deref()
            .or_else(|| get_str(row, "start_date"));
        let current_target = written_target
            .as_deref()
            .or_else(|| get_str(row, "target_date"));
        let new_start = item.get("start_date").unwrap_or(&Value::Null);
        let new_target = item.get("target_date").unwrap_or(&Value::Null);
        let merged_start = merge_date(current_start, new_start)?;
        let merged_target = merge_date(current_target, new_target)?;
        if exceeds(&merged_start, &merged_target) {
            return Err(dates_exceeded_body());
        }
        if is_truthy(new_start) {
            let parsed = parse_date_value(new_start).ok_or(Denial::ServerError.into_response())?;
            enqueue_message(
                &pool,
                bulk_dates_activity_message(
                    "start_date",
                    new_start,
                    &current_render(current_start),
                    id,
                    &gate.user_id,
                    &gate.project_id,
                    epoch,
                ),
            )
            .await;
            upsert_write(&mut writes, id, Some(parsed), None);
        }
        if is_truthy(new_target) {
            let parsed = parse_date_value(new_target).ok_or(Denial::ServerError.into_response())?;
            enqueue_message(
                &pool,
                bulk_dates_activity_message(
                    "target_date",
                    new_target,
                    &current_render(current_target),
                    id,
                    &gate.user_id,
                    &gate.project_id,
                    epoch,
                ),
            )
            .await;
            upsert_write(&mut writes, id, None, Some(parsed));
        }
    }
    apply_bulk_dates(&pool, &writes)
        .await
        .map_err(IntoResponse::into_response)?;
    Ok(json_response(
        StatusCode::OK,
        r#"{"message":"Issues updated successfully"}"#.to_owned(),
    ))
}

async fn fetch_bulk_issues(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    ids: &[uuid::Uuid],
) -> Result<Vec<Map<String, Value>>, Denial> {
    // `id__in=[]` short-circuits to no rows (`EmptyResultSet`) — and an
    // empty `IN ()` would be a syntax error.
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut binder = Binder::new();
    let mut holders = Vec::with_capacity(ids.len());
    for id in ids {
        holders.push(binder.bind_uuid(*id));
    }
    let project = binder.bind_uuid(*project_id);
    let slug_holder = binder.bind_string((*slug).to_owned());
    let inner = format!(
        "SELECT issue.id, issue.start_date, issue.target_date FROM issues AS issue \
        JOIN workspaces ON workspaces.id = issue.workspace_id \
        WHERE issue.deleted_at IS NULL AND issue.id IN ({}) \
        AND issue.project_id = {project} AND workspaces.slug = {slug_holder}",
        holders.join(",")
    );
    fetch_json_rows(pool, &inner, binder.values()).await
}

/// `request.data.get("updates", [])` plus iteration (`:1141-1143`): a
/// missing key defaults to `[]`; an explicit null or any other non-list
/// breaks iteration (`TypeError` → 500).
fn select_updates(object: &Map<String, Value>) -> Result<&[Value], Response> {
    match object.get("updates") {
        None => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        _ => Err(Denial::ServerError.into_response()),
    }
}

/// `update["id"]` as the `id__in` lookup sees it (`:1143-1147`):
/// `UUIDField.to_python` converts ints/bools (`uuid.UUID(int=…)`) and
/// passes nulls through (`IS NULL`), so those ids query harmlessly and
/// then miss the `str(issue.id)`-keyed dict → skipped with a 200; floats,
/// lists and dicts fail conversion (`ValidationError` → 400), as do
/// non-UUID strings (rejected at the parse step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateId<'a> {
    Raw(&'a str),
    Unmatchable,
}

fn classify_update_id(value: &Value) -> Result<UpdateId<'_>, Response> {
    match value {
        Value::String(text) => Ok(UpdateId::Raw(text)),
        Value::Number(number) if number.is_i64() || number.is_u64() => Ok(UpdateId::Unmatchable),
        Value::Bool(_) | Value::Null => Ok(UpdateId::Unmatchable),
        _ => Err(invalid_detail_body()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BulkWrite<'a> {
    id: &'a str,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
}

/// The in-request mutation a repeated id observes (`:1177-1191` assign
/// onto the shared in-memory issue): a previously written half renders
/// ISO, exactly like `str()` of the assigned date.
fn written_current(writes: &[BulkWrite<'_>], id: &str) -> (Option<String>, Option<String>) {
    match writes.iter().find(|write| write.id == id) {
        Some(write) => (
            write
                .start_date
                .map(|date| date.format("%Y-%m-%d").to_string()),
            write
                .target_date
                .map(|date| date.format("%Y-%m-%d").to_string()),
        ),
        None => (None, None),
    }
}

fn upsert_write<'a>(
    writes: &mut Vec<BulkWrite<'a>>,
    id: &'a str,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
) {
    if let Some(existing) = writes.iter_mut().find(|write| write.id == id) {
        if start_date.is_some() {
            existing.start_date = start_date;
        }
        if target_date.is_some() {
            existing.target_date = target_date;
        }
    } else {
        writes.push(BulkWrite {
            id,
            start_date,
            target_date,
        });
    }
}

/// `bulk_update(writes, ["start_date", "target_date"])`: one statement, no
/// signals, `updated_at` untouched. Unset halves keep their stored value
/// (Django rewrites the same value — a no-op either way).
async fn apply_bulk_dates(pool: &sqlx::PgPool, writes: &[BulkWrite<'_>]) -> Result<(), Denial> {
    if writes.is_empty() {
        return Ok(());
    }
    // Placeholder plan: the CASE values in arm order (start arms, then
    // target arms), then the id list. A side with no arms is omitted
    // entirely (an armless `CASE` is a syntax error; Django rewrites the
    // stored value there — a no-op either way).
    let mut ids: Vec<String> = Vec::new();
    let mut values: Vec<(String, chrono::NaiveDate)> = Vec::new();
    let mut target_values: Vec<(String, chrono::NaiveDate)> = Vec::new();
    for write in writes {
        ids.push(write.id.to_owned());
        if let Some(date) = write.start_date {
            values.push((write.id.to_owned(), date));
        }
        if let Some(date) = write.target_date {
            target_values.push((write.id.to_owned(), date));
        }
    }
    let mut sql = String::from("UPDATE issues SET ");
    let mut placeholder: usize = 1;
    let mut first_set = true;
    for (column, arms) in [("start_date", &values), ("target_date", &target_values)] {
        if arms.is_empty() {
            continue;
        }
        if !first_set {
            sql.push_str(", ");
        }
        first_set = false;
        sql.push_str(&format!("{column} = CASE id "));
        for _ in arms {
            sql.push_str(&format!("WHEN ${} THEN ${} ", placeholder, placeholder + 1));
            placeholder += 2;
        }
        sql.push_str(&format!("ELSE {column} END"));
    }
    sql.push_str(" WHERE id IN (");
    let mut first = true;
    for _ in &ids {
        if !first {
            sql.push(',');
        }
        first = false;
        sql.push_str(&format!("${placeholder}"));
        placeholder += 1;
    }
    sql.push(')');
    let mut query = sqlx::query(&sql);
    for (id, date) in values.iter().chain(target_values.iter()) {
        let parsed: uuid::Uuid = id.parse().map_err(|_| Denial::ServerError)?;
        query = query.bind(parsed).bind(*date);
    }
    for id in &ids {
        let parsed: uuid::Uuid = id.parse().map_err(|_| Denial::ServerError)?;
        query = query.bind(parsed);
    }
    query.execute(pool).await.map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Python truthiness for the `if start_date:` / `if target_date:` guards.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(float) = number.as_f64() {
                float != 0.0
            } else {
                true
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `validate_dates` merged value (`:1120-1137`): `new or current`, with
/// string halves parsed `%Y-%m-%d` (a bad format raises `ValueError` → 500;
/// a truthy non-string half raises `TypeError` → 500 on comparison, or
/// fails the later assignment — 500 either way).
fn merge_date(current: Option<&str>, new: &Value) -> Result<Option<chrono::NaiveDate>, Response> {
    if is_truthy(new) {
        parse_date_value(new)
            .map(Some)
            .ok_or_else(|| Denial::ServerError.into_response())
    } else {
        Ok(current.and_then(|text| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()))
    }
}

fn parse_date_value(value: &Value) -> Option<chrono::NaiveDate> {
    match value {
        Value::String(text) => chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok(),
        _ => None,
    }
}

fn exceeds(start: &Option<chrono::NaiveDate>, target: &Option<chrono::NaiveDate>) -> bool {
    match (start, target) {
        (Some(start), Some(target)) => start > target,
        _ => false,
    }
}

/// `str(issue.start_date)`: dates render ISO, `None` renders `"None"`.
fn current_render(current: Option<&str>) -> String {
    current.unwrap_or("None").to_owned()
}

// ---------------------------------------------------------------------------
// Meta (`base.py:1199-1211`)
// ---------------------------------------------------------------------------

/// `GET .../issues/<id>/meta/`: the issue's sequence id plus its project's
/// identifier. `@allow_permission([ADMIN, MEMBER, GUEST], level="PROJECT")`.
/// A non-UUID tail matches no Django route (`<uuid:issue_id>`) and proxies
/// so Django answers its own resolver 404.
async fn meta(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let Ok(issue_id) = issue_raw.parse::<uuid::Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let (pool, gate) =
        resolve_allow_gate(&state, &slug, &project_raw, extension, &[20, 15, 5]).await?;
    let mut binder = Binder::new();
    let sql = queries_engage::meta_sql(&mut binder, &slug, gate.project_id, issue_id);
    let mut rows = fetch_json_rows(&pool, &sql, binder.values()).await?;
    let Some(row) = rows.pop() else {
        return Err(Denial::NotFound);
    };
    let sequence_id = get_i32(&row, "sequence_id").ok_or(Denial::ServerError)?;
    let identifier = get_str(&row, "identifier").ok_or(Denial::ServerError)?;
    Ok(json_response(
        StatusCode::OK,
        format!(
            "{{\"sequence_id\":{sequence_id},\"project_identifier\":{}}}",
            json_string(identifier)
        ),
    ))
}

// ---------------------------------------------------------------------------
// Identifier (`base.py:1214-1489`)
// ---------------------------------------------------------------------------

/// `GET .../work-items/<ident>-<n>/`: the issue by human identifier, lite
/// (`?lite=`) or 36-key full. NO `@allow_permission` decorator — the member
/// and guest checks are manual, in view order: int shape → project → member
/// → issue → guest gate → enqueue → render.
async fn identifier(
    State(state): State<AppState>,
    Path((slug, tail)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Response> {
    // Django's `<str>-<str>` split: greedy first group = LAST dash; no dash
    // or an empty side matches no route → proxy the resolver 404.
    let Some((project_identifier, issue_identifier)) = split_identifier_tail(&tail) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state).map_err(IntoResponse::into_response)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized.into_response())?;
    let timezone = actor_timezone(&pool, &user_id)
        .await
        .map_err(IntoResponse::into_response)?;
    // The int check precedes the project fetch (`:1243-1253`).
    let sequence = match strict_str_to_int(issue_identifier) {
        None => {
            return Err(Denial::BadError("Invalid issue identifier".to_owned()).into_response());
        }
        Some(value) => value,
    };
    let project = fetch_identifier_project(&pool, &slug, project_identifier)
        .await
        .map_err(IntoResponse::into_response)?;
    let Some(project) = project else {
        return Err(Denial::NotFound.into_response());
    };
    let project_id = get_str(&project, "id")
        .and_then(|text| text.parse::<uuid::Uuid>().ok())
        .ok_or(Denial::ServerError.into_response())?;
    if !fetch_identifier_member(&pool, &slug, &project_id, &user_id)
        .await
        .map_err(IntoResponse::into_response)?
    {
        return Err(not_allowed_body());
    }
    // Out-of-`i32` integers are valid but unmatchable (`sequence_id` is an
    // `IntegerField`): 404 at the query position, after the member check.
    let sequence = match sequence {
        IdentifierInt::InRange(value) => Some(value),
        IdentifierInt::OutOfRange => None,
    };
    if is_lite_request(&query) {
        let row = match sequence {
            Some(value) => fetch_identifier_lite(&pool, &slug, &project_id, value)
                .await
                .map_err(IntoResponse::into_response)?,
            None => None,
        };
        let Some(row) = row else {
            return Err(Denial::NotFound.into_response());
        };
        check_identifier_guest(
            &pool,
            &slug,
            &project,
            &project_id,
            &user_id,
            get_str(&row, "created_by_id"),
        )
        .await
        .map_err(IntoResponse::into_response)?;
        let issue_id = get_str(&row, "id")
            .and_then(|text| text.parse::<uuid::Uuid>().ok())
            .ok_or(Denial::ServerError.into_response())?;
        enqueue_message(
            &pool,
            identifier_visited_message(&slug, &issue_id, &user_id, &project_id),
        )
        .await;
        return Ok(render_lite(&row));
    }
    let row = match sequence {
        Some(value) => fetch_identifier_full(&pool, &slug, &project_id, value, &user_id)
            .await
            .map_err(IntoResponse::into_response)?,
        None => None,
    };
    let Some(row) = row else {
        return Err(Denial::NotFound.into_response());
    };
    check_identifier_guest(
        &pool,
        &slug,
        &project,
        &project_id,
        &user_id,
        get_str(&row, "i_created_by_id"),
    )
    .await
    .map_err(IntoResponse::into_response)?;
    let issue_id = get_str(&row, "i_id")
        .and_then(|text| text.parse::<uuid::Uuid>().ok())
        .ok_or(Denial::ServerError.into_response())?;
    enqueue_message(
        &pool,
        identifier_visited_message(&slug, &issue_id, &user_id, &project_id),
    )
    .await;
    render_full(&pool, &row, &issue_id, &timezone)
        .await
        .map_err(IntoResponse::into_response)
}

/// `strict_str_to_int` (`:1215-1218`): ASCII digits with an optional leading
/// `-`; anything else (including `+1`, blanks, or unicode digits) is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentifierInt {
    InRange(i32),
    OutOfRange,
}

fn strict_str_to_int(text: &str) -> Option<IdentifierInt> {
    if text.is_empty() {
        return None;
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    match text.parse::<i64>() {
        Ok(value) => match i32::try_from(value) {
            Ok(narrow) => Some(IdentifierInt::InRange(narrow)),
            Err(_) => Some(IdentifierInt::OutOfRange),
        },
        // Unbounded Python ints that overflow `i64` are still valid — and
        // still unmatchable.
        Err(_) => Some(IdentifierInt::OutOfRange),
    }
}

/// `is_lite_request` (`:1220-1221`): `?lite=` in `1/true/yes`,
/// case-insensitive (`QueryDict.get` reads the LAST value).
fn is_lite_request(query: &QueryMap) -> bool {
    matches!(
        query_last(query, "lite")
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    )
}

async fn fetch_identifier_project(
    pool: &sqlx::PgPool,
    slug: &str,
    project_identifier: &str,
) -> Result<Option<Map<String, Value>>, Denial> {
    let mut binder = Binder::new();
    let sql = queries_engage::identifier_project_sql(&mut binder, slug, project_identifier);
    let mut rows = fetch_json_rows(pool, &sql, binder.values()).await?;
    Ok(rows.pop())
}

async fn fetch_identifier_member(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let mut binder = Binder::new();
    let sql =
        queries_engage::identifier_member_exists_sql(&mut binder, slug, *project_id, *user_id);
    let rows = fetch_json_rows(pool, &sql, binder.values()).await?;
    Ok(!rows.is_empty())
}

async fn fetch_identifier_lite(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    sequence_id: i32,
) -> Result<Option<Map<String, Value>>, Denial> {
    let mut binder = Binder::new();
    let sql = queries_engage::identifier_lite_sql(&mut binder, slug, *project_id, sequence_id);
    let mut rows = fetch_json_rows(pool, &sql, binder.values()).await?;
    Ok(rows.pop())
}

/// The guest gate (`:1460-1474`): a role-5 member on a project WITHOUT
/// `guest_view_all_features` sees only issues they created. Runs AFTER the
/// issue 404.
async fn check_identifier_guest(
    pool: &sqlx::PgPool,
    slug: &str,
    project: &Map<String, Value>,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    created_by: Option<&str>,
) -> Result<(), Response> {
    let mut binder = Binder::new();
    let sql = queries_core::guest_view_exists_sql(&mut binder, slug, *project_id, *user_id);
    let rows = fetch_json_rows(pool, &sql, binder.values())
        .await
        .map_err(IntoResponse::into_response)?;
    let is_guest = !rows.is_empty();
    let view_all = get_bool(project, "guest_view_all_features").unwrap_or(false);
    let owned = created_by == Some(user_id.to_string()).as_deref();
    if is_guest && !view_all && !owned {
        return Err(not_allowed_body());
    }
    Ok(())
}

/// `serialize_lite_issue` (`:1223-1240`): the 15-key hand-built dict in
/// source order. Datetimes render through DRF's plain JSON encoder (`Z`),
/// like every raw-dict path.
fn render_lite(row: &Map<String, Value>) -> Response {
    let created_at = render_utc_moment(row, "created_at");
    let updated_at = render_utc_moment(row, "updated_at");
    let archived_at = render_utc_moment(row, "archived_at");
    let is_synced = issue_is_actively_synced(
        get_str(row, "external_source"),
        get_bool(row, "is_synced"),
        || false,
        || false,
    );
    let null = Value::Null;
    let mut body = String::from("{");
    push_raw(&mut body, true, "id", row.get("id").unwrap_or(&null));
    push_raw(
        &mut body,
        false,
        "sequence_id",
        row.get("sequence_id").unwrap_or(&null),
    );
    push_raw(&mut body, false, "name", row.get("name").unwrap_or(&null));
    push_raw(
        &mut body,
        false,
        "description_html",
        row.get("description_html").unwrap_or(&null),
    );
    push_sort_order(&mut body, row.get("sort_order"));
    push_raw(
        &mut body,
        false,
        "project_id",
        row.get("project_id").unwrap_or(&null),
    );
    push_moment(&mut body, "created_at", created_at.as_deref());
    push_moment(&mut body, "updated_at", updated_at.as_deref());
    push_raw(
        &mut body,
        false,
        "created_by",
        row.get("created_by_id").unwrap_or(&null),
    );
    push_raw(
        &mut body,
        false,
        "updated_by",
        row.get("updated_by_id").unwrap_or(&null),
    );
    push_raw(
        &mut body,
        false,
        "is_draft",
        row.get("is_draft").unwrap_or(&null),
    );
    push_raw(
        &mut body,
        false,
        "is_epic",
        row.get("is_epic").unwrap_or(&null),
    );
    push_raw(
        &mut body,
        false,
        "is_intake",
        row.get("is_intake").unwrap_or(&null),
    );
    body.push_str(&format!(",\"is_synced\":{is_synced}"));
    push_moment(&mut body, "archived_at", archived_at.as_deref());
    body.push('}');
    json_response(StatusCode::OK, body)
}

fn push_raw(body: &mut String, first: bool, key: &str, value: &Value) {
    if !first {
        body.push(',');
    }
    let rendered = value.to_string();
    body.push_str(&format!("{key}:{rendered}", key = json_string(key)));
}

fn push_moment(body: &mut String, key: &str, rendered: Option<&str>) {
    body.push(',');
    match rendered {
        Some(text) => body.push_str(&format!("{}:{}", json_string(key), json_string(text))),
        None => body.push_str(&format!("{}:null", json_string(key))),
    }
}

fn push_sort_order(body: &mut String, value: Option<&Value>) {
    body.push_str(",\"sort_order\":");
    match value.and_then(Value::as_f64) {
        Some(float) => body.push_str(&crate::paginator::py_float_str(float)),
        None => body.push_str("null"),
    }
}

// ---------------------------------------------------------------------------
// Identifier full detail (`:1312-1489`)
// ---------------------------------------------------------------------------

/// Rename one fragment column list for the full-row composite: strip the
/// table qualifier (and quoting) and add the group prefix, so the 175
/// colliding `created_at`/`id`/… outputs become unique JSON keys.
fn prefixed_columns(columns: &str, table: &str, prefix: &str) -> Vec<String> {
    let qualifier = format!("{table}.");
    columns
        .split(',')
        .map(|column| {
            column
                .trim()
                .strip_prefix(qualifier.as_str())
                .unwrap_or(column.trim())
                .replace('"', "")
        })
        .map(|name| format!("{prefix}{name}"))
        .collect()
}

/// The full-statement column-alias list, derived from the same fragment
/// consts `identifier_full_sql` embeds — in the same order (issue 34, the 9
/// annotations, project 46, workspace 14, parent 34, state 18, ticker 20).
/// Postgres validates the COUNT at runtime (a wrong count errors); the unit
/// tests pin the ORDER against the consts, so transcription drift fails the
/// build instead of silently mislabeling columns.
fn full_row_aliases() -> Vec<String> {
    let mut aliases = prefixed_columns(queries_core::ISSUE_COLUMNS, "issue", "i_");
    aliases.extend(
        [
            "cycle_id",
            "link_count",
            "attachment_count",
            "sub_issues_count",
            "label_ids",
            "assignee_ids",
            "module_ids",
            "is_subscribed",
            "is_intake",
        ]
        .iter()
        .map(|name| (*name).to_owned()),
    );
    aliases.extend(prefixed_columns(
        queries_core::PROJECT_COLUMNS,
        "projects",
        "p_",
    ));
    aliases.extend(prefixed_columns(
        queries_engage::WORKSPACE_COLUMNS,
        "workspaces",
        "w_",
    ));
    aliases.extend(prefixed_columns(
        queries_engage::PARENT_ISSUE_COLUMNS,
        "parent",
        "par_",
    ));
    aliases.extend(prefixed_columns(queries_core::STATE_COLUMNS, "state", "s_"));
    aliases.extend(prefixed_columns(
        queries_engage::TICKER_COLUMNS,
        "issue_agent_ticker",
        "t_",
    ));
    aliases
}

async fn fetch_identifier_full(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    sequence_id: i32,
    user_id: &uuid::Uuid,
) -> Result<Option<Map<String, Value>>, Denial> {
    let mut binder = Binder::new();
    let full =
        queries_engage::identifier_full_sql(&mut binder, slug, *project_id, sequence_id, *user_id);
    let inner = format!(
        "SELECT * FROM ({full}) AS __f({})",
        full_row_aliases().join(",")
    );
    let mut rows = fetch_json_rows(pool, &inner, binder.values()).await?;
    Ok(rows.pop())
}

fn req_str<'a>(row: &'a Map<String, Value>, key: &str) -> Result<&'a str, Denial> {
    get_str(row, key).ok_or(Denial::ServerError)
}

fn str_refs(ids: &[String]) -> Vec<&str> {
    ids.iter().map(String::as_str).collect()
}

/// One sync-table `.exists()` probe (`issue.py:76`).
async fn sync_probe(pool: &sqlx::PgPool, sql: &str, issue_id: &uuid::Uuid) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(sql)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// The full path carries no `is_synced` annotation, so a set
/// `external_source` runs the probes — git first, github only on a miss
/// (the predicate's short-circuit, hence the probe counts).
async fn full_is_synced(
    pool: &sqlx::PgPool,
    row: &Map<String, Value>,
    issue_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let source = get_str(row, "i_external_source");
    let has_source = source.is_some_and(|text| !text.is_empty());
    let git_hit = if has_source {
        sync_probe(pool, GIT_ISSUE_SYNC_PROBE_SQL, issue_id).await?
    } else {
        false
    };
    let github_hit = if has_source && !git_hit {
        sync_probe(pool, GITHUB_ISSUE_SYNC_PROBE_SQL, issue_id).await?
    } else {
        false
    };
    Ok(issue_is_actively_synced(
        source,
        None,
        || git_hit,
        || github_hit,
    ))
}

/// An agent-run row plus its pre-rendered datetimes (plain `isoformat` —
/// NOT DRF's format — via the services renderer).
struct AgentRunOwned {
    row: Map<String, Value>,
    created_at: String,
    assigned_at: Option<String>,
    started_at: Option<String>,
    ended_at: Option<String>,
    live_last_event_at: Option<String>,
    live_updated_at: Option<String>,
}

/// The merged run statements select `runner.name` and two `llm_model`
/// columns, which would collide under `row_to_json`; alias the duplicates
/// (pinned by unit test).
fn agent_run_sql_aliased(sql: &str) -> String {
    sql.replace("runner.name", "runner.name AS runner_name")
        .replace(
            "runner_live_state.llm_model",
            "runner_live_state.llm_model AS live_llm_model",
        )
}

fn render_iso_moment(row: &Map<String, Value>, key: &str) -> Option<String> {
    get_moment(row, key).map(pidash_services::app_issues::serialize_iso_datetime)
}

async fn fetch_agent_run(
    pool: &sqlx::PgPool,
    sql: &str,
    issue_id: &uuid::Uuid,
) -> Result<Option<AgentRunOwned>, Denial> {
    let mut binder = Binder::new();
    binder.bind_uuid(*issue_id);
    let mut rows = fetch_json_rows(pool, &agent_run_sql_aliased(sql), binder.values()).await?;
    let Some(row) = rows.pop() else {
        return Ok(None);
    };
    let created_at = render_iso_moment(&row, "created_at").ok_or(Denial::ServerError)?;
    let live_updated_at = render_iso_moment(&row, "updated_at");
    Ok(Some(AgentRunOwned {
        assigned_at: render_iso_moment(&row, "assigned_at"),
        started_at: render_iso_moment(&row, "started_at"),
        ended_at: render_iso_moment(&row, "ended_at"),
        live_last_event_at: render_iso_moment(&row, "last_event_at"),
        live_updated_at,
        created_at,
        row,
    }))
}

fn agent_run_detail_row(run: &AgentRunOwned) -> AgentRunDetailRow<'_> {
    let row = &run.row;
    let live_state = run.live_updated_at.as_deref().map(|updated_at| {
        let usage = row.get("usage").unwrap_or(&Value::Null);
        AgentLiveStateRow {
            observed_run_id: get_str(row, "observed_run_id"),
            last_event_at: run.live_last_event_at.as_deref(),
            last_event_kind: get_str(row, "last_event_kind"),
            last_event_summary: get_str(row, "last_event_summary"),
            agent_pid: get_i32(row, "agent_pid"),
            agent_subprocess_alive: get_bool(row, "agent_subprocess_alive"),
            approvals_pending: get_i32(row, "approvals_pending"),
            usage,
            llm_model: get_str(row, "live_llm_model"),
            turn_count: get_i32(row, "turn_count"),
            updated_at,
        }
    });
    AgentRunDetailRow {
        id: get_str(row, "id").unwrap_or_default(),
        status: get_str(row, "status").unwrap_or_default(),
        executor_kind: get_str(row, "executor_kind").unwrap_or_default(),
        queue_position: get_i32(row, "queue_position").and_then(|value| i16::try_from(value).ok()),
        runner_id: get_str(row, "runner_id"),
        runner_name: get_str(row, "runner_name"),
        created_at: &run.created_at,
        assigned_at: run.assigned_at.as_deref(),
        started_at: run.started_at.as_deref(),
        ended_at: run.ended_at.as_deref(),
        done_payload: row.get("done_payload").filter(|value| !value.is_null()),
        error: get_str(row, "error").unwrap_or_default(),
        error_code: get_str(row, "error_code").unwrap_or_default(),
        llm_model: get_str(row, "llm_model").unwrap_or_default(),
        input_tokens: get_i64(row, "input_tokens"),
        output_tokens: get_i64(row, "output_tokens"),
        total_tokens: get_i64(row, "total_tokens"),
        live_state,
    }
}

/// One direction of the blocker fetch: the merged statement with its two
/// `:issue_id` placeholders bound to one positional.
async fn fetch_blocker_rows(
    pool: &sqlx::PgPool,
    sql: &str,
    issue_id: &uuid::Uuid,
) -> Result<Vec<BlockerRow>, Denial> {
    let mut binder = Binder::new();
    let holder = binder.bind_uuid(*issue_id);
    let sql = sql.replace(":issue_id", &holder);
    let rows = fetch_json_rows(pool, &sql, binder.values()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(BlockerRow {
            issue_id: get_str(row, "id")
                .and_then(|text| text.parse::<uuid::Uuid>().ok())
                .ok_or(Denial::ServerError)?,
            sequence_id: get_i32(row, "sequence_id").ok_or(Denial::ServerError)?,
            project_identifier: get_str(row, "identifier")
                .ok_or(Denial::ServerError)?
                .to_owned(),
            state_name: get_str(row, "name").map(str::to_owned),
            state_group: get_str(row, "group").map(str::to_owned),
        });
    }
    Ok(out)
}

/// The per-instance `_blocker_summary_cache` (`:1250-1258`): one
/// `relations_summary` call whose halves feed both blocker fields.
async fn fetch_relations_summary(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
) -> Result<RelationsSummary, Denial> {
    let blocked_by = fetch_blocker_rows(pool, &blockers_sql(), issue_id).await?;
    let blocking = fetch_blocker_rows(pool, &dependents_sql(), issue_id).await?;
    let mut binder = Binder::new();
    let holder = binder.bind_uuid(*issue_id);
    let sql = has_open_blockers_sql().replace(":issue_id", &holder);
    let open = fetch_json_rows(pool, &sql, binder.values()).await?;
    Ok(relations_summary(&blocked_by, &blocking, !open.is_empty()))
}

fn decode_ticker(row: &Map<String, Value>) -> Result<Option<IssueAgentTicker>, Denial> {
    if get_str(row, "t_id").is_none() {
        return Ok(None);
    }
    let id = |key: &str| -> Result<uuid::Uuid, Denial> {
        get_str(row, key)
            .and_then(|text| text.parse::<uuid::Uuid>().ok())
            .ok_or(Denial::ServerError)
    };
    let maybe_id = |key: &str| -> Result<Option<uuid::Uuid>, Denial> {
        match get_str(row, key) {
            None => Ok(None),
            Some(text) => text
                .parse::<uuid::Uuid>()
                .map(Some)
                .map_err(|_| Denial::ServerError),
        }
    };
    let moment = |key: &str| -> Result<DateTime<Utc>, Denial> {
        get_moment(row, key).ok_or(Denial::ServerError)
    };
    let int = |key: &str| -> Result<i32, Denial> { get_i32(row, key).ok_or(Denial::ServerError) };
    Ok(Some(IssueAgentTicker {
        id: id("t_id")?,
        created_at: moment("t_created_at")?,
        updated_at: moment("t_updated_at")?,
        created_by_id: maybe_id("t_created_by_id")?,
        updated_by_id: maybe_id("t_updated_by_id")?,
        deleted_at: get_moment(row, "t_deleted_at"),
        issue_id: id("t_issue_id")?,
        used: int("t_used")?,
        granted: int("t_granted")?,
        waited: int("t_waited")?,
        user_disabled: get_bool(row, "t_user_disabled").ok_or(Denial::ServerError)?,
        next_run_at: get_moment(row, "t_next_run_at"),
        last_tick_at: get_moment(row, "t_last_tick_at"),
        enabled: get_bool(row, "t_enabled").ok_or(Denial::ServerError)?,
        disarm_reason: get_str(row, "t_disarm_reason")
            .unwrap_or_default()
            .to_owned(),
        pending_entry: get_bool(row, "t_pending_entry").ok_or(Denial::ServerError)?,
        pending_entry_free: get_bool(row, "t_pending_entry_free").ok_or(Denial::ServerError)?,
        pending_entry_actor_id: maybe_id("t_pending_entry_actor_id")?,
        pending_entry_trigger: get_str(row, "t_pending_entry_trigger")
            .unwrap_or_default()
            .to_owned(),
        resume_parent_run_id: maybe_id("t_resume_parent_run_id")?,
    }))
}

/// `IssueDetailSerializer(issue)` (`:1488`): the 36-key full body —
/// `is_intake` INCLUDED on this path (unlike retrieve).
async fn render_full(
    pool: &sqlx::PgPool,
    row: &Map<String, Value>,
    issue_id: &uuid::Uuid,
    timezone: &Tz,
) -> Result<Response, Denial> {
    let created_at =
        render_actor_moment(row, "i_created_at", timezone).ok_or(Denial::ServerError)?;
    let updated_at =
        render_actor_moment(row, "i_updated_at", timezone).ok_or(Denial::ServerError)?;
    let completed_at = render_actor_moment(row, "i_completed_at", timezone);
    let module_ids = get_id_list(row, "module_ids");
    let label_ids = get_id_list(row, "label_ids");
    let assignee_ids = get_id_list(row, "assignee_ids");
    let is_synced = full_is_synced(pool, row, issue_id).await?;
    let latest = fetch_agent_run(pool, LATEST_AGENT_RUN_SQL, issue_id).await?;
    let active = fetch_agent_run(pool, ACTIVE_AGENT_RUN_SQL, issue_id).await?;
    let mut count_binder = Binder::new();
    count_binder.bind_uuid(*issue_id);
    let run_count = fetch_count(pool, AGENT_RUN_COUNT_SQL, count_binder.values()).await?;
    let blockers = fetch_relations_summary(pool, issue_id).await?;
    let ticker_row = decode_ticker(row)?;
    let policy = ProjectClockPolicy {
        agent_ticking_enabled: get_bool(row, "p_agent_ticking_enabled"),
        agent_default_max_ticks: get_i32(row, "p_agent_default_max_ticks"),
        agent_default_interval_seconds: get_i64(row, "p_agent_default_interval_seconds"),
        agent_review_default_interval_seconds: get_i64(
            row,
            "p_agent_review_default_interval_seconds",
        ),
        agent_test_default_interval_seconds: get_i64(row, "p_agent_test_default_interval_seconds"),
    };
    let state_owned;
    let state = match get_str(row, "i_state_id") {
        None => None,
        Some(_) => {
            state_owned = (
                get_str(row, "s_group").unwrap_or_default().to_owned(),
                get_str(row, "s_name").unwrap_or_default().to_owned(),
            );
            Some(StateRef {
                group: state_owned.0.as_str(),
                name: state_owned.1.as_str(),
            })
        }
    };
    let ticker = ticker_row.as_ref().map(|ticker| AgentTickerInput {
        ticker,
        policy: &policy,
        state,
    });
    let latest_row = latest.as_ref().map(agent_run_detail_row);
    let active_row = active.as_ref().map(agent_run_detail_row);
    let base = IssueDetailBaseRow {
        id: req_str(row, "i_id")?,
        name: req_str(row, "i_name")?,
        state_id: get_str(row, "i_state_id"),
        sort_order: get_f64(row, "i_sort_order").ok_or(Denial::ServerError)?,
        completed_at: completed_at.as_deref(),
        estimate_point: get_str(row, "i_estimate_point_id"),
        priority: req_str(row, "i_priority")?,
        complexity_score: get_i32(row, "i_complexity_score").ok_or(Denial::ServerError)?,
        start_date: get_str(row, "i_start_date"),
        target_date: get_str(row, "i_target_date"),
        sequence_id: get_i32(row, "i_sequence_id").ok_or(Denial::ServerError)?,
        project_id: req_str(row, "i_project_id")?,
        parent_id: get_str(row, "i_parent_id"),
        cycle_id: Some(get_str(row, "cycle_id")),
        assigned_pod_id: get_str(row, "i_assigned_pod_id"),
        agent_executor: get_str(row, "i_agent_executor"),
        module_ids: Some(str_refs(&module_ids)),
        label_ids: Some(str_refs(&label_ids)),
        assignee_ids: Some(str_refs(&assignee_ids)),
        sub_issues_count: Some(get_i64(row, "sub_issues_count")),
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: get_str(row, "i_created_by_id"),
        updated_by: get_str(row, "i_updated_by_id"),
        attachment_count: Some(get_i64(row, "attachment_count")),
        link_count: Some(get_i64(row, "link_count")),
        is_draft: get_bool(row, "i_is_draft").ok_or(Denial::ServerError)?,
        archived_at: get_str(row, "i_archived_at"),
        is_synced,
    };
    let detail = IssueDetailRow {
        base,
        description_html: req_str(row, "i_description_html")?,
        is_subscribed: get_bool(row, "is_subscribed").ok_or(Denial::ServerError)?,
        is_intake: Some(get_bool(row, "is_intake").ok_or(Denial::ServerError)?),
        ticker,
        latest_run: latest_row,
        active_run: active_row,
        run_count,
        blockers: &blockers,
    };
    let view = issue_detail_to_representation(&detail);
    let body = serde_json::to_string(&view).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

/// Shared sync-probe evaluation: `external_source` short-circuits false;
/// otherwise git first, github only on a miss.
async fn probe_synced(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
    source: Option<&str>,
) -> Result<bool, Denial> {
    let has_source = source.is_some_and(|text| !text.is_empty());
    let git_hit = if has_source {
        sync_probe(pool, GIT_ISSUE_SYNC_PROBE_SQL, issue_id).await?
    } else {
        false
    };
    let github_hit = if has_source && !git_hit {
        sync_probe(pool, GITHUB_ISSUE_SYNC_PROBE_SQL, issue_id).await?
    } else {
        false
    };
    Ok(issue_is_actively_synced(
        source,
        None,
        || git_hit,
        || github_hit,
    ))
}

// ---------------------------------------------------------------------------
// Sub-issues (`sub_issue.py:33-248`)
// ---------------------------------------------------------------------------

/// `GET .../issues/<id>/sub-issues/`: the issue's children over the
/// queries-A sub-issue querysets, with `order_by`, `group_by` and the
/// `state_distribution`. `ProjectEntityPermission` read: any active project
/// membership. The parent itself is never checked — an unknown parent id
/// simply lists nothing.
async fn sub_issues_get(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Response> {
    let Ok(parent_id) = issue_raw.parse::<uuid::Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let (pool, gate) = resolve_entity_gate(&state, &slug, &project_raw, extension, false).await?;
    let order_param = query_last(&query, "order_by").unwrap_or_else(|| "-created_at".to_owned());
    let order = sub_issues_order(&order_param).map_err(IntoResponse::into_response)?;
    let mut binder = Binder::new();
    let selects = queries_core::sub_issues_selects();
    let from_where = queries_core::sub_issues_from_where(&mut binder, &slug, parent_id);
    let inner = format!("SELECT {selects} {from_where} {order}");
    let rows = fetch_json_rows(&pool, &inner, binder.values())
        .await
        .map_err(IntoResponse::into_response)?;
    // The distribution reads the RAW rows (`:171-174`), before the timezone
    // conversion; a stateless child keys as JSON `null` → `"null"`.
    let mut distribution: Buckets = Vec::new();
    for row in &rows {
        let key = match row.get("state_group") {
            Some(Value::String(group)) => group.clone(),
            Some(Value::Null) | None => "null".to_owned(),
            Some(other) => other.to_string(),
        };
        let id = get_str(row, "id").unwrap_or_default().to_owned();
        bucket_push(&mut distribution, key, id);
    }
    let fields: Vec<String> = queries_core::SUB_ISSUES_KEYS
        .iter()
        .map(|key| (*key).to_owned())
        .collect();
    let shaped: Vec<String> = rows
        .iter()
        .map(|row| shape_row(row, &fields, &gate.timezone, true))
        .collect();
    match query_last(&query, "group_by") {
        None => Ok(render_sub_list(&shaped, &distribution)),
        Some(param) if param.is_empty() => Ok(render_sub_list(&shaped, &distribution)),
        Some(param) if param == "assignees__ids" => {
            Ok(render_sub_grouped_assignees(&rows, &shaped, &distribution))
        }
        Some(param) => {
            if !queries_core::SUB_ISSUES_KEYS.contains(&param.as_str()) {
                return Err(
                    Denial::BadError("The required key does not exist.".to_owned()).into_response(),
                );
            }
            Ok(render_sub_grouped(
                &rows,
                &shaped,
                &param,
                &gate.timezone,
                &distribution,
            ))
        }
    }
}

/// `ORDER BY` for the sub-issue list: `order_issue_queryset` through the
/// merged `order_sql`, resolved like pilot-2's flat path. An empty param
/// skips the port explicitly and falls back to the model ordering
/// (`-created_at` — the same statement); an unknown field is Django's
/// `FieldError` → generic 500.
fn sub_issues_order(order_by_param: &str) -> Result<String, Denial> {
    if order_by_param.is_empty() {
        return Ok(queries_core::SUB_ISSUES_DEFAULT_ORDER.to_owned());
    }
    let spec = order_sql(order_by_param, "state.\"group\"", |name| {
        format!("min_{}", name.replace("__", "_"))
    });
    let out = spec.out_param.as_str();
    if out == "priority_order"
        || out == "-priority_order"
        || out == "state_order"
        || out == "-state_order"
    {
        // The CASE fragments are already issue-qualified; only the bare
        // `created_at` tiebreak needs the table (Django orders the issue
        // queryset — an unqualified `created_at` would be ambiguous over
        // the joined tables).
        let sql = spec
            .order_by_sql
            .replace("created_at DESC", "issue.created_at DESC");
        return Ok(format!("ORDER BY {sql}"));
    }
    if out == "min_values" || out == "-min_values" {
        let (expr, descending) = order_key(out, order_by_param)?;
        return Ok(format!(
            "ORDER BY {expr} {}, issue.created_at DESC",
            if descending { "DESC" } else { "ASC" }
        ));
    }
    let (expr, descending) = order_key(out, order_by_param)?;
    let mut sql = format!(
        "ORDER BY {expr} {}",
        if descending { "DESC" } else { "ASC" }
    );
    if !order_by_param.contains("created_at") {
        sql.push_str(", issue.created_at DESC");
    }
    Ok(sql)
}

/// Insertion-ordered string→list buckets (what `defaultdict(list)` renders).
type Buckets = Vec<(String, Vec<String>)>;

fn bucket_push(buckets: &mut Buckets, key: String, value: String) {
    match buckets.iter_mut().find(|(bucket, _)| *bucket == key) {
        Some((_, items)) => items.push(value),
        None => buckets.push((key, vec![value])),
    }
}

fn render_string_list_map(map: &Buckets) -> String {
    let pairs: Vec<String> = map
        .iter()
        .map(|(key, ids)| {
            let items: Vec<String> = ids.iter().map(|id| json_string(id)).collect();
            format!("{}:[{}]", json_string(key), items.join(","))
        })
        .collect();
    format!("{{{}}}", pairs.join(","))
}

fn render_sub_list(shaped: &[String], distribution: &Buckets) -> Response {
    json_response(
        StatusCode::OK,
        format!(
            "{{\"sub_issues\":[{}],\"state_distribution\":{}}}",
            shaped.join(","),
            render_string_list_map(distribution)
        ),
    )
}

/// `group_by=assignees__ids` (`:183-189`): one bucket per assignee id,
/// `None` for the unassigned. A null list falls through both branches, so
/// the row is dropped (unreachable — the column is `COALESCE`d).
fn render_sub_grouped_assignees(
    rows: &[Map<String, Value>],
    shaped: &[String],
    distribution: &Buckets,
) -> Response {
    let mut groups: Buckets = Vec::new();
    for (row, rendered) in rows.iter().zip(shaped.iter()) {
        match row.get("assignee_ids").and_then(Value::as_array) {
            Some(items) if !items.is_empty() => {
                for item in items {
                    let bucket = item.as_str().unwrap_or("None").to_owned();
                    bucket_push(&mut groups, bucket, rendered.clone());
                }
            }
            Some(_) => {
                bucket_push(&mut groups, "None".to_owned(), rendered.clone());
            }
            None => {}
        }
    }
    render_sub_buckets(&groups, distribution)
}

fn render_sub_buckets(groups: &Buckets, distribution: &Buckets) -> Response {
    let buckets: Vec<String> = groups
        .iter()
        .map(|(bucket, items)| format!("{}:[{}]", json_string(bucket), items.join(",")))
        .collect();
    json_response(
        StatusCode::OK,
        format!(
            "{{\"sub_issues\":{{{}}},\"state_distribution\":{}}}",
            buckets.join(","),
            render_string_list_map(distribution)
        ),
    )
}

/// Generic `group_by=<values key>` (`:191-192`): `str()` of the CONVERTED
/// row value (datetimes render in the actor zone, space-separated).
fn render_sub_grouped(
    rows: &[Map<String, Value>],
    shaped: &[String],
    key: &str,
    timezone: &Tz,
    distribution: &Buckets,
) -> Response {
    let mut groups: Buckets = Vec::new();
    for (row, rendered) in rows.iter().zip(shaped.iter()) {
        let bucket = match key {
            "created_at" | "updated_at" => {
                py_moment_str(row.get(key).unwrap_or(&Value::Null), timezone)
            }
            _ => py_str(row.get(key).unwrap_or(&Value::Null)),
        };
        bucket_push(&mut groups, bucket, rendered.clone());
    }
    render_sub_buckets(&groups, distribution)
}

/// Python `str()` for JSON scalars (the `group_by` / assignee-bucket keys).
/// Containers fall back to compact JSON — grouping by an id list has no
/// sane Django rendering to match (`str()` of UUID objects).
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(flag) => {
            if *flag {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(float) = number.as_f64() {
                crate::paginator::py_float_str(float)
            } else {
                number.to_string()
            }
        }
        Value::String(text) => text.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

/// Python `str()` of a converted datetime: space-separated, six microsecond
/// digits when nonzero, `+HH:MM` suffix.
fn py_moment_str(value: &Value, timezone: &Tz) -> String {
    let Some(text) = value.as_str() else {
        return "None".to_owned();
    };
    let Ok(aware) = chrono::DateTime::parse_from_rfc3339(text) else {
        return "None".to_owned();
    };
    let local = aware.with_timezone(timezone);
    let base = local.format("%Y-%m-%d %H:%M:%S").to_string();
    let suffix = local.format("%:z").to_string();
    let micros = local.timestamp_subsec_micros();
    if micros == 0 {
        format!("{base}{suffix}")
    } else {
        format!("{base}.{micros:06}{suffix}")
    }
}

/// Django's `<str:project_identifier>-<str:issue_identifier>` split: greedy
/// first group = LAST dash; no dash or an empty side matches no route.
fn split_identifier_tail(tail: &str) -> Option<(&str, &str)> {
    match tail.rsplit_once('-') {
        Some((project, ident)) if !project.is_empty() && !ident.is_empty() => {
            Some((project, ident))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Sub-issue link (`sub_issue.py:203-248`)
// ---------------------------------------------------------------------------

/// `POST .../issues/<id>/sub-issues/`: link children via `bulk_update`
/// (no signals — FX-ISS-21), one `issue_activity` enqueue per REQUESTED id,
/// then the `IssueSerializer` rows + distribution over the found rows.
/// `ProjectEntityPermission` write: ADMIN/MEMBER only.
async fn sub_issues_post(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Response> {
    let Ok(parent_id) = issue_raw.parse::<uuid::Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let (pool, gate) = resolve_entity_gate(&state, &slug, &project_raw, extension, true).await?;
    let body = read_body(&state, req).await?;
    // The parent lookup runs FIRST (`:205`): scoped-manager `get(pk)` with
    // NO workspace/project filter — missing, triage, archived or draft →
    // 404.
    let parent = fetch_scoped_issue(&pool, &parent_id)
        .await
        .map_err(IntoResponse::into_response)?;
    if parent.is_none() {
        return Err(Denial::NotFound.into_response());
    }
    let object = body
        .as_object()
        .ok_or(Denial::ServerError.into_response())?;
    let sub_ids = parse_sub_issue_ids(object)?;
    if sub_ids.is_empty() {
        return Err(Denial::BadError("Sub Issue IDs are required".to_owned()).into_response());
    }
    let mut parsed: Vec<uuid::Uuid> = Vec::with_capacity(sub_ids.len());
    for id in &sub_ids {
        // Converted ids (`int`/`bool`/`None`) never match a row — Django's
        // `id__in` converts them and finds nothing — but they still get
        // their enqueue below. A non-UUID STRING is `ValidationError` → 400.
        if let SubIssueId::Fetch(text) = id {
            match text.parse::<uuid::Uuid>() {
                Ok(value) => parsed.push(value),
                Err(_) => return Err(invalid_detail_body()),
            }
        }
    }
    let found = fetch_scoped_issue_ids(&pool, &parsed)
        .await
        .map_err(IntoResponse::into_response)?;
    if !found.is_empty() {
        link_sub_issues(&pool, &parent_id, &found)
            .await
            .map_err(IntoResponse::into_response)?;
    }
    // Enqueues cover every REQUESTED id — even ids with no row — with a
    // fresh epoch each (`timezone.now()` inside the comprehension).
    if let Some(origin) = request_origin(&state) {
        for id in &sub_ids {
            enqueue_message(
                &pool,
                sub_issue_activity_message(
                    &parent_id,
                    id.raw(),
                    &gate.user_id,
                    &gate.project_id,
                    Utc::now().timestamp(),
                    &origin,
                ),
            )
            .await;
        }
    }
    let rows = fetch_post_rows(&pool, &found)
        .await
        .map_err(IntoResponse::into_response)?;
    let mut distribution: Buckets = Vec::new();
    for row in &rows {
        let key = match row.get("state_group") {
            Some(Value::String(group)) => group.clone(),
            _ => "null".to_owned(),
        };
        let id = get_str(row, "id").unwrap_or_default().to_owned();
        bucket_push(&mut distribution, key, id);
    }
    let mut rendered: Vec<String> = Vec::with_capacity(rows.len());
    for row in &rows {
        rendered.push(
            render_post_row(&pool, row, &gate.timezone)
                .await
                .map_err(IntoResponse::into_response)?,
        );
    }
    Ok(json_response(
        StatusCode::OK,
        format!(
            "{{\"sub_issues\":[{}],\"state_distribution\":{}}}",
            rendered.join(","),
            render_string_list_map(&distribution)
        ),
    ))
}

fn invalid_detail_body() -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        r#"{"error":"Please provide valid detail"}"#.to_owned(),
    )
}

/// One requested sub-issue id (`sub_issue.py:206-214`): `Fetch` ids join
/// the `id__in` lookup (a non-UUID string is `ValidationError` → 400);
/// `Converted` ids (`int`/`bool`/`None`, in Python-`str()` form) convert
/// in the lookup, match nothing, but still get their enqueue.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SubIssueId {
    Fetch(String),
    Converted(String),
}

impl SubIssueId {
    fn raw(&self) -> &str {
        match self {
            SubIssueId::Fetch(text) | SubIssueId::Converted(text) => text,
        }
    }
}

/// `request.data.get("sub_issue_ids", [])` plus the `len()` gate (`:206-212`):
/// missing → `[]` → the required-400; explicit null/number/bool breaks
/// `len()` → 500; a string iterates its chars downstream (any non-empty
/// string fails UUID validation → the invalid-detail 400); a dict iterates
/// its keys. Array items convert like the bulk-dates ids: `int`/`bool`/
/// `None` match nothing but still enqueue; floats/lists/dicts → 400.
fn parse_sub_issue_ids(object: &Map<String, Value>) -> Result<Vec<SubIssueId>, Response> {
    match object.get("sub_issue_ids") {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(text) => out.push(SubIssueId::Fetch(text.clone())),
                    Value::Number(number) if number.is_i64() || number.is_u64() => {
                        out.push(SubIssueId::Converted(number.to_string()))
                    }
                    Value::Bool(flag) => out.push(SubIssueId::Converted(
                        if *flag { "True" } else { "False" }.to_owned(),
                    )),
                    Value::Null => out.push(SubIssueId::Converted("None".to_owned())),
                    _ => return Err(invalid_detail_body()),
                }
            }
            Ok(out)
        }
        Some(Value::String(text)) => {
            if text.is_empty() {
                Ok(Vec::new())
            } else {
                Err(invalid_detail_body())
            }
        }
        Some(Value::Object(map)) => Ok(map
            .keys()
            .map(|key| SubIssueId::Fetch(key.clone()))
            .collect()),
        Some(_) => Err(Denial::ServerError.into_response()),
    }
}

/// The `issue_objects` manager scope for pk-oriented reads (same predicates
/// as `sub_issues_from_where`, which binds parent+slug instead): live,
/// non-triage, non-archived, non-draft.
const SCOPED_ISSUE_FROM: &str = "FROM issues AS issue \
    LEFT JOIN states AS state ON state.id = issue.state_id \
    JOIN projects AS project ON project.id = issue.project_id";
const SCOPED_ISSUE_SCOPE: &str = "issue.deleted_at IS NULL \
    AND NOT (state.\"group\" = 'triage' AND state.\"group\" IS NOT NULL) \
    AND issue.archived_at IS NULL AND project.archived_at IS NULL AND issue.is_draft = FALSE";

async fn fetch_scoped_issue(
    pool: &sqlx::PgPool,
    id: &uuid::Uuid,
) -> Result<Option<Map<String, Value>>, Denial> {
    let mut binder = Binder::new();
    let holder = binder.bind_uuid(*id);
    let inner = format!("SELECT issue.id {SCOPED_ISSUE_FROM} WHERE {SCOPED_ISSUE_SCOPE} AND issue.id = {holder} LIMIT 1");
    let mut rows = fetch_json_rows(pool, &inner, binder.values()).await?;
    Ok(rows.pop())
}

async fn fetch_scoped_issue_ids(
    pool: &sqlx::PgPool,
    ids: &[uuid::Uuid],
) -> Result<Vec<uuid::Uuid>, Denial> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut binder = Binder::new();
    let mut holders = Vec::with_capacity(ids.len());
    for id in ids {
        holders.push(binder.bind_uuid(*id));
    }
    let inner = format!(
        "SELECT issue.id {SCOPED_ISSUE_FROM} WHERE {SCOPED_ISSUE_SCOPE} AND issue.id IN ({})",
        holders.join(",")
    );
    let rows = fetch_json_rows(pool, &inner, binder.values()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id = get_str(row, "id")
            .and_then(|text| text.parse::<uuid::Uuid>().ok())
            .ok_or(Denial::ServerError)?;
        out.push(id);
    }
    Ok(out)
}

/// `bulk_update(sub_issues, ["parent"])`: one statement, no signals,
/// `updated_at` untouched.
async fn link_sub_issues(
    pool: &sqlx::PgPool,
    parent_id: &uuid::Uuid,
    found: &[uuid::Uuid],
) -> Result<(), Denial> {
    let mut binder = Binder::new();
    let parent = binder.bind_uuid(*parent_id);
    let mut holders = Vec::with_capacity(found.len());
    for id in found {
        holders.push(binder.bind_uuid(*id));
    }
    let sql = format!(
        "UPDATE issues SET parent_id = {parent} WHERE id IN ({})",
        holders.join(",")
    );
    let mut query = sqlx::query(&sql);
    query = query.bind(parent_id);
    for id in found {
        query = query.bind(id);
    }
    query.execute(pool).await.map_err(|_| Denial::ServerError)?;
    Ok(())
}

const POST_ROW_COLUMNS: &str = "issue.id, issue.name, issue.state_id, issue.sort_order, \
    issue.completed_at, issue.estimate_point_id AS estimate_point, issue.priority, \
    issue.complexity_score, issue.start_date, issue.target_date, issue.sequence_id, \
    issue.project_id, issue.parent_id, issue.assigned_pod_id, issue.agent_executor, \
    issue.created_at, issue.updated_at, issue.created_by_id AS created_by, \
    issue.updated_by_id AS updated_by, issue.is_draft, issue.archived_at, \
    issue.external_source, state.\"group\" AS state_group";

async fn fetch_post_rows(
    pool: &sqlx::PgPool,
    found: &[uuid::Uuid],
) -> Result<Vec<Map<String, Value>>, Denial> {
    if found.is_empty() {
        return Ok(Vec::new());
    }
    let mut binder = Binder::new();
    let mut holders = Vec::with_capacity(found.len());
    for id in found {
        holders.push(binder.bind_uuid(*id));
    }
    // Model ordering (`-created_at`): the queryset carries no explicit
    // order, so input order is NOT preserved.
    let inner = format!(
        "SELECT {POST_ROW_COLUMNS} {SCOPED_ISSUE_FROM} WHERE {SCOPED_ISSUE_SCOPE} \
        AND issue.id IN ({}) ORDER BY issue.created_at DESC",
        holders.join(",")
    );
    fetch_json_rows(pool, &inner, binder.values()).await
}

/// `IssueSerializer(updated_sub_issues, many=True)` (`:244`): the view
/// annotates only `state_group` — which the serializer does not read — so
/// the seven annotation-driven base fields are absent and DRF's `SkipField`
/// omits them (the unannotated shape, like archive retrieve).
async fn render_post_row(
    pool: &sqlx::PgPool,
    row: &Map<String, Value>,
    timezone: &Tz,
) -> Result<String, Denial> {
    let issue_id = get_str(row, "id")
        .and_then(|text| text.parse::<uuid::Uuid>().ok())
        .ok_or(Denial::ServerError)?;
    let is_synced = probe_synced(pool, &issue_id, get_str(row, "external_source")).await?;
    let created_at = render_actor_moment(row, "created_at", timezone).ok_or(Denial::ServerError)?;
    let updated_at = render_actor_moment(row, "updated_at", timezone).ok_or(Denial::ServerError)?;
    let completed_at = render_actor_moment(row, "completed_at", timezone);
    let base = IssueDetailBaseRow {
        id: req_str(row, "id")?,
        name: req_str(row, "name")?,
        state_id: get_str(row, "state_id"),
        sort_order: get_f64(row, "sort_order").ok_or(Denial::ServerError)?,
        completed_at: completed_at.as_deref(),
        estimate_point: get_str(row, "estimate_point"),
        priority: req_str(row, "priority")?,
        complexity_score: get_i32(row, "complexity_score").ok_or(Denial::ServerError)?,
        start_date: get_str(row, "start_date"),
        target_date: get_str(row, "target_date"),
        sequence_id: get_i32(row, "sequence_id").ok_or(Denial::ServerError)?,
        project_id: req_str(row, "project_id")?,
        parent_id: get_str(row, "parent_id"),
        cycle_id: None,
        assigned_pod_id: get_str(row, "assigned_pod_id"),
        agent_executor: get_str(row, "agent_executor"),
        module_ids: None,
        label_ids: None,
        assignee_ids: None,
        sub_issues_count: None,
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: get_str(row, "created_by"),
        updated_by: get_str(row, "updated_by"),
        attachment_count: None,
        link_count: None,
        is_draft: get_bool(row, "is_draft").ok_or(Denial::ServerError)?,
        archived_at: get_str(row, "archived_at"),
        is_synced,
    };
    let view = issue_detail_base_to_representation(&base);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_issues::OneOrMany;

    #[test]
    fn strict_int_matrix() {
        assert_eq!(strict_str_to_int("1"), Some(IdentifierInt::InRange(1)));
        assert_eq!(strict_str_to_int("-5"), Some(IdentifierInt::InRange(-5)));
        assert_eq!(strict_str_to_int(""), None);
        assert_eq!(strict_str_to_int("abc"), None);
        assert_eq!(strict_str_to_int("+1"), None);
        assert_eq!(strict_str_to_int(" 1"), None);
        assert_eq!(strict_str_to_int("-"), None);
        assert_eq!(strict_str_to_int("1-2"), None);
        assert_eq!(strict_str_to_int("²"), None);
        assert_eq!(
            strict_str_to_int("2147483647"),
            Some(IdentifierInt::InRange(2147483647))
        );
        assert_eq!(
            strict_str_to_int("2147483648"),
            Some(IdentifierInt::OutOfRange)
        );
        assert_eq!(
            strict_str_to_int("99999999999999999999999"),
            Some(IdentifierInt::OutOfRange)
        );
    }

    #[test]
    fn tail_split_is_last_dash() {
        assert_eq!(split_identifier_tail("IS-1"), Some(("IS", "1")));
        assert_eq!(split_identifier_tail("A-B-2"), Some(("A-B", "2")));
        assert_eq!(split_identifier_tail("nodash"), None);
        assert_eq!(split_identifier_tail("-1"), None);
        assert_eq!(split_identifier_tail("IS-"), None);
        assert_eq!(split_identifier_tail("-"), None);
        assert_eq!(split_identifier_tail(""), None);
    }

    #[test]
    fn lite_request_matrix() {
        let query = QueryMap::new();
        assert!(!is_lite_request(&query));
        for value in ["1", "true", "yes", "TRUE", "Yes", "TrUe"] {
            let mut query = QueryMap::new();
            query.insert("lite".to_owned(), OneOrMany::One(value.to_owned()));
            assert!(is_lite_request(&query), "{value}");
        }
        for value in ["", "0", "false", "no", "2"] {
            let mut query = QueryMap::new();
            query.insert("lite".to_owned(), OneOrMany::One(value.to_owned()));
            assert!(!is_lite_request(&query), "{value}");
        }
        // `QueryDict.get` reads the LAST repeat.
        let mut query = QueryMap::new();
        query.insert(
            "lite".to_owned(),
            OneOrMany::Many(vec!["true".to_owned(), "0".to_owned()]),
        );
        assert!(!is_lite_request(&query));
    }

    /// Independently re-derive the expected alias list straight from the
    /// fragment consts: any drift in `identifier_full_sql` select order or
    /// in `full_row_aliases` fails here instead of mislabeling columns.
    #[test]
    fn full_aliases_match_fragment_consts() {
        fn stems(columns: &str, table: &str, prefix: &str) -> Vec<String> {
            let qualifier = format!("{table}.");
            columns
                .split(',')
                .map(|column| {
                    let bare = column.trim().strip_prefix(qualifier.as_str()).unwrap();
                    format!("{prefix}{}", bare.replace('"', ""))
                })
                .collect()
        }
        let mut expected = stems(queries_core::ISSUE_COLUMNS, "issue", "i_");
        assert_eq!(expected.len(), 34);
        expected.extend(
            [
                "cycle_id",
                "link_count",
                "attachment_count",
                "sub_issues_count",
                "label_ids",
                "assignee_ids",
                "module_ids",
                "is_subscribed",
                "is_intake",
            ]
            .iter()
            .map(|name| (*name).to_string()),
        );
        expected.extend(stems(queries_core::PROJECT_COLUMNS, "projects", "p_"));
        expected.extend(stems(queries_engage::WORKSPACE_COLUMNS, "workspaces", "w_"));
        expected.extend(stems(
            queries_engage::PARENT_ISSUE_COLUMNS,
            "parent",
            "par_",
        ));
        expected.extend(stems(queries_core::STATE_COLUMNS, "state", "s_"));
        expected.extend(stems(
            queries_engage::TICKER_COLUMNS,
            "issue_agent_ticker",
            "t_",
        ));
        let actual = full_row_aliases();
        assert_eq!(actual.len(), 175);
        assert_eq!(actual, expected);
        // Spot the group boundaries: a shifted block fails loudly.
        assert_eq!(actual[0], "i_created_at");
        assert_eq!(actual[33], "i_agent_executor");
        assert_eq!(actual[34], "cycle_id");
        assert_eq!(actual[42], "is_intake");
        assert_eq!(actual[43], "p_created_at");
        assert_eq!(actual[88], "p_default_agent_executor");
        assert_eq!(actual[89], "w_created_at");
        assert_eq!(actual[102], "w_background_color");
        assert_eq!(actual[103], "par_created_at");
        assert_eq!(actual[136], "par_agent_executor");
        assert_eq!(actual[137], "s_created_at");
        assert_eq!(actual[154], "s_external_id");
        assert_eq!(actual[155], "t_created_at");
        assert_eq!(actual[174], "t_resume_parent_run_id");
        // Unique: no `row_to_json` key collision survives.
        let mut unique = actual.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), actual.len());
    }

    #[test]
    fn agent_run_aliases_break_collisions() {
        for sql in [LATEST_AGENT_RUN_SQL, ACTIVE_AGENT_RUN_SQL] {
            let aliased = agent_run_sql_aliased(sql);
            assert!(aliased.contains("runner.name AS runner_name"));
            assert!(aliased.contains("runner_live_state.llm_model AS live_llm_model"));
            // The run's own `llm_model` keeps its bare name.
            assert!(
                aliased.contains("agent_run.llm_model,")
                    || aliased.contains("agent_run.llm_model ")
            );
        }
    }

    #[test]
    fn blocker_statements_bind_one_issue() {
        for sql in [blockers_sql(), dependents_sql(), has_open_blockers_sql()] {
            assert_eq!(sql.matches(":issue_id").count(), 2);
        }
    }

    #[test]
    fn sub_issues_order_branches() {
        assert_eq!(
            sub_issues_order("").unwrap(),
            queries_core::SUB_ISSUES_DEFAULT_ORDER
        );
        assert_eq!(
            sub_issues_order("-created_at").unwrap(),
            "ORDER BY issue.\"created_at\" DESC"
        );
        let priority = sub_issues_order("priority").unwrap();
        assert!(priority.contains("WHEN issue.priority = 'urgent'"));
        assert!(priority.contains("issue.created_at DESC"));
        assert!(!priority.contains(" created_at DESC"));
        let min = sub_issues_order("-labels__name").unwrap();
        assert!(min.contains("MIN(l.name)"));
        assert!(min.contains("issue.created_at DESC"));
        assert!(sub_issues_order("nope").is_err());
    }

    #[test]
    fn python_scalars() {
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_str(&Value::Bool(true)), "True");
        assert_eq!(py_str(&Value::Bool(false)), "False");
        assert_eq!(py_str(&serde_json::json!(3)), "3");
        assert_eq!(py_str(&serde_json::json!("backlog")), "backlog");
        let utc: Tz = "UTC".parse().unwrap();
        assert_eq!(
            py_moment_str(
                &Value::String("2026-10-03T12:34:56.789012+00:00".to_owned()),
                &utc
            ),
            "2026-10-03 12:34:56.789012+00:00"
        );
        assert_eq!(
            py_moment_str(&Value::String("2026-10-03T12:34:56+00:00".to_owned()), &utc),
            "2026-10-03 12:34:56+00:00"
        );
        assert_eq!(py_moment_str(&Value::Null, &utc), "None");
    }

    #[test]
    fn date_merge_rules() {
        let day = |text: &str| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
        // New beats current; bad formats fail.
        assert_eq!(
            merge_date(Some("2026-01-01"), &serde_json::json!("2026-02-01")).unwrap(),
            Some(day("2026-02-01"))
        );
        assert_eq!(
            merge_date(Some("2026-01-01"), &Value::Null).unwrap(),
            Some(day("2026-01-01"))
        );
        assert!(merge_date(None, &serde_json::json!("2026-13-45")).is_err());
        assert!(merge_date(None, &serde_json::json!(123)).is_err());
        assert!(exceeds(&Some(day("2026-03-01")), &Some(day("2026-02-01"))));
        assert!(!exceeds(&Some(day("2026-02-01")), &Some(day("2026-02-01"))));
        assert!(!exceeds(&None, &Some(day("2026-02-01"))));
        assert_eq!(current_render(Some("2026-01-01")), "2026-01-01");
        assert_eq!(current_render(None), "None");
        assert!(is_truthy(&serde_json::json!("x")));
        assert!(!is_truthy(&serde_json::json!("")));
        assert!(!is_truthy(&Value::Null));
        assert!(!is_truthy(&serde_json::json!(0)));
    }

    #[test]
    fn float_field_matrix() {
        assert_eq!(parse_float_field(&serde_json::json!(3)), Some(3.0));
        assert_eq!(parse_float_field(&serde_json::json!("1.5")), Some(1.5));
        assert_eq!(parse_float_field(&serde_json::json!(true)), Some(1.0));
        assert_eq!(parse_float_field(&serde_json::json!(false)), Some(0.0));
        assert_eq!(parse_float_field(&serde_json::json!("abc")), None);
        assert_eq!(parse_float_field(&Value::Null), None);
        assert_eq!(parse_float_field(&serde_json::json!([1])), None);
    }

    #[test]
    fn type_names_match_python() {
        assert_eq!(json_type_name(&Value::Null), "NoneType");
        assert_eq!(json_type_name(&serde_json::json!(true)), "bool");
        assert_eq!(json_type_name(&serde_json::json!(3)), "int");
        assert_eq!(json_type_name(&serde_json::json!(3.5)), "float");
        assert_eq!(json_type_name(&serde_json::json!("x")), "str");
        assert_eq!(json_type_name(&serde_json::json!([1])), "list");
    }

    #[test]
    fn property_patch_matrix() {
        let mut valid = Map::new();
        valid.insert(
            "display_properties".to_owned(),
            serde_json::json!({"key": "priority"}),
        );
        valid.insert("user".to_owned(), serde_json::json!("ignored"));
        let patch = validate_property_patch(&valid).unwrap();
        assert_eq!(
            patch.display_properties,
            Some(serde_json::json!({"key": "priority"}))
        );
        assert!(patch.sort_order.is_none());
        let mut bad = Map::new();
        bad.insert("display_properties".to_owned(), Value::Null);
        bad.insert("sort_order".to_owned(), serde_json::json!("abc"));
        assert!(validate_property_patch(&bad).is_err());
        let mut null_order = Map::new();
        null_order.insert("sort_order".to_owned(), Value::Null);
        assert!(validate_property_patch(&null_order).is_err());
        assert!(validate_property_patch(&Map::new()).is_ok());
    }

    #[test]
    fn sub_ids_matrix() {
        let mut missing = Map::new();
        assert!(parse_sub_issue_ids(&missing).unwrap().is_empty());
        let mut null = Map::new();
        null.insert("sub_issue_ids".to_owned(), Value::Null);
        assert!(parse_sub_issue_ids(&null).is_err());
        let mut empty = Map::new();
        empty.insert("sub_issue_ids".to_owned(), serde_json::json!([]));
        assert!(parse_sub_issue_ids(&empty).unwrap().is_empty());
        let mut one = Map::new();
        one.insert("sub_issue_ids".to_owned(), serde_json::json!(["a"]));
        assert_eq!(
            parse_sub_issue_ids(&one).unwrap(),
            vec![SubIssueId::Fetch("a".to_owned())]
        );
        // `int`/`bool`/`None` items convert (`uuid.UUID(int=…)` / `IS NULL`)
        // and enqueue in Python-`str()` form; floats/lists/dicts → 400.
        let mut number = Map::new();
        number.insert("sub_issue_ids".to_owned(), serde_json::json!([3]));
        assert_eq!(
            parse_sub_issue_ids(&number).unwrap(),
            vec![SubIssueId::Converted("3".to_owned())]
        );
        let mut flag = Map::new();
        flag.insert("sub_issue_ids".to_owned(), serde_json::json!([true]));
        assert_eq!(
            parse_sub_issue_ids(&flag).unwrap(),
            vec![SubIssueId::Converted("True".to_owned())]
        );
        let mut null_item = Map::new();
        null_item.insert("sub_issue_ids".to_owned(), serde_json::json!([null]));
        assert_eq!(
            parse_sub_issue_ids(&null_item).unwrap(),
            vec![SubIssueId::Converted("None".to_owned())]
        );
        let mut bad_item = Map::new();
        bad_item.insert("sub_issue_ids".to_owned(), serde_json::json!([1.5]));
        assert!(parse_sub_issue_ids(&bad_item).is_err());
        let mut nested = Map::new();
        nested.insert("sub_issue_ids".to_owned(), serde_json::json!([[3]]));
        assert!(parse_sub_issue_ids(&nested).is_err());
        let mut text = Map::new();
        text.insert("sub_issue_ids".to_owned(), serde_json::json!("abc"));
        assert!(parse_sub_issue_ids(&text).is_err());
        let mut empty_text = Map::new();
        empty_text.insert("sub_issue_ids".to_owned(), serde_json::json!(""));
        assert!(parse_sub_issue_ids(&empty_text).unwrap().is_empty());
        let _ = &mut missing;
    }

    #[test]
    fn updates_selector_matrix() {
        // Missing → `[]`; an explicit null breaks iteration, like any
        // other non-list.
        let missing = Map::new();
        assert!(select_updates(&missing).unwrap().is_empty());
        let mut null = Map::new();
        null.insert("updates".to_owned(), Value::Null);
        assert!(select_updates(&null).is_err());
        let mut number = Map::new();
        number.insert("updates".to_owned(), serde_json::json!(3));
        assert!(select_updates(&number).is_err());
        let mut list = Map::new();
        list.insert("updates".to_owned(), serde_json::json!([{"id": "x"}]));
        assert_eq!(select_updates(&list).unwrap().len(), 1);
    }

    #[test]
    fn update_id_matrix() {
        assert_eq!(
            classify_update_id(&serde_json::json!("abc")).unwrap(),
            UpdateId::Raw("abc")
        );
        // Converted (`uuid.UUID(int=…)` / `IS NULL`): skipped with a 200.
        assert_eq!(
            classify_update_id(&serde_json::json!(3)).unwrap(),
            UpdateId::Unmatchable
        );
        assert_eq!(
            classify_update_id(&serde_json::json!(true)).unwrap(),
            UpdateId::Unmatchable
        );
        assert_eq!(
            classify_update_id(&Value::Null).unwrap(),
            UpdateId::Unmatchable
        );
        // Unconvertible: `ValidationError` → 400.
        assert!(classify_update_id(&serde_json::json!(1.5)).is_err());
        assert!(classify_update_id(&serde_json::json!(["a"])).is_err());
        assert!(classify_update_id(&serde_json::json!({"id": "a"})).is_err());
    }

    #[test]
    fn repeated_id_observes_earlier_write() {
        let day = |text: &str| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
        let writes = vec![BulkWrite {
            id: "a",
            start_date: Some(day("2026-02-01")),
            target_date: None,
        }];
        assert_eq!(
            written_current(&writes, "a"),
            (Some("2026-02-01".to_owned()), None)
        );
        assert_eq!(written_current(&writes, "b"), (None, None));
        assert_eq!(written_current(&[], "a"), (None, None));
    }

    #[test]
    fn property_defaults_carry_expected_keys() {
        for key in ["priority", "state", "labels"] {
            assert!(default_display_properties().get(key).is_some());
        }
        assert_eq!(
            default_display_filters().get("order_by"),
            Some(&serde_json::json!("-created_at"))
        );
        assert_eq!(
            default_preferences()
                .get("navigation")
                .and_then(|nav| nav.get("default_tab")),
            Some(&serde_json::json!("work_items"))
        );
    }

    #[test]
    fn enqueue_payload_shapes() {
        let user = uuid::Uuid::nil();
        let project = uuid::Uuid::nil();
        let bulk = bulk_dates_activity_message(
            "start_date",
            &serde_json::json!("2026-01-01"),
            "None",
            "issue",
            &user,
            &project,
            7,
        );
        assert_eq!(bulk.task, ISSUE_ACTIVITY_TASK);
        assert!(bulk.args.is_empty());
        let keys: Vec<&str> = bulk.kwargs.keys().map(String::as_str).collect();
        for key in [
            "type",
            "requested_data",
            "current_instance",
            "issue_id",
            "actor_id",
            "project_id",
            "epoch",
        ] {
            assert!(keys.contains(&key), "{key}");
        }
        assert!(!keys.contains(&"notification"));
        assert!(!keys.contains(&"origin"));
        assert_eq!(
            bulk.kwargs.get("requested_data"),
            Some(&Value::String(
                "{\"start_date\": \"2026-01-01\"}".to_owned()
            ))
        );
        let link = sub_issue_activity_message(&project, "sub", &user, &project, 7, "https://app");
        assert_eq!(
            link.kwargs.get("requested_data"),
            Some(&Value::String(format!("{{\"parent\": \"{project}\"}}")))
        );
        assert_eq!(
            link.kwargs.get("current_instance"),
            Some(&Value::String("{\"parent\": \"sub\"}".to_owned()))
        );
        let visited = identifier_visited_message("ws", &project, &user, &project);
        assert_eq!(visited.task, RECENT_VISITED_TASK);
        assert_eq!(
            visited.kwargs.get("entity_name"),
            Some(&Value::String("issue".to_owned()))
        );
    }
}

#[cfg(test)]
mod pidashconv_736_tests {
    use super::normalize_resolve_identifier;

    #[test]
    fn resolve_identifier_strips_py_whitespace() {
        assert_eq!(normalize_resolve_identifier("  eng "), "ENG");
        // Python `str.strip()` also strips U+001C-U+001F (PIDASHCONV-736):
        // `%1C`-padded identifiers must resolve, not 404.
        for sep in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let padded = format!("{sep}eng{sep}");
            assert_eq!(
                normalize_resolve_identifier(&padded),
                "ENG",
                "U+{:04X} padding must strip like Python",
                sep as u32
            );
        }
        // TAB and U+0085 padding already matched Django; pin the behavior.
        assert_eq!(normalize_resolve_identifier("\teng\t"), "ENG");
        assert_eq!(normalize_resolve_identifier("\u{85}eng\u{85}"), "ENG");
    }
}
