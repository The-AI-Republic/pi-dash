//! App cycles handler units C: date-check, transfer, user-properties (D-27, stage 5).
//!
//! Ports `app/views/cycle/base.py:520-657` onto the foundation crates:
//!
//! * `POST cycles/date-check/` — [`date_check`]
//!   (`CycleDateCheckEndpoint.post`, `base.py:520-557`, F-C27-10).
//! * `POST cycles/<cycle_id>/transfer-issues/` — [`transfer_issues`]
//!   (`TransferCycleIssueEndpoint.post`, `base.py:594-622`, driving
//!   `transfer_cycle_issues` in `utils/cycle_transfer_issues.py:36-479`,
//!   F-C27-06).
//! * `GET` + `PATCH cycles/<cycle_id>/user-properties/` —
//!   [`get_user_properties`] / [`patch_user_properties`]
//!   (`CycleUserPropertiesEndpoint`, `base.py:624-657`, F-C27-10).
//!
//! Routes 5, 8, 9 of `app/urls/cycle.py:54-81`; registration is the
//! cutover granularity (the `app_issues` / `app_intake` rule): owned
//! methods serve from Rust, every other method on these paths proxies to
//! Django through the edge fallback (its 405s and metadata responses
//! live there).
//!
//! Layering: gates in [`crate::app_cycles::gates`] (F-C27-07,
//! PIDASHCONV-290) over the F-06 kernel; transfer query fragments in
//! `pidash_services::app_cycles::queries` (F-C27-03/06, PIDASHCONV-289).
//! This module owns the HTTP shell (routes, session auth, tenant +
//! membership resolution), the SQL text for the handler-owned
//! lookups/writes, the `convert_to_utc` mirror, the snapshot math, and
//! the row rendering. The request-context plumbing (`Actor`, `Tenant`,
//! `Membership`, `Denial`, [`actor`], [`resolve_tenant`],
//! [`load_membership`]) mirrors the D-32 `app_intake` layout so sibling
//! handler issues (PIDASHCONV-321/323/377/410) can reuse it through
//! `super::handlers_misc` instead of copying it.
//!
//! Ported bugs (translate, don't redesign):
//! - BUG-date-check-overlap-200 (`base.py:548-554`): an overlap answers
//!   200 with `{"error": ..., "status": false}`, NOT a 4xx.
//! - BUG-date-check-exclude-none (`base.py:547`): `.exclude(pk=None)`
//!   excludes nothing, so a missing `cycle_id` disables the exclusion.
//! - BUG-transfer-unknown-target-500 (`cycle_transfer_issues.py:59-62`):
//!   an unknown `new_cycle_id` reads `.first()` (`None`) then
//!   dereferences `.end_date` unconditionally → `AttributeError` → 500,
//!   never 400/404.
//! - BUG-transfer-source-400 (`base.py:615-620`): a missing source cycle
//!   answers HTTP 400 (the endpoint maps every `result["error"]` to 400),
//!   not 404.
//! - BUG-userprops-patch-201 (`base.py:644`): PATCH answers 201, not 200.
//! - BUG-userprops-patch-no-create (`base.py:626-634`): PATCH uses
//!   `.get` (missing row → 404), unlike GET's `get_or_create`.
//! - BUG-date-check-bad-format-500: an unparseable date fails inside
//!   `strptime` (`ValueError` → generic 500), never 400.
//! - BUG-date-check-garbage-cycle-400: a non-UUID `cycle_id` fails the
//!   `exclude(pk=...)` lookup (`ValidationError` → 400
//!   `{"error": "Please provide valid detail"}`).
//! - Parity boundary (same as the queries layer, PIDASHCONV-289): the
//!   `IssueManager` extras (triage exclusion, project-archived guard)
//!   are not re-applied beyond the explicit `Q` filters — the Done
//!   fragments in `queries.rs` define the contract and seeds never carry
//!   triage or archived-project rows, so both readings agree on every
//!   tested input.
//!
//! Fixture inputs: F-C27-06 (`transfer.json`), F-C27-07 (`perms.json`),
//! F-C27-10 (`misc.json`) under `rust-api/fixtures/app_cycles/`.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook
//! (updated_at 2026-09-30T04:04:40.144056Z); PIDASHCONV-85 oracle Done.

// Every handler returns a fully-rendered `Response` by design (like the
// intake handlers, which carry the same per-function allow).
#![allow(clippy::result_large_err)]

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::gates::{decide_gate, tenant_context, Gate, GateOutcome};
use crate::state::AppState;
use pidash_auth::permissions::allow::AllowFacts;
use pidash_types::WorkspaceId;

// ---------------------------------------------------------------------------
// Exact wire bodies
// ---------------------------------------------------------------------------

/// DRF `IsAuthenticated` denial (`app/views/base.py:87`).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`app/views/base.py:129-133`).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `Http404("Project not found")` (`db/models/project.py:218`),
/// propagated by DRF's exception handler as `NotFound(*exc.args)`
/// (unresolvable project identifier).
pub const NOT_FOUND_DETAIL_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `ValidationError` branch
/// (`app/views/base.py:125-128`).
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s `IntegrityError` branch
/// (`app/views/base.py:121-124`): an FK miss or unique violation on
/// write (e.g. user-properties created for an unknown cycle).
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// Django prod `custom_404_view` (`app/views/error_404.py`): unmatched
/// paths, including non-UUID ids under `<uuid:>` converters (the D-32
/// intake `PageNotFound` twin).
pub const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found."}"#;

/// Date-check without both dates (`base.py:526-530`).
pub const DATE_CHECK_REQUIRED_BODY: &str =
    r#"{"error":"Start date and end date both are required"}"#;
/// Date-check overlap (`base.py:548-554`): 200 with the error key first
/// and `status: false` — NOT a 4xx (BUG-date-check-overlap-200).
pub const DATE_CHECK_CONFLICT_BODY: &str = "{\"error\":\"You have a cycle already on the given dates, if you want to create a draft cycle you can do that by removing dates\",\"status\":false}";
/// Date-check clear (`base.py:555-556`).
pub const DATE_CHECK_CLEAR_BODY: &str = r#"{"status":true}"#;

/// Gate-table paths for the three owned routes (the [`super::gates`]
/// keys; the handler gates resolve through [`gate_for_path`]).
pub const DATE_CHECK_PATH: &str = "workspaces/<slug>/projects/<id>/cycles/date-check/";
pub const TRANSFER_PATH: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/transfer-issues/";
pub const USER_PROPERTIES_PATH: &str =
    "workspaces/<slug>/projects/<id>/cycles/<uuid>/user-properties/";

/// The [`Gate`] for one owned route+method: the single source of truth
/// is the F-C27-07 table, so an unknown row is a 500, never a pass.
pub fn gate_for_path(method: &str, path: &str) -> Result<Gate, Denial> {
    super::gates::gate_for(method, path)
        .map(|row| row.gate)
        .ok_or(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Denial
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded route).
    Unauthorized,
    /// 403, `allow_permission` fallthrough.
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `Http404("Project not found")` (unresolvable identifier).
    NotFoundDetail,
    /// 404, Django prod `custom_404_view` (garbage `<uuid:>` segment).
    PageNotFound,
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 400, Django `ValidationError` branch.
    InvalidDetail,
    /// 400, Django `IntegrityError` branch.
    InvalidPayload,
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::gates::FORBIDDEN_BODY.to_owned(),
            ),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::NotFoundDetail => (StatusCode::NOT_FOUND, NOT_FOUND_DETAIL_BODY.to_owned()),
            Denial::PageNotFound => (StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY.to_owned()),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::InvalidDetail => (StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY.to_owned()),
            Denial::InvalidPayload => (StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned()),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("static denial response")
    }
}

pub fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render pre-serialized JSON bytes as a 200 response body.
pub fn raw_json_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

/// Render pre-serialized JSON bytes with an explicit status.
pub fn raw_json_status(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

/// `json.dumps` with CPython defaults (`, ` / `: ` separators): the
/// transfer `requested_data` / `current_instance` payloads. Same
/// algorithm as the D-32 `app_intake` twin; kept per domain module so
/// sibling handler issues never fork a shared helper.
pub fn python_dumps(value: &serde_json::Value) -> String {
    let mut out = String::new();
    python_dump_into(&mut out, value);
    out
}

fn python_dump_into(out: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(true) => out.push_str("true"),
        serde_json::Value::Bool(false) => out.push_str("false"),
        serde_json::Value::Number(number) => out.push_str(&number.to_string()),
        serde_json::Value::String(text) => out.push_str(&json_string(text)),
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_into(out, item);
            }
            out.push(']');
        }
        serde_json::Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(&json_string(key));
                out.push_str(": ");
                python_dump_into(out, item);
            }
            out.push('}');
        }
    }
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

/// Authenticated actor plus time zone (`TimezoneMixin.initial` activates
/// the user's zone; datetimes render in it).
pub struct Actor {
    pub id: Uuid,
    pub timezone: Tz,
}

/// `request.user` through Django-session auth.
///
/// `BaseSessionAuthentication` + `IsAuthenticated`
/// (`views/base.py:48,52`): anonymous answers the DRF `NotAuthenticated`
/// body before anything else runs.
pub async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Actor, Denial> {
    let pool = pool_of(state)?;
    let resolved =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| Denial::ServerError)?
            .ok_or(Denial::Unauthorized)?;
    Ok(Actor {
        id: resolved.id,
        timezone: resolved.timezone,
    })
}

