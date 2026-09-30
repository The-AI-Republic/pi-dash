#![forbid(unsafe_code)]

//! Module CRUD handlers (D-28, stage 5, PIDASHCONV-391).
//!
//! Ports `ModuleViewSet` from
//! `apps/api/pi_dash/app/views/module/base.py:71-759`:
//!
//! - `get_serializer_class` (`:75-76`): write serializer on
//!   create/update/partial_update, read serializer otherwise.
//! - `get_queryset` (`:78-292`): tenant scope (project + workspace slug),
//!   `is_favorite` `Exists`, five group `Count`s + total, six estimate
//!   `Sum`s, `member_ids` `ArrayAgg`, `-is_favorite,-created_at` order.
//!   SQL fragments come from
//!   `pidash_services::app_modules::queries` (FX-MOD-03, PIDASHCONV-374);
//!   the services crate owns the text, this module only binds it.
//! - `create` (`:294-351`): ADMIN/MEMBER gate, write validation, duplicate
//!   name check, annotated-row re-read, `model_activity` enqueue.
//! - `list` (`:353-393`): ADMIN/MEMBER/GUEST gate, archived excluded, bare
//!   JSON array (NOT paginated — `Response(modules)` directly), `fields=`
//!   accepted-but-discarded (ported bug, `DYNAMIC_FIELDS_KWARG_HONORED`).
//! - `retrieve` (`:395-649`): ADMIN/MEMBER gate, archived excluded,
//!   `ModuleDetailSerializer` + `estimate_distribution` +
//!   `distribution`, `recent_visited_task` deferred publish.
//! - `partial_update` (`:651-721`): ADMIN/MEMBER gate, archived guard,
//!   before-save snapshot into `model_activity`, annotated-row re-read.
//! - `update` (no override): DRF `ModelViewSet.update` default over the
//!   write serializer under `IsAuthenticated` only (ported quirk — any
//!   authenticated caller, no archived guard), rendering the bare write
//!   shape, not the annotated row.
//! - `destroy` (`:723-759`): ADMIN + creator fast-path gate, per-issue
//!   `issue_activity.deleted` publishes, soft-delete module + bridges,
//!   soft-delete requester favorites + hard-delete all recent visits, 204 empty.
//!
//! Routes (`app/urls/module.py:19-35`): collection
//! `workspaces/<slug>/projects/<project_id>/modules/` (GET list, POST
//! create) and detail `.../modules/<pk>/` (GET retrieve, PUT update,
//! PATCH partial_update, DELETE destroy). Every other method on those
//! paths proxies to Django.
//!
//! Fixture ids: FX-MOD-06
//! (`rust-api/fixtures/app_modules/handlers/module_crud.golden.json`);
//! FX-MOD-05 enqueues (`tasks/enqueue_payloads.golden.json`) via
//! `pidash_services::app_modules::tasks` (PIDASHCONV-384); gates via
//! `super::gates` (FX-MOD-04, PIDASHCONV-379).
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * PUT has no gate and no archived guard: any authenticated user can
//!   full-update, and it renders the bare write shape
//!   (`WRITE_RESPONSE_ORDER`, incl `created_by`/`updated_by`/
//!   `project`/`workspace`/`lead`/`members`/`member_ids`), never the
//!   annotated row.
//! * `list` ignores `fields=` (the constructor kwarg is overwritten with
//!   `expand`); the full 28-key row always renders.
//! * `validate` only sees input dates: a partial update carrying a single
//!   date skips the start/target check.
//! * Duplicate-name checks race outside a transaction (check-then-insert).
//! * `update` (PUT) replaces memberships through the same soft-delete +
//!   `bulk_create(ignore_conflicts)` path as `member_ids`.
//! * `retrieve` evaluates the row query twice over (existence probe, then
//!   the detail read) — kept as sequencing, not deduped.
//! * `destroy`'s creator fast-path bypasses the role check entirely: a
//!   MEMBER creator is ALLOWED (`base.py:19-88` — there is no
//!   fall-through after `if obj: return view_func(...)`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::Router;
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::Row;

use pidash_services::app_modules::{queries, shape, tasks};

use super::gates;
use crate::app_issues::Denial;
use crate::state::AppState;

/// Collection path in `app/urls/module.py:19-23` form.
pub const MODULES_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/modules/";
/// Detail path in `app/urls/module.py:24-35` form.
pub const MODULE_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/";

/// `model_activity` publishes under `model_name="module"`
/// (`base.py:340,709`).
pub const MODULE_MODEL_NAME: &str = tasks::MODULE_MODEL_NAME;

/// `POST`/`GET` on the collection path and
/// `GET`/`PUT`/`PATCH`/`DELETE` on the detail path are owned; every other
/// method on those paths proxies to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/modules/",
            axum::routing::post(module_create)
                .get(module_list)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
            axum::routing::get(module_retrieve)
                .put(module_update)
                .patch(module_partial_update)
                .delete(module_destroy)
                .post(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

fn gate_for_create() -> &'static gates::Gate {
    &gates::gate_for("POST", "workspaces/<slug>/projects/<project_id>/modules/")
        .expect("module create gate")
        .gate
}

fn gate_for_list() -> &'static gates::Gate {
    &gates::gate_for("GET", "workspaces/<slug>/projects/<project_id>/modules/")
        .expect("module list gate")
        .gate
}

fn gate_for_retrieve() -> &'static gates::Gate {
    &gates::gate_for("GET", "workspaces/<slug>/projects/<project_id>/modules/<pk>/")
        .expect("module retrieve gate")
        .gate
}

fn gate_for_partial_update() -> &'static gates::Gate {
    &gates::gate_for(
        "PATCH",
        "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
    )
    .expect("module partial_update gate")
    .gate
}

fn gate_for_destroy() -> &'static gates::Gate {
    &gates::gate_for(
        "DELETE",
        "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
    )
    .expect("module destroy gate")
    .gate
}

// ---------------------------------------------------------------------------
// Shared request plumbing (pilot-2 / D-27 precedent)
// ---------------------------------------------------------------------------

pub(crate) type HandlerResult = Result<Response, Denial>;

pub(crate) fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

pub(crate) fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(Vec::new()))
        .expect("empty response")
}

/// `request.user` from the Django session: missing session, missing key,
/// or a non-UUID id is anonymous → 401.
pub(crate) fn actor_user_id(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<uuid::Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    session
        .get("_auth_user_id")
        .and_then(|value| value.as_str())
        .and_then(|raw| raw.parse::<uuid::Uuid>().ok())
        .ok_or(Denial::Unauthorized)
}

/// `_rewrite_project_kwarg` (`app/views/base.py:49-80`): authenticated
/// callers only; UUIDs pass through unchecked; other identifiers resolve
/// `UPPER(identifier)` in the workspace, else `Http404` (detail 404).
pub(crate) async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = raw.trim().to_uppercase();
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::NotFoundDetail)
}

/// Badly-formed UUIDs in paths render the `ValidationError` branch
/// (`app/views/base.py:126-130`): 400 `{"error": "Please provide valid
/// detail"}`.
pub(crate) const INVALID_DETAIL_MSG: &str = "Please provide valid detail";

pub(crate) fn parse_uuid_or_invalid(raw: &str) -> Result<uuid::Uuid, Denial> {
    raw.parse::<uuid::Uuid>()
        .map_err(|_| Denial::BadError(INVALID_DETAIL_MSG.to_owned()))
}

/// The role list one allow-gate checks (`allow_facts_for` in `gates.rs`
/// builds fixture facts the same way): `Project`/`ProjectCreator` check
/// their own roles, `Open` checks none.
fn gate_roles(gate: &gates::Gate) -> &[i32] {
    match gate {
        gates::Gate::Project { roles } | gates::Gate::ProjectCreator { roles } => roles,
        gates::Gate::Open | gates::Gate::Entity | gates::Gate::Lite => &[],
    }
}

