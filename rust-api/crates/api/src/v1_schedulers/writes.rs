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
//! plus its `schedulers` update, the create
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
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use sea_query::{Alias, Expr, Query as SeaQuery};
use serde_json::{Map, Value};
use sqlx::PgPool;
use uuid::Uuid;

use pidash_db::entity::{
    agent_run, project_members, projects, scheduler_bindings, schedulers, workspace_members,
    workspaces,
};
use pidash_db::orm_tx::TxConn;
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
// Queries (same JOIN/WHERE/lock semantics as the Django querysets, on SeaORM)
// ---------------------------------------------------------------------------

/// The role facts both decorators and both class gates read, one
/// `exists()` each (`is_active` rows, soft-delete scope), in one statement.
fn role_facts_stmt(
    workspace_id: Uuid,
    user_id: Uuid,
    project_id: Uuid,
) -> sea_query::SelectStatement {
    fn ws_exists(
        workspace_id: Uuid,
        user_id: Uuid,
        admin_or_member: bool,
    ) -> sea_query::SimpleExpr {
        let mut sub = SeaQuery::select();
        sub.expr(Expr::val(1))
            .from(workspace_members::Entity)
            .and_where(workspace_members::Column::WorkspaceId.eq(workspace_id))
            .and_where(workspace_members::Column::MemberId.eq(user_id))
            .and_where(workspace_members::Column::IsActive.eq(true))
            .and_where(workspace_members::Column::DeletedAt.is_null());
        if admin_or_member {
            sub.and_where(
                workspace_members::Column::Role.is_in([ROLE_ADMIN as i16, ROLE_MEMBER as i16]),
            );
        } else {
            sub.and_where(workspace_members::Column::Role.eq(ROLE_ADMIN as i16));
        }
        Expr::exists(sub.to_owned())
    }

    #[derive(Clone, Copy)]
    enum PmRole {
        Any,
        Admin,
        AdminOrMember,
    }

    fn pm_exists(
        workspace_id: Uuid,
        user_id: Uuid,
        project_id: Uuid,
        role: PmRole,
    ) -> sea_query::SimpleExpr {
        let mut sub = SeaQuery::select();
        sub.expr(Expr::val(1))
            .from(project_members::Entity)
            .and_where(project_members::Column::WorkspaceId.eq(workspace_id))
            .and_where(project_members::Column::MemberId.eq(user_id))
            .and_where(project_members::Column::ProjectId.eq(project_id))
            .and_where(project_members::Column::IsActive.eq(true))
            .and_where(project_members::Column::DeletedAt.is_null());
        match role {
            PmRole::Any => {}
            PmRole::Admin => {
                sub.and_where(project_members::Column::Role.eq(ROLE_ADMIN as i16));
            }
            PmRole::AdminOrMember => {
                sub.and_where(
                    project_members::Column::Role.is_in([ROLE_ADMIN as i16, ROLE_MEMBER as i16]),
                );
            }
        }
        Expr::exists(sub.to_owned())
    }

    let mut stmt = SeaQuery::select();
    stmt.expr_as(
        ws_exists(workspace_id, user_id, true),
        Alias::new("ws_admin_or_member"),
    )
    .expr_as(
        ws_exists(workspace_id, user_id, false),
        Alias::new("ws_admin"),
    )
    .expr_as(
        pm_exists(workspace_id, user_id, project_id, PmRole::Any),
        Alias::new("project_member"),
    )
    .expr_as(
        pm_exists(workspace_id, user_id, project_id, PmRole::AdminOrMember),
        Alias::new("project_admin_or_member"),
    )
    .expr_as(
        pm_exists(workspace_id, user_id, project_id, PmRole::Admin),
        Alias::new("project_admin"),
    );
    stmt
}

#[derive(Debug, FromQueryResult)]
struct RoleFactsRow {
    ws_admin_or_member: bool,
    ws_admin: bool,
    project_member: bool,
    project_admin_or_member: bool,
    project_admin: bool,
}

/// The locked binding the write holds: the entity row plus the scheduler's
/// `is_enabled` the run-now check reads, with the workspace-slug predicate
/// the old `FOR UPDATE OF scheduler_bindings` join carried.
struct LockedBinding {
    binding: SchedulerBinding,
    scheduler_enabled: bool,
}