pub fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// The resolved tenant scope: workspace + project + the project's
/// timezone and estimate link (both read by these handlers).
pub struct Tenant {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub timezone: String,
    pub estimate_id: Option<Uuid>,
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

/// Resolve `slug` + `project_id` the way the view kwargs do:
/// `_rewrite_project_kwarg` (`app/views/base.py:49-79`) accepts a UUID
/// or a workspace-scoped project identifier (upper-cased, like
/// `Project.save` normalizes it). An unresolvable identifier answers
/// `Http404("Project not found")` (`db/models/project.py:218`); a missing project row answers the
/// `ObjectDoesNotExist` branch (which is what `convert_to_utc`'s
/// `Project.objects.get` raises on the date-check path).
pub async fn resolve_tenant(
    pool: &PgPool,
    slug: &str,
    project_raw: &str,
) -> Result<Tenant, Denial> {
    if let Ok(id) = project_raw.parse::<Uuid>() {
        let row: Option<(Uuid, Uuid, String, Option<Uuid>)> = sqlx::query_as(
            r#"SELECT p.id, p.workspace_id, p.timezone, p.estimate_id FROM projects p WHERE p.id = $1 AND p.deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        let Some((project_id, workspace_id, timezone, estimate_id)) = row else {
            return Err(Denial::NotFound);
        };
        return Ok(Tenant {
            workspace_id,
            project_id,
            timezone,
            estimate_id,
        });
    }
    let upper = normalize_resolve_identifier(project_raw);
    let row: Option<(Uuid, Uuid, String, Option<Uuid>)> = sqlx::query_as(
        r#"SELECT p.id, p.workspace_id, p.timezone, p.estimate_id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match row {
        Some((project_id, workspace_id, timezone, estimate_id)) => Ok(Tenant {
            workspace_id,
            project_id,
            timezone,
            estimate_id,
        }),
        None => Err(Denial::NotFoundDetail),
    }
}

/// Membership facts for the guards layer, resolved with the same row
/// filters Python uses (`app/permissions/base.py:26-30,45-59`).
pub struct Membership {
    pub project_role: Option<i16>,
    pub workspace_member: bool,
    pub workspace_admin: bool,
}

pub async fn load_membership(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<Membership, Denial> {
    let project_role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
             AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `EXISTS` (never aggregates): an empty membership set reads as
    // `False`, exactly like the Django `.exists()` probes.
    let workspace_member: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT wm.id FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2
             AND wm.is_active AND wm.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let workspace_admin: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT wm.id FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.role = 20
             AND wm.is_active AND wm.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (workspace_member, workspace_admin) =
        (workspace_member.is_some(), workspace_admin.is_some());
    Ok(Membership {
        project_role: project_role.map(|row| row.0),
        workspace_member,
        workspace_admin,
    })
}

/// Run one [`Gate`] the way `@allow_permission` does: Django-session
/// authN first, then the role check over caller-scoped membership facts.
/// `is_creator` is always false on these routes (only
/// `CycleViewSet.destroy` sets `creator=True`).
pub fn check_gate(gate: &Gate, slug: &str, membership: &Membership) -> Result<(), Denial> {
    // `has_allowed_project_role` is role-in-`allowed_roles` for THIS
    // gate's list (`app/permissions/base.py:45-59`); auth-only rows
    // carry no list and any authenticated caller passes. Callers run
    // [`actor`] first, so `authenticated` is always true here.
    let roles: &[i32] = match gate {
        Gate::Project { roles } | Gate::ProjectCreator { roles } => roles,
        Gate::Authenticated => &[],
    };
    let facts = AllowFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        is_workspace_member: membership.workspace_member,
        has_allowed_workspace_role: false,
        is_creator: false,
        has_allowed_project_role: membership
            .project_role
            .map(|role| roles.contains(&(role as i32)))
            .unwrap_or(false),
        is_project_member: membership.project_role.is_some(),
        is_workspace_admin: membership.workspace_admin,
    };
    match decide_gate(gate, &tenant_context(slug), &facts) {
        GateOutcome::Allow => Ok(()),
        GateOutcome::Deny => Err(Denial::Forbidden),
        GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

/// Parse a UUID path segment: Django's `<uuid:>` converter 404s on
/// garbage through the prod 404 view, so unparseable ids behave as
/// unmatched paths, not missing rows.
#[allow(clippy::result_large_err)]
pub fn parse_id(raw: &str) -> Result<Uuid, Response> {
    raw.parse::<Uuid>()
        .map_err(|_| Denial::PageNotFound.into_response())
}

/// Best-effort post-commit task enqueue (the D-32 intake pattern):
/// without a queue table the response still stands — Django would have
/// answered 500 only when its own broker write failed, and the proxy
/// contract tests never run a worker.
pub async fn enqueue_message(pool: &PgPool, message: pidash_jobs::celery::CeleryTaskMessage) {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-28`):
/// `WEB_URL` else `APP_BASE_URL`; unset is `ImproperlyConfigured` → 500.
pub fn request_origin(state: &AppState) -> Result<String, Denial> {
    state
        .settings()
        .urls
        .web_url
        .clone()
        .or_else(|| state.settings().urls.app_base_url.clone())
        .ok_or(Denial::ServerError)
}

/// Parse the JSON request body the way DRF does: a non-object (or
/// unparseable) body has no `.get`, so attribute access dies with
/// `AttributeError` → generic 500 on the paths below.
pub fn parse_object_body(body: &Bytes) -> Result<Map<String, Value>, Denial> {
    let value: Value = serde_json::from_slice(body).map_err(|_| Denial::ServerError)?;
    value.as_object().cloned().ok_or(Denial::ServerError)
}

/// Python truthiness for a JSON body value (`request.data.get(...)`
/// followed by `if not ...`): `None`/`False`/`0`/`""`/`[]`/`{}` are
/// falsy, everything else truthy.
pub fn py_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Number(number)) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().map(|float| float != 0.0).unwrap_or(false)
            }
        }
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

/// A `new_cycle_id` body value after the ORM coercion
/// (`base.py:597-603` + `UUIDField.to_python`, probed live in
/// `manage.py shell`): ints (and `True == 1`) become
/// `uuid.UUID(int=v)` and miss the destination lookup (the ported
/// 500); floats, negative ints, lists, dicts and non-UUID strings
/// raise `ValidationError` → 400.
pub enum NewCycleId {
    /// Falsy (`false`/`0`/`""`/`[]`/`{}`/null/missing): 400 "required".
    Required,
    /// A destination id to look up (unknown → the ported 500).
    Id(Uuid),
    /// `ValidationError` → 400 InvalidDetail.
    InvalidDetail,
}

pub fn classify_new_cycle_id(value: Option<&Value>) -> NewCycleId {
    if !py_truthy(value) {
        return NewCycleId::Required;
    }
    match value {
        Some(Value::String(raw)) => match raw.parse::<Uuid>() {
            Ok(id) => NewCycleId::Id(id),
            Err(_) => NewCycleId::InvalidDetail,
        },
        // `True == 1`: `uuid.UUID(int=True)`.
        Some(Value::Bool(true)) => NewCycleId::Id(Uuid::from_u128(1)),
        Some(Value::Number(number)) => {
            // Only non-negative integer reprs coerce (`uuid.UUID(int=v)`
            // needs the 128-bit range; floats and negative ints raise).
            if number.is_u64() {
                NewCycleId::Id(Uuid::from_u128(u128::from(number.as_u64().expect("u64"))))
            } else {
                NewCycleId::InvalidDetail
            }
        }
        _ => NewCycleId::InvalidDetail,
    }
}

/// A date-check `cycle_id` body value after the ORM coercion
/// (`base.py:525` + `UUIDField.to_python`, probed live in
/// `manage.py shell`): unlike `new_cycle_id` there is no `if not`
/// guard, so falsy ints/bools still coerce — `0`/`False` become
/// `uuid.UUID(int=0)` and miss every pk (the normal 200), never
/// "required". Missing/null excludes nothing; floats, negative ints,
/// lists, dicts and non-UUID strings raise `ValidationError` → 400.
pub fn classify_date_check_cycle_id(value: Option<&Value>) -> Result<Option<Uuid>, Denial> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => match raw.parse::<Uuid>() {
            Ok(id) => Ok(Some(id)),
            Err(_) => Err(Denial::InvalidDetail),
        },
        // `True == 1`, `False == 0`: `uuid.UUID(int=v)`.
        Some(Value::Bool(true)) => Ok(Some(Uuid::from_u128(1))),
        Some(Value::Bool(false)) => Ok(Some(Uuid::from_u128(0))),
        Some(Value::Number(number)) => {
            // Only non-negative integer reprs coerce (`uuid.UUID(int=v)`
            // needs the 128-bit range; floats and negative ints raise).
            if number.is_u64() {
                Ok(Some(Uuid::from_u128(u128::from(
                    number.as_u64().expect("u64"),
                ))))
            } else {
                Err(Denial::InvalidDetail)
            }
        }
        _ => Err(Denial::InvalidDetail),
    }
}

// ---------------------------------------------------------------------------
// Date-check (`CycleDateCheckEndpoint.post`, `base.py:520-557`)
// ---------------------------------------------------------------------------

/// `POST cycles/date-check/`, gated MEMBER (`base.py:521`).
pub async fn date_check(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let tenant = match resolve_tenant(pool, &slug, &project_raw).await {
        Ok(tenant) => tenant,
        Err(denial) => return denial.into_response(),
    };
    let membership = match load_membership(pool, &slug, &tenant.project_id, &actor.id).await {
        Ok(membership) => membership,
        Err(denial) => return denial.into_response(),
    };
    let gate = match gate_for_path("POST", DATE_CHECK_PATH) {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_gate(&gate, &slug, &membership) {
        return denial.into_response();
    }
    let data = match parse_object_body(&body) {
        Ok(data) => data,
        Err(denial) => return denial.into_response(),
    };
    // `request.data.get("start_date", False)` + `if not ...`
    // (`base.py:522-530`): missing AND falsy both answer 400.
    if !py_truthy(data.get("start_date")) || !py_truthy(data.get("end_date")) {
        return Denial::BadError("Start date and end date both are required".to_owned())
            .into_response();
    }
    // `str(...)` of the raw value, exactly like `convert_to_utc(date=str(...))`.
    let start_raw = py_str(data.get("start_date"));
    let end_raw = py_str(data.get("end_date"));
    let start_date = match convert_start_date(&start_raw, &tenant.timezone) {
        Ok(start) => start,
        Err(denial) => return denial.into_response(),
    };
    let end_date = match convert_end_date(&end_raw, &tenant.timezone) {
        Ok(end) => end,
        Err(denial) => return denial.into_response(),
    };
    // `.exclude(pk=cycle_id)` (`base.py:547`): a missing id excludes
    // nothing (BUG-date-check-exclude-none); ints/bools coerce via
    // `UUID(int=v)` and miss every pk ([`classify_date_check_cycle_id`]);
    // a garbage id fails the UUID lookup (`ValidationError` → 400:
    // BUG-date-check-garbage-cycle-400).
    let exclude: Option<Uuid> = match classify_date_check_cycle_id(data.get("cycle_id")) {
        Ok(exclude) => exclude,
        Err(denial) => return denial.into_response(),
    };
    let overlap = match overlap_exists(
        pool,
        &slug,
        &tenant.project_id,
        &start_date,
        &end_date,
        exclude,
    )
    .await
    {
        Ok(overlap) => overlap,
        Err(denial) => return denial.into_response(),
    };
    if overlap {
        raw_json_response(DATE_CHECK_CONFLICT_BODY.to_owned())
    } else {
        raw_json_response(DATE_CHECK_CLEAR_BODY.to_owned())
    }
}

/// Python `str()` for a JSON body scalar, as passed to `convert_to_utc`
/// via `str(start_date)`: strings pass through, `True`/`False` render
/// title-cased, numbers render as-is (arbitrary_precision keeps the
/// input text, so this echoes it).
pub fn py_str(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => "False".to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Null) | None => "None".to_owned(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => {
            // Unreachable on this path (falsy containers 400 above;
            // truthy ones die in `strptime` → 500 either way).
            String::new()
        }
    }
}

/// `convert_to_utc(date, project_id, is_start_date=True)`
/// (`utils/timezone_converter.py:40-94`): the date at 00:00:01 project
/// time, in UTC — except when that day is today in the project zone,
/// when the answer is now (the `localized_datetime.date() ==
/// current_datetime_in_project_tz.date()` branch). A bad zone or an
/// unparseable date raises in Python (`handle_exception` 500), so both
/// are [`Denial::ServerError`] here, never a fallback.
pub fn convert_start_date(raw: &str, project_tz: &str) -> Result<DateTime<Utc>, Denial> {
    let tz: Tz = project_tz.parse().map_err(|_| Denial::ServerError)?;
    // `datetime.strptime(date, "%Y-%m-%d").date()` — strict calendar
    // date (ValueError → 500 on garbage: BUG-date-check-bad-format-500).
    let day = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| Denial::ServerError)?;
    let naive = day.and_hms_opt(0, 0, 1).ok_or(Denial::ServerError)?;
    let local = tz
        .from_local_datetime(&naive)
        .single()
        .ok_or(Denial::ServerError)?;
    let now_utc = Utc::now();
    if local.date_naive() == now_utc.with_timezone(&tz).date_naive() {
        return Ok(now_utc);
    }
    Ok(local.with_timezone(&Utc))
}

/// `convert_to_utc(date, project_id)` (`timezone_converter.py:78-94`):
/// the date at 23:59:00 project time, in UTC. Same error contract as
/// [`convert_start_date`].
pub fn convert_end_date(raw: &str, project_tz: &str) -> Result<DateTime<Utc>, Denial> {
    let tz: Tz = project_tz.parse().map_err(|_| Denial::ServerError)?;
    let day = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| Denial::ServerError)?;
    let naive = day.and_hms_opt(23, 59, 0).ok_or(Denial::ServerError)?;
    let local = tz
        .from_local_datetime(&naive)
        .single()
        .ok_or(Denial::ServerError)?;
    Ok(local.with_timezone(&Utc))
}

/// The overlap probe (`base.py:539-547`): a cycle in this workspace +
/// project intersecting `[start_date, end_date]` under any of the three
/// interval clauses, minus the excluded pk. NULL stored dates never
/// match (SQL three-valued logic, same as Django).
pub async fn overlap_exists(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    start_date: &DateTime<Utc>,
    end_date: &DateTime<Utc>,
    exclude: Option<Uuid>,
) -> Result<bool, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT c.id FROM cycles c
           JOIN workspaces w ON w.id = c.workspace_id
           WHERE w.slug = $1 AND c.project_id = $2 AND c.deleted_at IS NULL
             AND ($3 IS NULL OR c.id != $3)
             AND ((c.start_date <= $4 AND c.end_date >= $4)
               OR (c.start_date <= $5 AND c.end_date >= $5)
               OR (c.start_date >= $4 AND c.end_date <= $5))
           LIMIT 1"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(exclude)
    .bind(start_date)
    .bind(end_date)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// Transfer (`TransferCycleIssueEndpoint.post`, `base.py:594-622`, driving
// `transfer_cycle_issues`, `utils/cycle_transfer_issues.py:36-479`)
// ---------------------------------------------------------------------------

/// Celery wire name for `issue_activity` (bare `@shared_task` default;
/// also pinned by the D-32 intake `services::app_intake::tasks`).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";
/// `type` kwarg of the transfer emit (`cycle_transfer_issues.py:463`).
pub const TRANSFER_ACTIVITY_TYPE: &str = "cycle.activity.created";

/// `POST cycles/<cycle_id>/transfer-issues/`, gated MEMBER (`base.py:595`).
pub async fn transfer_issues(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let cycle_id = match parse_id(&cycle_raw) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let tenant = match resolve_tenant(pool, &slug, &project_raw).await {
        Ok(tenant) => tenant,
        Err(denial) => return denial.into_response(),
    };
    let membership = match load_membership(pool, &slug, &tenant.project_id, &actor.id).await {
        Ok(membership) => membership,
        Err(denial) => return denial.into_response(),
    };
    let gate = match gate_for_path("POST", TRANSFER_PATH) {
        Ok(gate) => gate,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_gate(&gate, &slug, &membership) {
        return denial.into_response();
    }
    let data = match parse_object_body(&body) {
        Ok(data) => data,
        Err(denial) => return denial.into_response(),
    };
    // `request.data.get("new_cycle_id", False)` + `if not ...`
    // (`base.py:597-603`): falsy answers 400 "required"; truthy
    // shapes follow the ORM coercion ([`classify_new_cycle_id`]).
    let new_cycle_id = match classify_new_cycle_id(data.get("new_cycle_id")) {
        NewCycleId::Required => {
            return raw_json_status(
                StatusCode::BAD_REQUEST,
                pidash_services::app_cycles::queries::NEW_CYCLE_ID_REQUIRED_BODY.to_owned(),
            );
        }
        NewCycleId::Id(id) => id,
        NewCycleId::InvalidDetail => return Denial::InvalidDetail.into_response(),
    };
    match run_transfer(
        &state,
        pool,
        &slug,
        &tenant,
        &actor,
        &cycle_id,
        &new_cycle_id,
    )
    .await
    {
        Ok(()) => raw_json_response(
            pidash_services::app_cycles::queries::TRANSFER_SUCCESS_BODY.to_owned(),
        ),
        Err(denial) => denial.into_response(),
    }
}

/// Decoded `old_cycle_counts` row: identity + date bounds + the six
/// group counts.
pub type OldCycleCountsRow = (
    Uuid,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
);

/// Decoded distribution row: identity triple + total/completed/pending.
pub type DistributionRowTuple = (Option<String>, Option<Uuid>, Option<String>, i64, i64, i64);

/// Decoded label row: name/color/id + total/completed/pending.
pub type LabelRowTuple = (Option<String>, Option<String>, Option<Uuid>, i64, i64, i64);

/// Decoded estimate row: identity triple + float total/completed/pending.
pub type EstimateRowTuple = (
    Option<String>,
    Option<Uuid>,
    Option<String>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);

/// Decoded `cycle_user_properties` row in [`USER_PROPERTIES_SELECT`] order.
pub type UserPropertiesRowTuple = (
    Uuid,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Value,
    Value,
    Value,
    Value,
    Option<Uuid>,
    Option<Uuid>,
    Uuid,
    Uuid,
    Uuid,
    Uuid,
);

/// Old-cycle count annotation row (`cycle_transfer_issues.py:72-143`).
pub struct OldCycleCounts {
    pub total: i64,
    pub completed: i64,
    pub cancelled: i64,
    pub started: i64,
    pub unstarted: i64,
    pub backlog: i64,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
}

/// The `transfer_cycle_issues` sequence after the endpoint guards.
pub async fn run_transfer(
    state: &AppState,
    pool: &PgPool,
    slug: &str,
    tenant: &Tenant,
    actor: &Actor,
    cycle_id: &Uuid,
    new_cycle_id: &Uuid,
) -> Result<(), Denial> {
    // The destination lookup (`.first()`; `None.end_date` is the ported
    // 500 — BUG-transfer-unknown-target-500, never guarded).
    let new_cycle: Option<(Uuid, Option<DateTime<Utc>>)> = sqlx::query_as(
        r#"SELECT c.id, c.end_date FROM cycles c
           JOIN workspaces w ON w.id = c.workspace_id
           WHERE w.slug = $1 AND c.project_id = $2 AND c.id = $3 AND c.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(tenant.project_id)
    .bind(new_cycle_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((_, new_end)) = new_cycle else {
        return Err(Denial::ServerError);
    };
    // Completed-destination refusal (`:59-66`, surfaced as HTTP 400 by
    // `base.py:615-620`).
    if new_end.map(|end| end < Utc::now()).unwrap_or(false) {
        return Err(Denial::BadError(
            pidash_services::app_cycles::queries::DESTINATION_COMPLETED_ERROR.to_owned(),
        ));
    }
    // Old-cycle counts (`:72-143`); a missing source answers 400, not
    // 404 (BUG-transfer-source-400).
    let Some(counts) = old_cycle_counts(pool, slug, &tenant.project_id, cycle_id).await? else {
        return Err(Denial::BadError(
            pidash_services::app_cycles::queries::SOURCE_CYCLE_MISSING_ERROR.to_owned(),
        ));
    };
    // Points estimates only when the project links a points estimate
    // (`:152-157`).
    let use_estimates = project_uses_points(pool, tenant).await?;
    let mut assignee_estimates: Vec<EstimateRow> = Vec::new();
    let mut label_estimates: Vec<EstimateRow> = Vec::new();
    let mut points_chart: Value = Value::Object(Map::new());
    if use_estimates {
        assignee_estimates =
            assignee_estimate_distribution(pool, slug, &tenant.project_id, cycle_id).await?;
        label_estimates =
            label_estimate_distribution(pool, slug, &tenant.project_id, cycle_id).await?;
        points_chart = burndown_points_value(
            pool,
            slug,
            &tenant.project_id,
            cycle_id,
            counts.start_date,
            counts.end_date,
            &actor.timezone,
        )
        .await?;
    }
    let assignees = assignee_distribution(pool, slug, &tenant.project_id, cycle_id).await?;
    let labels = label_distribution(pool, slug, &tenant.project_id, cycle_id).await?;
    let chart = burndown_issues_value(
        pool,
        slug,
        &tenant.project_id,
        cycle_id,
        counts.total,
        counts.start_date,
        counts.end_date,
        &actor.timezone,
    )
    .await?;
    // Snapshot write (`:408-433`, `save(update_fields=["progress_snapshot"])`).
    let snapshot = build_snapshot(
        &counts,
        &labels,
        &assignees,
        &chart,
        use_estimates,
        &label_estimates,
        &assignee_estimates,
        &points_chart,
    );
    sqlx::query(r#"UPDATE cycles SET progress_snapshot = $1 WHERE id = $2"#)
        .bind(&snapshot)
        .bind(cycle_id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    // Move the open-group bridges (`:435-459`, `bulk_update(["cycle_id"],
    // batch_size=100)` — one UPDATE lands the same rows).
    let moved = movable_bridges(pool, slug, &tenant.project_id, cycle_id).await?;
    if !moved.is_empty() {
        let ids: Vec<Uuid> = moved.iter().map(|bridge| bridge.bridge_id).collect();
        sqlx::query(r#"UPDATE cycle_issues SET cycle_id = $1 WHERE id = ANY($2)"#)
            .bind(new_cycle_id)
            .bind(&ids)
            .execute(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    // `issue_activity.delay(type="cycle.activity.created", ...)`
    // (`:462-477`): transactional Celery-protocol enqueue (the D-32
    // intake pattern); `requested_data` is ALWAYS `{"cycles_list": []}`
    // here, `issue_id` is null, `epoch` is `int(timezone.now()
    // .timestamp())`.
    let origin = request_origin(state)?;
    let updated: Vec<Value> = moved
        .iter()
        .map(|bridge| {
            let mut row = Map::with_capacity(3);
            row.insert(
                "old_cycle_id".to_owned(),
                Value::String(cycle_id.to_string()),
            );
            row.insert(
                "new_cycle_id".to_owned(),
                Value::String(new_cycle_id.to_string()),
            );
            row.insert(
                "issue_id".to_owned(),
                Value::String(bridge.issue_id.to_string()),
            );
            Value::Object(row)
        })
        .collect();
    let mut current = Map::with_capacity(2);
    current.insert("updated_cycle_issues".to_owned(), Value::Array(updated));
    current.insert("created_cycle_issues".to_owned(), Value::Array(Vec::new()));
    let mut kwargs = Map::with_capacity(9);
    kwargs.insert(
        "type".to_owned(),
        Value::String(TRANSFER_ACTIVITY_TYPE.to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(
            pidash_services::app_cycles::queries::TRANSFER_ACTIVITY_REQUESTED_DATA.to_owned(),
        ),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor.id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::Null);
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(tenant.project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        Value::String(python_dumps(&Value::Object(current))),
    );
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(Utc::now().timestamp().into()),
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin));
    enqueue_message(
        pool,
        pidash_jobs::celery::CeleryTaskMessage::new(ISSUE_ACTIVITY_TASK, vec![], kwargs),
    )
    .await;
    Ok(())
}

/// Old-cycle count annotation (`cycle_transfer_issues.py:72-143`):
/// non-distinct `COUNT` over the bridge with the archived / draft /
/// bridge-deleted / issue-deleted guards. `None` is a missing source
/// cycle (the `.first()` check at `:145-149`).
pub async fn old_cycle_counts(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
) -> Result<Option<OldCycleCounts>, Denial> {
    let row: Option<OldCycleCountsRow> =
        sqlx::query_as(
            r#"SELECT c.id, c.start_date, c.end_date,
              COUNT(ci.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ci.deleted_at IS NULL AND i.deleted_at IS NULL),
              COUNT(s."group") FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ci.deleted_at IS NULL AND i.deleted_at IS NULL AND s."group" = 'completed'),
              COUNT(s."group") FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ci.deleted_at IS NULL AND i.deleted_at IS NULL AND s."group" = 'cancelled'),
              COUNT(s."group") FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ci.deleted_at IS NULL AND i.deleted_at IS NULL AND s."group" = 'started'),
              COUNT(s."group") FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ci.deleted_at IS NULL AND i.deleted_at IS NULL AND s."group" = 'unstarted'),
              COUNT(s."group") FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE AND ci.deleted_at IS NULL AND i.deleted_at IS NULL AND s."group" = 'backlog')
           FROM cycles c
           JOIN workspaces w ON w.id = c.workspace_id
           LEFT JOIN cycle_issues ci ON ci.cycle_id = c.id
           LEFT JOIN issues i ON i.id = ci.issue_id
           LEFT JOIN states s ON s.id = i.state_id
           WHERE w.slug = $1 AND c.project_id = $2 AND c.id = $3 AND c.deleted_at IS NULL
           GROUP BY c.id, c.start_date, c.end_date"#,
        )
        .bind(slug)
        .bind(project_id)
        .bind(cycle_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(
        |(_, start_date, end_date, total, completed, cancelled, started, unstarted, backlog)| {
            OldCycleCounts {
                total,
                completed,
                cancelled,
                started,
                unstarted,
                backlog,
                start_date,
                end_date,
            }
        },
    ))
}

/// Points-estimate check (`:152-157`): the project links an estimate of
/// type `points`.
pub async fn project_uses_points(pool: &PgPool, tenant: &Tenant) -> Result<bool, Denial> {
    let Some(estimate_id) = tenant.estimate_id else {
        return Ok(false);
    };
    let row: Option<(String,)> =
        sqlx::query_as(r#"SELECT e.type FROM estimates e WHERE e.id = $1"#)
            .bind(estimate_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0 == "points").unwrap_or(false))
}

/// One assignee/label distribution row (`:214-229`, `:318-329`).
pub struct DistributionRow {
    pub display_name: Option<String>,
    pub entity_id: Option<Uuid>,
    pub image: Option<String>,
    pub total: i64,
    pub completed: i64,
    pub pending: i64,
}

/// One estimate distribution row (`:214-222`, `:264-277`).
pub struct EstimateRow {
    pub display_name: Option<String>,
    pub entity_id: Option<Uuid>,
    pub image: Option<String>,
    pub total: Option<f64>,
    pub completed: Option<f64>,
    pub pending: Option<f64>,
}

/// Avatar URL `Case` (`:176-196`, repeated `:300-315`): the asset URL
/// when `avatar_asset` is set, else the raw `avatar` field, else NULL.
const AVATAR_URL_CASE: &str = "CASE WHEN u.avatar_asset_id IS NOT NULL THEN CONCAT('/api/assets/v2/static/', u.avatar_asset_id, '/') WHEN u.avatar_asset_id IS NULL THEN u.avatar ELSE NULL END";

/// Assignee issue distribution (`:286-329`): this cycle's live bridges,
/// tenant-scoped, grouped by assignee (`order_by("display_name")`;
/// unassigned issues form the NULL group via the LEFT JOIN, exactly
/// like the Django m2m traversal).
pub async fn assignee_distribution(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
) -> Result<Vec<DistributionRow>, Denial> {
    let rows: Vec<DistributionRowTuple> = sqlx::query_as(&format!(
        r#"SELECT u.display_name, u.id, {AVATAR_URL_CASE},
              COUNT(i.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE),
              COUNT(i.id) FILTER (WHERE i.completed_at IS NOT NULL AND i.archived_at IS NULL AND i.is_draft = FALSE),
              COUNT(i.id) FILTER (WHERE i.completed_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE)
           FROM issues i
           JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $1 AND ci.deleted_at IS NULL
           JOIN workspaces w ON w.id = i.workspace_id
           LEFT JOIN issue_assignees ia ON ia.issue_id = i.id
           LEFT JOIN users u ON u.id = ia.assignee_id
           WHERE w.slug = $2 AND i.project_id = $3 AND i.deleted_at IS NULL
           GROUP BY u.display_name, u.id, u.avatar_asset_id, u.avatar
           ORDER BY u.display_name"#
    ))
    .bind(cycle_id)
    .bind(slug)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(
            |(display_name, entity_id, image, total, completed, pending)| DistributionRow {
                display_name,
                entity_id,
                image,
                total,
                completed,
                pending,
            },
        )
        .collect())
}

/// Label issue distribution (`:331-363`): same shape, grouped by label
/// (`order_by("label_name")`).
pub async fn label_distribution(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
) -> Result<Vec<DistributionRow>, Denial> {
    let rows: Vec<LabelRowTuple> =
        sqlx::query_as(
            r#"SELECT l.name, l.color, l.id,
              COUNT(i.id) FILTER (WHERE i.archived_at IS NULL AND i.is_draft = FALSE),
              COUNT(i.id) FILTER (WHERE i.completed_at IS NOT NULL AND i.archived_at IS NULL AND i.is_draft = FALSE),
              COUNT(i.id) FILTER (WHERE i.completed_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE)
           FROM issues i
           JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $1 AND ci.deleted_at IS NULL
           JOIN workspaces w ON w.id = i.workspace_id
           LEFT JOIN issue_labels il ON il.issue_id = i.id
           LEFT JOIN labels l ON l.id = il.label_id
           WHERE w.slug = $2 AND i.project_id = $3 AND i.deleted_at IS NULL
           GROUP BY l.name, l.color, l.id
           ORDER BY l.name"#,
    )
    .bind(cycle_id)
    .bind(slug)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(
            |(display_name, image, entity_id, total, completed, pending)| DistributionRow {
                display_name,
                entity_id,
                image,
                total,
                completed,
                pending,
            },
        )
        .collect())
}

/// Assignee estimate distribution (`:164-229`).
pub async fn assignee_estimate_distribution(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
) -> Result<Vec<EstimateRow>, Denial> {
    estimate_distribution(pool, slug, project_id, cycle_id, true).await
}

/// Label estimate distribution (`:238-284`).
pub async fn label_estimate_distribution(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
) -> Result<Vec<EstimateRow>, Denial> {
    estimate_distribution(pool, slug, project_id, cycle_id, false).await
}

async fn estimate_distribution(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
    by_assignee: bool,
) -> Result<Vec<EstimateRow>, Denial> {
    let (join, names, order) = if by_assignee {
        (
            "LEFT JOIN issue_assignees ia ON ia.issue_id = i.id LEFT JOIN users u ON u.id = ia.assignee_id",
            format!("u.display_name, u.id, {AVATAR_URL_CASE}"),
            "u.display_name",
        )
    } else {
        (
            "LEFT JOIN issue_labels il ON il.issue_id = i.id LEFT JOIN labels l ON l.id = il.label_id",
            "l.name, l.id, l.color".to_owned(),
            "l.name",
        )
    };
    let group = if by_assignee {
        "u.display_name, u.id, u.avatar_asset_id, u.avatar"
    } else {
        "l.name, l.id, l.color"
    };
    let rows: Vec<EstimateRowTuple> =
        sqlx::query_as(&format!(
            r#"SELECT {names},
              SUM(CAST(ep.value AS DOUBLE PRECISION)),
              SUM(CAST(ep.value AS DOUBLE PRECISION)) FILTER (WHERE i.completed_at IS NOT NULL AND i.archived_at IS NULL AND i.is_draft = FALSE),
              SUM(CAST(ep.value AS DOUBLE PRECISION)) FILTER (WHERE i.completed_at IS NULL AND i.archived_at IS NULL AND i.is_draft = FALSE)
           FROM issues i
           JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $1 AND ci.deleted_at IS NULL
           JOIN workspaces w ON w.id = i.workspace_id
           {join}
           LEFT JOIN estimate_points ep ON ep.id = i.estimate_point_id
           WHERE w.slug = $2 AND i.project_id = $3 AND i.deleted_at IS NULL
           GROUP BY {group}
           ORDER BY {order}"#
        ))
        .bind(cycle_id)
        .bind(slug)
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(
            |(display_name, entity_id, image, total, completed, pending)| EstimateRow {
                display_name,
                entity_id,
                image,
                total,
                completed,
                pending,
            },
        )
        .collect())
}

/// Bridges to move (`:435-443`): this cycle's open-group bridges —
/// completed, cancelled, draft and archived issues stay.
pub struct MovableBridge {
    pub bridge_id: Uuid,
    pub issue_id: Uuid,
}

pub async fn movable_bridges(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
) -> Result<Vec<MovableBridge>, Denial> {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT ci.id, ci.issue_id FROM cycle_issues ci
           JOIN issues i ON i.id = ci.issue_id
           JOIN states s ON s.id = i.state_id
           JOIN workspaces w ON w.id = ci.workspace_id
           WHERE ci.cycle_id = $1 AND ci.project_id = $2 AND w.slug = $3
             AND ci.deleted_at IS NULL
             AND i.archived_at IS NULL AND i.is_draft = FALSE AND i.deleted_at IS NULL
             AND s."group" IN ('backlog', 'unstarted', 'started', 'review', 'test')
           ORDER BY ci.created_at DESC"#,
    )
    .bind(cycle_id)
    .bind(project_id)
    .bind(slug)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows
        .into_iter()
        .map(|(bridge_id, issue_id)| MovableBridge {
            bridge_id,
            issue_id,
        })
        .collect())
}

