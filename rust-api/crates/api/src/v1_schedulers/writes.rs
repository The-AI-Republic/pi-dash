//! v1 scheduler writes (PDASHOSS01-278, PDASHOSS01-318).
//!
//! Ports the write endpoints `apps/api/pi_dash/api/views/scheduler.py`
//! gained in PDASHOSS01-278 and PDASHOSS01-318, with their routes in
//! `api/urls/scheduler.py`:
//!
//! * `PATCH workspaces/<slug>/projects/<project_id>/schedulers/<uuid>/binding/`
//!   (`ProjectSchedulerBindingAPIEndpoint`): edit the install —
//!   `extra_context`, `enabled`, the cadence, `outcome_mode`, `pod`.
//! * `POST .../schedulers/<uuid>/run-now/` (`ProjectSchedulerRunNowAPIEndpoint`):
//!   make the install due so the scanner fires it on its next tick.
//! * `PATCH workspaces/<slug>/schedulers/<uuid>/`
//!   (`WorkspaceSchedulerDetailAPIEndpoint`): edit a definition's `prompt`,
//!   `name`, `description`, `color`, `is_enabled`; never a built-in one.
//! * `POST workspaces/<slug>/schedulers/` (`WorkspaceSchedulerListAPIEndpoint`,
//!   -318): create a definition; a built-in slug is refused.
//! * `DELETE workspaces/<slug>/schedulers/<uuid>/` (-318): soft-delete a
//!   definition and every install of it; never a built-in one.
//! * `POST workspaces/<slug>/projects/<project_id>/schedulers/`
//!   (`ProjectSchedulerListAPIEndpoint`, -318): install a scheduler, named
//!   by id or slug.
//! * `DELETE .../schedulers/<uuid>/binding/` (-318): uninstall.
//!
//! Validation and the write itself are the web app's — Python routes both
//! surfaces through `scheduler/binding_writes.py` and
//! `scheduler/definition_writes.py`; here the binding edit calls
//! [`crate::app_scheduler::handlers_bind::apply_binding_patch`], the
//! install [`crate::app_scheduler::handlers_bind::apply_install`], the
//! uninstall [`crate::app_scheduler::handlers_bind::uninstall_binding`], the
//! definition edit [`crate::app_scheduler::handlers_sched::validate_patch_body`]
//! plus the app's `SCHEDULER_PATCH_SQL`, the create
//! [`crate::app_scheduler::handlers_sched::create_scheduler`] and the delete
//! [`crate::app_scheduler::handlers_sched::soft_delete_scheduler`], so the
//! two surfaces cannot drift.
//!
//! Built-in definitions are told apart by their slug (the catalog the
//! reseed matches on), not by `source`, which reads `builtin` for
//! user-created rows too (PDASHOSS01-290).
//!
//! Request order (Django's, preserved): API-key authentication (401/403),
//! the slug→UUID project rewrite (project routes), the class gate
//! (`ProjectEntityPermission` / `WorkspaceEntityPermission`, unsafe-method
//! branch: role Admin or Member; 403 [`super::perms::CLASS_DENIAL_BODY`]),
//! the time-zone activation (`TimezoneMixin.initial`), the
//! `@allow_permission([ADMIN])` decorator (403
//! [`crate::permissions::PERMISSION_DENIED_BODY`]), the `SCHEDULER_ENABLED`
//! flag, then the view body. Every other method on these paths proxies to
//! Django, which answers its 405s.
//!
//! Each write runs in one transaction holding the row lock
//! (`select_for_update`), so the `If-Match` / `expected_updated_at` check
//! and the write see the same row. Every 200 carries `previous`: the
//! pre-write value of each field written.

use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{patch, MethodRouter};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_db::tasks_ticker::models::scheduler_binding::SchedulerBinding;
use pidash_services::app_scheduler::queries;

use crate::app_scheduler::handlers_bind::{self as app_bind, BindingPatchTarget, InstallTarget};
use crate::app_scheduler::handlers_sched as app_sched;
use crate::state::AppState;

use super::handlers::{render_binding, render_scheduler, BindingRow, SchedulerRow};
use super::{
    activate_timezone, db_error, ensure_feature_enabled, preamble, rewrite_project_id, Denial,
};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// The three detail write paths. Only the written methods serve Rust; the
/// rest proxy so Django keeps answering its 405s and OPTIONS. The two POSTs
/// on the list routes are wired in [`super::handlers::routes`].
pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/api/v1/workspaces/{slug}/schedulers/{scheduler_id}/",
            proxy_others(patch(definition_patch).delete(definition_delete)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/binding/",
            proxy_others(patch(binding_patch).delete(binding_uninstall)),
        )
        .route(
            "/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/run-now/",
            proxy_others(axum::routing::post(run_now)),
        )
}