/// Membership facts for one `(user, slug, project)` over the same rows the
/// decorator reads (`app/permissions/base.py:19-86`), with the
/// allowed-role flags computed against the calling gate's roles.
pub(crate) async fn fetch_allow_facts(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    allowed: &[i32],
) -> Result<pidash_auth::permissions::allow::AllowFacts, Denial> {
    let project_role: Option<(i16,)> = sqlx::query_as(
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
    use pidash_auth::permissions::ROLE_ADMIN;
    let workspace_role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(pidash_auth::permissions::allow::AllowFacts {
        workspace: pidash_types::WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: workspace_role.is_some(),
        has_allowed_workspace_role: workspace_role
            .map(|(role,)| allowed.contains(&i32::from(role)))
            .unwrap_or(false),
        is_creator: false,
        has_allowed_project_role: project_role
            .map(|(role,)| allowed.contains(&i32::from(role)))
            .unwrap_or(false),
        is_project_member: project_role.is_some(),
        is_workspace_admin: workspace_role
            .map(|(role,)| i32::from(role) == ROLE_ADMIN)
            .unwrap_or(false),
    })
}

/// Run one `super::gates` allow-gate row: `Allow` runs the body, `Deny`
/// answers the decorator 403, anonymous already 401'd.
pub(crate) fn check_gate(
    gate: &gates::Gate,
    slug: &str,
    facts: &pidash_auth::permissions::allow::AllowFacts,
) -> Result<(), Denial> {
    match gates::decide_gate(gate, &gates::tenant_context(slug), facts) {
        gates::GateOutcome::Allow => Ok(()),
        gates::GateOutcome::Deny => Err(Denial::Forbidden),
        gates::GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

pub(crate) fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Best-effort deferred publish of a `.delay(...)` call (handlers
/// precedent): enqueue failures never change the response.
pub(crate) async fn enqueue_task(pool: &sqlx::PgPool, task: &str, kwargs: Map<String, Value>) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, vec![], kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; response stands");
    }
}

/// `request.user.user_timezone` (`TimezoneMixin`): unknown zones 500
/// through the same branch Django's `zoneinfo` activation raises into.
async fn actor_timezone(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> =
        sqlx::query_as(r#"SELECT u.user_timezone FROM users u WHERE u.id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// `origin=base_host(request, is_app=True)`: `WEB_URL or APP_BASE_URL`.
fn request_origin(state: &AppState) -> Result<String, Denial> {
    state
        .settings()
        .urls
        .app_base_url
        .clone()
        .ok_or(Denial::ServerError)
}
// ---------------------------------------------------------------------------
// Annotated module rows (Q1 over `get_queryset`, `base.py:78-292`)
// ---------------------------------------------------------------------------

/// Base module columns selected on every annotated read, in model order.
const MODULE_BASE_COLUMNS: &[&str] = &[
    "m.id",
    "m.workspace_id",
    "m.project_id",
    "m.name",
    "m.description",
    "m.description_text",
    "m.description_html",
    "m.start_date",
    "m.target_date",
    "m.status",
    "m.lead_id",
    "m.view_props",
    "m.sort_order",
    "m.external_source",
    "m.external_id",
    "m.logo_props",
    "m.archived_at",
    "m.created_at",
    "m.updated_at",
];

/// Annotated-row key order shared by create/list/partial_update: the
/// `.values()` calls list the same 28 keys in different orders, but
/// Django renders concrete model fields first (in call order) and the
/// requested annotations after (in queryset annotation order), so all
/// three responses share this order (verified live).
const SHARED_ROW_ORDER: &[&str] = &[
    "id", "workspace_id", "project_id", "name", "description",
    "description_text", "description_html", "start_date", "target_date",
    "status", "lead_id", "view_props", "sort_order", "external_source",
    "external_id", "logo_props", "created_at", "updated_at", "is_favorite",
    "completed_issues", "cancelled_issues", "started_issues",
    "unstarted_issues", "backlog_issues", "total_issues",
    "completed_estimate_points", "total_estimate_points", "member_ids",
];

/// Detail shell order: `ModuleSerializer.Meta.fields` + detail extras
/// (`serializers/module.py:220-273`), then the view-appended blocks.
const DETAIL_ROW_ORDER: &[&str] = shape::MODULE_LIST_FIELD_ORDER;

/// Swap the services builders' symbolic placeholders for positional
/// binds (`$1` project, `$2` slug, `$3` user, `$4` module) and re-point
/// the correlated `modules.id` at this query's `m` alias.
fn bind_placeholders(fragment: String) -> String {
    fragment
        .replace(":project_id", "$1")
        .replace(":module_id", "$4")
        .replace(":slug", "$2")
        .replace(":user", "$3")
        .replace("modules.id", "m.id")
}

/// The Q1 annotation select list: `is_favorite`, the six `Count`s, the six
/// estimate `Sum`s, `member_ids` (`base.py:216-290`).
fn annotation_selects() -> String {
    let mut selects = vec![bind_placeholders(format!(
        "{} AS is_favorite",
        queries::favorite_exists_sql()
    ))];
    // NB: `queries::COUNT_GROUPS` leads with `cancelled` while
    // `queries::COUNT_ALIASES` leads with `completed_issues`; zipping
    // them swaps the two labels. `base.py:86-135` and the live wire both
    // put `completed` first, so the pairs are spelled out explicitly
    // (PIDASHCONV-504 filed for the groups const).
    for (group, alias) in [
        (Some("completed"), "completed_issues"),
        (Some("cancelled"), "cancelled_issues"),
        (Some("started"), "started_issues"),
        (Some("unstarted"), "unstarted_issues"),
        (Some("backlog"), "backlog_issues"),
        (None, "total_issues"),
    ] {
        let filter = match group {
            Some(group) => queries::GroupFilter::Eq(group),
            None => queries::GroupFilter::All,
        };
        selects.push(bind_placeholders(queries::count_coalesce_sql(
            filter, alias,
        )));
    }
    for (group, alias) in queries::ESTIMATE_GROUPS
        .iter()
        .zip(queries::ESTIMATE_POINT_ALIASES.iter())
    {
        selects.push(bind_placeholders(queries::estimate_coalesce_sql(
            *group, alias,
        )));
    }
    selects.push(bind_placeholders(format!(
        "{} AS member_ids",
        queries::member_ids_sql(true)
    )));
    selects.join(", ")
}

/// Fetch annotated module rows via `row_to_json`: Postgres renders every
/// column and the caller re-renders only the datetime/float keys, so key
/// order and scalar bytes stay under handler control (pilot-2 precedent).
async fn fetch_module_rows(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    slug: &str,
    user_id: &uuid::Uuid,
    module_id: Option<&uuid::Uuid>,
    archived_null: bool,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let mut columns = MODULE_BASE_COLUMNS.join(", ");
    columns.push_str(", ");
    columns.push_str(&annotation_selects());
    let mut sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM (SELECT {columns} \
         FROM modules m JOIN workspaces w ON w.id = m.workspace_id \
         LEFT JOIN module_members ON module_members.module_id = m.id \
         WHERE m.project_id = $1 AND w.slug = $2 AND m.deleted_at IS NULL"
    );
    if archived_null {
        sql.push_str(" AND m.archived_at IS NULL");
    }
    if module_id.is_some() {
        sql.push_str(" AND m.id = $4");
    }
    sql.push_str(" GROUP BY m.id ORDER BY ");
    sql.push_str(&queries::MODULE_ORDER_SQL.replace("modules.", "m."));
    sql.push_str(") AS __r");
    let mut query = sqlx::query(&sql)
        .bind(project_id)
        .bind(slug)
        .bind(user_id);
    if let Some(id) = module_id {
        query = query.bind(id);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
        match value {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

trait Pipe: Sized {
    fn pipe<F, T>(self, f: F) -> T
    where
        F: FnOnce(Self) -> T,
    {
        f(self)
    }
}

impl Pipe for String {}

fn quote_json(rendered: String) -> String {
    serde_json::to_string(&rendered).unwrap_or("null".to_owned())
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// `user_timezone_converter` + DRF rendering for one value.
fn shift_datetime(value: &Value, timezone: &Tz) -> String {
    let text = match value {
        Value::String(text) => text,
        _ => return serde_json::to_string(value).unwrap_or("null".to_owned()),
    };
    match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(aware) => crate::serializer::render_datetime_in(&aware, timezone),
        Err(_) => serde_json::to_string(value).unwrap_or("null".to_owned()),
    }
    .pipe(quote_json)
}

/// Render one float exactly like DRF `FloatField` (Postgres JSON drops the
/// `.0`; `py_float_str` is the `app_issues` kernel for that).
fn render_float(value: &Value) -> String {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(float) => crate::paginator::py_float_str(float),
            None => serde_json::to_string(value).unwrap_or("null".to_owned()),
        },
        _ => serde_json::to_string(value).unwrap_or("null".to_owned()),
    }
}

/// Fields rendered through the float kernel on the annotated row.
fn is_float_field(field: &str) -> bool {
    matches!(
        field,
        "sort_order"
            | "backlog_estimate_points"
            | "unstarted_estimate_points"
            | "started_estimate_points"
            | "cancelled_estimate_points"
            | "completed_estimate_points"
            | "total_estimate_points"
    )
}

fn shape_value(field: &str, value: &Value, timezone: &Tz) -> String {
    if field == "created_at" || field == "updated_at" || field == "archived_at" {
        // `user_timezone_converter` covers created/updated; the detail
        // shell's `archived_at` renders through the same active zone.
        return shift_datetime(value, timezone);
    }
    if is_float_field(field) {
        return render_float(value);
    }
    serde_json::to_string(value).unwrap_or("null".to_owned())
}

/// Render one annotated row's keys in the view's `.values()` order as a
/// compact JSON object.
fn shape_row(row: &Map<String, Value>, fields: &[&str], timezone: &Tz) -> String {
    let mut out = String::from("{");
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(field);
        out.push_str("\":");
        out.push_str(&shape_value(
            field,
            row.get(*field).unwrap_or(&Value::Null),
            timezone,
        ));
    }
    out.push('}');
    out
}
// ---------------------------------------------------------------------------
// Write path (`ModuleWriteSerializer`, `module.py:26-120`)
// ---------------------------------------------------------------------------

/// Module status choices (`db/models/module.py:74-85`).
const MODULE_STATUS_CHOICES: &[&str] = &[
    "backlog",
    "planned",
    "in-progress",
    "paused",
    "completed",
    "cancelled",
];

/// Validated write input: `None` = key absent (keep on update), `Some`
/// = provided value. `lead_id`/`member_ids` carry the parsed UUIDs.
struct WriteInput {
    name: Option<String>,
    description: Option<String>,
    description_text: Option<Value>,
    description_html: Option<Value>,
    start_date: Option<Option<String>>,
    target_date: Option<Option<String>>,
    status: Option<String>,
    lead_id: Option<Option<uuid::Uuid>>,
    member_ids: Option<Vec<uuid::Uuid>>,
    view_props: Option<Value>,
    sort_order: Option<f64>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    logo_props: Option<Value>,
}

/// One field's error node, in DRF field order: usually a string array,
/// or an index-keyed object for per-item list errors (`module.py`
/// `member_ids` children render `{0: [...]}`).
type FieldErrors = Vec<(String, Value)>;

fn push_error(errors: &mut FieldErrors, field: &str, message: String) {
    let message = Value::String(message);
    match errors.iter_mut().find(|(name, _)| name == field) {
        Some((_, Value::Array(list))) => list.push(message),
        Some((_, node)) => {
            let first = std::mem::replace(node, Value::Null);
            *node = Value::Array(vec![first, message]);
        }
        None => errors.push((field.to_owned(), Value::Array(vec![message]))),
    }
}

/// Per-item list error (`ListField` children): `{"0": ["..."]}`.
fn push_index_error(errors: &mut FieldErrors, field: &str, index: usize, message: String) {
    let key = index.to_string();
    match errors.iter_mut().find(|(name, _)| name == field) {
        Some((_, Value::Object(map))) => match map.get_mut(&key) {
            Some(Value::Array(list)) => list.push(Value::String(message)),
            _ => {
                map.insert(key, Value::Array(vec![Value::String(message)]));
            }
        },
        Some((_, node)) => {
            let first = std::mem::replace(node, Value::Null);
            let mut map = Map::with_capacity(2);
            map.insert(String::from("_"), first);
            map.insert(key, Value::Array(vec![Value::String(message)]));
            *node = Value::Object(map);
        }
        None => {
            let mut map = Map::new();
            map.insert(key, Value::Array(vec![Value::String(message)]));
            errors.push((field.to_owned(), Value::Object(map)));
        }
    }
}

fn render_field_errors(errors: &FieldErrors) -> String {
    let mut out = String::from("{");
    for (index, (field, node)) in errors.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(field));
        out.push(':');
        out.push_str(&serde_json::to_string(node).unwrap_or("null".to_owned()));
    }
    out.push('}');
    out
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_i64() || number.is_u64() => "int",
        Value::Number(_) => "float",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `{"non_field_errors": ["Invalid data. Expected a dictionary, but got
/// <type>."]}` for non-object write bodies (verified live).
fn not_dict_body(value: &Value) -> String {
    format!(
        "{{\"non_field_errors\":[{}]}}",
        json_string(&format!(
            "Invalid data. Expected a dictionary, but got {}.",
            json_type_name(value)
        ))
    )
}

/// `Http404` for the default update's `get_object()` miss
/// (`No Module matches the given query.`, verified live).
const NO_MODULE_DETAIL_BODY: &str = "{\"detail\":\"No Module matches the given query.\"}";

/// DRF `CharField` string coercion: strings pass, ints/floats stringify,
/// everything else (bool, list, dict, null) fails `Not a valid string.`
fn coerce_string(value: &Value) -> Result<String, ()> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        _ => Err(()),
    }
}

fn validate_string_field(
    errors: &mut FieldErrors,
    field: &str,
    value: &Value,
    max_length: usize,
    allow_blank: bool,
) -> Option<String> {
    match coerce_string(value) {
        Err(()) => {
            push_error(errors, field, "Not a valid string.".to_owned());
            None
        }
        Ok(text) => {
            if text.is_empty() && !allow_blank {
                push_error(errors, field, "This field may not be blank.".to_owned());
                return None;
            }
            if text.chars().count() > max_length {
                push_error(
                    errors,
                    field,
                    format!("Ensure this field has no more than {max_length} characters."),
                );
                return None;
            }
            Some(text)
        }
    }
}

fn validate_date_field(
    errors: &mut FieldErrors,
    field: &str,
    value: &Value,
) -> Option<String> {
    match value {
        Value::Null => {
            push_error(errors, field, "This field may not be null.".to_owned());
            None
        }
        Value::String(text) => {
            match chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
                Ok(_) => Some(text.clone()),
                Err(_) => {
                    push_error(
                        errors,
                        field,
                        "Date has wrong format. Use one of these formats instead: YYYY-MM-DD."
                            .to_owned(),
                    );
                    None
                }
            }
        }
        _ => {
            push_error(
                errors,
                field,
                "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.".to_owned(),
            );
            None
        }
    }
}