/// Burndown range/today bound (`analytics_plot.py:160-166,246,260`):
/// `(queryset.start_date + timedelta(days=x)).date()` and
/// `timezone.now().date()`. The ORM datetimes are UTC-aware
/// (`USE_TZ=True`) and `now()` is UTC, so both read UTC calendar
/// dates. Only the `TruncDate("completed_at")` completion buckets are
/// zone-local (mirrored with `AT TIME ZONE` in the queries below).
pub fn burndown_utc_day(moment: DateTime<Utc>) -> NaiveDate {
    moment.date_naive()
}

/// `burndown_plot(..., plot_type="issues")` (`analytics_plot.py:123-265`)
/// over the OLD cycle: per-day pending counts from the start/end date
/// range, future days `None`. `total` is the annotated `total_issues`
/// (archived/draft-guarded); the per-day completed counts run over the
/// same live-bridge scope. Range endpoints and "today" are UTC dates
/// ([`burndown_utc_day`]); only the completion buckets truncate in the
/// caller's zone — all UTC in the contract seeds.
#[allow(clippy::too_many_arguments)]
pub async fn burndown_issues_value(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
    total: i64,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    timezone: &Tz,
) -> Result<Value, Denial> {
    let rows: Vec<(Option<NaiveDate>, i64)> = sqlx::query_as(
        r#"SELECT (i.completed_at AT TIME ZONE $4)::date AS day, COUNT(i.id)
           FROM issues i
           JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $1 AND ci.deleted_at IS NULL
           JOIN workspaces w ON w.id = i.workspace_id
           WHERE w.slug = $2 AND i.project_id = $3 AND i.deleted_at IS NULL
           GROUP BY day ORDER BY day"#,
    )
    .bind(cycle_id)
    .bind(slug)
    .bind(project_id)
    .bind(timezone.name())
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let today = burndown_utc_day(Utc::now());
    let completed: Vec<(Option<NaiveDate>, f64)> = rows
        .into_iter()
        .map(|(day, count)| (day, count as f64))
        .collect();
    Ok(Value::Object(burndown_chart(
        total,
        start.map(burndown_utc_day),
        end.map(burndown_utc_day),
        &completed,
        today,
    )))
}