/// Fill every method the handler does not own with the Django proxy.
fn proxy_others(router: MethodRouter<AppState>) -> MethodRouter<AppState> {
    router.fallback(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// `BINDING_WRITABLE_FIELDS`: the app serializer's fields an API-key caller
/// may edit, in the Python tuple's order.
pub const BINDING_WRITABLE_FIELDS: &[&str] = &[
    "extra_context",
    "enabled",
    "dtstart",
    "rrule",
    "tzid",
    "rdates",
    "exdates",
    "outcome_mode",
    "pod",
];

/// `DEFINITION_WRITABLE_FIELDS`.
pub const DEFINITION_WRITABLE_FIELDS: &[&str] =
    &["prompt", "name", "description", "color", "is_enabled"];

/// `DEFINITION_CREATE_FIELDS`: the slug plus the editable fields.
pub const DEFINITION_CREATE_FIELDS: &[&str] = &[
    "slug",
    "prompt",
    "name",
    "description",
    "color",
    "is_enabled",
];

/// `BINDING_CREATE_FIELDS`: the scheduler (id or slug) plus the editable
/// binding fields.
pub const BINDING_CREATE_FIELDS: &[&str] = &[
    "scheduler",
    "extra_context",
    "enabled",
    "dtstart",
    "rrule",
    "tzid",
    "rdates",
    "exdates",
    "outcome_mode",
    "pod",
];

/// `CADENCE_FIELDS` (`scheduler/binding_writes.py`): editing any of these
/// moves `next_run_at`, so its old value joins `previous`.
pub const CADENCE_FIELDS: &[&str] = &["dtstart", "rrule", "rdates", "exdates", "tzid"];

/// `EXPECTED_UPDATED_AT`: the body twin of `If-Match`.
pub const EXPECTED_UPDATED_AT: &str = "expected_updated_at";

/// The non-object body 400 both PATCH views answer.
pub const NOT_AN_OBJECT_MESSAGE: &str = "The request body must be a JSON object";

/// `BUILTIN_CONFLICT` message (`code: builtin_scheduler`).
pub const BUILTIN_MESSAGE: &str = "Built-in schedulers cannot be edited or deleted; put per-project instructions in the binding's extra_context instead.";

/// The create 400 for a built-in slug.
pub const RESERVED_SLUG_MESSAGE: &str = "This slug is reserved for a built-in scheduler.";

/// The install 400 for a missing, blank or non-string `scheduler`.
pub const SCHEDULER_REQUIRED_MESSAGE: &str = "The scheduler's id or slug is required.";

/// `get_object_or_404(Project, pk=..., workspace__slug=...)` miss on install.
pub const PROJECT_LOOKUP_NOT_FOUND: &str = "No Project matches the given query.";

/// Project admin (`ROLE.ADMIN`) and member role values.
const ROLE_ADMIN: i32 = 20;
const ROLE_MEMBER: i32 = 15;

// ---------------------------------------------------------------------------
// SQL
// ---------------------------------------------------------------------------

/// `_locked_binding`: the install, row-locked
/// (`select_for_update(of=("self",))`), with `select_related("scheduler")`
/// — whose `is_enabled` the run-now check reads. Columns 0..22 are
/// `BINDING_DETAIL_SQL`'s, decoded by `decode_binding`.
const LOCKED_BINDING_SQL: &str = concat!(
    r#"SELECT "scheduler_bindings"."created_at", "scheduler_bindings"."updated_at", "scheduler_bindings"."created_by_id", "scheduler_bindings"."updated_by_id", "scheduler_bindings"."deleted_at", "scheduler_bindings"."id", "scheduler_bindings"."workspace_id", "scheduler_bindings"."project_id", "scheduler_bindings"."scheduler_id", "scheduler_bindings"."dtstart", "scheduler_bindings"."tzid", "scheduler_bindings"."rrule", "scheduler_bindings"."rdates", "scheduler_bindings"."exdates", "scheduler_bindings"."extra_context", "scheduler_bindings"."enabled", "scheduler_bindings"."outcome_mode", "scheduler_bindings"."next_run_at", "scheduler_bindings"."last_run_id", "scheduler_bindings"."last_error", "scheduler_bindings"."actor_id", "scheduler_bindings"."pod_id", "schedulers"."is_enabled""#,
    r#" FROM "scheduler_bindings" INNER JOIN "schedulers" ON ("scheduler_bindings"."scheduler_id" = "schedulers"."id") INNER JOIN "workspaces" ON ("scheduler_bindings"."workspace_id" = "workspaces"."id")"#,
    r#" WHERE ("scheduler_bindings"."deleted_at" IS NULL AND "scheduler_bindings"."scheduler_id" = $1 AND "scheduler_bindings"."project_id" = $2 AND "workspaces"."slug" = $3)"#,
    r#" FOR UPDATE OF "scheduler_bindings""#,
);

/// The v1 render columns of `_binding_queryset()` (the read handlers'
/// `BindingRow`), shared by the by-id and by-scheduler fetches below.
const BINDING_RENDER_SELECT: &str = concat!(
    "SELECT ",
    r#""b"."created_at", "b"."updated_at", "b"."id", "b"."workspace_id", "b"."project_id", "b"."scheduler_id", "b"."dtstart", "b"."tzid", "b"."rrule", "b"."rdates", "b"."exdates", "b"."extra_context", "b"."enabled", "b"."outcome_mode", "b"."next_run_at", "b"."last_run_id", "b"."last_error", "b"."pod_id", "s"."slug" AS "scheduler_slug", "s"."name" AS "scheduler_name", "p"."name" AS "pod_name", "lr"."status" AS "last_run_status", "lr"."started_at" AS "last_run_started_at", "lr"."ended_at" AS "last_run_ended_at""#,
    r#" FROM "scheduler_bindings" "b" JOIN "schedulers" "s" ON "s"."id" = "b"."scheduler_id" LEFT JOIN "pod" "p" ON "p"."id" = "b"."pod_id" LEFT JOIN "agent_run" "lr" ON "lr"."id" = "b"."last_run_id""#,
);

/// `_binding_queryset().get(pk=...)`.
fn binding_by_id_sql() -> String {
    format!(r#"{BINDING_RENDER_SELECT} WHERE "b"."deleted_at" IS NULL AND "b"."id" = $1"#)
}

/// The definition edit's blast radius: `_binding_queryset()
/// .filter(scheduler=...).select_related("project").order_by("project__name")`
/// — every live install, whichever projects the caller belongs to, with
/// the project's identifier and name.
fn bindings_of_scheduler_sql() -> String {
    format!(
        r#"SELECT "x".*, "pr"."identifier" AS "project_identifier", "pr"."name" AS "project_name" FROM ({BINDING_RENDER_SELECT} WHERE "b"."deleted_at" IS NULL AND "b"."scheduler_id" = $1) "x" JOIN "projects" "pr" ON "pr"."id" = "x"."project_id" ORDER BY "pr"."name" ASC"#
    )
}

/// `get_object_or_404(Scheduler.objects.select_for_update(), pk=...,
/// workspace__slug=...)`: soft-delete scope, `FOR UPDATE` over the join.
const LOCKED_SCHEDULER_SQL: &str = concat!(
    r#"SELECT "schedulers"."created_at", "schedulers"."updated_at", "schedulers"."created_by_id", "schedulers"."updated_by_id", "schedulers"."deleted_at", "schedulers"."id", "schedulers"."workspace_id", "schedulers"."slug", "schedulers"."name", "schedulers"."description", "schedulers"."prompt", "schedulers"."source", "schedulers"."is_enabled", "schedulers"."color""#,
    r#" FROM "schedulers" INNER JOIN "workspaces" ON ("schedulers"."workspace_id" = "workspaces"."id")"#,
    r#" WHERE ("schedulers"."deleted_at" IS NULL AND "schedulers"."id" = $1 AND "workspaces"."slug" = $2) FOR UPDATE"#,
);

/// `AgentRun.objects.filter(scheduler_binding=..., status__in=NON_TERMINAL_STATUSES).exists()`.
const ACTIVE_RUN_SQL: &str = r#"SELECT EXISTS(SELECT 1 FROM "agent_run" WHERE "scheduler_binding_id" = $1 AND "status" = ANY($2))"#;

/// The role facts both decorators and both class gates read, one
/// `exists()` each (`is_active` rows, soft-delete scope).
const ROLE_FACTS_SQL: &str = r#"SELECT
  EXISTS(SELECT 1 FROM "workspace_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "is_active" AND "deleted_at" IS NULL AND "role" = ANY($4)) AS "ws_admin_or_member",
  EXISTS(SELECT 1 FROM "workspace_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "is_active" AND "deleted_at" IS NULL AND "role" = $5) AS "ws_admin",
  EXISTS(SELECT 1 FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL) AS "project_member",
  EXISTS(SELECT 1 FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL AND "role" = ANY($4)) AS "project_admin_or_member",
  EXISTS(SELECT 1 FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL AND "role" = $5) AS "project_admin""#;

/// One blast-radius row: the rendered binding plus its project.
#[derive(Debug, Clone, sqlx::FromRow)]
struct AffectedBindingRow {
    #[sqlx(flatten)]
    binding: BindingRow,
    project_identifier: String,
    project_name: String,
}

// ---------------------------------------------------------------------------
// Gates
// ---------------------------------------------------------------------------

/// The caller's memberships for one request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RoleFacts {
    pub ws_admin_or_member: bool,
    pub ws_admin: bool,
    pub project_member: bool,
    pub project_admin_or_member: bool,
    pub project_admin: bool,
}

async fn role_facts(
    pool: &PgPool,
    workspace_id: Option<&Uuid>,
    user_id: &Uuid,
    project_id: Option<&Uuid>,
) -> Result<RoleFacts, Denial> {
    // An unknown slug matches no membership row: every gate denies.
    let Some(workspace_id) = workspace_id else {
        return Ok(RoleFacts::default());
    };
    let row = sqlx::query(ROLE_FACTS_SQL)
        .bind(workspace_id)
        .bind(user_id)
        .bind(project_id.copied().unwrap_or_else(Uuid::nil))
        .bind(vec![ROLE_ADMIN, ROLE_MEMBER])
        .bind(ROLE_ADMIN)
        .fetch_one(pool)
        .await
        .map_err(|error| db_error(error, "v1-scheduler-roles"))?;
    let flag = |name: &str| -> Result<bool, Denial> {
        row.try_get::<bool, _>(name)
            .map_err(|error| db_error(error, "v1-scheduler-roles-decode"))
    };
    let mut facts = RoleFacts {
        ws_admin_or_member: flag("ws_admin_or_member")?,
        ws_admin: flag("ws_admin")?,
        ..RoleFacts::default()
    };
    if project_id.is_some() {
        facts.project_member = flag("project_member")?;
        facts.project_admin_or_member = flag("project_admin_or_member")?;
        facts.project_admin = flag("project_admin")?;
    }
    Ok(facts)
}

/// `ProjectEntityPermission` on PATCH/POST: project Admin or Member.
pub fn project_class_allows(facts: &RoleFacts) -> bool {
    facts.project_admin_or_member
}

/// `@allow_permission([ROLE.ADMIN], level="PROJECT")`: project Admin, or
/// any project membership plus workspace Admin.
pub fn project_admin_decorator_allows(facts: &RoleFacts) -> bool {
    facts.project_admin || (facts.project_member && facts.ws_admin)
}

/// `WorkspaceEntityPermission` on PATCH: workspace Admin or Member.
pub fn workspace_class_allows(facts: &RoleFacts) -> bool {
    facts.ws_admin_or_member
}

/// `@allow_permission([ROLE.ADMIN], level="WORKSPACE")`.
pub fn workspace_admin_decorator_allows(facts: &RoleFacts) -> bool {
    facts.ws_admin
}

/// The authenticated, authorized context of one write.
struct WriteContext {
    pool: PgPool,
    user_id: Uuid,
    timezone: Tz,
    project_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
}

/// Which pair of gates a route carries.
#[derive(Clone, Copy)]
enum Scope {
    Project,
    Workspace,
}

/// Authentication, rewrite, class gate, time zone, decorator, flag — in
/// Django's order.
async fn write_context(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_raw: Option<&str>,
    scope: Scope,
) -> Result<WriteContext, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = match project_raw {
        Some(raw) => Some(rewrite_project_id(&pre.pool, slug, raw).await?),
        None => None,
    };
    let facts = role_facts(
        &pre.pool,
        pre.workspace_id.as_ref(),
        &pre.actor.id,
        project_id.as_ref(),
    )
    .await?;
    let (class, decorator) = match scope {
        Scope::Project => (
            project_class_allows(&facts),
            project_admin_decorator_allows(&facts),
        ),
        Scope::Workspace => (
            workspace_class_allows(&facts),
            workspace_admin_decorator_allows(&facts),
        ),
    };
    if !class {
        return Err(Denial::Forbidden);
    }
    let timezone = activate_timezone(pre.actor.timezone.as_deref())?;
    if !decorator {
        return Err(Denial::RoleDenied);
    }
    ensure_feature_enabled(state.settings())?;
    Ok(WriteContext {
        pool: pre.pool,
        user_id: pre.actor.id,
        timezone,
        project_id,
        workspace_id: pre.workspace_id,
    })
}

// ---------------------------------------------------------------------------
// Shared body helpers
// ---------------------------------------------------------------------------

/// `_unwritable_response`: 400 when `data` is empty or names a field
/// outside `writable`.
pub fn unwritable_denial(
    data: &Map<String, Value>,
    writable: &[&str],
    noun: &str,
) -> Option<Denial> {
    let mut unknown: Vec<&str> = data
        .keys()
        .map(String::as_str)
        .filter(|key| !writable.contains(key))
        .collect();
    if unknown.is_empty() && !data.is_empty() {
        return None;
    }
    unknown.sort_unstable();
    let message = if unknown.is_empty() {
        "No fields to update".to_owned()
    } else {
        format!("Not writable on {noun}: {}", unknown.join(", "))
    };
    let mut body = Map::with_capacity(2);
    body.insert("error".to_owned(), Value::String(message));
    body.insert(
        "writable_fields".to_owned(),
        Value::Array(
            writable
                .iter()
                .map(|field| Value::String((*field).to_owned()))
                .collect(),
        ),
    );
    Some(Denial::Body(StatusCode::BAD_REQUEST, Value::Object(body)))
}

/// `{k: v for k, v in request.data.items() if k != EXPECTED_UPDATED_AT}`.
fn without_token(body: &Map<String, Value>) -> Map<String, Value> {
    body.iter()
        .filter(|(key, _)| key.as_str() != EXPECTED_UPDATED_AT)
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// `request.headers.get("If-Match")`: WSGI decodes header bytes as
/// latin-1 and joins repeats with `", "`.
fn if_match_header(headers: &HeaderMap) -> Option<String> {
    let values: Vec<String> = headers
        .get_all(axum::http::header::IF_MATCH)
        .iter()
        .map(|value| value.as_bytes().iter().map(|&byte| byte as char).collect())
        .collect();
    if values.is_empty() {
        None
    } else {
        Some(values.join(", "))
    }
}

/// The body's `expected_updated_at` as Python's `str(raw)` would see it:
/// `None` for an absent or null key; a non-string value can never parse
/// as a datetime, so its exact spelling does not matter.
fn body_token(body: Option<&Map<String, Value>>) -> Option<String> {
    match body?.get(EXPECTED_UPDATED_AT)? {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

/// Django `parse_datetime` to an aware instant: `None` when the text does
/// not parse, or parses without an offset.
fn parse_aware(text: &str) -> Option<DateTime<Utc>> {
    let (naive, offset) =
        app_bind::parse_fromiso_datetime(text).or_else(|| app_bind::parse_regex_datetime(text))?;
    let offset = offset?;
    naive.and_utc().checked_sub_signed(offset)
}

/// `datetime.isoformat()` of a stored (UTC) timestamp.
pub fn python_isoformat_utc(instant: &DateTime<Utc>) -> String {
    use chrono::Timelike;
    let micros = instant.nanosecond() / 1_000;
    if micros == 0 {
        instant.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
    } else {
        format!("{}.{micros:06}+00:00", instant.format("%Y-%m-%dT%H:%M:%S"))
    }
}

/// `_precondition_failure`: `None` when there is no token, the token is
/// `*`, or it equals `updated_at`; otherwise the 400 (unreadable) or 412
/// (stale) denial.
pub fn precondition_denial(
    raw: Option<String>,
    updated_at: &DateTime<Utc>,
    noun: &str,
) -> Option<Denial> {
    let raw = raw?;
    let raw = raw
        .trim_matches(app_bind::is_python_space)
        .trim_matches('"');
    if raw == "*" {
        return None;
    }
    let Some(expected) = parse_aware(raw) else {
        return Some(Denial::BadError(format!(
            "If-Match / expected_updated_at must be the {noun}'s updated_at, as an ISO 8601 datetime with an offset"
        )));
    };
    if expected == *updated_at {
        return None;
    }
    let mut body = Map::with_capacity(2);
    body.insert(
        "error".to_owned(),
        Value::String(format!("The {noun} changed since it was read")),
    );
    body.insert(
        "updated_at".to_owned(),
        Value::String(python_isoformat_utc(updated_at)),
    );
    Some(Denial::Body(
        StatusCode::PRECONDITION_FAILED,
        Value::Object(body),
    ))
}

/// A 409 `{"error": ..., "code": ...}`.
fn conflict(code: &str, message: &str) -> Denial {
    let mut body = Map::with_capacity(2);
    body.insert("error".to_owned(), Value::String(message.to_owned()));
    body.insert("code".to_owned(), Value::String(code.to_owned()));
    Denial::Body(StatusCode::CONFLICT, Value::Object(body))
}

/// The web app write path's failures, re-rendered through the v1
/// `handle_exception` (the two differ only in the `ObjectDoesNotExist` body
/// and share every other one).
fn from_app(denial: app_bind::Denial) -> Denial {
    match denial {
        app_bind::Denial::ObjectNotFound => Denial::ObjectNotFound,
        app_bind::Denial::ServerError => Denial::ServerError,
        other => Denial::App(Box::new(other.into_response())),
    }
}

/// Whether [`app_bind::read_request_data`] parses this request itself: an empty body
/// (content-length 0) or an `application/json` one. Anything else is
/// proxied to Django.
fn json_or_empty(headers: &HeaderMap) -> bool {
    let content_length = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length == 0 {
        return true;
    }
    let raw_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    raw_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("application/json")
}

/// The canonical `<uuid:>` spelling, else `None` (Django's resolver 404).
fn canonical_uuid(raw: &str) -> Option<Uuid> {
    if super::handlers::is_canonical_uuid(raw) {
        raw.parse().ok()
    } else {
        None
    }
}

fn respond(result: Result<Response, Denial>) -> Response {
    match result {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Fetches
// ---------------------------------------------------------------------------

/// `_locked_binding`: the install plus its scheduler's `is_enabled`.
async fn lock_binding(
    conn: &mut sqlx::PgConnection,
    scheduler_id: &Uuid,
    project_id: &Uuid,
    slug: &str,
) -> Result<(SchedulerBinding, bool), Denial> {
    let row = sqlx::query(LOCKED_BINDING_SQL)
        .bind(scheduler_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|error| db_error(error, "v1-binding-lock"))?
        .ok_or(Denial::BindingNotFound)?;
    let binding = app_bind::decode_binding(&row).map_err(from_app)?;
    let scheduler_enabled: bool = row
        .try_get(22)
        .map_err(|error| db_error(error, "v1-binding-lock-decode"))?;
    Ok((binding, scheduler_enabled))
}

/// The binding as the v1 reads render it.
async fn render_row(
    executor: impl sqlx::PgExecutor<'_>,
    binding_id: &Uuid,
) -> Result<BindingRow, Denial> {
    sqlx::query_as::<_, BindingRow>(&binding_by_id_sql())
        .bind(binding_id)
        .fetch_optional(executor)
        .await
        .map_err(|error| db_error(error, "v1-binding-render"))?
        .ok_or(Denial::ObjectNotFound)
}

/// `_binding_payload`: the fresh binding plus `previous`.
fn binding_payload(fresh: &BindingRow, previous: Map<String, Value>, tz: &Tz) -> Response {
    let mut body = match render_binding(fresh, tz) {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    body.insert("previous".to_owned(), Value::Object(previous));
    super::ok_json(Value::Object(body))
}

// ---------------------------------------------------------------------------
// `PATCH .../schedulers/<uuid>/binding/`
// ---------------------------------------------------------------------------

/// `ProjectSchedulerBindingAPIEndpoint.patch`.
async fn binding_patch(
    State(state): State<AppState>,
    Path((slug, project_raw, scheduler_raw)): Path<(String, String, String)>,
    req: Request,
) -> Response {
    let Some(scheduler_id) = canonical_uuid(&scheduler_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let headers = req.headers().clone();
    let context =
        match write_context(&state, &headers, &slug, Some(&project_raw), Scope::Project).await {
            Ok(context) => context,
            Err(denial) => return denial.into_response(),
        };
    let data = match app_bind::read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    respond(binding_patch_inner(&context, &headers, &slug, &scheduler_id, &data).await)
}

async fn binding_patch_inner(
    context: &WriteContext,
    headers: &HeaderMap,
    slug: &str,
    scheduler_id: &Uuid,
    data: &Value,
) -> Result<Response, Denial> {
    let Value::Object(body) = data else {
        return Err(Denial::BadError(NOT_AN_OBJECT_MESSAGE.to_owned()));
    };
    let fields = without_token(body);
    // Reject rather than ignore: a caller sending `prompt` or
    // `next_run_at` should learn it did nothing, not get a 200.
    if let Some(denial) = unwritable_denial(&fields, BINDING_WRITABLE_FIELDS, "a scheduler binding")
    {
        return Err(denial);
    }
    let project_id = context.project_id.ok_or(Denial::ServerError)?;
    let mut tx = context
        .pool
        .begin()
        .await
        .map_err(|error| db_error(error, "v1-binding-begin"))?;
    let (binding, _) = lock_binding(&mut tx, scheduler_id, &project_id, slug).await?;
    let token = if_match_header(headers).or_else(|| body_token(Some(body)));
    if let Some(denial) = precondition_denial(token, &binding.updated_at, "scheduler binding") {
        return Err(denial);
    }
    let before = match render_binding(&render_row(&mut *tx, &binding.id).await?, &context.timezone)
    {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let mut previous = Map::with_capacity(fields.len() + 1);
    for key in fields.keys() {
        previous.insert(key.clone(), before.get(key).cloned().unwrap_or(Value::Null));
    }
    if fields
        .keys()
        .any(|key| CADENCE_FIELDS.contains(&key.as_str()))
    {
        // A cadence edit moves next_run_at too; record where it was.
        previous.insert(
            "next_run_at".to_owned(),
            before.get("next_run_at").cloned().unwrap_or(Value::Null),
        );
    }
    let fields_value = Value::Object(fields);
    app_bind::apply_binding_patch(
        &context.pool,
        &mut tx,
        &BindingPatchTarget {
            binding_id: &binding.id,
            project_id: &project_id,
            slug,
        },
        &binding,
        &fields_value,
        &context.timezone,
        &context.user_id,
    )
    .await
    .map_err(from_app)?;
    tx.commit()
        .await
        .map_err(|error| db_error(error, "v1-binding-commit"))?;
    let fresh = render_row(&context.pool, &binding.id).await?;
    Ok(binding_payload(&fresh, previous, &context.timezone))
}

// ---------------------------------------------------------------------------
// `POST .../schedulers/<uuid>/run-now/`
// ---------------------------------------------------------------------------

/// `ProjectSchedulerRunNowAPIEndpoint.post`.
async fn run_now(
    State(state): State<AppState>,
    Path((slug, project_raw, scheduler_raw)): Path<(String, String, String)>,
    req: Request,
) -> Response {
    let Some(scheduler_id) = canonical_uuid(&scheduler_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let headers = req.headers().clone();
    let context =
        match write_context(&state, &headers, &slug, Some(&project_raw), Scope::Project).await {
            Ok(context) => context,
            Err(denial) => return denial.into_response(),
        };
    let Some(project_id) = context.project_id else {
        return Denial::ServerError.into_response();
    };
    let mut tx = match context.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => return db_error(error, "v1-run-now-begin").into_response(),
    };
    let (binding, scheduler_enabled) =
        match lock_binding(&mut tx, &scheduler_id, &project_id, &slug).await {
            Ok(locked) => locked,
            Err(denial) => return denial.into_response(),
        };
    // `request.data` is read only when no `If-Match` header was sent. A
    // media type Django must parse is proxied — after releasing the lock,
    // which Django's own run of this view takes again.
    let token = match if_match_header(&headers) {
        Some(token) => Some(token),
        None => {
            if !json_or_empty(&headers) {
                drop(tx);
                return crate::edge::proxy(State(state), req).await;
            }
            match app_bind::read_request_data(&state, req).await {
                Ok(data) => body_token(data.as_object()),
                Err(response) => return response,
            }
        }
    };
    if let Some(denial) = precondition_denial(token, &binding.updated_at, "scheduler binding") {
        return denial.into_response();
    }
    respond(run_now_write(&context, tx, &binding, scheduler_enabled).await)
}

/// `run_now_conflict` (`scheduler/binding_writes.py`): why a tick could not
/// fire `binding` now, as `(code, message)`.
pub fn run_now_conflict_of(
    binding: &SchedulerBinding,
    scheduler_enabled: bool,
    run_active: bool,
    now: DateTime<Utc>,
) -> Option<(&'static str, &'static str)> {
    if !binding.enabled {
        return Some((
            "binding_disabled",
            "The scheduler is disabled on this project.",
        ));
    }
    if !scheduler_enabled {
        return Some((
            "scheduler_disabled",
            "The scheduler definition is disabled.",
        ));
    }
    if run_active {
        return Some(("run_active", "A run from this scheduler is already active."));
    }
    let bundle = queries::RruleBundle {
        dtstart: binding.dtstart,
        rrule: binding.rrule.as_str(),
        tzid: binding.tzid.as_str(),
        rdates: &binding.rdates,
        exdates: &binding.exdates,
    };
    if app_bind::next_fire_for_binding(&bundle, now).is_none() {
        return Some((
            "no_future_occurrence",
            "The schedule has no future occurrence; edit the cadence first.",
        ));
    }
    None
}

async fn run_now_write(
    context: &WriteContext,
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    binding: &SchedulerBinding,
    scheduler_enabled: bool,
) -> Result<Response, Denial> {
    // The active-run probe only runs once the two enabled checks pass,
    // like Python's short-circuiting `if` chain.
    let run_active = if binding.enabled && scheduler_enabled {
        let statuses: Vec<String> = pidash_jobs::tasks_ticker::fire_binding::NON_TERMINAL_STATUSES
            .iter()
            .map(|status| (*status).to_owned())
            .collect();
        sqlx::query_scalar::<_, bool>(ACTIVE_RUN_SQL)
            .bind(binding.id)
            .bind(statuses)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| db_error(error, "v1-run-now-active"))?
    } else {
        false
    };
    if let Some((code, message)) =
        run_now_conflict_of(binding, scheduler_enabled, run_active, Utc::now())
    {
        return Err(conflict(code, message));
    }
    let before = render_row(&mut *tx, &binding.id).await?;
    let mut previous = Map::with_capacity(1);
    previous.insert(
        "next_run_at".to_owned(),
        match render_binding(&before, &context.timezone) {
            Value::Object(map) => map.get("next_run_at").cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        },
    );
    // `mark_due_now`: `next_run_at = now`, then `save(update_fields=...)`
    // stamps `updated_at` with its own `now()`.
    let due = Utc::now();
    let stamp = Utc::now();
    app_bind::write_next_run_at(&mut *tx, &binding.id, Some(&due), &stamp)
        .await
        .map_err(from_app)?;
    tx.commit()
        .await
        .map_err(|error| db_error(error, "v1-run-now-commit"))?;
    let fresh = render_row(&context.pool, &binding.id).await?;
    Ok(binding_payload(&fresh, previous, &context.timezone))
}

// ---------------------------------------------------------------------------
// `PATCH workspaces/<slug>/schedulers/<uuid>/`
// ---------------------------------------------------------------------------

/// `WorkspaceSchedulerDetailAPIEndpoint.patch`.
async fn definition_patch(
    State(state): State<AppState>,
    Path((slug, scheduler_raw)): Path<(String, String)>,
    req: Request,
) -> Response {
    let Some(scheduler_id) = canonical_uuid(&scheduler_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let headers = req.headers().clone();
    let context = match write_context(&state, &headers, &slug, None, Scope::Workspace).await {
        Ok(context) => context,
        Err(denial) => return denial.into_response(),
    };
    let data = match app_bind::read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    respond(definition_patch_inner(&context, &headers, &slug, &scheduler_id, &data).await)
}

/// `is_builtin_slug` / `is_builtin` (`scheduler/definition_writes.py`):
/// whether `slug` is in the built-in catalog. The reseed matches rows by
/// slug, so that, not `source` (which defaults to `builtin` for
/// user-created rows too, PDASHOSS01-290), is what makes a row built-in.
pub fn is_builtin_slug(slug: &str) -> bool {
    pidash_services::scheduler::builtins::BUILTINS
        .iter()
        .any(|builtin| builtin.slug == slug)
}

async fn definition_patch_inner(
    context: &WriteContext,
    headers: &HeaderMap,
    slug: &str,
    scheduler_id: &Uuid,
    data: &Value,
) -> Result<Response, Denial> {
    let Value::Object(body) = data else {
        return Err(Denial::BadError(NOT_AN_OBJECT_MESSAGE.to_owned()));
    };
    let fields = without_token(body);
    if let Some(denial) = unwritable_denial(
        &fields,
        DEFINITION_WRITABLE_FIELDS,
        "a scheduler definition",
    ) {
        return Err(denial);
    }
    let mut tx = context
        .pool
        .begin()
        .await
        .map_err(|error| db_error(error, "v1-definition-begin"))?;
    let record: app_sched::SchedulerRow = sqlx::query_as(LOCKED_SCHEDULER_SQL)
        .bind(scheduler_id)
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| db_error(error, "v1-definition-lock"))?
        .ok_or(Denial::SchedulerNotFound)?;
    if is_builtin_slug(&record.slug) {
        return Err(conflict("builtin_scheduler", BUILTIN_MESSAGE));
    }
    let token = if_match_header(headers).or_else(|| body_token(Some(body)));
    if let Some(denial) = precondition_denial(token, &record.updated_at, "scheduler") {
        return Err(denial);
    }
    let mut previous = Map::with_capacity(fields.len());
    for key in fields.keys() {
        let value = match key.as_str() {
            "prompt" => Value::String(record.prompt.clone()),
            "name" => Value::String(record.name.clone()),
            "color" => Value::String(record.color.clone()),
            "is_enabled" => Value::Bool(record.is_enabled),
            _ => Value::String(record.description.clone()),
        };
        previous.insert(key.clone(), value);
    }
    let write = app_sched::validate_patch_body(&Value::Object(fields), &record)
        .map_err(|errors| Denial::Body(StatusCode::BAD_REQUEST, errors))?;
    // `serializer.save()`: every writable column with the merged values,
    // plus `updated_at` / `updated_by_id` (the app's R4 statement).
    let now = Utc::now();
    let sql = app_sched::positional(
        queries::SCHEDULER_PATCH_SQL,
        queries::SCHEDULER_PATCH_PARAMS,
    );
    let saved = sqlx::query(&sql)
        .bind(now)
        .bind(context.user_id)
        .bind(&write.slug)
        .bind(&write.name)
        .bind(&write.description)
        .bind(&write.prompt)
        .bind(&write.color)
        .bind(write.is_enabled)
        .bind(scheduler_id)
        .execute(&mut *tx)
        .await;
    if let Err(error) = saved {
        let integrity = error
            .as_database_error()
            .and_then(|db| db.code())
            .is_some_and(|code| code.starts_with("23"));
        return Err(if integrity {
            Denial::BadError("The payload is not valid".to_owned())
        } else {
            db_error(error, "v1-definition-save")
        });
    }
    tx.commit()
        .await
        .map_err(|error| db_error(error, "v1-definition-commit"))?;
    // Every live install, not only the caller's projects: the caller is a
    // workspace admin and this is the blast radius of the edit.
    let affected: Vec<AffectedBindingRow> = sqlx::query_as(&bindings_of_scheduler_sql())
        .bind(scheduler_id)
        .fetch_all(&context.pool)
        .await
        .map_err(|error| db_error(error, "v1-definition-bindings"))?;
    let scheduler = SchedulerRow {
        created_at: record.created_at,
        updated_at: now,
        id: record.id,
        workspace_id: record.workspace_id,
        slug: write.slug,
        name: write.name,
        description: write.description,
        prompt: write.prompt,
        source: record.source,
        is_enabled: write.is_enabled,
        color: write.color,
    };
    let bindings: Vec<BindingRow> = affected.iter().map(|row| row.binding.clone()).collect();
    let mut rendered = render_scheduler(&scheduler, &bindings, &context.timezone);
    rendered.insert(
        "affected_projects".to_owned(),
        Value::Array(affected.iter().map(affected_project).collect()),
    );
    rendered.insert("previous".to_owned(), Value::Object(previous));
    Ok(super::ok_json(Value::Object(rendered)))
}

/// One `affected_projects` entry, in the Python dict's key order.
fn affected_project(row: &AffectedBindingRow) -> Value {
    let mut map = Map::with_capacity(4);
    map.insert(
        "id".to_owned(),
        Value::String(row.binding.project_id.to_string()),
    );
    map.insert(
        "identifier".to_owned(),
        Value::String(row.project_identifier.clone()),
    );
    map.insert("name".to_owned(), Value::String(row.project_name.clone()));
    map.insert(
        "binding_enabled".to_owned(),
        Value::Bool(row.binding.enabled),
    );
    Value::Object(map)
}

// ---------------------------------------------------------------------------
// PDASHOSS01-318: create / delete a definition, install / uninstall it
// ---------------------------------------------------------------------------

/// `_scheduler_by_ref` by id: the workspace's live scheduler with that pk
/// (`Scheduler.objects.filter(workspace_id=..., pk=...).first()`).
const SCHEDULER_BY_ID_SQL: &str = r#"SELECT "id", "is_enabled" FROM "schedulers" WHERE "deleted_at" IS NULL AND "workspace_id" = $1 AND "id" = $2 ORDER BY "created_at" DESC LIMIT 1"#;

/// `_scheduler_by_ref` by slug.
const SCHEDULER_BY_SLUG_SQL: &str = r#"SELECT "id", "is_enabled" FROM "schedulers" WHERE "deleted_at" IS NULL AND "workspace_id" = $1 AND "slug" = $2 ORDER BY "created_at" DESC LIMIT 1"#;

/// `SchedulerBinding.objects.filter(scheduler=..., project=...,
/// deleted_at__isnull=True).exists()`.
const INSTALLED_SQL: &str = r#"SELECT EXISTS(SELECT 1 FROM "scheduler_bindings" WHERE "deleted_at" IS NULL AND "scheduler_id" = $1 AND "project_id" = $2)"#;

/// A JSON response with `status` (key order is the construction order).
fn json_status(status: StatusCode, value: &Value) -> Response {
    axum::http::Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::to_string(value).expect("json body"),
        ))
        .expect("json response")
}

fn no_content() -> Response {
    axum::http::Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(axum::body::Body::empty())
        .expect("empty 204 response")
}

/// A 400 `{field: [message]}` field error.
fn field_error(field: &str, message: &str) -> Denial {
    let mut body = Map::with_capacity(1);
    body.insert(
        field.to_owned(),
        Value::Array(vec![Value::String(message.to_owned())]),
    );
    Denial::Body(StatusCode::BAD_REQUEST, Value::Object(body))
}

/// The web app scheduler path's failures, re-rendered through the v1
/// `handle_exception`.
fn from_sched(denial: app_sched::Denial) -> Denial {
    match denial {
        app_sched::Denial::ObjectNotFound => Denial::SchedulerNotFound,
        app_sched::Denial::BadDetail(detail) => Denial::BadDetail(detail),
        app_sched::Denial::BadError(message) => Denial::BadError(message),
        app_sched::Denial::BadJson(body) => Denial::Body(StatusCode::BAD_REQUEST, body),
        app_sched::Denial::ServerError => Denial::ServerError,
    }
}

/// The concurrency token of a bodiless write (`DELETE`): the `If-Match`
/// header, else the body's `expected_updated_at` — read only when there
/// is no header, like `request.data` in `_precondition_failure`. `Err`
/// hands back the request when Django must parse its media type.
async fn delete_token(
    state: &AppState,
    headers: &HeaderMap,
    req: Request,
) -> Result<Option<String>, Box<Result<Request, Response>>> {
    if let Some(token) = if_match_header(headers) {
        return Ok(Some(token));
    }
    if !json_or_empty(headers) {
        return Err(Box::new(Ok(req)));
    }
    match app_bind::read_request_data(state, req).await {
        Ok(data) => Ok(body_token(data.as_object())),
        Err(response) => Err(Box::new(Err(response))),
    }
}

/// `WorkspaceSchedulerListAPIEndpoint.post`.
pub async fn definition_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    req: Request,
) -> Response {
    let headers = req.headers().clone();
    let context = match write_context(&state, &headers, &slug, None, Scope::Workspace).await {
        Ok(context) => context,
        Err(denial) => return denial.into_response(),
    };
    let data = match app_bind::read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    respond(definition_create_inner(&context, &data).await)
}

async fn definition_create_inner(context: &WriteContext, data: &Value) -> Result<Response, Denial> {
    let Value::Object(body) = data else {
        return Err(Denial::BadError(NOT_AN_OBJECT_MESSAGE.to_owned()));
    };
    // Unknown keys only: an empty body goes on to the serializer's
    // required-field errors.
    if body
        .keys()
        .any(|key| !DEFINITION_CREATE_FIELDS.contains(&key.as_str()))
    {
        if let Some(denial) = unwritable_denial(body, DEFINITION_CREATE_FIELDS, "a new scheduler") {
            return Err(denial);
        }
    }
    if let Some(Value::String(slug)) = body.get("slug") {
        if is_builtin_slug(slug.trim_matches(app_bind::is_python_space)) {
            return Err(field_error("slug", RESERVED_SLUG_MESSAGE));
        }
    }
    let workspace_id = context.workspace_id.ok_or(Denial::ServerError)?;
    let (row, _count) =
        app_sched::create_scheduler(&context.pool, &workspace_id, &context.user_id, data)
            .await
            .map_err(from_sched)?;
    let scheduler = SchedulerRow {
        created_at: row.created_at,
        updated_at: row.updated_at,
        id: row.id,
        workspace_id: row.workspace_id,
        slug: row.slug,
        name: row.name,
        description: row.description,
        prompt: row.prompt,
        source: row.source,
        is_enabled: row.is_enabled,
        color: row.color,
    };
    let rendered = render_scheduler(&scheduler, &[], &context.timezone);
    Ok(json_status(StatusCode::CREATED, &Value::Object(rendered)))
}

/// `WorkspaceSchedulerDetailAPIEndpoint.delete`.
async fn definition_delete(
    State(state): State<AppState>,
    Path((slug, scheduler_raw)): Path<(String, String)>,
    req: Request,
) -> Response {
    let Some(scheduler_id) = canonical_uuid(&scheduler_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let headers = req.headers().clone();
    let context = match write_context(&state, &headers, &slug, None, Scope::Workspace).await {
        Ok(context) => context,
        Err(denial) => return denial.into_response(),
    };
    let mut tx = match context.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => return db_error(error, "v1-definition-delete-begin").into_response(),
    };
    let record: app_sched::SchedulerRow = match sqlx::query_as(LOCKED_SCHEDULER_SQL)
        .bind(scheduler_id)
        .bind(&slug)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(Some(record)) => record,
        Ok(None) => return Denial::SchedulerNotFound.into_response(),
        Err(error) => return db_error(error, "v1-definition-delete-lock").into_response(),
    };
    if is_builtin_slug(&record.slug) {
        return conflict("builtin_scheduler", BUILTIN_MESSAGE).into_response();
    }
    let token = match delete_token(&state, &headers, req).await {
        Ok(token) => token,
        Err(other) => {
            drop(tx);
            return match *other {
                Ok(req) => crate::edge::proxy(State(state), req).await,
                Err(response) => response,
            };
        }
    };
    if let Some(denial) = precondition_denial(token, &record.updated_at, "scheduler") {
        return denial.into_response();
    }
    if let Err(denial) =
        app_sched::soft_delete_scheduler(&mut tx, &scheduler_id, &context.user_id).await
    {
        return from_sched(denial).into_response();
    }
    if let Err(error) = tx.commit().await {
        return db_error(error, "v1-definition-delete-commit").into_response();
    }
    // Instance `delete()` → `.delay(...)` after the stamp, as the app does.
    app_sched::enqueue_soft_delete(&context.pool, "scheduler", &scheduler_id).await;
    no_content()
}

/// `ProjectSchedulerListAPIEndpoint.post`.
pub async fn binding_install(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    req: Request,
) -> Response {
    let headers = req.headers().clone();
    let context =
        match write_context(&state, &headers, &slug, Some(&project_raw), Scope::Project).await {
            Ok(context) => context,
            Err(denial) => return denial.into_response(),
        };
    let data = match app_bind::read_request_data(&state, req).await {
        Ok(data) => data,
        Err(response) => return response,
    };
    respond(binding_install_inner(&context, &slug, &data).await)
}

/// The scheduler a ref names, as `(id, is_enabled)`: a ref that parses
/// as a UUID (`uuid.UUID(ref)`) is only ever an id, anything else a slug.
async fn scheduler_by_ref(
    pool: &PgPool,
    workspace_id: &Uuid,
    reference: &str,
) -> Result<Option<(Uuid, bool)>, Denial> {
    let query = match app_bind::uuid_from_hex_forms(reference) {
        Some(id) => sqlx::query_as(SCHEDULER_BY_ID_SQL)
            .bind(workspace_id)
            .bind(id),
        None => sqlx::query_as(SCHEDULER_BY_SLUG_SQL)
            .bind(workspace_id)
            .bind(reference),
    };
    query
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "v1-install-scheduler"))
}

async fn binding_install_inner(
    context: &WriteContext,
    slug: &str,
    data: &Value,
) -> Result<Response, Denial> {
    let Value::Object(body) = data else {
        return Err(Denial::BadError(NOT_AN_OBJECT_MESSAGE.to_owned()));
    };
    if body
        .keys()
        .any(|key| !BINDING_CREATE_FIELDS.contains(&key.as_str()))
    {
        if let Some(denial) = unwritable_denial(body, BINDING_CREATE_FIELDS, "a scheduler install")
        {
            return Err(denial);
        }
    }
    let reference = match body.get("scheduler") {
        Some(Value::String(text)) => text.trim_matches(app_bind::is_python_space),
        _ => "",
    };
    if reference.is_empty() {
        return Err(field_error("scheduler", SCHEDULER_REQUIRED_MESSAGE));
    }
    let project_id = context.project_id.ok_or(Denial::ServerError)?;
    let project = app_bind::fetch_install_project(&context.pool, &project_id, slug)
        .await
        .map_err(from_app)?
        .ok_or_else(|| {
            Denial::Body(
                StatusCode::NOT_FOUND,
                serde_json::json!({ "detail": PROJECT_LOOKUP_NOT_FOUND }),
            )
        })?;
    let Some((scheduler_id, scheduler_enabled)) =
        scheduler_by_ref(&context.pool, &project.workspace_id, reference).await?
    else {
        return Err(Denial::Body(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "Scheduler not found" }),
        ));
    };
    if !scheduler_enabled {
        return Err(conflict(
            "scheduler_disabled",
            "The scheduler definition is disabled; enable it before installing it.",
        ));
    }
    let already = || {
        conflict(
            "already_installed",
            "The scheduler is already installed on this project.",
        )
    };
    let installed: bool = sqlx::query_scalar(INSTALLED_SQL)
        .bind(scheduler_id)
        .bind(project_id)
        .fetch_one(&context.pool)
        .await
        .map_err(|error| db_error(error, "v1-install-exists"))?;
    if installed {
        return Err(already());
    }
    // The serializer's identity fields come from the URL and the resolved
    // scheduler, never from the body (`{**data, "scheduler": ...,
    // "project": ...}`).
    let mut fields = body.clone();
    fields.insert(
        "scheduler".to_owned(),
        Value::String(scheduler_id.to_string()),
    );
    fields.insert("project".to_owned(), Value::String(project_id.to_string()));
    let target = InstallTarget {
        scheduler_id,
        project_id,
        workspace_id: project.workspace_id,
        actor_id: context.user_id,
    };
    let saved =
        match app_bind::apply_install(&context.pool, &fields, &target, &context.timezone).await {
            Ok(saved) => saved,
            // A concurrent install won the per-project unique constraint.
            Err(app_bind::Denial::InvalidPayload) => return Err(already()),
            Err(other) => return Err(from_app(other)),
        };
    let fresh = render_row(&context.pool, &saved.row.id).await?;
    Ok(json_status(
        StatusCode::CREATED,
        &render_binding(&fresh, &context.timezone),
    ))
}

/// `ProjectSchedulerBindingAPIEndpoint.delete`.
async fn binding_uninstall(
    State(state): State<AppState>,
    Path((slug, project_raw, scheduler_raw)): Path<(String, String, String)>,
    req: Request,
) -> Response {
    let Some(scheduler_id) = canonical_uuid(&scheduler_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let headers = req.headers().clone();
    let context =
        match write_context(&state, &headers, &slug, Some(&project_raw), Scope::Project).await {
            Ok(context) => context,
            Err(denial) => return denial.into_response(),
        };
    let Some(project_id) = context.project_id else {
        return Denial::ServerError.into_response();
    };
    let mut tx = match context.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => return db_error(error, "v1-uninstall-begin").into_response(),
    };
    let (binding, _) = match lock_binding(&mut tx, &scheduler_id, &project_id, &slug).await {
        Ok(locked) => locked,
        Err(denial) => return denial.into_response(),
    };
    let token = match delete_token(&state, &headers, req).await {
        Ok(token) => token,
        Err(other) => {
            drop(tx);
            return match *other {
                Ok(req) => crate::edge::proxy(State(state), req).await,
                Err(response) => response,
            };
        }
    };
    if let Some(denial) = precondition_denial(token, &binding.updated_at, "scheduler binding") {
        return denial.into_response();
    }
    // `binding.delete()`: two `now()` samples (deleted_at, then save's
    // updated_at), then the related-objects sweep after commit.
    let now = Utc::now();
    let now2 = Utc::now();
    if let Err(denial) =
        app_bind::uninstall_binding(&mut *tx, &binding.id, &context.user_id, &now, &now2).await
    {
        return from_app(denial).into_response();
    }
    if let Err(error) = tx.commit().await {
        return db_error(error, "v1-uninstall-commit").into_response();
    }
    app_bind::enqueue_soft_delete(&context.pool, &binding.id).await;
    no_content()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn body_of(denial: Denial) -> (StatusCode, Value) {
        match denial {
            Denial::Body(status, body) => (status, body),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({ "error": message }),
            ),
            other => panic!("unexpected denial {other:?}"),
        }
    }

    fn fields(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn unwritable_passes_writable_keys() {
        let data = fields(&[
            ("extra_context", Value::from("x")),
            ("enabled", Value::from(false)),
        ]);
        assert!(unwritable_denial(&data, BINDING_WRITABLE_FIELDS, "a scheduler binding").is_none());
    }

    #[test]
    fn unwritable_empty_body_says_no_fields() {
        let (status, body) = body_of(
            unwritable_denial(
                &Map::new(),
                DEFINITION_WRITABLE_FIELDS,
                "a scheduler definition",
            )
            .expect("empty body is refused"),
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"error":"No fields to update","writable_fields":["prompt","name","description","color","is_enabled"]}"#
        );
    }

    #[test]
    fn unwritable_lists_unknown_keys_sorted() {
        let data = fields(&[
            ("prompt", Value::from("p")),
            ("next_run_at", Value::Null),
            ("extra_context", Value::from("x")),
        ]);
        let (_, body) = body_of(
            unwritable_denial(&data, BINDING_WRITABLE_FIELDS, "a scheduler binding").unwrap(),
        );
        assert_eq!(
            body["error"],
            "Not writable on a scheduler binding: next_run_at, prompt"
        );
        assert_eq!(body["writable_fields"].as_array().unwrap().len(), 9);
        assert_eq!(body["writable_fields"][0], "extra_context");
        assert_eq!(body["writable_fields"][8], "pod");
    }

    #[test]
    fn token_is_dropped_from_the_written_fields() {
        let body = fields(&[
            ("expected_updated_at", Value::from("2026-10-01T00:00:00Z")),
            ("enabled", Value::from(true)),
        ]);
        let kept = without_token(&body);
        assert_eq!(kept.keys().collect::<Vec<_>>(), vec!["enabled"]);
        assert_eq!(
            body_token(Some(&body)).as_deref(),
            Some("2026-10-01T00:00:00Z")
        );
        let null = fields(&[("expected_updated_at", Value::Null)]);
        assert_eq!(body_token(Some(&null)), None);
        assert_eq!(body_token(None), None);
    }

    fn stamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 3, 9, 30, 0).unwrap()
            + chrono::Duration::microseconds(123_456)
    }

    #[test]
    fn precondition_absent_star_or_matching_passes() {
        let at = stamp();
        assert!(precondition_denial(None, &at, "scheduler binding").is_none());
        assert!(precondition_denial(Some(" * ".into()), &at, "scheduler binding").is_none());
        for token in [
            "2026-10-03T09:30:00.123456Z",
            "\"2026-10-03T09:30:00.123456+00:00\"",
            "2026-10-03T11:30:00.123456+02:00",
            " 2026-10-03 09:30:00.123456+00:00 ",
        ] {
            assert!(
                precondition_denial(Some(token.into()), &at, "scheduler binding").is_none(),
                "{token}"
            );
        }
    }

    #[test]
    fn precondition_stale_is_412_naming_current_value() {
        let (status, body) = body_of(
            precondition_denial(
                Some("2026-10-03T09:29:00Z".into()),
                &stamp(),
                "scheduler binding",
            )
            .unwrap(),
        );
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"error":"The scheduler binding changed since it was read","updated_at":"2026-10-03T09:30:00.123456+00:00"}"#
        );
    }

    #[test]
    fn precondition_unreadable_or_naive_is_400() {
        for token in [
            "yesterday",
            "2026-10-03T09:30:00",
            "2026-13-01T00:00:00+00:00",
            "true",
        ] {
            let (status, body) =
                body_of(precondition_denial(Some(token.into()), &stamp(), "scheduler").unwrap());
            assert_eq!(status, StatusCode::BAD_REQUEST, "{token}");
            assert_eq!(
                body["error"],
                "If-Match / expected_updated_at must be the scheduler's updated_at, as an ISO 8601 datetime with an offset"
            );
        }
    }

    #[test]
    fn isoformat_drops_zero_microseconds() {
        let whole = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(python_isoformat_utc(&whole), "2026-01-02T03:04:05+00:00");
        let frac = whole + chrono::Duration::microseconds(7);
        assert_eq!(
            python_isoformat_utc(&frac),
            "2026-01-02T03:04:05.000007+00:00"
        );
    }

    #[test]
    fn if_match_joins_repeats_and_reads_latin1() {
        let mut headers = HeaderMap::new();
        assert_eq!(if_match_header(&headers), None);
        headers.append(axum::http::header::IF_MATCH, "a".parse().unwrap());
        headers.append(axum::http::header::IF_MATCH, "b".parse().unwrap());
        assert_eq!(if_match_header(&headers).as_deref(), Some("a, b"));
    }

    #[test]
    fn json_or_empty_matches_the_parser() {
        let mut headers = HeaderMap::new();
        assert!(json_or_empty(&headers));
        headers.insert(axum::http::header::CONTENT_LENGTH, "2".parse().unwrap());
        assert!(!json_or_empty(&headers));
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            "Application/JSON; charset=utf-8".parse().unwrap(),
        );
        assert!(json_or_empty(&headers));
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        assert!(!json_or_empty(&headers));
    }

    #[test]
    fn gates_match_class_then_decorator() {
        let member = RoleFacts {
            ws_admin_or_member: true,
            project_member: true,
            project_admin_or_member: true,
            ..RoleFacts::default()
        };
        // A project member passes the class gate but not the decorator.
        assert!(project_class_allows(&member));
        assert!(!project_admin_decorator_allows(&member));
        // A project admin passes both.
        let admin = RoleFacts {
            project_admin: true,
            ..member
        };
        assert!(project_admin_decorator_allows(&admin));
        // A workspace admin who is a project member passes the decorator.
        let ws_admin = RoleFacts {
            ws_admin: true,
            ..member
        };
        assert!(project_admin_decorator_allows(&ws_admin));
        // A guest (project member, role 5) fails the class gate.
        let guest = RoleFacts {
            ws_admin_or_member: true,
            project_member: true,
            ..RoleFacts::default()
        };
        assert!(!project_class_allows(&guest));
        // The definition needs workspace admin; a project admin who is only
        // a workspace member is refused by the decorator.
        assert!(workspace_class_allows(&admin));
        assert!(!workspace_admin_decorator_allows(&admin));
        assert!(workspace_admin_decorator_allows(&ws_admin));
        assert!(!workspace_class_allows(&RoleFacts::default()));
    }

    fn binding(enabled: bool, rrule: &str) -> SchedulerBinding {
        let at = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        SchedulerBinding {
            created_at: at,
            updated_at: at,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            project_id: Some(Uuid::nil()),
            scheduler_id: Uuid::nil(),
            dtstart: at,
            tzid: "UTC".to_owned(),
            rrule: rrule.to_owned(),
            rdates: Value::Array(vec![]),
            exdates: Value::Array(vec![]),
            extra_context: String::new(),
            enabled,
            outcome_mode: "create_issue".to_owned(),
            next_run_at: None,
            last_run_id: None,
            last_error: String::new(),
            actor_id: None,
            pod_id: None,
        }
    }

    #[test]
    fn run_now_conflicts_in_python_order() {
        let now = Utc.with_ymd_and_hms(2026, 10, 21, 0, 0, 0).unwrap();
        let hourly = binding(true, "FREQ=HOURLY");
        assert_eq!(run_now_conflict_of(&hourly, true, false, now), None);
        assert_eq!(
            run_now_conflict_of(&binding(false, "FREQ=HOURLY"), false, true, now).map(|c| c.0),
            Some("binding_disabled")
        );
        assert_eq!(
            run_now_conflict_of(&hourly, false, true, now).map(|c| c.0),
            Some("scheduler_disabled")
        );
        assert_eq!(
            run_now_conflict_of(&hourly, true, true, now).map(|c| c.0),
            Some("run_active")
        );
        let spent = binding(true, "FREQ=DAILY;COUNT=1");
        assert_eq!(
            run_now_conflict_of(&spent, true, false, now),
            Some((
                "no_future_occurrence",
                "The schedule has no future occurrence; edit the cadence first."
            ))
        );
    }

    #[test]
    fn builtin_is_a_catalog_slug() {
        assert!(is_builtin_slug("security-audit"));
        assert!(is_builtin_slug("fable-security-audit"));
        assert!(!is_builtin_slug("contract-test-scheduler"));
        assert!(!is_builtin_slug("Security-Audit"));
    }

    #[test]
    fn routes_build_and_merge_with_reads() {
        let _router: axum::Router<AppState> = super::super::routes();
    }
}