/// Python `str()` rendering for scalars nested in the UUID-invalid
/// message (`str([])` is `"[]"`, `str(1.5)` is `"1.5"`).
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => format!("'{text}'"),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("'{key}': {}", py_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Whether one integer PK input reaches the row lookup: non-negative ints
/// convert via `uuid.UUID(int=…)` and miss (`Invalid pk …`), while negative
/// ints fail UUID parsing first (`UUIDField.to_python`,
/// `django/db/models/fields/__init__.py`, curly-quote branch — verified
/// against Django 4.2: `to_python(-5)` raises `“−5” is not a valid UUID.`).
fn pk_int_reaches_lookup(number: &serde_json::Number) -> bool {
    number.as_i64().map(|n| n >= 0).unwrap_or(true)
}

/// Validate one user PK (`PrimaryKeyRelatedField` over users with a UUID
/// pk field, verified live): null fails the null branch; bools fail the
/// incorrect-type branch; non-negative ints go straight to the lookup
/// (never matching a random user id); negative ints fail UUID parsing;
/// strings parse as UUIDs first (`“…” is not a valid UUID.` with curly
/// quotes); every other JSON type fails UUID validation with its Python
/// `str()` rendering.
async fn validate_pk_value(
    pool: &sqlx::PgPool,
    value: &Value,
) -> Result<uuid::Uuid, String> {
    match value {
        Value::Null => Err("This field may not be null.".to_owned()),
        Value::Bool(_) => Err("Incorrect type. Expected pk value, received bool.".to_owned()),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            if pk_int_reaches_lookup(number) {
                Err(format!("Invalid pk \"{number}\" - object does not exist."))
            } else {
                Err(format!("\u{201c}{number}\u{201d} is not a valid UUID."))
            }
        }
        Value::String(raw) => match raw.parse::<uuid::Uuid>() {
            Err(_) => Err(format!("\u{201c}{raw}\u{201d} is not a valid UUID.")),
            Ok(id) => {
                let exists: Option<(uuid::Uuid,)> =
                    sqlx::query_as(r#"SELECT id FROM users WHERE id = $1"#)
                        .bind(id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|_| "Invalid pk \"?\" - object does not exist.".to_owned())?;
                match exists {
                    Some(_) => Ok(id),
                    None => Err(format!("Invalid pk \"{raw}\" - object does not exist.")),
                }
            }
        },
        other => Err(format!("\u{201c}{}\u{201d} is not a valid UUID.", py_str(other))),
    }
}

/// Top-level Python `str()`: strings render bare, everything else like
/// [`py_repr`].
fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => py_repr(other),
    }
}

/// Rejected `status` input under DRF `ChoiceField` (`fields.py`,
/// `ChoiceField.to_internal_value`): the offending value renders with
/// Python `str()`, so booleans keep their capitals (`"True"`/`"False"`,
/// verified live against DRF 3.15.2).
fn invalid_choice_message(value: &Value) -> String {
    format!("\"{}\" is not a valid choice.", py_str(value))
}

/// One `sort_order` input under DRF `FloatField` (`fields.py:947-957`):
/// bools coerce (`float(True)` is `1.0`, verified live), numbers pass
/// through, strings parse like Python `float()` (surrounding whitespace
/// allowed, underscores between digits accepted); anything else is the
/// caller's `invalid` branch. Returns `None` only on failure. The
/// `MAX_STRING_LENGTH` guard lives with the caller, mirroring DRF's
/// order (length check runs before parsing).
fn parse_sort_order_value(value: &Value) -> Option<f64> {
    match value {
        Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => {
            let trimmed = text.trim();
            if let Ok(parsed) = trimmed.parse::<f64>() {
                return Some(parsed);
            }
            strip_float_underscores(trimmed).and_then(|stripped| stripped.parse::<f64>().ok())
        }
        _ => None,
    }
}

/// Python `float()` underscores: each must sit between two ASCII digits
/// (`float("1_0")` is `10.0`; `"1__0"`, `"_1"`, `"1_"` all fail).
fn strip_float_underscores(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    if !chars.contains(&'_') {
        return None;
    }
    for (index, char) in chars.iter().enumerate() {
        if *char == '_'
            && !(matches!(chars.get(index.wrapping_sub(1)), Some(left) if left.is_ascii_digit())
                && matches!(chars.get(index + 1), Some(right) if right.is_ascii_digit()))
        {
            return None;
        }
    }
    Some(chars.iter().filter(|char| **char != '_').collect())
}

/// Validate the write body (`ModuleWriteSerializer` field rules,
/// `module.py:26-62`). `partial` skips the required check on absent keys
/// (PATCH); full updates (POST/PUT) require `name`.
async fn validate_write(
    pool: &sqlx::PgPool,
    body: &Map<String, Value>,
    partial: bool,
) -> Result<WriteInput, String> {
    let mut errors: FieldErrors = Vec::new();

    // Declared `lead_id` first, then the auto `lead` field (DRF field
    // order); errors key by input key, and `lead` wins when both are
    // present (verified live). The auto m2m `members` is read-only
    // (`ManyRelatedField read_only=True`, verified live), so only the
    // declared write-only `member_ids` replaces memberships.
    let mut lead_id: Option<Option<uuid::Uuid>> = None;
    for key in ["lead_id", "lead"] {
        match body.get(key) {
            // The nullable lead accepts an explicit null (clears it).
            None => continue,
            Some(Value::Null) => lead_id = Some(None),
            Some(value) => match validate_pk_value(pool, value).await {
                Ok(id) => lead_id = Some(Some(id)),
                Err(message) => {
                    push_error(&mut errors, key, message);
                }
            },
        }
    }
    let member_ids: Option<Vec<uuid::Uuid>> = match body.get("member_ids") {
        None => None,
        Some(Value::Null) => {
            push_error(
                &mut errors,
                "member_ids",
                "This field may not be null.".to_owned(),
            );
            None
        }
        Some(Value::Array(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                match validate_pk_value(pool, item).await {
                    Ok(id) => ids.push(id),
                    Err(message) => push_index_error(&mut errors, "member_ids", index, message),
                }
            }
            Some(ids)
        }
        Some(other) => {
            push_error(
                &mut errors,
                "member_ids",
                format!(
                    "Expected a list of items but got type \"{}\".",
                    json_type_name(other)
                ),
            );
            None
        }
    };

    let name = match body.get("name") {
        None if partial => None,
        None => {
            push_error(&mut errors, "name", "This field is required.".to_owned());
            None
        }
        Some(Value::Null) => {
            push_error(&mut errors, "name", "This field may not be null.".to_owned());
            None
        }
        Some(value) => validate_string_field(&mut errors, "name", value, 255, false),
    };
    let description = match body.get("description") {
        None => None,
        Some(Value::Null) => {
            push_error(
                &mut errors,
                "description",
                "This field may not be null.".to_owned(),
            );
            None
        }
        // `TextField`: no max length; numbers coerce like `CharField`.
        Some(value) => match coerce_string(value) {
            Ok(text) => Some(text),
            Err(()) => {
                push_error(
                    &mut errors,
                    "description",
                    "Not a valid string.".to_owned(),
                );
                None
            }
        },
    };
    let description_text = body.get("description_text").cloned();
    let description_html = body.get("description_html").cloned();
    let start_date = match body.get("start_date") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => validate_date_field(&mut errors, "start_date", value).map(Some),
    };
    let target_date = match body.get("target_date") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => validate_date_field(&mut errors, "target_date", value).map(Some),
    };
    let status = match body.get("status") {
        None => None,
        Some(Value::Null) => {
            push_error(&mut errors, "status", "This field may not be null.".to_owned());
            None
        }
        Some(Value::String(text)) if MODULE_STATUS_CHOICES.contains(&text.as_str()) => {
            Some(text.clone())
        }
        Some(Value::String(text)) => {
            push_error(
                &mut errors,
                "status",
                format!("\"{text}\" is not a valid choice."),
            );
            None
        }
        Some(other) => {
            push_error(&mut errors, "status", invalid_choice_message(other));
            None
        }
    };
    // `JSONField` without `null=True`: explicit nulls fail (verified live).
    for key in ["view_props", "logo_props"] {
        if body.get(key) == Some(&Value::Null) {
            push_error(&mut errors, key, "This field may not be null.".to_owned());
        }
    }
    let view_props = match body.get("view_props") {
        Some(Value::Null) => None,
        other => other.cloned(),
    };
    let logo_props = match body.get("logo_props") {
        Some(Value::Null) => None,
        other => other.cloned(),
    };
    // `FloatField` without `allow_null`: explicit nulls fail the null
    // branch (verified live against DRF 3.15.2: `This field may not be
    // null.`); strings over `MAX_STRING_LENGTH` (1000 chars) fail before
    // parsing (`fields.py:949-950`); everything else parses per
    // [`parse_sort_order_value`].
    let sort_order = match body.get("sort_order") {
        None => None,
        Some(Value::Null) => {
            push_error(
                &mut errors,
                "sort_order",
                "This field may not be null.".to_owned(),
            );
            None
        }
        Some(Value::String(text)) if text.chars().count() > 1000 => {
            push_error(
                &mut errors,
                "sort_order",
                "String value too large.".to_owned(),
            );
            None
        }
        Some(value) => parse_sort_order_value(value),
    };
    if body.get("sort_order").is_some() && sort_order.is_none() && errors.iter().all(|(f, _)| f != "sort_order") {
        push_error(
            &mut errors,
            "sort_order",
            "A valid number is required.".to_owned(),
        );
    }
    let external_source = match body.get("external_source") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => {
            validate_string_field(&mut errors, "external_source", value, 255, true).map(Some)
        }
    };
    let external_id = match body.get("external_id") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(value) => {
            validate_string_field(&mut errors, "external_id", value, 255, true).map(Some)
        }
    };

    if !errors.is_empty() {
        return Err(render_field_errors(&errors));
    }
    // Object-level `validate` (`module.py:55-62`): input dates only — a
    // partial update carrying a single date skips the check (ported quirk).
    if let (Some(Some(start)), Some(Some(target))) = (start_date.as_ref(), target_date.as_ref()) {
        shape::validate_module_dates(Some(start), Some(target))
            .map_err(|_| shape::date_violation_body().to_string())?;
    }
    Ok(WriteInput {
        name,
        description,
        description_text,
        description_html,
        start_date,
        target_date,
        status,
        lead_id,
        member_ids,
        view_props,
        sort_order,
        external_source,
        external_id,
        logo_props,
    })
}
// ---------------------------------------------------------------------------
// List + create
// ---------------------------------------------------------------------------