fn binding_from_model(model: &scheduler_bindings::Model) -> Result<SchedulerBinding, Denial> {
    let workspace_id = model.workspace_id.ok_or(Denial::ServerError)?;
    Ok(SchedulerBinding {
        id: model.id,
        created_at: model.created_at.with_timezone(&Utc),
        updated_at: model.updated_at.with_timezone(&Utc),
        created_by_id: model.created_by_id,
        updated_by_id: model.updated_by_id,
        // Deleted bindings never reach here (the lock filters them out).
        deleted_at: model.deleted_at.map(|t| t.with_timezone(&Utc)),
        workspace_id,
        project_id: model.project_id,
        scheduler_id: model.scheduler_id,
        dtstart: model.dtstart.with_timezone(&Utc),
        tzid: model.tzid.clone(),
        rrule: model.rrule.clone(),
        rdates: model.rdates.clone(),
        exdates: model.exdates.clone(),
        extra_context: model.extra_context.clone(),
        enabled: model.enabled,
        outcome_mode: model.outcome_mode.clone(),
        next_run_at: model.next_run_at.map(|t| t.with_timezone(&Utc)),
        last_run_id: model.last_run_id,
        last_error: model.last_error.clone(),
        actor_id: model.actor_id,
        pod_id: model.pod_id,
    })
}

fn scheduler_from_model(model: &schedulers::Model) -> app_sched::SchedulerRow {
    app_sched::SchedulerRow {
        created_at: model.created_at.with_timezone(&Utc),
        updated_at: model.updated_at.with_timezone(&Utc),
        created_by_id: model.created_by_id,
        updated_by_id: model.updated_by_id,
        deleted_at: model.deleted_at.map(|t| t.with_timezone(&Utc)),
        id: model.id,
        workspace_id: model.workspace_id,
        slug: model.slug.clone(),
        name: model.name.clone(),
        description: model.description.clone(),
        prompt: model.prompt.clone(),
        source: model.source.clone(),
        is_enabled: model.is_enabled,
        color: model.color.clone(),
    }
}