/// `burndown_plot(..., plot_type="points")`: same shape over estimate
/// sums; the total is the float point sum (an int `0` when there are no
/// estimates — `sum([])`).
pub async fn burndown_points_value(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    timezone: &Tz,
) -> Result<Value, Denial> {
    let total: Option<f64> = sqlx::query_scalar(
        r#"SELECT SUM(CAST(ep.value AS DOUBLE PRECISION))
           FROM issues i
           JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $1 AND ci.deleted_at IS NULL
           JOIN workspaces w ON w.id = i.workspace_id
           LEFT JOIN estimate_points ep ON ep.id = i.estimate_point_id
           WHERE w.slug = $2 AND i.project_id = $3 AND i.deleted_at IS NULL
             AND ep.id IS NOT NULL"#,
    )
    .bind(cycle_id)
    .bind(slug)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?
    .unwrap_or(None);
    let rows: Vec<(Option<NaiveDate>, Option<f64>)> = sqlx::query_as(
        r#"SELECT (i.completed_at AT TIME ZONE $4)::date AS day, SUM(CAST(ep.value AS DOUBLE PRECISION))
           FROM issues i
           JOIN cycle_issues ci ON ci.issue_id = i.id AND ci.cycle_id = $1 AND ci.deleted_at IS NULL
           JOIN workspaces w ON w.id = i.workspace_id
           LEFT JOIN estimate_points ep ON ep.id = i.estimate_point_id
           WHERE w.slug = $2 AND i.project_id = $3 AND i.deleted_at IS NULL
             AND ep.id IS NOT NULL
           GROUP BY day ORDER BY day"#,
    )
    .bind(cycle_id)
    .bind(slug)
    .bind(project_id)
    .bind(timezone.name())
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let today = burndown_utc_day(Utc::now());
    let total_value = match total {
        Some(sum) => Value::from(sum),
        // `sum([])` is int `0`, not `0.0` — the JSON shape differs.
        None => Value::from(0),
    };
    let completed: Vec<(Option<NaiveDate>, f64)> = rows
        .into_iter()
        .map(|(day, sum)| (day, sum.unwrap_or(0.0)))
        .collect();
    Ok(Value::Object(burndown_chart_float(
        total_value,
        start.map(burndown_utc_day),
        end.map(burndown_utc_day),
        &completed,
        today,
    )))
}