/// Common preamble: session auth, project rewrite, route gate. Returns
/// `(pool, user_id, project_id, timezone)`.
async fn module_context(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    gate: &gates::Gate,
) -> Result<(sqlx::PgPool, uuid::Uuid, uuid::Uuid, Tz), Denial> {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let facts =
        fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    check_gate(gate, slug, &facts)?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    Ok((pool, user_id, project_id, timezone))
}

/// `ModuleViewSet.list` (`base.py:353-393`): archived excluded, bare JSON
/// array, `fields=` accepted-but-discarded.
async fn module_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let (pool, user_id, project_id, timezone) =
        module_context(&state, &slug, &project_raw, extension, gate_for_list()).await?;
    let rows = fetch_module_rows(&pool, &project_id, &slug, &user_id, None, true).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&shape_row(row, SHARED_ROW_ORDER, &timezone));
    }
    out.push(']');
    Ok(json_response(StatusCode::OK, out))
}

/// Live project row for the serializer context (`base.py:296`):
/// `Project.objects.get(workspace__slug, pk)` or the DoesNotExist 404.
async fn fetch_context_project(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
) -> Result<(uuid::Uuid, uuid::Uuid), Denial> {
    let row: Option<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
        r#"SELECT p.id, p.workspace_id FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::NotFound)
}

/// Duplicate-name probe (`module.py:68-72,99-100`): live rows only, with
/// the self exclusion on update.
async fn duplicate_name_exists(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    name: &str,
    exclude: Option<&uuid::Uuid>,
) -> Result<bool, Denial> {
    let row: Option<(uuid::Uuid,)> = if let Some(self_id) = exclude {
        sqlx::query_as(
            r#"SELECT id FROM modules
               WHERE project_id = $1 AND name = $2 AND id != $3 AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(project_id)
        .bind(name)
        .bind(self_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query_as(
            r#"SELECT id FROM modules
               WHERE project_id = $1 AND name = $2 AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    Ok(row.is_some())
}

/// Replace the membership set (`module.py:102-118`): soft-delete the old
/// bridges, then `bulk_create(ignore_conflicts=True)`.
async fn replace_members(
    pool: &sqlx::PgPool,
    module_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    member_ids: &[uuid::Uuid],
) -> Result<(), Denial> {
    // Live rows only: the (module_id, member_id, deleted_at) unique
    // constraint rejects stamping several same-member rows with one
    // now() (Django's soft-delete manager filters the same way).
    sqlx::query(r#"UPDATE module_members SET deleted_at = now() WHERE module_id = $1 AND deleted_at IS NULL"#)
        .bind(module_id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    for chunk in member_ids.chunks(shape::MEMBER_BULK_BATCH_SIZE) {
        let mut sql = String::from(
            "INSERT INTO module_members (id, workspace_id, project_id, module_id, member_id, \
             created_by_id, updated_by_id, created_at, updated_at) VALUES ",
        );
        for (index, _) in chunk.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            // Six binds per row: workspace, project, module, member,
            // created_by, updated_by (`gen_random_uuid()` is inline).
            let base = index * 6 + 1;
            sql.push_str(&format!(
                "(gen_random_uuid(), ${}, ${}, ${}, ${}, ${}, ${}, now(), now())",
                base,
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5
            ));
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        let mut query = sqlx::query(&sql);
        for member_id in chunk {
            query = query
                .bind(workspace_id)
                .bind(project_id)
                .bind(module_id)
                .bind(member_id)
                .bind(created_by)
                .bind(updated_by);
        }
        query.execute(pool).await.map_err(|_| Denial::ServerError)?;
    }
    Ok(())
}

/// `ModuleViewSet.create` (`base.py:294-351`).
async fn module_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let (pool, user_id, project_id, timezone) =
        module_context(&state, &slug, &project_raw, extension, gate_for_create()).await?;
    let data = match body.0.as_object() {
        Some(map) => map.clone(),
        None => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                not_dict_body(&body.0),
            ));
        }
    };
    let input = match validate_write(&pool, &data, false).await {
        Ok(input) => input,
        Err(errors) => return Ok(json_response(StatusCode::BAD_REQUEST, errors)),
    };
    let (_, workspace_id) = fetch_context_project(&pool, &slug, &project_id).await?;
    let name = input.name.clone().expect("name required on create");
    if duplicate_name_exists(&pool, &project_id, &name, None).await? {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            shape::duplicate_name_body().to_string(),
        ));
    }
    // `Module.save` side effect (`db/models/module.py:115-124`):
    // `MIN(sibling sort_order) - 10000` when siblings exist, else the
    // field default `65535.0`.
    let min_sort: Option<(Option<f64>,)> = sqlx::query_as(
        r#"SELECT MIN(sort_order) FROM modules WHERE project_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `Module.save` (`db/models/module.py:115-124`) only clobbers input
    // `sort_order` when siblings exist (`MIN(sibling) - 10000`); with no
    // siblings the validated input lands, defaulting to `65535.0` when
    // absent (verified live).
    let sort_order = match min_sort.and_then(|(min,)| min) {
        Some(min) => min - 10_000.0,
        None => input.sort_order.unwrap_or(65535.0),
    };
    let module_id: uuid::Uuid = uuid::Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO modules (id, workspace_id, project_id, name, description, description_text,
           description_html, start_date, target_date, status, lead_id, view_props, sort_order,
           external_source, external_id, archived_at, logo_props,
           created_by_id, updated_by_id, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
                   NULL, $16, $17, NULL, now(), now())"#,
    )
    .bind(module_id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(input.name.clone().expect("name"))
    .bind(input.description.clone().unwrap_or_default())
    .bind(input.description_text.clone().unwrap_or(Value::Null))
    .bind(input.description_html.clone().unwrap_or(Value::Null))
    .bind(parse_date_opt(&input.start_date.clone().flatten()))
    .bind(parse_date_opt(&input.target_date.clone().flatten()))
    .bind(input.status.clone().unwrap_or_else(|| "planned".to_owned()))
    .bind(input.lead_id.flatten())
    .bind(input.view_props.clone().unwrap_or(Value::Object(Map::new())))
    .bind(sort_order)
    .bind(input.external_source.clone().flatten())
    .bind(input.external_id.clone().flatten())
    .bind(input.logo_props.clone().unwrap_or(Value::Object(Map::new())))
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if let Some(members) = input.member_ids.as_ref() {
        // `created_by=module.created_by, updated_by=module.updated_by`
        // (`module.py:83-84`): the crum auto-set row, user/NULL on create.
        replace_members(
            &pool, &module_id, &project_id, &workspace_id,
            Some(user_id), None, members,
        )
        .await?;
    }
    let rows = fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), false).await?;
    let row = rows.into_iter().next().ok_or(Denial::ServerError)?;
    // `model_activity.delay(model_name="module", current_instance=None)`
    // (`base.py:339-347`).
    let origin = request_origin(&state)?;
    let emit = tasks::module_create_activity(
        &module_id.to_string(),
        Value::Object(data),
        &user_id.to_string(),
        &slug,
        &origin,
    );
    enqueue_task(&pool, emit.task_name(), emit.kwargs()).await;
    Ok(json_response(
        StatusCode::CREATED,
        shape_row(&row, SHARED_ROW_ORDER, &timezone),
    ))
}
// ---------------------------------------------------------------------------
// Retrieve extras (`base.py:395-649`)
// ---------------------------------------------------------------------------