/// One blast-radius row: the rendered binding plus its project.
struct AffectedBindingRow {
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

pub(crate) async fn role_facts(
    pool: &PgPool,
    workspace_id: Option<&Uuid>,
    user_id: &Uuid,
    project_id: Option<&Uuid>,
) -> Result<RoleFacts, Denial> {
    // An unknown slug matches no membership row: every gate denies.
    let Some(workspace_id) = workspace_id else {
        return Ok(RoleFacts::default());
    };
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row = role_facts_stmt(
        *workspace_id,
        *user_id,
        project_id.copied().unwrap_or_else(Uuid::nil),
    );
    let decoded = match db
        .query_one(sea_orm::DbBackend::Postgres.build(&row))
        .await
        .map_err(|error| db_error(error, "v1-scheduler-roles"))?
    {
        Some(row) => RoleFactsRow::from_query_result(&row, "")
            .map_err(|error| db_error(error, "v1-scheduler-roles-decode"))?,
        None => RoleFactsRow {
            ws_admin_or_member: false,
            ws_admin: false,
            project_member: false,
            project_admin_or_member: false,
            project_admin: false,
        },
    };
    let mut facts = RoleFacts {
        ws_admin_or_member: decoded.ws_admin_or_member,
        ws_admin: decoded.ws_admin,
        ..RoleFacts::default()
    };
    if project_id.is_some() {
        facts.project_member = decoded.project_member;
        facts.project_admin_or_member = decoded.project_admin_or_member;
        facts.project_admin = decoded.project_admin;
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
pub(crate) struct WriteContext {
    pub(crate) pool: PgPool,
    pub(crate) user_id: Uuid,
    pub(crate) timezone: Tz,
    pub(crate) project_id: Option<Uuid>,
    pub(crate) workspace_id: Option<Uuid>,
}

/// Which pair of gates a route carries.
#[derive(Clone, Copy)]
pub(crate) enum Scope {
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

/// `_locked_binding`: the install (row-locked), its scheduler's
/// `is_enabled` the run-now check reads, and the workspace-slug predicate
/// the old `FOR UPDATE OF scheduler_bindings` join carried. The slug and
/// `is_enabled` legs are unlocked reads (nothing writes those rows inside
/// this transaction), so the binding row lock is the lock the write holds.
/// The predicate is then three statements in one transaction, not one.
async fn lock_binding(
    conn: &TxConn<'_>,
    scheduler_id: &Uuid,
    project_id: &Uuid,
    slug: &str,
) -> Result<LockedBinding, Denial> {
    let model = scheduler_bindings::Entity::find()
        .filter(scheduler_bindings::Column::SchedulerId.eq(*scheduler_id))
        .filter(scheduler_bindings::Column::ProjectId.eq(*project_id))
        .filter(scheduler_bindings::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(conn)
        .await
        .map_err(|error| db_error(error, "v1-binding-lock"))?
        .ok_or(Denial::BindingNotFound)?;
    let binding = binding_from_model(&model)?;
    let workspace_slug = workspaces::Entity::find_by_id(binding.workspace_id)
        .select_only()
        .columns([workspaces::Column::Slug])
        .into_model::<WorkspaceSlugRow>()
        .one(conn)
        .await
        .map_err(|error| db_error(error, "v1-binding-lock"))?;
    if workspace_slug.as_ref().is_none_or(|row| row.slug != slug) {
        return Err(Denial::BindingNotFound);
    }
    let scheduler_enabled = schedulers::Entity::find_by_id(binding.scheduler_id)
        .select_only()
        .columns([schedulers::Column::IsEnabled])
        .into_model::<SchedulerEnabledRow>()
        .one(conn)
        .await
        .map_err(|error| db_error(error, "v1-binding-lock"))?
        .map(|row| row.is_enabled)
        .ok_or(Denial::BindingNotFound)?;
    Ok(LockedBinding {
        binding,
        scheduler_enabled,
    })
}

#[derive(Debug, FromQueryResult)]
struct WorkspaceSlugRow {
    slug: String,
}

#[derive(Debug, FromQueryResult)]
struct SchedulerEnabledRow {
    is_enabled: bool,
}

/// The binding as the v1 reads render it.
async fn render_row<C: ConnectionTrait>(conn: &C, binding_id: &Uuid) -> Result<BindingRow, Denial> {
    super::handlers::binding_select()
        .filter(scheduler_bindings::Column::DeletedAt.is_null())
        .filter(scheduler_bindings::Column::Id.eq(*binding_id))
        .into_model::<BindingRow>()
        .one(conn)
        .await
        .map_err(|error| db_error(error, "v1-binding-render"))?
        .ok_or(Denial::ObjectNotFound)
}

/// The definition edit's blast radius: `_binding_queryset()
/// .filter(scheduler=...).select_related("project").order_by("project__name")`
/// — every live install, whichever projects the caller belongs to, with
/// the project's identifier and name. The bindings and their projects are
/// two statements in one call (the project leg joins in Rust), not one.
async fn bindings_of_scheduler(
    pool: &PgPool,
    scheduler_id: &Uuid,
) -> Result<Vec<AffectedBindingRow>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let bindings = super::handlers::binding_select()
        .filter(scheduler_bindings::Column::DeletedAt.is_null())
        .filter(scheduler_bindings::Column::SchedulerId.eq(*scheduler_id))
        .into_model::<BindingRow>()
        .all(&db)
        .await
        .map_err(|error| db_error(error, "v1-definition-bindings"))?;
    let project_ids: Vec<Uuid> = bindings.iter().map(|row| row.project_id).collect();
    #[derive(Debug, FromQueryResult)]
    struct ProjectRow {
        id: Uuid,
        identifier: String,
        name: String,
    }
    let project_rows = projects::Entity::find()
        .select_only()
        .columns([
            projects::Column::Id,
            projects::Column::Identifier,
            projects::Column::Name,
        ])
        .filter(projects::Column::Id.is_in(project_ids.iter().copied()))
        .into_model::<ProjectRow>()
        .all(&db)
        .await
        .map_err(|error| db_error(error, "v1-definition-bindings"))?;
    let by_project: std::collections::HashMap<Uuid, (String, String)> = project_rows
        .into_iter()
        .map(|row| (row.id, (row.identifier, row.name)))
        .collect();
    let mut affected: Vec<AffectedBindingRow> = bindings
        .into_iter()
        .filter_map(|binding| {
            let (identifier, name) = by_project.get(&binding.project_id)?.clone();
            Some(AffectedBindingRow {
                binding,
                project_identifier: identifier,
                project_name: name,
            })
        })
        .collect();
    // `ORDER BY project__name ASC` (the old single statement ordered in
    // the database; here the join leg is in Rust).
    affected.sort_by(|left, right| left.project_name.cmp(&right.project_name));
    Ok(affected)
}

/// `get_object_or_404(Scheduler.objects.select_for_update(), pk=...,
/// workspace__slug=...)`: the locked definition row. The slug predicate
/// the old `FOR UPDATE` join carried is checked against the resolved
/// workspace id before the lock (two statements in one transaction).
async fn lock_scheduler(
    conn: &TxConn<'_>,
    scheduler_id: &Uuid,
    slug: &str,
) -> Result<app_sched::SchedulerRow, Denial> {
    let workspace_id = workspaces::Entity::find()
        .select_only()
        .columns([workspaces::Column::Id])
        .filter(workspaces::Column::Slug.eq(slug.to_owned()))
        .into_model::<WorkspaceIdRow>()
        .one(conn)
        .await
        .map_err(|error| db_error(error, "v1-definition-lock"))?
        .map(|row| row.id)
        .ok_or(Denial::SchedulerNotFound)?;
    let model = schedulers::Entity::find_by_id(*scheduler_id)
        .filter(schedulers::Column::WorkspaceId.eq(workspace_id))
        .filter(schedulers::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(conn)
        .await
        .map_err(|error| db_error(error, "v1-definition-lock"))?
        .ok_or(Denial::SchedulerNotFound)?;
    Ok(scheduler_from_model(&model))
}

#[derive(Debug, FromQueryResult)]
struct WorkspaceIdRow {
    id: Uuid,
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
    let token = if_match_header(&headers);
    respond(binding_patch_inner(&context, &slug, &scheduler_id, &data, token).await)
}

/// The binding-edit body past auth, the gates, the flag and body parsing.
/// Shared by the v1 handler above and the [`super::service`] entry (same
/// statements, so the two surfaces cannot drift).
pub(crate) async fn binding_patch_inner(
    context: &WriteContext,
    slug: &str,
    scheduler_id: &Uuid,
    data: &Value,
    if_match: Option<String>,
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
    let binding = {
        let conn = TxConn::new(&mut tx);
        lock_binding(&conn, scheduler_id, &project_id, slug)
            .await?
            .binding
    };
    let token = if_match.or_else(|| body_token(Some(body)));
    if let Some(denial) = precondition_denial(token, &binding.updated_at, "scheduler binding") {
        return Err(denial);
    }
    let before_row = {
        let conn = TxConn::new(&mut tx);
        render_row(&conn, &binding.id).await?
    };
    let before = match render_binding(&before_row, &context.timezone) {
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
    let fresh_db = pidash_db::pool::sea_orm_connection(&context.pool);
    let fresh = render_row(&fresh_db, &binding.id).await?;
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
    // `request.data` is read only when no `If-Match` header was sent. A
    // media type Django must parse is proxied. This juggling needs the
    // request, so it stays handler-side while the lock moves into the
    // core with the write it guards; responses are identical either way.
    let token = match if_match_header(&headers) {
        Some(token) => Some(token),
        None => {
            if !json_or_empty(&headers) {
                return crate::edge::proxy(State(state), req).await;
            }
            match app_bind::read_request_data(&state, req).await {
                Ok(data) => body_token(data.as_object()),
                Err(response) => return response,
            }
        }
    };
    respond(run_now_core(&context, &slug, &scheduler_id, token).await)
}

/// The run-now body past auth, the gates, the flag and token parsing:
/// lock, precondition, conflict probe, write. Shared by the v1 handler
/// above and the [`super::service`] entry (same statements, so the two
/// surfaces cannot drift).
pub(crate) async fn run_now_core(
    context: &WriteContext,
    slug: &str,
    scheduler_id: &Uuid,
    if_match: Option<String>,
) -> Result<Response, Denial> {
    let project_id = context.project_id.ok_or(Denial::ServerError)?;
    let mut tx = context
        .pool
        .begin()
        .await
        .map_err(|error| db_error(error, "v1-run-now-begin"))?;
    let locked = {
        let conn = TxConn::new(&mut tx);
        lock_binding(&conn, scheduler_id, &project_id, slug).await?
    };
    let binding = locked.binding;
    let scheduler_enabled = locked.scheduler_enabled;
    if let Some(denial) = precondition_denial(if_match, &binding.updated_at, "scheduler binding") {
        return Err(denial);
    }
    run_now_write(context, tx, &binding, scheduler_enabled).await
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
        let conn = TxConn::new(&mut tx);
        let active = agent_run::Entity::find()
            .filter(agent_run::Column::SchedulerBindingId.eq(binding.id))
            .filter(agent_run::Column::Status.is_in(statuses))
            .exists(&conn)
            .await
            .map_err(|error| db_error(error, "v1-run-now-active"))?;
        active
    } else {
        false
    };
    if let Some((code, message)) =
        run_now_conflict_of(binding, scheduler_enabled, run_active, Utc::now())
    {
        return Err(conflict(code, message));
    }
    let before = {
        let conn = TxConn::new(&mut tx);
        render_row(&conn, &binding.id).await?
    };
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
    let fresh_db = pidash_db::pool::sea_orm_connection(&context.pool);
    let fresh = render_row(&fresh_db, &binding.id).await?;
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
    let token = if_match_header(&headers);
    respond(definition_patch_inner(&context, &slug, &scheduler_id, &data, token).await)
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

/// The definition-edit body past auth, the gates, the flag and body
/// parsing. Shared by the v1 handler above and the [`super::service`]
/// entry (same statements, so the two surfaces cannot drift).
pub(crate) async fn definition_patch_inner(
    context: &WriteContext,
    slug: &str,
    scheduler_id: &Uuid,
    data: &Value,
    if_match: Option<String>,
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
    let record = {
        let conn = TxConn::new(&mut tx);
        lock_scheduler(&conn, scheduler_id, slug).await?
    };
    if is_builtin_slug(&record.slug) {
        return Err(conflict("builtin_scheduler", BUILTIN_MESSAGE));
    }
    let token = if_match.or_else(|| body_token(Some(body)));
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
    let saved = {
        let conn = TxConn::new(&mut tx);
        schedulers::Entity::update_many()
            .col_expr(schedulers::Column::UpdatedAt, Expr::value(now))
            .col_expr(
                schedulers::Column::UpdatedById,
                Expr::value(Some(context.user_id)),
            )
            .col_expr(schedulers::Column::Slug, Expr::value(write.slug.clone()))
            .col_expr(schedulers::Column::Name, Expr::value(write.name.clone()))
            .col_expr(
                schedulers::Column::Description,
                Expr::value(write.description.clone()),
            )
            .col_expr(
                schedulers::Column::Prompt,
                Expr::value(write.prompt.clone()),
            )
            .col_expr(schedulers::Column::Color, Expr::value(write.color.clone()))
            .col_expr(schedulers::Column::IsEnabled, Expr::value(write.is_enabled))
            .filter(schedulers::Column::Id.eq(*scheduler_id))
            .exec(&conn)
            .await
    };
    if let Err(error) = saved {
        return Err(if pidash_db::orm_tx::is_integrity_violation(&error) {
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
    let affected = bindings_of_scheduler(&context.pool, scheduler_id).await?;
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

/// The definition-create body past auth, the gates, the flag and body
/// parsing. Shared by the v1 handler above and the [`super::service`]
/// entry (same statements, so the two surfaces cannot drift).
pub(crate) async fn definition_create_inner(
    context: &WriteContext,
    data: &Value,
) -> Result<Response, Denial> {
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
    // The token is parsed before the lock now (it needs the request);
    // responses are identical either way.
    let token = match delete_token(&state, &headers, req).await {
        Ok(token) => token,
        Err(other) => {
            return match *other {
                Ok(req) => crate::edge::proxy(State(state), req).await,
                Err(response) => response,
            };
        }
    };
    respond(definition_delete_core(&context, &slug, &scheduler_id, token).await)
}

/// The definition-delete body past auth, the gates, the flag and token
/// parsing. Shared by the v1 handler above and the [`super::service`]
/// entry (same statements, so the two surfaces cannot drift).
pub(crate) async fn definition_delete_core(
    context: &WriteContext,
    slug: &str,
    scheduler_id: &Uuid,
    if_match: Option<String>,
) -> Result<Response, Denial> {
    let mut tx = context
        .pool
        .begin()
        .await
        .map_err(|error| db_error(error, "v1-definition-delete-begin"))?;
    let record = {
        let conn = TxConn::new(&mut tx);
        lock_scheduler(&conn, scheduler_id, slug).await?
    };
    if is_builtin_slug(&record.slug) {
        return Err(conflict("builtin_scheduler", BUILTIN_MESSAGE));
    }
    if let Some(denial) = precondition_denial(if_match, &record.updated_at, "scheduler") {
        return Err(denial);
    }
    app_sched::soft_delete_scheduler(&mut tx, scheduler_id, &context.user_id)
        .await
        .map_err(from_sched)?;
    tx.commit()
        .await
        .map_err(|error| db_error(error, "v1-definition-delete-commit"))?;
    // Instance `delete()` → `.delay(...)` after the stamp, as the app does.
    app_sched::enqueue_soft_delete(&context.pool, "scheduler", scheduler_id).await;
    Ok(no_content())
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
#[derive(Debug, FromQueryResult)]
struct SchedulerRefRow {
    id: Uuid,
    is_enabled: bool,
}

async fn scheduler_by_ref(
    pool: &PgPool,
    workspace_id: &Uuid,
    reference: &str,
) -> Result<Option<(Uuid, bool)>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let query = match app_bind::uuid_from_hex_forms(reference) {
        Some(id) => schedulers::Entity::find()
            .select_only()
            .columns([schedulers::Column::Id, schedulers::Column::IsEnabled])
            .filter(schedulers::Column::DeletedAt.is_null())
            .filter(schedulers::Column::WorkspaceId.eq(*workspace_id))
            .filter(schedulers::Column::Id.eq(id))
            .order_by_desc(schedulers::Column::CreatedAt)
            .limit(1)
            .into_model::<SchedulerRefRow>(),
        None => schedulers::Entity::find()
            .select_only()
            .columns([schedulers::Column::Id, schedulers::Column::IsEnabled])
            .filter(schedulers::Column::DeletedAt.is_null())
            .filter(schedulers::Column::WorkspaceId.eq(*workspace_id))
            .filter(schedulers::Column::Slug.eq(reference.to_owned()))
            .order_by_desc(schedulers::Column::CreatedAt)
            .limit(1)
            .into_model::<SchedulerRefRow>(),
    };
    let row = query
        .one(&db)
        .await
        .map_err(|error| db_error(error, "v1-install-scheduler"))?;
    Ok(row.map(|row| (row.id, row.is_enabled)))
}

/// The id a UUID-or-slug scheduler ref names, for the non-HTTP entries
/// (NEWPIDASH-37 review: the service resolves post-gate, so a slug probe
/// answers the route's 403 instead of leaking existence).
///
/// A ref that parses as a UUID (`uuid.UUID(ref)`, the install path's
/// rule) passes straight through with NO existence check: the cores
/// answer their own 404s for unknown ids. Anything else goes through
/// [`scheduler_by_ref`], whose slug leg is the install path's exact
/// lookup (live rows, `ORDER BY created_at DESC LIMIT 1`); `None` is a
/// slug miss, which each service entry maps to its core's unknown-id
/// denial.
pub(crate) async fn resolve_scheduler_ref(
    pool: &PgPool,
    workspace_id: &Uuid,
    reference: &str,
) -> Result<Option<Uuid>, Denial> {
    if let Some(id) = app_bind::uuid_from_hex_forms(reference) {
        return Ok(Some(id));
    }
    // Not UUID-parseable, so `scheduler_by_ref` takes its slug leg.
    let row = scheduler_by_ref(pool, workspace_id, reference).await?;
    Ok(row.map(|(id, _enabled)| id))
}

/// The install body past auth, the gates, the flag and body parsing.
/// Shared by the v1 handler above and the [`super::service`] entry (same
/// statements, so the two surfaces cannot drift).
pub(crate) async fn binding_install_inner(
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
    let installed_db = pidash_db::pool::sea_orm_connection(&context.pool);
    let installed = scheduler_bindings::Entity::find()
        .filter(scheduler_bindings::Column::DeletedAt.is_null())
        .filter(scheduler_bindings::Column::SchedulerId.eq(scheduler_id))
        .filter(scheduler_bindings::Column::ProjectId.eq(project_id))
        .exists(&installed_db)
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
    let fresh_db = pidash_db::pool::sea_orm_connection(&context.pool);
    let fresh = render_row(&fresh_db, &saved.row.id).await?;
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
    // The token is parsed before the lock now (it needs the request);
    // responses are identical either way.
    let token = match delete_token(&state, &headers, req).await {
        Ok(token) => token,
        Err(other) => {
            return match *other {
                Ok(req) => crate::edge::proxy(State(state), req).await,
                Err(response) => response,
            };
        }
    };
    respond(binding_uninstall_core(&context, &slug, &scheduler_id, token).await)
}

/// The uninstall body past auth, the gates, the flag and token parsing.
/// Shared by the v1 handler above and the [`super::service`] entry (same
/// statements, so the two surfaces cannot drift).
pub(crate) async fn binding_uninstall_core(
    context: &WriteContext,
    slug: &str,
    scheduler_id: &Uuid,
    if_match: Option<String>,
) -> Result<Response, Denial> {
    let project_id = context.project_id.ok_or(Denial::ServerError)?;
    let mut tx = context
        .pool
        .begin()
        .await
        .map_err(|error| db_error(error, "v1-uninstall-begin"))?;
    let binding = {
        let conn = TxConn::new(&mut tx);
        lock_binding(&conn, scheduler_id, &project_id, slug)
            .await?
            .binding
    };
    if let Some(denial) = precondition_denial(if_match, &binding.updated_at, "scheduler binding") {
        return Err(denial);
    }
    // `binding.delete()`: two `now()` samples (deleted_at, then save's
    // updated_at), then the related-objects sweep after commit.
    let now = Utc::now();
    let now2 = Utc::now();
    app_bind::uninstall_binding(&mut *tx, &binding.id, &context.user_id, &now, &now2)
        .await
        .map_err(from_app)?;
    tx.commit()
        .await
        .map_err(|error| db_error(error, "v1-uninstall-commit"))?;
    app_bind::enqueue_soft_delete(&context.pool, &binding.id).await;
    Ok(no_content())
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

    async fn live_pool() -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        let migrated: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.workspace_members')::text")
                .fetch_one(&pool)
                .await
                .expect("probe");
        if migrated.is_none() {
            eprintln!("skipping live-db test: the database is not migrated");
            return None;
        }
        Some(pool)
    }

    #[tokio::test]
    async fn live_role_facts_read_memberships() {
        let Some(pool) = live_pool().await else {
            return;
        };
        for table in ["workspace_members", "project_members"] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let workspace_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO workspace_members (id, workspace_id, member_id, role, is_active, created_at, updated_at, view_props, default_props, issue_props, explored_features, getting_started_checklist, tips) \
             VALUES ($1, $2, $3, 15, true, $4, $4, '{}', '{}', '{}', '{}', '{}', '{}')",
        )
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(user_id)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed workspace membership");
        sqlx::query(
            "INSERT INTO project_members (id, workspace_id, member_id, project_id, role, is_active, created_at, updated_at, view_props, default_props, sort_order, preferences) \
             VALUES ($1, $2, $3, $4, 20, true, $5, $5, '{}', '{}', 0, '{}')",
        )
        .bind(Uuid::new_v4())
        .bind(workspace_id)
        .bind(user_id)
        .bind(project_id)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed project membership");
        let facts = role_facts(&pool, Some(&workspace_id), &user_id, Some(&project_id))
            .await
            .expect("role facts");
        assert!(facts.ws_admin_or_member, "member passes");
        assert!(!facts.ws_admin, "workspace member is not admin");
        assert!(facts.project_member);
        assert!(facts.project_admin_or_member);
        assert!(facts.project_admin, "project admin passes");
        assert!(project_class_allows(&facts));
        assert!(project_admin_decorator_allows(&facts));
        // No project: the project gates stay closed.
        let bare = role_facts(&pool, Some(&workspace_id), &user_id, None)
            .await
            .expect("role facts");
        assert!(bare.ws_admin_or_member);
        assert!(!bare.project_member);
        assert!(!bare.project_admin);
        // Unknown workspace: every gate denies.
        let denied = role_facts(&pool, None, &user_id, None)
            .await
            .expect("role facts");
        assert_eq!(denied, RoleFacts::default());
    }
}