/// Pure date-range chart core shared by both plot types: every date in
/// `[start, end]` maps to `total - completed-on-or-before`, future
/// dates map to `None`; a missing bound (or an inverted range) yields
/// `{}`. Key order is ascending date order.
pub fn burndown_chart(
    total: i64,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    completed_by_day: &[(Option<NaiveDate>, f64)],
    today: NaiveDate,
) -> Map<String, Value> {
    let mut chart = Map::new();
    let (Some(start), Some(end)) = (start, end) else {
        return chart;
    };
    // `(end.date() - start.date()).days + 1` dates (`analytics_plot.py`);
    // an inverted range yields zero dates, like `range(negative)`.
    let days = (end - start).num_days();
    if days < 0 {
        return chart;
    }
    for offset in 0..=days {
        let day = start + chrono::Days::new(offset as u64);
        let completed: f64 = completed_by_day
            .iter()
            .filter(|(date, _)| date.map(|date| date <= day).unwrap_or(false))
            .map(|(_, count)| count)
            .sum();
        let pending = total as f64 - completed;
        chart.insert(
            day.to_string(),
            if day > today {
                Value::Null
            } else {
                Value::from(pending as i64)
            },
        );
    }
    chart
}

/// Float-total variant of [`burndown_chart`] (the points branch).
pub fn burndown_chart_float(
    total: Value,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    completed_by_day: &[(Option<NaiveDate>, f64)],
    today: NaiveDate,
) -> Map<String, Value> {
    let mut chart = Map::new();
    let (Some(start), Some(end)) = (start, end) else {
        return chart;
    };
    let total_float = total.as_f64().unwrap_or(0.0);
    let days = (end - start).num_days();
    if days < 0 {
        return chart;
    }
    for offset in 0..=days {
        let day = start + chrono::Days::new(offset as u64);
        let completed: f64 = completed_by_day
            .iter()
            .filter(|(date, _)| date.map(|date| date <= day).unwrap_or(false))
            .map(|(_, count)| count)
            .sum();
        chart.insert(
            day.to_string(),
            if day > today {
                Value::Null
            } else {
                Value::from(total_float - completed)
            },
        );
    }
    chart
}

/// Snapshot row serializers (`:219-229`, `:354-363`): key order is the
/// dict-insertion order Python writes.
pub fn distribution_row_json(row: &DistributionRow, assignee: bool) -> Value {
    let mut map = Map::with_capacity(6);
    if assignee {
        map.insert(
            "display_name".to_owned(),
            row.display_name
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "assignee_id".to_owned(),
            row.entity_id
                .map(|id| Value::String(id.to_string()))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "avatar_url".to_owned(),
            row.image.clone().map(Value::String).unwrap_or(Value::Null),
        );
    } else {
        map.insert(
            "label_name".to_owned(),
            row.display_name
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "color".to_owned(),
            row.image.clone().map(Value::String).unwrap_or(Value::Null),
        );
        map.insert(
            "label_id".to_owned(),
            row.entity_id
                .map(|id| Value::String(id.to_string()))
                .unwrap_or(Value::Null),
        );
    }
    map.insert("total_issues".to_owned(), Value::from(row.total));
    map.insert("completed_issues".to_owned(), Value::from(row.completed));
    map.insert("pending_issues".to_owned(), Value::from(row.pending));
    Value::Object(map)
}

/// Estimate row serializers (`:214-222`, `:264-277`).
pub fn estimate_row_json(row: &EstimateRow, assignee: bool) -> Value {
    let mut map = Map::with_capacity(6);
    if assignee {
        map.insert(
            "display_name".to_owned(),
            row.display_name
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "assignee_id".to_owned(),
            row.entity_id
                .map(|id| Value::String(id.to_string()))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "avatar_url".to_owned(),
            row.image.clone().map(Value::String).unwrap_or(Value::Null),
        );
    } else {
        map.insert(
            "label_name".to_owned(),
            row.display_name
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "color".to_owned(),
            row.image.clone().map(Value::String).unwrap_or(Value::Null),
        );
        map.insert(
            "label_id".to_owned(),
            row.entity_id
                .map(|id| Value::String(id.to_string()))
                .unwrap_or(Value::Null),
        );
    }
    map.insert(
        "total_estimates".to_owned(),
        row.total.map(Value::from).unwrap_or(Value::Null),
    );
    map.insert(
        "completed_estimates".to_owned(),
        row.completed.map(Value::from).unwrap_or(Value::Null),
    );
    map.insert(
        "pending_estimates".to_owned(),
        row.pending.map(Value::from).unwrap_or(Value::Null),
    );
    Value::Object(map)
}

/// The progress snapshot (`:411-431`): top-level keys in write order,
/// `estimate_distribution` `{}` when the project has no points estimate.
#[allow(clippy::too_many_arguments)]
pub fn build_snapshot(
    counts: &OldCycleCounts,
    labels: &[DistributionRow],
    assignees: &[DistributionRow],
    chart: &Value,
    use_estimates: bool,
    label_estimates: &[EstimateRow],
    assignee_estimates: &[EstimateRow],
    points_chart: &Value,
) -> Value {
    let mut distribution = Map::with_capacity(3);
    distribution.insert(
        "labels".to_owned(),
        Value::Array(
            labels
                .iter()
                .map(|row| distribution_row_json(row, false))
                .collect(),
        ),
    );
    distribution.insert(
        "assignees".to_owned(),
        Value::Array(
            assignees
                .iter()
                .map(|row| distribution_row_json(row, true))
                .collect(),
        ),
    );
    distribution.insert("completion_chart".to_owned(), chart.clone());
    let estimate_distribution = if use_estimates {
        let mut estimate = Map::with_capacity(3);
        estimate.insert(
            "labels".to_owned(),
            Value::Array(
                label_estimates
                    .iter()
                    .map(|row| estimate_row_json(row, false))
                    .collect(),
            ),
        );
        estimate.insert(
            "assignees".to_owned(),
            Value::Array(
                assignee_estimates
                    .iter()
                    .map(|row| estimate_row_json(row, true))
                    .collect(),
            ),
        );
        estimate.insert("completion_chart".to_owned(), points_chart.clone());
        Value::Object(estimate)
    } else {
        Value::Object(Map::new())
    };
    let mut snapshot = Map::with_capacity(8);
    snapshot.insert("total_issues".to_owned(), Value::from(counts.total));
    snapshot.insert("completed_issues".to_owned(), Value::from(counts.completed));
    snapshot.insert("cancelled_issues".to_owned(), Value::from(counts.cancelled));
    snapshot.insert("started_issues".to_owned(), Value::from(counts.started));
    snapshot.insert("unstarted_issues".to_owned(), Value::from(counts.unstarted));
    snapshot.insert("backlog_issues".to_owned(), Value::from(counts.backlog));
    snapshot.insert("distribution".to_owned(), Value::Object(distribution));
    snapshot.insert("estimate_distribution".to_owned(), estimate_distribution);
    Value::Object(snapshot)
}