/// `link_module` rows (`link_module` prefetch, `ModuleLink` ordering
/// `-created_at`): rendered in `LINK_KEY_ORDER` with datetimes in the
/// active zone.
async fn fetch_module_links(
    pool: &sqlx::PgPool,
    module_id: &uuid::Uuid,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT row_to_json(__r)::text AS __row FROM (
             SELECT id, created_at, updated_at, deleted_at, title, url, metadata,
                    created_by_id AS created_by, updated_by_id AS updated_by,
                    project_id AS project, workspace_id AS workspace,
                    module_id AS module
             FROM module_links WHERE module_id = $1 AND deleted_at IS NULL
             ORDER BY created_at DESC) AS __r"#,
    )
    .bind(module_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
        match value {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

fn shape_link(row: &Map<String, Value>, timezone: &Tz) -> String {
    let mut out = String::from("{");
    for (index, field) in shape::LINK_KEY_ORDER.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(field);
        out.push_str("\":");
        let value = row.get(*field).unwrap_or(&Value::Null);
        if *field == "created_at" || *field == "updated_at" || *field == "deleted_at" {
            out.push_str(&shift_datetime(value, timezone));
        } else {
            out.push_str(&serde_json::to_string(value).unwrap_or("null".to_owned()));
        }
    }
    out.push('}');
    out
}

/// `sub_issues` annotation (`base.py:401-411`): `COUNT` over live module
/// bridges of child issues under the `IssueManager` base.
async fn fetch_sub_issues(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
) -> Result<i64, Denial> {
    let sql = queries::sub_issues_sql()
        .replace(":project_id", "$1")
        .replace(":module_id", "$2");
    let row: Option<(Option<i64>,)> = sqlx::query_as(&format!("SELECT ({sql}) AS sub_issues"))
        .bind(project_id)
        .bind(module_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.and_then(|(count,)| count).unwrap_or(0))
}

/// `estimate_type` gate (`base.py:417-422`): a points estimate exists for
/// the project.
async fn fetch_estimate_type(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let sql = queries::estimate_type_exists_sql()
        .replace(":slug", "$1")
        .replace(":project_id", "$2");
    let row: Option<(Option<i32>,)> =
        sqlx::query_as(&format!("SELECT ({sql}) AS estimate_type"))
            .bind(slug)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    Ok(row.and_then(|(found,)| found).is_some())
}
/// Shared `FROM/WHERE` for the distribution queries: live module bridges
/// of this project/workspace under the `IssueManager` base
/// (`base.py:431-436,493-498,539-544,592-597`).
fn distribution_from_where(assignee_join: bool, label_join: bool) -> String {
    let mut sql = String::from(
        "FROM issues \
         JOIN projects ON projects.id = issues.project_id \
         JOIN workspaces ON workspaces.id = issues.workspace_id \
         LEFT OUTER JOIN states ON states.id = issues.state_id \
         JOIN module_issues ON module_issues.issue_id = issues.id \
         LEFT JOIN estimate_points ON estimate_points.id = issues.estimate_point_id ",
    );
    if assignee_join {
        sql.push_str(
            "LEFT JOIN issue_assignees ON issue_assignees.issue_id = issues.id \
             AND issue_assignees.deleted_at IS NULL \
             LEFT JOIN users AS assignees ON assignees.id = issue_assignees.assignee_id ",
        );
    }
    if label_join {
        sql.push_str(
            "LEFT JOIN issue_labels ON issue_labels.issue_id = issues.id \
             AND issue_labels.deleted_at IS NULL \
             LEFT JOIN labels ON labels.id = issue_labels.label_id \
             AND labels.deleted_at IS NULL ",
        );
    }
    sql.push_str("WHERE ");
    sql.push_str(&queries::issue_manager_guards_sql());
    sql.push_str(
        " AND workspaces.slug = $1 AND issues.project_id = $2 \
         AND module_issues.module_id = $3 AND module_issues.deleted_at IS NULL",
    );
    sql
}

async fn fetch_json_maps(
    pool: &sqlx::PgPool,
    sql: &str,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(sql)
        .bind(slug)
        .bind(project_id)
        .bind(module_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
        match value {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

/// Assignee distribution rows (`base.py:430-490,538-589`):
/// `estimate=true` sums estimates, else counts issues.
async fn fetch_assignee_distribution(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    estimate: bool,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let (total, completed, pending) = if estimate {
        (
            queries::estimate_sum_sql(None),
            queries::estimate_sum_sql(Some(true)),
            queries::estimate_sum_sql(Some(false)),
        )
    } else {
        (
            queries::distribution_count_sql(None),
            queries::distribution_count_sql(Some(true)),
            queries::distribution_count_sql(Some(false)),
        )
    };
    let (total_key, completed_key, pending_key) = if estimate {
        ("total_estimates", "completed_estimates", "pending_estimates")
    } else {
        ("total_issues", "completed_issues", "pending_issues")
    };
    // NB: `queries::avatar_url_case_sql()` spells the Django field name
    // (`assignees.avatar_asset`); the column is `avatar_asset_id`
    // (PIDASHCONV-502 filed for the services builder). This local CASE
    // keeps the builder's SQL semantics with the real column.
    let avatar = "CASE WHEN assignees.avatar_asset_id IS NOT NULL THEN CONCAT('/api/assets/v2/static/', assignees.avatar_asset_id, '/') WHEN assignees.avatar_asset_id IS NULL THEN assignees.avatar ELSE NULL END";
    let sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM (SELECT assignees.first_name AS first_name, \
         assignees.last_name AS last_name, assignees.id AS assignee_id, ({avatar}) AS avatar_url, \
         assignees.display_name AS display_name, {total} AS {total_key}, \
         {completed} AS {completed_key}, {pending} AS {pending_key} {} \
         GROUP BY assignees.first_name, assignees.last_name, assignees.id, \
         assignees.display_name, ({avatar}) \
         ORDER BY assignees.first_name, assignees.last_name) AS __r",
        distribution_from_where(true, false)
    );
    fetch_json_maps(pool, &sql, slug, project_id, module_id).await
}

/// Label distribution rows (`base.py:492-525,591-624`).
async fn fetch_label_distribution(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    estimate: bool,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let (total, completed, pending) = if estimate {
        (
            queries::estimate_sum_sql(None),
            queries::estimate_sum_sql(Some(true)),
            queries::estimate_sum_sql(Some(false)),
        )
    } else {
        (
            queries::distribution_count_sql(None),
            queries::distribution_count_sql(Some(true)),
            queries::distribution_count_sql(Some(false)),
        )
    };
    let (total_key, completed_key, pending_key) = if estimate {
        ("total_estimates", "completed_estimates", "pending_estimates")
    } else {
        ("total_issues", "completed_issues", "pending_issues")
    };
    let sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM (SELECT labels.name AS label_name, \
         labels.color AS color, labels.id AS label_id, {total} AS {total_key}, \
         {completed} AS {completed_key}, {pending} AS {pending_key} {} \
         GROUP BY labels.name, labels.color, labels.id ORDER BY labels.name) AS __r",
        distribution_from_where(false, true)
    );
    fetch_json_maps(pool, &sql, slug, project_id, module_id).await
}

/// Assignee row orders: `.values()` lists `avatar_url` (a `Case`
/// annotation) before `display_name` (a concrete joined field), but Django
/// renders concrete fields first in call order and annotations after, so
/// `display_name` precedes `avatar_url` on the wire (verified live;
/// PIDASHCONV-503 filed for the services key consts).
const ASSIGNEE_COUNT_ORDER: &[&str] = &[
    "first_name",
    "last_name",
    "assignee_id",
    "display_name",
    "avatar_url",
    "total_issues",
    "completed_issues",
    "pending_issues",
];
const ASSIGNEE_ESTIMATE_ORDER: &[&str] = &[
    "first_name",
    "last_name",
    "assignee_id",
    "display_name",
    "avatar_url",
    "total_estimates",
    "completed_estimates",
    "pending_estimates",
];

fn render_distribution_value(keys: &[&str], row: &Map<String, Value>) -> String {
    let mut out = String::from("{");
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(key);
        out.push_str("\":");
        let value = row.get(*key).unwrap_or(&Value::Null);
        if *key == "total_estimates"
            || *key == "completed_estimates"
            || *key == "pending_estimates"
        {
            out.push_str(&render_float(value));
        } else {
            out.push_str(&serde_json::to_string(value).unwrap_or("null".to_owned()));
        }
    }
    out.push('}');
    out
}

fn render_distribution_array(keys: &[&str], rows: &[Map<String, Value>]) -> String {
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&render_distribution_value(keys, row));
    }
    out.push(']');
    out
}

/// `burndown_plot` module branch (`utils/analytics_plot.py:198-264`,
/// `plot_type="points"` or `"issues"`): per-day remaining over the
/// module's date range, future days `null`.
#[allow(clippy::too_many_arguments)]
async fn fetch_burndown_chart(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    start: chrono::NaiveDate,
    target: chrono::NaiveDate,
    total_issues: i64,
    points: bool,
) -> Result<String, Denial> {
    let days = (target - start).num_days();
    let mut chart = String::from("{");
    if days < 0 {
        chart.push('}');
        return Ok(chart);
    }
    // Per-day completed inputs, truncated in UTC (`TIME_ZONE "UTC"`,
    // the D-27 `burndown_*_sql` precedent).
    let scope_where = distribution_from_where(false, false);
    // Points total sums ALL live module-issue estimates, not just the
    // completed ones (`analytics_plot.py:212-214` — no completed filter
    // on the total). `SUM` over no rows is NULL, and then Python's
    // `sum([])` is the INT `0`, so the chart renders ints, not `0.0`.
    let total_points: Option<f64> = if points {
        let row: Option<(Option<f64>,)> = sqlx::query_as(&format!(
            "SELECT SUM(CAST(estimate_points.value AS FLOAT)) {scope_where} \
             AND estimate_points.id IS NOT NULL"
        ))
        .bind(slug)
        .bind(project_id)
        .bind(module_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        row.and_then(|(total,)| total)
    } else {
        None
    };
    let rows: Vec<(Option<chrono::NaiveDate>, Option<f64>, Option<i64>)> = if points {
        sqlx::query_as(&format!(
            "SELECT (issues.completed_at AT TIME ZONE 'UTC')::date AS d, \
             CAST(estimate_points.value AS FLOAT) AS v, NULL::bigint AS n \
             {scope_where} AND estimate_points.id IS NOT NULL AND issues.completed_at IS NOT NULL \
             ORDER BY issues.completed_at"
        ))
        .bind(slug)
        .bind(project_id)
        .bind(module_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query_as(&format!(
            "SELECT (issues.completed_at AT TIME ZONE 'UTC')::date AS d, NULL::float AS v, \
             COUNT(*) AS n {scope_where} AND issues.completed_at IS NOT NULL \
             GROUP BY d ORDER BY d"
        ))
        .bind(slug)
        .bind(project_id)
        .bind(module_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    let today = chrono::Utc::now().date_naive();
    for (index, offset) in (0..=days).enumerate() {
        let date = start + chrono::Duration::days(offset);
        if index > 0 {
            chart.push(',');
        }
        chart.push('"');
        chart.push_str(&date.to_string());
        chart.push_str("\":");
        if date > today {
            chart.push_str("null");
            continue;
        }
        if points {
            match total_points {
                // No estimated issues: Python's int `0` arithmetic.
                None => chart.push('0'),
                Some(total) => {
                    let completed: f64 = rows
                        .iter()
                        .filter(|(d, v, _)| v.is_some() && d.is_some() && d.unwrap() <= date)
                        .map(|(_, v, _)| v.unwrap_or(0.0))
                        .sum();
                    chart.push_str(&crate::paginator::py_float_str(total - completed));
                }
            }
        } else {
            let completed: i64 = rows
                .iter()
                .filter(|(d, _, _)| d.is_some() && d.unwrap() <= date)
                .map(|(_, _, n)| n.unwrap_or(0))
                .sum();
            chart.push_str(&(total_issues - completed).to_string());
        }
    }
    chart.push('}');
    Ok(chart)
}

/// `ModuleViewSet.retrieve` (`base.py:395-649`).
async fn module_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let retrieve_gate = gate_for_retrieve();
    let facts =
        fetch_allow_facts(&pool, &slug, &project_id, &user_id, gate_roles(retrieve_gate)).await?;
    check_gate(retrieve_gate, &slug, &facts)?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let module_id = parse_uuid_or_invalid(&pk_raw)?;
    // Existence probe (`queryset.exists()`, `:414`).
    let rows = fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), true).await?;
    if rows.is_empty() {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            queries::MODULE_NOT_FOUND_BODY.to_owned(),
        ));
    }
    // The detail read (`queryset.first()` twice, `:424-425` — kept as
    // sequencing, not deduped).
    let rows = fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), true).await?;
    let row = rows.into_iter().next().ok_or(Denial::ServerError)?;
    let estimate_type = fetch_estimate_type(&pool, &slug, &project_id).await?;
    let sub_issues = fetch_sub_issues(&pool, &project_id, &module_id).await?;
    let links = fetch_module_links(&pool, &module_id).await?;

    let mut out = String::from("{");
    for (index, field) in DETAIL_ROW_ORDER.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(field);
        out.push_str("\":");
        out.push_str(&shape_value(
            field,
            row.get(*field).unwrap_or(&Value::Null),
            &timezone,
        ));
    }
    for field in shape::MODULE_DETAIL_EXTRA_FIELDS {
        out.push(',');
        out.push('"');
        out.push_str(field);
        out.push_str("\":");
        match *field {
            "link_module" => {
                out.push('[');
                for (index, link) in links.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&shape_link(link, &timezone));
                }
                out.push(']');
            }
            "sub_issues" => out.push_str(&sub_issues.to_string()),
            _ => out.push_str(&render_float(row.get(*field).unwrap_or(&Value::Null))),
        }
    }
    // `estimate_distribution` (`:427-536`).
    out.push_str(",\"estimate_distribution\":");
    if estimate_type {
        let assignees = fetch_assignee_distribution(&pool, &slug, &project_id, &module_id, true).await?;
        let labels = fetch_label_distribution(&pool, &slug, &project_id, &module_id, true).await?;
        out.push_str("{\"assignees\":");
        out.push_str(&render_distribution_array(ASSIGNEE_ESTIMATE_ORDER, &assignees));
        out.push_str(",\"labels\":");
        out.push_str(&render_distribution_array(queries::LABEL_ESTIMATE_ROW_KEYS, &labels));
        let has_dates = row.get("start_date").is_some_and(|v| !v.is_null())
            && row.get("target_date").is_some_and(|v| !v.is_null());
        if has_dates {
            let start = row
                .get("start_date")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            let target = row
                .get("target_date")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            if let (Some(start), Some(target)) = (start, target) {
                let total_issues = row
                    .get("total_issues")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let chart = fetch_burndown_chart(
                    &pool, &slug, &project_id, &module_id, start, target, total_issues, true,
                )
                .await?;
                out.push_str(",\"completion_chart\":");
                out.push_str(&chart);
            }
        }
        out.push('}');
    } else {
        out.push_str("{}");
    }
    // `distribution` (`:538-639`).
    let assignees = fetch_assignee_distribution(&pool, &slug, &project_id, &module_id, false).await?;
    let labels = fetch_label_distribution(&pool, &slug, &project_id, &module_id, false).await?;
    out.push_str(",\"distribution\":{\"assignees\":");
    out.push_str(&render_distribution_array(ASSIGNEE_COUNT_ORDER, &assignees));
    out.push_str(",\"labels\":");
    out.push_str(&render_distribution_array(queries::LABEL_COUNT_ROW_KEYS, &labels));
    out.push_str(",\"completion_chart\":");
    let has_dates = row.get("start_date").is_some_and(|v| !v.is_null())
        && row.get("target_date").is_some_and(|v| !v.is_null());
    let total_issues = row.get("total_issues").and_then(|v| v.as_i64()).unwrap_or(0);
    if has_dates && total_issues > 0 {
        let start = row
            .get("start_date")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
        let target = row
            .get("target_date")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
        if let (Some(start), Some(target)) = (start, target) {
            let chart = fetch_burndown_chart(
                &pool, &slug, &project_id, &module_id, start, target, total_issues, false,
            )
            .await?;
            out.push_str(&chart);
        } else {
            out.push_str("{}");
        }
    } else {
        out.push_str("{}");
    }
    out.push('}');
    out.push('}');
    // `recent_visited_task.delay(...)` (`:641-647`): deferred publish
    // only — the suite environment never consumes it.
    let project_wire = if project_raw.parse::<uuid::Uuid>().is_ok() {
        project_raw.clone()
    } else {
        project_id.to_string()
    };
    let visit = tasks::retrieve_visit_emit(
        &slug,
        &pk_raw,
        &user_id.to_string(),
        &project_wire,
    );
    enqueue_task(&pool, visit.task_name(), visit.kwargs()).await;
    Ok(json_response(StatusCode::OK, out))
}
// ---------------------------------------------------------------------------
// PUT (DRF default `update`) + PATCH (`partial_update`)
// ---------------------------------------------------------------------------

/// `json.dumps` with CPython defaults (`, ` / `: ` separators) for the
/// `requested_data` / `current_instance` task payloads. Local copy —
/// sibling handler issues never fork a shared helper (D-32 precedent).
fn python_dumps(value: &Value) -> String {
    let mut out = String::new();
    python_dump_into(&mut out, value);
    out
}

fn python_dump_into(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => {
            out.push('"');
            for ch in text.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_into(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_into(out, &Value::String(key.clone()));
                out.push_str(": ");
                python_dump_into(out, item);
            }
            out.push('}');
        }
    }
}

/// Live membership ids for the write shape (`to_representation`,
/// `module.py:52`): stringified ids in iteration order.
async fn fetch_live_member_ids(
    pool: &sqlx::PgPool,
    module_id: &uuid::Uuid,
) -> Result<Vec<String>, Denial> {
    let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT module_members.member_id FROM module_members
           WHERE module_members.module_id = $1 AND module_members.member_id IS NOT NULL
           AND module_members.deleted_at IS NULL"#,
    )
    .bind(module_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows.into_iter().map(|(id,)| id.to_string()).collect())
}

/// One write-shape row (`ModuleWriteSerializer` over the stored module,
/// `WRITE_RESPONSE_ORDER`): FKs as PK strings, `members` + `member_ids`
/// as id arrays, datetimes in the active zone.
async fn fetch_write_row(
    pool: &sqlx::PgPool,
    module_id: &uuid::Uuid,
    timezone: &Tz,
) -> Result<String, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT row_to_json(__r)::text AS __row FROM (
             SELECT id, lead_id, created_at, updated_at, deleted_at, name, description,
                    description_text, description_html, start_date, target_date, status,
                    view_props, sort_order, external_source, external_id, archived_at,
                    logo_props, created_by_id AS created_by, updated_by_id AS updated_by,
                    project_id AS project, workspace_id AS workspace, lead_id AS lead
             FROM modules WHERE id = $1) AS __r"#,
    )
    .bind(module_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let row = row.ok_or(Denial::ServerError)?;
    let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
    let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
    let map = match value {
        Value::Object(map) => map,
        _ => return Err(Denial::ServerError),
    };
    let member_ids = fetch_live_member_ids(pool, module_id).await?;
    let mut out = String::from("{");
    for (index, field) in shape::WRITE_RESPONSE_ORDER.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(field);
        out.push_str("\":");
        match *field {
            "members" | "member_ids" => {
                out.push('[');
                for (i, id) in member_ids.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&json_string(id));
                }
                out.push(']');
            }
            "created_at" | "updated_at" | "deleted_at" | "archived_at" => {
                out.push_str(&shift_datetime(
                    map.get(*field).unwrap_or(&Value::Null),
                    timezone,
                ));
            }
            "sort_order" => {
                out.push_str(&render_float(map.get(*field).unwrap_or(&Value::Null)));
            }
            _ => out.push_str(
                &serde_json::to_string(map.get(*field).unwrap_or(&Value::Null))
                    .unwrap_or("null".to_owned()),
            ),
        }
    }
    out.push('}');
    Ok(out)
}