// ---------------------------------------------------------------------------
// User-properties (`CycleUserPropertiesEndpoint`, `base.py:624-657`)
// ---------------------------------------------------------------------------

/// DRF field order of `CycleUserPropertiesSerializer` (`__all__`):
/// the declared `id` first, then concrete fields in `_meta` order,
/// then forward relations. Verified against the live serializer
/// (`CycleUserPropertiesSerializer(CycleUserProperties()).data` keys).
pub const USER_PROPERTIES_FIELDS: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "filters",
    "display_filters",
    "display_properties",
    "rich_filters",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "cycle",
    "user",
];

/// `CycleUserProperties` defaults (`db/models/cycle.py:16-56`).
pub fn default_filters() -> Value {
    serde_json::json!({
        "priority": null, "state": null, "state_group": null,
        "assignees": null, "created_by": null, "labels": null,
        "start_date": null, "target_date": null, "subscriber": null,
    })
}

/// `get_default_display_filters` (`cycle.py:30-41`).
pub fn default_display_filters() -> Value {
    serde_json::json!({
        "group_by": null, "order_by": "-created_at", "type": null,
        "sub_issue": true, "show_empty_groups": true, "layout": "list",
        "calendar_date_range": "",
    })
}

/// `get_default_display_properties` (`cycle.py:42-56`).
pub fn default_display_properties() -> Value {
    serde_json::json!({
        "assignee": true, "attachment_count": true, "created_on": true,
        "due_date": true, "estimate": true, "key": true, "labels": true,
        "link": true, "priority": true, "start_date": true, "state": true,
        "sub_issue_count": true, "updated_on": true,
    })
}

/// One `cycle_user_properties` row for rendering.
#[allow(clippy::too_many_arguments)]
pub struct UserPropertiesRow {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub filters: Value,
    pub display_filters: Value,
    pub display_properties: Value,
    pub rich_filters: Value,
    pub created_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub cycle_id: Uuid,
    pub user_id: Uuid,
}

/// Render a row exactly like `CycleUserPropertiesSerializer`: FKs as
/// pk strings, datetimes in the caller's zone
/// (`TimezoneMixin.initial`), JSON fields raw, key order per
/// [`USER_PROPERTIES_FIELDS`].
pub fn render_user_properties(row: &UserPropertiesRow, timezone: &Tz) -> String {
    let datetime = |value: &DateTime<Utc>| -> Value {
        Value::String(crate::serializer::render_datetime_in(value, timezone))
    };
    let uuid_or_null = |id: &Option<Uuid>| -> Value {
        id.map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null)
    };
    let mut map = Map::with_capacity(USER_PROPERTIES_FIELDS.len());
    map.insert("id".to_owned(), Value::String(row.id.to_string()));
    map.insert("created_at".to_owned(), datetime(&row.created_at));
    map.insert("updated_at".to_owned(), datetime(&row.updated_at));
    map.insert(
        "deleted_at".to_owned(),
        row.deleted_at.as_ref().map(datetime).unwrap_or(Value::Null),
    );
    map.insert("filters".to_owned(), row.filters.clone());
    map.insert("display_filters".to_owned(), row.display_filters.clone());
    map.insert(
        "display_properties".to_owned(),
        row.display_properties.clone(),
    );
    map.insert("rich_filters".to_owned(), row.rich_filters.clone());
    map.insert("created_by".to_owned(), uuid_or_null(&row.created_by));
    map.insert("updated_by".to_owned(), uuid_or_null(&row.updated_by));
    map.insert(
        "project".to_owned(),
        Value::String(row.project_id.to_string()),
    );
    map.insert(
        "workspace".to_owned(),
        Value::String(row.workspace_id.to_string()),
    );
    map.insert("cycle".to_owned(), Value::String(row.cycle_id.to_string()));
    map.insert("user".to_owned(), Value::String(row.user_id.to_string()));
    serde_json::to_string(&Value::Object(map)).expect("serializable row")
}

const USER_PROPERTIES_SELECT: &str = r#"SELECT cup.id, cup.created_at, cup.updated_at, cup.deleted_at,
    cup.filters, cup.display_filters, cup.display_properties, cup.rich_filters,
    cup.created_by_id, cup.updated_by_id, cup.project_id, cup.workspace_id, cup.cycle_id, cup.user_id
    FROM cycle_user_properties cup
    JOIN workspaces w ON w.id = cup.workspace_id"#;

pub async fn fetch_user_properties(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    cycle_id: &Uuid,
    user_id: &Uuid,
) -> Result<Option<UserPropertiesRow>, Denial> {
    let row: Option<UserPropertiesRowTuple> = sqlx::query_as(&format!(
        "{USER_PROPERTIES_SELECT} WHERE w.slug = $1 AND cup.project_id = $2 AND cup.cycle_id = $3 AND cup.user_id = $4 AND cup.deleted_at IS NULL"
    ))
    .bind(slug)
    .bind(project_id)
    .bind(cycle_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(
        |(
            id,
            created_at,
            updated_at,
            deleted_at,
            filters,
            display_filters,
            display_properties,
            rich_filters,
            created_by,
            updated_by,
            project_id,
            workspace_id,
            cycle_id,
            user_id,
        )| {
            UserPropertiesRow {
                id,
                created_at,
                updated_at,
                deleted_at,
                filters,
                display_filters,
                display_properties,
                rich_filters,
                created_by,
                updated_by,
                project_id,
                workspace_id,
                cycle_id,
                user_id,
            }
        },
    ))
}

/// Shared prelude for both user-properties methods: authN, tenant,
/// GUEST gate, cycle segment. There is deliberately NO cycle existence
/// check — the view filters by raw `cycle_id`, so GET creates (and
/// PATCH 404s) for unknown cycles exactly like Python.
pub struct UserPropertiesContext {
    pub pool: PgPool,
    pub tenant: Tenant,
    pub actor: Actor,
    pub cycle_id: Uuid,
    pub slug: String,
}

pub async fn user_properties_context(
    state: &AppState,
    slug: String,
    project_raw: String,
    cycle_raw: String,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: &str,
) -> Result<UserPropertiesContext, Response> {
    let actor = actor(state, extension)
        .await
        .map_err(|denial| denial.into_response())?;
    let pool = pool_of(state)
        .map_err(|denial| denial.into_response())?
        .clone();
    // The `workspace__slug` lookup: an unknown slug answers the
    // `ObjectDoesNotExist` branch, exactly like the view filters.
    let workspace: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT w.id FROM workspaces w WHERE w.slug = $1 AND w.deleted_at IS NULL"#,
    )
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError.into_response())?;
    if workspace.is_none() {
        return Err(Denial::NotFound.into_response());
    }
    let cycle_id = parse_id(&cycle_raw)?;
    let tenant = resolve_tenant(&pool, &slug, &project_raw)
        .await
        .map_err(|denial| denial.into_response())?;
    let membership = load_membership(&pool, &slug, &tenant.project_id, &actor.id)
        .await
        .map_err(|denial| denial.into_response())?;
    let gate =
        gate_for_path(method, USER_PROPERTIES_PATH).map_err(|denial| denial.into_response())?;
    check_gate(&gate, &slug, &membership).map_err(|denial| denial.into_response())?;
    Ok(UserPropertiesContext {
        pool,
        tenant,
        actor,
        cycle_id,
        slug,
    })
}

/// `GET cycles/<cycle_id>/user-properties/` (`base.py:646-655`):
/// `get_or_create` — the first read inserts the JSON defaults.
pub async fn get_user_properties(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    let context =
        match user_properties_context(&state, slug, project_raw, cycle_raw, extension, "GET").await
        {
            Ok(context) => context,
            Err(response) => return response,
        };
    let pool = &context.pool;
    if let Some(row) = match fetch_user_properties(
        pool,
        &context.slug,
        &context.tenant.project_id,
        &context.cycle_id,
        &context.actor.id,
    )
    .await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    } {
        return raw_json_response(render_user_properties(&row, &context.actor.timezone));
    }
    // `get_or_create`: model defaults for the JSON columns,
    // `created_by` from the request user (`BaseModel.save` + crum),
    // `workspace` from the project (`ProjectBaseModel.save`).
    let now = Utc::now();
    let row = UserPropertiesRow {
        id: Uuid::new_v4(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
        filters: default_filters(),
        display_filters: default_display_filters(),
        display_properties: default_display_properties(),
        rich_filters: Value::Object(Map::new()),
        created_by: Some(context.actor.id),
        updated_by: None,
        project_id: context.tenant.project_id,
        workspace_id: context.tenant.workspace_id,
        cycle_id: context.cycle_id,
        user_id: context.actor.id,
    };
    if let Err(denial) = insert_user_properties(pool, &row).await {
        return denial.into_response();
    }
    raw_json_response(render_user_properties(&row, &context.actor.timezone))
}

async fn insert_user_properties(pool: &PgPool, row: &UserPropertiesRow) -> Result<(), Denial> {
    sqlx::query(
        r#"INSERT INTO cycle_user_properties
           (id, created_at, updated_at, deleted_at, filters, display_filters, display_properties, rich_filters,
            created_by_id, updated_by_id, project_id, workspace_id, cycle_id, user_id)
           VALUES ($1, $2, $3, $4, $5::jsonb, $6::jsonb, $7::jsonb, $8::jsonb, $9, $10, $11, $12, $13, $14)"#,
    )
    .bind(row.id)
    .bind(row.created_at)
    .bind(row.updated_at)
    .bind(row.deleted_at)
    .bind(serde_json::to_string(&row.filters).map_err(|_| Denial::ServerError)?)
    .bind(serde_json::to_string(&row.display_filters).map_err(|_| Denial::ServerError)?)
    .bind(serde_json::to_string(&row.display_properties).map_err(|_| Denial::ServerError)?)
    .bind(serde_json::to_string(&row.rich_filters).map_err(|_| Denial::ServerError)?)
    .bind(row.created_by)
    .bind(row.updated_by)
    .bind(row.project_id)
    .bind(row.workspace_id)
    .bind(row.cycle_id)
    .bind(row.user_id)
    .execute(pool)
    .await
    .map_err(|error| {
        // FK miss (unknown cycle) or unique violation: DRF's
        // `IntegrityError` branch (the D-32 intake twin).
        if let sqlx::Error::Database(db_error) = &error {
            if db_error.code().is_some() {
                return Denial::InvalidPayload;
            }
        }
        Denial::ServerError
    })?;
    Ok(())
}