/// Apply validated write fields to the stored row (`super().update` tail,
/// `module.py:120`): only provided keys move; `updated_at` always does.
async fn apply_write_fields(
    pool: &sqlx::PgPool,
    module_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    input: &WriteInput,
) -> Result<(), Denial> {
    // `$1` is the module id; every SET column takes the next placeholder.
    let mut parts: Vec<String> = Vec::new();
    let mut binds: Vec<WriteBind> = Vec::new();
    macro_rules! set {
        ($col:expr, $bind:expr) => {{
            parts.push(format!("{} = ${}", $col, binds.len() + 2));
            binds.push($bind);
        }};
    }
    if let Some(name) = input.name.as_ref() {
        set!("name", WriteBind::Text(name.clone()));
    }
    if let Some(description) = input.description.as_ref() {
        set!("description", WriteBind::Text(description.clone()));
    }
    if let Some(description_text) = input.description_text.as_ref() {
        set!("description_text", WriteBind::Json(description_text.clone()));
    }
    if let Some(description_html) = input.description_html.as_ref() {
        set!("description_html", WriteBind::Json(description_html.clone()));
    }
    if let Some(start_date) = input.start_date.as_ref() {
        set!("start_date", WriteBind::Date(start_date.clone()));
    }
    if let Some(target_date) = input.target_date.as_ref() {
        set!("target_date", WriteBind::Date(target_date.clone()));
    }
    if let Some(status) = input.status.as_ref() {
        set!("status", WriteBind::Text(status.clone()));
    }
    if let Some(lead_id) = input.lead_id.as_ref() {
        set!("lead_id", WriteBind::UuidOpt(*lead_id));
    }
    if let Some(view_props) = input.view_props.as_ref() {
        set!("view_props", WriteBind::Json(view_props.clone()));
    }
    if let Some(sort_order) = input.sort_order {
        set!("sort_order", WriteBind::Float(sort_order));
    }
    if let Some(external_source) = input.external_source.as_ref() {
        set!("external_source", WriteBind::TextOpt(external_source.clone()));
    }
    if let Some(external_id) = input.external_id.as_ref() {
        set!("external_id", WriteBind::TextOpt(external_id.clone()));
    }
    if let Some(logo_props) = input.logo_props.as_ref() {
        set!("logo_props", WriteBind::Json(logo_props.clone()));
    }
    parts.push("updated_at = now()".to_owned());
    parts.push(format!("updated_by_id = ${}", binds.len() + 2));
    let sql = format!("UPDATE modules SET {} WHERE id = $1", parts.join(", "));
    let mut query = sqlx::query(&sql).bind(module_id);
    for bind in binds {
        query = bind.apply(query);
    }
    query = query.bind(user_id);
    query.execute(pool).await.map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Validated `YYYY-MM-DD` input as a date bind (sqlx text params do not
/// coerce to `DATE` columns).
fn parse_date_opt(date: &Option<String>) -> Option<chrono::NaiveDate> {
    date.as_ref()
        .and_then(|text| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
}

/// One `UPDATE modules` bind value.
enum WriteBind {
    Text(String),
    TextOpt(Option<String>),
    Json(Value),
    Date(Option<String>),
    UuidOpt(Option<uuid::Uuid>),
    Float(f64),
}

impl WriteBind {
    fn apply<'a>(
        self,
        query: sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>,
    ) -> sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments> {
        match self {
            WriteBind::Text(text) => query.bind(text),
            WriteBind::TextOpt(text) => query.bind(text),
            WriteBind::Json(value) => query.bind(sqlx::types::Json(value)),
            WriteBind::Date(date) => query.bind(
                date.and_then(|text| chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d").ok()),
            ),
            WriteBind::UuidOpt(id) => query.bind(id),
            WriteBind::Float(float) => query.bind(float),
        }
    }
}

/// DRF default `update` over `ModuleWriteSerializer` (no custom method in
/// `ModuleViewSet`): `IsAuthenticated` only, no archived guard, bare
/// write-shape response.
async fn module_update(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let facts = fetch_allow_facts(&pool, &slug, &project_id, &user_id, &[]).await?;
    check_gate(&gates::Gate::Open, &slug, &facts)?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let module_id = parse_uuid_or_invalid(&pk_raw)?;
    // `get_object()` over the base queryset (no archived filter):
    // a miss raises `Http404`, the DRF detail body.
    let rows = fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), false).await?;
    if rows.is_empty() {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            NO_MODULE_DETAIL_BODY.to_owned(),
        ));
    }
    let data = match body.0.as_object() {
        Some(map) => map.clone(),
        None => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                not_dict_body(&body.0),
            ));
        }
    };
    let input = match validate_write(&pool, &data, false).await {
        Ok(input) => input,
        Err(errors) => return Ok(json_response(StatusCode::BAD_REQUEST, errors)),
    };
    if let Some(name) = input.name.as_ref() {
        if duplicate_name_exists(&pool, &project_id, name, Some(&module_id)).await? {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                shape::duplicate_name_body().to_string(),
            ));
        }
    }
    // Stored audit for the member rows (`module.py:111-112` reads the
    // instance BEFORE `super().update()` saves the new `updated_by`).
    let stored: Option<(uuid::Uuid, Option<uuid::Uuid>, Option<uuid::Uuid>)> =
        if input.member_ids.is_some() {
            sqlx::query_as(
                r#"SELECT workspace_id, created_by_id, updated_by_id FROM modules WHERE id = $1"#,
            )
            .bind(module_id)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?
        } else {
            None
        };
    apply_write_fields(&pool, &module_id, &user_id, &input).await?;
    if let Some(members) = input.member_ids.as_ref() {
        let (workspace_id, created_by, updated_by) = stored.ok_or(Denial::ServerError)?;
        replace_members(
            &pool, &module_id, &project_id, &workspace_id,
            created_by, updated_by, members,
        )
        .await?;
    }
    Ok(json_response(
        StatusCode::OK,
        fetch_write_row(&pool, &module_id, &timezone).await?,
    ))
}

/// `ModuleViewSet.partial_update` (`base.py:651-721`).
async fn module_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let (pool, user_id, project_id, timezone) =
        module_context(&state, &slug, &project_raw, extension, gate_for_partial_update()).await?;
    let module_id = parse_uuid_or_invalid(&pk_raw)?;
    // Current row over `.filter(pk=pk)` (no archived filter, `:653`).
    let rows = fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), false).await?;
    let current = match rows.into_iter().next() {
        Some(row) => row,
        None => {
            return Ok(json_response(
                StatusCode::NOT_FOUND,
                queries::MODULE_NOT_FOUND_BODY.to_owned(),
            ));
        }
    };
    if current.get("archived_at").is_some_and(|v| !v.is_null()) {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            queries::ARCHIVED_MODULE_UPDATE_BODY.to_owned(),
        ));
    }
    // Before-save snapshot (`:668`): `ModuleSerializer` data through
    // `DjangoJSONEncoder`.
    let mut snapshot = Map::with_capacity(shape::MODULE_LIST_FIELD_ORDER.len());
    for field in shape::MODULE_LIST_FIELD_ORDER {
        let rendered = shape_value(
            field,
            current.get(*field).unwrap_or(&Value::Null),
            &timezone,
        );
        let value: Value = serde_json::from_str(&rendered).unwrap_or(Value::Null);
        snapshot.insert((*field).to_owned(), value);
    }
    let snapshot_dump = python_dumps(&Value::Object(snapshot));
    let data = match body.0.as_object() {
        Some(map) => map.clone(),
        None => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                not_dict_body(&body.0),
            ));
        }
    };
    let input = match validate_write(&pool, &data, true).await {
        Ok(input) => input,
        Err(errors) => return Ok(json_response(StatusCode::BAD_REQUEST, errors)),
    };
    if let Some(name) = input.name.as_ref() {
        if duplicate_name_exists(&pool, &project_id, name, Some(&module_id)).await? {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                shape::duplicate_name_body().to_string(),
            ));
        }
    }
    let stored: Option<(uuid::Uuid, Option<uuid::Uuid>, Option<uuid::Uuid>)> =
        if input.member_ids.is_some() {
            sqlx::query_as(
                r#"SELECT workspace_id, created_by_id, updated_by_id FROM modules WHERE id = $1"#,
            )
            .bind(module_id)
            .fetch_optional(&pool)
            .await
            .map_err(|_| Denial::ServerError)?
        } else {
            None
        };
    apply_write_fields(&pool, &module_id, &user_id, &input).await?;
    if let Some(members) = input.member_ids.as_ref() {
        let (workspace_id, created_by, updated_by) = stored.ok_or(Denial::ServerError)?;
        replace_members(
            &pool, &module_id, &project_id, &workspace_id,
            created_by, updated_by, members,
        )
        .await?;
    }
    let rows = fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), false).await?;
    let row = rows.into_iter().next().ok_or(Denial::ServerError)?;
    // `model_activity.delay(..., current_instance=snapshot)` (`:708-716`).
    let origin = request_origin(&state)?;
    let emit = tasks::module_update_activity(
        &module_id.to_string(),
        Value::Object(data),
        &snapshot_dump,
        &user_id.to_string(),
        &slug,
        &origin,
    );
    enqueue_task(&pool, emit.task_name(), emit.kwargs()).await;
    Ok(json_response(
        StatusCode::OK,
        shape_row(&row, SHARED_ROW_ORDER, &timezone),
    ))
}
// ---------------------------------------------------------------------------
// Destroy (`base.py:723-759`)
// ---------------------------------------------------------------------------