/// `PATCH cycles/<cycle_id>/user-properties/` (`base.py:625-644`):
/// `.get` (missing row → 404, no auto-create —
/// BUG-userprops-patch-no-create), per-key fallback to the current
/// value, full-row serialize, HTTP 201 (BUG-userprops-patch-201).
pub async fn patch_user_properties(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> Response {
    let context =
        match user_properties_context(&state, slug, project_raw, cycle_raw, extension, "PATCH")
            .await
        {
            Ok(context) => context,
            Err(response) => return response,
        };
    let pool = &context.pool;
    let fetched = match fetch_user_properties(
        pool,
        &context.slug,
        &context.tenant.project_id,
        &context.cycle_id,
        &context.actor.id,
    )
    .await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(mut row) = fetched else {
        return Denial::NotFound.into_response();
    };
    let data = match parse_object_body(&body) {
        Ok(data) => data,
        Err(denial) => return denial.into_response(),
    };
    // `request.data.get("filters", cycle_properties.filters)` per key:
    // a present key wins even when null; an absent key keeps the row.
    if let Some(filters) = data.get("filters") {
        row.filters = filters.clone();
    }
    if let Some(rich_filters) = data.get("rich_filters") {
        row.rich_filters = rich_filters.clone();
    }
    if let Some(display_filters) = data.get("display_filters") {
        row.display_filters = display_filters.clone();
    }
    if let Some(display_properties) = data.get("display_properties") {
        row.display_properties = display_properties.clone();
    }
    // `save()`: full-row write stamping `updated_at`/`updated_by`
    // (`BaseModel.save` on an existing row).
    row.updated_at = Utc::now();
    row.updated_by = Some(context.actor.id);
    if let Err(denial) = update_user_properties(pool, &row).await {
        return denial.into_response();
    }
    raw_json_status(
        StatusCode::CREATED,
        render_user_properties(&row, &context.actor.timezone),
    )
}

async fn update_user_properties(pool: &PgPool, row: &UserPropertiesRow) -> Result<(), Denial> {
    sqlx::query(
        r#"UPDATE cycle_user_properties
           SET filters = $1::jsonb, rich_filters = $2::jsonb, display_filters = $3::jsonb,
               display_properties = $4::jsonb, updated_at = $5, updated_by_id = $6
           WHERE id = $7"#,
    )
    .bind(serde_json::to_string(&row.filters).map_err(|_| Denial::ServerError)?)
    .bind(serde_json::to_string(&row.rich_filters).map_err(|_| Denial::ServerError)?)
    .bind(serde_json::to_string(&row.display_filters).map_err(|_| Denial::ServerError)?)
    .bind(serde_json::to_string(&row.display_properties).map_err(|_| Denial::ServerError)?)
    .bind(row.updated_at)
    .bind(row.updated_by)
    .bind(row.id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the three owned paths. Unowned methods fall through to
/// Django through the edge fallback (never a Rust 405); sibling
/// handler issues (PIDASHCONV-321/323/377/410) merge their own routers
/// into `super::routes` — merges keep both sides.
pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/cycles/date-check/",
            post(date_check)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/transfer-issues/",
            post(transfer_issues)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/user-properties/",
            get(get_user_properties)
                .patch(patch_user_properties)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn membership_with_role(role: Option<i16>) -> Membership {
        Membership {
            project_role: role,
            workspace_member: role.is_some(),
            workspace_admin: role == Some(20),
        }
    }

    #[test]
    fn gates_resolve_for_all_three_paths() {
        // The F-C27-07 table is the single source of truth: date-check
        // + transfer are MEMBER, user-properties PATCH + GET are GUEST.
        let date_check = gate_for_path("POST", DATE_CHECK_PATH).expect("date-check gate");
        assert_eq!(date_check, Gate::Project { roles: &[20, 15] });
        let transfer = gate_for_path("POST", TRANSFER_PATH).expect("transfer gate");
        assert_eq!(transfer, Gate::Project { roles: &[20, 15] });
        for method in ["GET", "PATCH"] {
            let userprops =
                gate_for_path(method, USER_PROPERTIES_PATH).expect("user-properties gate");
            assert_eq!(
                userprops,
                Gate::Project {
                    roles: &[20, 15, 5]
                }
            );
        }
        assert!(gate_for_path("POST", DATE_CHECK_PATH).is_ok());
        assert!(gate_for_path("DELETE", TRANSFER_PATH).is_err());
    }

    #[test]
    fn matrix_matches_django_allow_deny() {
        // Replays the F-C27-07 rows for these three routes: ADMIN and
        // MEMBER run everywhere, GUEST runs only user-properties, an
        // outsider (no membership) runs nowhere. Deleting a gate turns
        // this red — the same removal proof the gates module uses.
        let cases: &[(&str, bool, bool, bool, bool)] = &[
            (DATE_CHECK_PATH, true, true, false, false),
            (TRANSFER_PATH, true, true, false, false),
            (USER_PROPERTIES_PATH, true, true, true, false),
        ];
        for (path, admin, member, guest, outsider) in cases {
            let method = if path == &DATE_CHECK_PATH || path == &TRANSFER_PATH {
                "POST"
            } else {
                "GET"
            };
            let gate = gate_for_path(method, path).expect("gate");
            for (role, expected) in [(Some(20), *admin), (Some(15), *member), (Some(5), *guest)] {
                let outcome = check_gate(&gate, "acme", &membership_with_role(role));
                assert_eq!(outcome.is_ok(), expected, "{method} {path} role {role:?}");
            }
            assert_eq!(
                check_gate(&gate, "acme", &membership_with_role(None)).is_ok(),
                *outsider,
                "{method} {path} outsider"
            );
        }
    }

    #[test]
    fn workspace_admin_bypass_matches_decorator() {
        // Active project member with a non-listed role + workspace ADMIN
        // passes (`app/permissions/base.py:72-86`).
        let gate = gate_for_path("POST", TRANSFER_PATH).expect("gate");
        let membership = Membership {
            project_role: Some(5),
            workspace_member: true,
            workspace_admin: true,
        };
        assert!(check_gate(&gate, "acme", &membership).is_ok());
        let membership = Membership {
            project_role: Some(5),
            workspace_member: true,
            workspace_admin: false,
        };
        assert!(check_gate(&gate, "acme", &membership).is_err());
    }

    #[test]
    fn date_check_bodies_match_fixture() {
        // F-C27-10 (`misc.json`): missing → 400, overlap → 200 with the
        // error key FIRST (not a 4xx), clear → 200 `{"status": true}`.
        assert_eq!(
            DATE_CHECK_REQUIRED_BODY,
            r#"{"error":"Start date and end date both are required"}"#
        );
        assert_eq!(
            DATE_CHECK_CONFLICT_BODY,
            "{\"error\":\"You have a cycle already on the given dates, if you want to create a draft cycle you can do that by removing dates\",\"status\":false}"
        );
        assert_eq!(DATE_CHECK_CLEAR_BODY, r#"{"status":true}"#);
    }

    #[test]
    fn transfer_bodies_match_fixture() {
        // F-C27-06 (`transfer.json`): required/refusal/success shapes.
        assert_eq!(
            pidash_services::app_cycles::queries::NEW_CYCLE_ID_REQUIRED_BODY,
            r#"{"error":"New Cycle Id is required"}"#
        );
        assert_eq!(
            pidash_services::app_cycles::queries::DESTINATION_COMPLETED_ERROR,
            "The cycle where the issues are transferred is already completed"
        );
        assert_eq!(
            pidash_services::app_cycles::queries::SOURCE_CYCLE_MISSING_ERROR,
            "Source cycle not found"
        );
        assert_eq!(
            pidash_services::app_cycles::queries::TRANSFER_SUCCESS_BODY,
            r#"{"message":"Success"}"#
        );
        assert_eq!(TRANSFER_ACTIVITY_TYPE, "cycle.activity.created");
        assert_eq!(
            pidash_services::app_cycles::queries::TRANSFER_ACTIVITY_REQUESTED_DATA,
            "{\"cycles_list\": []}"
        );
    }

    #[test]
    fn python_dumps_matches_cpython_separators() {
        // The transfer `requested_data`/`current_instance` payloads use
        // `json.dumps` defaults (`, ` / `: `).
        let requested: Value = serde_json::from_str(r#"{"cycles_list": []}"#).expect("json");
        assert_eq!(
            python_dumps(&requested),
            pidash_services::app_cycles::queries::TRANSFER_ACTIVITY_REQUESTED_DATA
        );
        let mut row = Map::new();
        row.insert("old_cycle_id".to_owned(), Value::String("a".to_owned()));
        row.insert("new_cycle_id".to_owned(), Value::String("b".to_owned()));
        row.insert("issue_id".to_owned(), Value::String("c".to_owned()));
        let mut current = Map::new();
        current.insert(
            "updated_cycle_issues".to_owned(),
            Value::Array(vec![Value::Object(row)]),
        );
        current.insert("created_cycle_issues".to_owned(), Value::Array(Vec::new()));
        assert_eq!(
            python_dumps(&Value::Object(current)),
            r#"{"updated_cycle_issues": [{"old_cycle_id": "a", "new_cycle_id": "b", "issue_id": "c"}], "created_cycle_issues": []}"#
        );
    }

    #[test]
    fn convert_start_end_match_python_math() {
        // UTC project: start is 00:00:01Z, end is 23:59:00Z
        // (`timezone_converter.py`).
        let start = convert_start_date("2024-03-10", "UTC").expect("start");
        assert_eq!(start, Utc.with_ymd_and_hms(2024, 3, 10, 0, 0, 1).unwrap());
        let end = convert_end_date("2024-03-10", "UTC").expect("end");
        assert_eq!(end, Utc.with_ymd_and_hms(2024, 3, 10, 23, 59, 0).unwrap());
        // Non-UTC project shifts by the zone offset.
        let start = convert_start_date("2024-01-15", "America/New_York").expect("start");
        assert_eq!(start, Utc.with_ymd_and_hms(2024, 1, 15, 5, 0, 1).unwrap());
        // Garbage fails as a 500 (ValueError → generic branch), never 400.
        assert!(matches!(
            convert_start_date("not-a-date", "UTC"),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            convert_end_date("2024-13-99", "UTC"),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            convert_start_date("2024-01-01", "No/Such_Zone"),
            Err(Denial::ServerError)
        ));
    }

    #[test]
    fn py_truthy_matches_django_falsiness() {
        assert!(!py_truthy(None));
        assert!(!py_truthy(Some(&Value::Null)));
        assert!(!py_truthy(Some(&Value::Bool(false))));
        assert!(!py_truthy(Some(&Value::String(String::new()))));
        assert!(!py_truthy(Some(&serde_json::json!(0))));
        assert!(!py_truthy(Some(&serde_json::json!([]))));
        assert!(!py_truthy(Some(&serde_json::json!({}))));
        assert!(py_truthy(Some(&Value::String("2024-01-01".to_owned()))));
        assert!(py_truthy(Some(&serde_json::json!(1))));
        // `str()` mirrors Python for scalars.
        assert_eq!(py_str(Some(&Value::String("x".to_owned()))), "x");
        assert_eq!(py_str(Some(&Value::Bool(true))), "True");
        assert_eq!(py_str(Some(&serde_json::json!(0))), "0");
    }

    #[test]
    fn burndown_chart_matches_python_shape() {
        // No bounds → `{}` (cycles without dates).
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        assert!(burndown_chart(3, None, None, &[], today).is_empty());
        // Past range: total minus completed-on-or-before per day.
        let start = NaiveDate::from_ymd_opt(2024, 6, 10).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 6, 12).unwrap();
        let completed = vec![(Some(NaiveDate::from_ymd_opt(2024, 6, 11).unwrap()), 2.0)];
        let chart = burndown_chart(5, Some(start), Some(end), &completed, today);
        let keys: Vec<&String> = chart.keys().collect();
        assert_eq!(keys, &["2024-06-10", "2024-06-11", "2024-06-12"]);
        assert_eq!(chart["2024-06-10"], Value::from(5));
        assert_eq!(chart["2024-06-11"], Value::from(3));
        assert_eq!(chart["2024-06-12"], Value::from(3));
        // NULL completion dates never accumulate; future days are null.
        let completed = vec![(None, 99.0)];
        let chart = burndown_chart(5, Some(start), Some(end), &completed, start);
        assert_eq!(chart["2024-06-10"], Value::from(5));
        assert_eq!(chart["2024-06-11"], Value::Null);
        assert_eq!(chart["2024-06-12"], Value::Null);
    }

    #[test]
    fn new_cycle_id_classification_matches_orm_coercion() {
        // `base.py:597-603` + `UUIDField.to_python`, probed live in
        // `manage.py shell`: falsy → required; ints/`True` coerce to a
        // `UUID(int=v)` lookup id (unknown → the ported 500); floats,
        // negative ints, lists, dicts and non-UUID strings →
        // `ValidationError` → 400.
        assert!(matches!(classify_new_cycle_id(None), NewCycleId::Required));
        assert!(matches!(
            classify_new_cycle_id(Some(&Value::Bool(false))),
            NewCycleId::Required
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&serde_json::json!(0))),
            NewCycleId::Required
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&serde_json::json!([]))),
            NewCycleId::Required
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&serde_json::json!({}))),
            NewCycleId::Required
        ));
        let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        assert!(matches!(
            classify_new_cycle_id(Some(&Value::String(id.to_string()))),
            NewCycleId::Id(got) if got == id
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&serde_json::json!(5))),
            NewCycleId::Id(got) if got == Uuid::from_u128(5)
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&Value::Bool(true))),
            NewCycleId::Id(got) if got == Uuid::from_u128(1)
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&serde_json::json!(5.0))),
            NewCycleId::InvalidDetail
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&serde_json::json!(-5))),
            NewCycleId::InvalidDetail
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&serde_json::json!([1]))),
            NewCycleId::InvalidDetail
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&serde_json::json!({"a": 1}))),
            NewCycleId::InvalidDetail
        ));
        assert!(matches!(
            classify_new_cycle_id(Some(&Value::String("not-a-uuid".to_owned()))),
            NewCycleId::InvalidDetail
        ));
    }

    #[test]
    fn date_check_cycle_id_coercion_matches_exclude() {
        // Regression (`base.py:525` + `UUIDField.to_python`, probed live
        // in `manage.py shell`): no `if not` guard on `cycle_id`, so
        // ints/bools — including falsy `0`/`False` — coerce to
        // `UUID(int=v)` and miss every pk (the normal 200); floats,
        // negative ints, lists, dicts and non-UUID strings raise
        // `ValidationError` → 400.
        assert!(matches!(classify_date_check_cycle_id(None), Ok(None)));
        assert!(matches!(
            classify_date_check_cycle_id(Some(&Value::Null)),
            Ok(None)
        ));
        assert!(matches!(
            classify_date_check_cycle_id(Some(&serde_json::json!(5))),
            Ok(Some(got)) if got == Uuid::from_u128(5)
        ));
        assert!(matches!(
            classify_date_check_cycle_id(Some(&Value::Bool(true))),
            Ok(Some(got)) if got == Uuid::from_u128(1)
        ));
        // Falsy but unguarded: `0`/`False` still coerce to `UUID(int=0)`.
        assert!(matches!(
            classify_date_check_cycle_id(Some(&serde_json::json!(0))),
            Ok(Some(got)) if got == Uuid::from_u128(0)
        ));
        assert!(matches!(
            classify_date_check_cycle_id(Some(&Value::Bool(false))),
            Ok(Some(got)) if got == Uuid::from_u128(0)
        ));
        let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        assert!(matches!(
            classify_date_check_cycle_id(Some(&Value::String(id.to_string()))),
            Ok(Some(got)) if got == id
        ));
        assert!(matches!(
            classify_date_check_cycle_id(Some(&serde_json::json!(5.0))),
            Err(Denial::InvalidDetail)
        ));
        assert!(matches!(
            classify_date_check_cycle_id(Some(&serde_json::json!(-5))),
            Err(Denial::InvalidDetail)
        ));
        assert!(matches!(
            classify_date_check_cycle_id(Some(&serde_json::json!([1]))),
            Err(Denial::InvalidDetail)
        ));
        assert!(matches!(
            classify_date_check_cycle_id(Some(&serde_json::json!({"a": 1}))),
            Err(Denial::InvalidDetail)
        ));
        assert!(matches!(
            classify_date_check_cycle_id(Some(&Value::String("not-a-uuid".to_owned()))),
            Err(Denial::InvalidDetail)
        ));
    }

    #[test]
    fn burndown_bounds_use_utc_dates() {
        // Regression (`analytics_plot.py:160-166`): the range endpoints
        // and "today" are UTC dates, not caller-zone dates.
        // 2025-12-31T23:30Z is already 2026-01-01 in Asia/Kolkata, so
        // the zone-local derivation opens the range a day late.
        let moment = Utc.with_ymd_and_hms(2025, 12, 31, 23, 30, 0).unwrap();
        let kolkata: Tz = "Asia/Kolkata".parse().expect("tz");
        assert_eq!(
            moment.with_timezone(&kolkata).date_naive(),
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
        );
        assert_eq!(
            burndown_utc_day(moment),
            NaiveDate::from_ymd_opt(2025, 12, 31).unwrap()
        );
    }

    #[test]
    fn snapshot_keys_match_python_write_order() {
        // F-C27-06 `progress_snapshot_shape`: top-level + distribution
        // keys in the order `cycle_transfer_issues.py:411-431` writes.
        let counts = OldCycleCounts {
            total: 2,
            completed: 1,
            cancelled: 0,
            started: 0,
            unstarted: 1,
            backlog: 0,
            start_date: None,
            end_date: None,
        };
        let snapshot = build_snapshot(
            &counts,
            &[],
            &[],
            &Value::Object(Map::new()),
            false,
            &[],
            &[],
            &Value::Object(Map::new()),
        );
        let map = snapshot.as_object().expect("object");
        let keys: Vec<&String> = map.keys().collect();
        assert_eq!(
            keys,
            &[
                "total_issues",
                "completed_issues",
                "cancelled_issues",
                "started_issues",
                "unstarted_issues",
                "backlog_issues",
                "distribution",
                "estimate_distribution"
            ]
        );
        let distribution = map["distribution"].as_object().expect("distribution");
        let dist_keys: Vec<&String> = distribution.keys().collect();
        assert_eq!(dist_keys, &["labels", "assignees", "completion_chart"]);
        assert_eq!(map["estimate_distribution"], Value::Object(Map::new()));
        assert_eq!(map["total_issues"], Value::from(2));
    }

    #[test]
    fn distribution_rows_match_python_key_order() {
        // F-C27-06 `ASSIGNEE_ROW_KEYS` / `LABEL_ROW_KEYS` families.
        let row = DistributionRow {
            display_name: Some("Ada".to_owned()),
            entity_id: None,
            image: None,
            total: 2,
            completed: 1,
            pending: 1,
        };
        let rendered = distribution_row_json(&row, true);
        let keys: Vec<&String> = rendered.as_object().expect("object").keys().collect();
        assert_eq!(
            keys,
            &[
                "display_name",
                "assignee_id",
                "avatar_url",
                "total_issues",
                "completed_issues",
                "pending_issues"
            ]
        );
        let rendered = distribution_row_json(&row, false);
        let keys: Vec<&String> = rendered.as_object().expect("object").keys().collect();
        assert_eq!(
            keys,
            &[
                "label_name",
                "color",
                "label_id",
                "total_issues",
                "completed_issues",
                "pending_issues"
            ]
        );
        let estimate = EstimateRow {
            display_name: None,
            entity_id: None,
            image: None,
            total: Some(5.0),
            completed: None,
            pending: Some(5.0),
        };
        let rendered = estimate_row_json(&estimate, true);
        assert_eq!(rendered["total_estimates"], Value::from(5.0));
        assert_eq!(rendered["completed_estimates"], Value::Null);
    }

    #[test]
    fn user_properties_render_matches_serializer_order() {
        // Key order verified against the live DRF serializer; datetimes
        // render in the caller zone with the `Z` rewrite.
        let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let stamp = Utc.with_ymd_and_hms(2024, 5, 1, 12, 0, 0).unwrap();
        let row = UserPropertiesRow {
            id,
            created_at: stamp,
            updated_at: stamp,
            deleted_at: None,
            filters: default_filters(),
            display_filters: default_display_filters(),
            display_properties: default_display_properties(),
            rich_filters: Value::Object(Map::new()),
            created_by: Some(id),
            updated_by: None,
            project_id: id,
            workspace_id: id,
            cycle_id: id,
            user_id: id,
        };
        let rendered: Value =
            serde_json::from_str(&render_user_properties(&row, &Tz::UTC)).expect("json");
        let map = rendered.as_object().expect("object");
        let keys: Vec<&String> = map.keys().collect();
        let want: Vec<String> = USER_PROPERTIES_FIELDS
            .iter()
            .map(|key| key.to_string())
            .collect();
        assert_eq!(keys, want.iter().collect::<Vec<&String>>());
        assert_eq!(
            map["created_at"],
            Value::String("2024-05-01T12:00:00Z".to_owned())
        );
        assert_eq!(map["deleted_at"], Value::Null);
        assert_eq!(map["updated_by"], Value::Null);
        assert_eq!(map["id"], Value::String(id.to_string()));
        assert_eq!(map["rich_filters"], Value::Object(Map::new()));
        // Defaults match `db/models/cycle.py:16-56`.
        assert_eq!(map["filters"]["priority"], Value::Null);
        assert_eq!(
            map["display_filters"]["order_by"],
            Value::String("-created_at".to_owned())
        );
        assert_eq!(map["display_properties"]["key"], Value::Bool(true));
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