/// `ModuleViewSet.destroy`: ADMIN + creator fast-path gate, per-issue
/// `issue_activity.deleted` publishes, soft-delete module + bridges,
/// soft-delete requester favorites + hard-delete all recent visits, 204 empty.
async fn module_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let module_id = parse_uuid_or_invalid(&pk_raw)?;
    // Creator fast-path input (`Module.objects.filter(id=pk,
    // created_by=user).exists()`): a MEMBER creator passes without
    // reaching the role check.
    let creator_row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM modules WHERE id = $1 AND created_by_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(module_id)
    .bind(user_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let destroy_gate = gate_for_destroy();
    let mut facts = fetch_allow_facts(
        &pool,
        &slug,
        &project_id,
        &user_id,
        gate_roles(destroy_gate),
    )
    .await?;
    facts.is_creator = creator_row.is_some();
    check_gate(gate_for_destroy(), &slug, &facts)?;
    // `Module.objects.get(workspace__slug, project_id, pk)`: a miss
    // raises `DoesNotExist`, the `handle_exception` 404.
    let module: Option<(uuid::Uuid, String)> = sqlx::query_as(
        r#"SELECT m.id, m.name FROM modules m
           JOIN workspaces w ON w.id = m.workspace_id
           WHERE m.id = $1 AND m.project_id = $2 AND w.slug = $3 AND m.deleted_at IS NULL"#,
    )
    .bind(module_id)
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (_, module_name) = match module {
        Some(row) => row,
        None => return Err(Denial::NotFound),
    };
    // Live bridge issues for the per-issue publishes (`:727`).
    let issue_rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT issue_id FROM module_issues WHERE module_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(module_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let origin = request_origin(&state)?;
    let epoch = chrono::Utc::now().timestamp();
    let project_wire = if project_raw.parse::<uuid::Uuid>().is_ok() {
        project_raw.clone()
    } else {
        project_id.to_string()
    };
    let requested_data = python_dumps(&serde_json::json!({"module_id": module_id.to_string()}));
    let current_instance =
        python_dumps(&serde_json::json!({"module_name": module_name.clone()}));
    for (issue_id,) in &issue_rows {
        let emit = tasks::module_destroy_activity(
            &module_id.to_string(),
            &user_id.to_string(),
            &issue_id.to_string(),
            &project_wire,
            &current_instance,
            epoch,
            &origin,
        );
        // `requested_data` is fixed per call site; the builder takes the
        // id only, so assert the wire form here.
        debug_assert_eq!(
            emit.requested_data,
            requested_data,
            "destroy requested_data matches json.dumps"
        );
        enqueue_task(&pool, emit.task_name(), emit.kwargs()).await;
    }
    // Soft-delete the module (`:742`) and its bridges (`:744`).
    sqlx::query(r#"UPDATE modules SET deleted_at = now() WHERE id = $1"#)
        .bind(module_id)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    sqlx::query(
        r#"UPDATE module_issues SET deleted_at = now() WHERE module_id = $1 AND project_id = $2"#,
    )
    .bind(module_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // Soft-delete the requester's favorites (`:746-751`, plain
    // `.delete()` over the soft-delete manager — verified live).
    sqlx::query(
        r#"UPDATE user_favorites SET deleted_at = now()
           WHERE user_id = $1 AND entity_type = 'module' AND entity_identifier = $2
           AND project_id = $3 AND deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(module_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // Hard-delete every recent visit of this module (`:753-758`,
    // `soft=False`).
    sqlx::query(
        r#"DELETE FROM user_recent_visits ur USING workspaces w
           WHERE ur.workspace_id = w.id AND ur.project_id = $1 AND w.slug = $2
           AND ur.entity_identifier = $3 AND ur.entity_name = 'module'"#,
    )
    .bind(project_id)
    .bind(&slug)
    .bind(module_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gates_cover_all_six_owned_actions() {
        let rows = [
            ("POST", "workspaces/<slug>/projects/<project_id>/modules/", "base.py:294"),
            ("GET", "workspaces/<slug>/projects/<project_id>/modules/", "base.py:353"),
            ("GET", "workspaces/<slug>/projects/<project_id>/modules/<pk>/", "base.py:395"),
            ("PATCH", "workspaces/<slug>/projects/<project_id>/modules/<pk>/", "base.py:651"),
            ("DELETE", "workspaces/<slug>/projects/<project_id>/modules/<pk>/", "base.py:723"),
        ];
        for (method, path, source) in rows {
            let row = gates::gate_for(method, path).expect("gate row");
            assert_eq!(row.source.split(' ').next(), Some(source), "{method} {path}");
        }
        // PUT is the open quirk: no custom action, `IsAuthenticated` only.
        let put = gates::gate_for(
            "PUT",
            "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
        )
        .expect("put gate");
        assert!(matches!(put.gate, gates::Gate::Open));
        assert!(matches!(
            gate_for_destroy(),
            gates::Gate::ProjectCreator { .. }
        ));
    }

    #[test]
    fn shared_row_order_matches_the_values_set() {
        use std::collections::BTreeSet;
        let expected: BTreeSet<&str> = [
            "id", "workspace_id", "project_id", "name", "description", "description_text",
            "description_html", "start_date", "target_date", "status", "lead_id", "member_ids",
            "view_props", "sort_order", "external_source", "external_id", "logo_props",
            "is_favorite", "completed_issues", "cancelled_issues", "started_issues",
            "unstarted_issues", "backlog_issues", "total_issues", "completed_estimate_points",
            "total_estimate_points", "created_at", "updated_at",
        ]
        .into_iter()
        .collect();
        let actual: BTreeSet<&str> = SHARED_ROW_ORDER.iter().copied().collect();
        assert_eq!(actual, expected, "row order keys");
        assert_eq!(SHARED_ROW_ORDER.len(), 28);
        // Concrete model fields first (call order), then the requested
        // annotations in queryset annotation order (verified live).
        let tail = &SHARED_ROW_ORDER[18..];
        assert_eq!(
            tail,
            [
                "is_favorite", "completed_issues", "cancelled_issues", "started_issues",
                "unstarted_issues", "backlog_issues", "total_issues", "completed_estimate_points",
                "total_estimate_points", "member_ids",
            ]
        );
    }

    #[test]
    fn detail_shell_covers_serializer_plus_extras() {
        // `MODULE_LIST_FIELD_ORDER` (29) + `MODULE_DETAIL_EXTRA_FIELDS` (6).
        assert_eq!(DETAIL_ROW_ORDER.len(), 29);
        assert_eq!(shape::MODULE_DETAIL_EXTRA_FIELDS.len(), 6);
        assert!(DETAIL_ROW_ORDER.contains(&"archived_at"));
        assert!(!SHARED_ROW_ORDER.contains(&"archived_at"));
    }

    #[test]
    fn count_annotations_pair_completed_first() {
        // `base.py:86-135` annotates `completed` before `cancelled`;
        // the live wire counts a completed-group issue under
        // `completed_issues` (PIDASHCONV-504).
        let selects = annotation_selects();
        let completed = selects.find("AS completed_issues").expect("alias");
        let cancelled = selects.find("AS cancelled_issues").expect("alias");
        let completed_group = selects[..completed].rfind("states.group = 'completed'").expect("group");
        let cancelled_group = selects[..cancelled].rfind("states.group = 'cancelled'").expect("group");
        assert!(completed_group < completed);
        assert!(cancelled_group < cancelled);
        assert!(completed < cancelled);
    }

    #[test]
    fn python_str_matches_cpython() {
        assert_eq!(py_str(&serde_json::json!("x")), "x");
        assert_eq!(py_str(&serde_json::json!([])), "[]");
        assert_eq!(py_str(&serde_json::json!({})), "{}");
        assert_eq!(py_str(&serde_json::json!(1.5)), "1.5");
        assert_eq!(py_str(&serde_json::json!(["a"])), "['a']");
        assert_eq!(
            not_dict_body(&serde_json::json!([1])),
            "{\"non_field_errors\":[\"Invalid data. Expected a dictionary, but got list.\"]}"
        );
    }

    #[test]
    fn choice_errors_render_python_str() {
        // DRF `ChoiceField` formats the raw input with `str()`
        // (probed on DRF 3.15.2): booleans keep their capitals.
        assert_eq!(
            invalid_choice_message(&serde_json::json!(true)),
            "\"True\" is not a valid choice."
        );
        assert_eq!(
            invalid_choice_message(&serde_json::json!(false)),
            "\"False\" is not a valid choice."
        );
        assert_eq!(
            invalid_choice_message(&serde_json::json!(5)),
            "\"5\" is not a valid choice."
        );
        assert_eq!(
            invalid_choice_message(&serde_json::json!("abc")),
            "\"abc\" is not a valid choice."
        );
        assert_eq!(
            invalid_choice_message(&serde_json::json!({"a": 1})),
            "\"{'a': 1}\" is not a valid choice."
        );
    }

    #[test]
    fn sort_order_coerces_like_drf_float() {
        // DRF `FloatField` is `float(data)` (probed on DRF 3.15.2):
        // bools coerce, padded numeric strings parse.
        assert_eq!(
            parse_sort_order_value(&serde_json::json!(true)),
            Some(1.0)
        );
        assert_eq!(
            parse_sort_order_value(&serde_json::json!(false)),
            Some(0.0)
        );
        assert_eq!(
            parse_sort_order_value(&serde_json::json!(2)),
            Some(2.0)
        );
        assert_eq!(
            parse_sort_order_value(&serde_json::json!(" 1.5 ")),
            Some(1.5)
        );
        assert_eq!(
            parse_sort_order_value(&serde_json::json!("abc")),
            None
        );
        assert_eq!(parse_sort_order_value(&serde_json::json!([1])), None);
        // Python `float()` accepts underscores between digits
        // (`float("1_0")` is `10.0`, verified against CPython).
        assert_eq!(
            parse_sort_order_value(&serde_json::json!("1_0")),
            Some(10.0)
        );
        assert_eq!(
            parse_sort_order_value(&serde_json::json!("1_000.5")),
            Some(1000.5)
        );
        assert_eq!(
            parse_sort_order_value(&serde_json::json!("1__0")),
            None
        );
        assert_eq!(parse_sort_order_value(&serde_json::json!("_1")), None);
        assert_eq!(parse_sort_order_value(&serde_json::json!("1_")), None);
    }

    #[test]
    fn negative_int_pks_skip_the_lookup() {
        // Django `UUIDField.to_python` rejects negative ints before the
        // row lookup (probed on Django 4.2); non-negative ints convert
        // via `uuid.UUID(int=…)` and miss.
        assert!(!pk_int_reaches_lookup(&serde_json::Number::from(-5)));
        assert!(pk_int_reaches_lookup(&serde_json::Number::from(5)));
        assert!(pk_int_reaches_lookup(&serde_json::Number::from(0)));
    }

    #[test]
    fn write_shape_order_matches_contract_keys() {
        // `test_modules.py::WRITE_MODULE_KEYS` (25 keys).
        assert_eq!(shape::WRITE_RESPONSE_ORDER.len(), 25);
        for key in [
            "id", "lead_id", "member_ids", "members", "lead", "project", "workspace",
            "created_by", "updated_by", "deleted_at", "archived_at",
        ] {
            assert!(
                shape::WRITE_RESPONSE_ORDER.contains(&key),
                "write order has {key}"
            );
        }
    }

    #[test]
    fn status_choices_match_the_model() {
        assert_eq!(MODULE_STATUS_CHOICES.len(), 6);
        for status in ["backlog", "planned", "in-progress", "paused", "completed", "cancelled"] {
            assert!(MODULE_STATUS_CHOICES.contains(&status));
        }
    }

    #[test]
    fn date_rule_rejects_start_after_target() {
        assert!(shape::validate_module_dates(Some("2026-10-01"), Some("2026-09-01")).is_err());
        assert!(shape::validate_module_dates(Some("2026-09-01"), Some("2026-10-01")).is_ok());
        assert!(shape::validate_module_dates(None, Some("2026-10-01")).is_ok());
    }

    #[test]
    fn task_names_match_the_python_call_sites() {
        assert_eq!(MODULE_MODEL_NAME, "module");
        assert_eq!(
            tasks::MODEL_ACTIVITY_TASK,
            "pi_dash.bgtasks.webhook_task.model_activity"
        );
        assert_eq!(
            tasks::ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        assert_eq!(
            tasks::RECENT_VISITED_TASK,
            "pi_dash.bgtasks.recent_visited_task.recent_visited_task"
        );
    }

    #[test]
    fn python_dumps_uses_cpython_separators() {
        let value = serde_json::json!({"module_id": "abc"});
        assert_eq!(python_dumps(&value), "{\"module_id\": \"abc\"}");
    }

    #[test]
    fn paths_match_the_url_conf() {
        assert!(MODULES_PATH.ends_with("/modules/"));
        assert!(MODULE_PATH.ends_with("/modules/{pk}/"));
    }
}
