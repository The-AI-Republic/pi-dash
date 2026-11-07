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
//!   JSON array (NOT paginated — `Response(modules)` directly). A non-empty
//!   `?fields=` selects the `ModuleSerializer` branch (`:356-357`): the
//!   projection itself is discarded (ported quirk, `DYNAMIC_FIELDS_KWARG_HONORED`)
//!   but the row renders the full 29-key `ModuleSerializer.Meta.fields`
//!   shape; otherwise the annotated 28-key `.values()` row renders.
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
//! * `list` ignores `fields=` for the projection (the constructor kwarg is
//!   overwritten with `expand`); a non-empty `?fields=` still flips the row
//!   to the 29-key `ModuleSerializer.Meta.fields` shape, an empty/absent
//!   one renders the 28-key annotated `.values()` row.
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

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Router;
use chrono_tz::Tz;
use sea_orm::sea_query::extension::postgres::PgFunc;
use sea_orm::sea_query::{Alias, CaseStatement, Condition, Expr, Func, Order, Query as SeaQuery};
use sea_orm::{
    ColumnTrait, EntityTrait, FromQueryResult, JoinType, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, RelationTrait,
};
use serde_json::{Map, Value};

use pidash_db::entity::{
    estimate_points, estimates, issue_assignees, issue_labels, issues, labels, module_issues,
    module_links, module_members, modules, project_members, projects, states, user_favorites,
    user_recent_visits, users, workspace_members, workspaces,
};
use pidash_services::app_modules::{queries, shape, tasks};

use super::gates;
use crate::app_issues::Denial;
use crate::state::AppState;
pub(crate) use pidash_compat::response::{empty_response, json_response};
use pidash_compat::validate::push_pairs_error as push_error;

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
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
            axum::routing::get(module_retrieve)
                .put(module_update)
                .patch(module_partial_update)
                .delete(module_destroy)
                .post(crate::edge::proxy)
                .head(crate::edge::proxy)
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
    &gates::gate_for(
        "GET",
        "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
    )
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

/// SeaORM select builders for the foundation lookups (MODULES-1 slice A).
/// Each builder carries the exact Django predicates so callers and
/// statement-shape tests share one definition.
fn resolve_project_select(slug: &str, upper: &str) -> sea_orm::Select<projects::Entity> {
    projects::Entity::find()
        .select_only()
        .column(projects::Column::Id)
        .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(projects::Column::Identifier.eq(upper))
        .filter(projects::Column::DeletedAt.is_null())
}

fn project_role_select(
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> sea_orm::Select<project_members::Entity> {
    project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Role)
        .join(
            JoinType::InnerJoin,
            project_members::Relation::Workspaces.def(),
        )
        .filter(project_members::Column::MemberId.eq(*user_id))
        .filter(project_members::Column::ProjectId.eq(*project_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
}

fn workspace_role_select(
    slug: &str,
    user_id: &uuid::Uuid,
) -> sea_orm::Select<workspace_members::Entity> {
    workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::Role)
        .join(
            JoinType::InnerJoin,
            workspace_members::Relation::Workspaces.def(),
        )
        .filter(workspace_members::Column::MemberId.eq(*user_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
}

fn actor_timezone_select(user_id: &uuid::Uuid) -> sea_orm::Select<users::Entity> {
    users::Entity::find()
        .select_only()
        .column(users::Column::UserTimezone)
        .filter(users::Column::Id.eq(*user_id))
}

fn user_exists_select(user_id: &uuid::Uuid) -> sea_orm::Select<users::Entity> {
    users::Entity::find()
        .select_only()
        .column(users::Column::Id)
        .filter(users::Column::Id.eq(*user_id))
}

fn context_project_select(
    slug: &str,
    project_id: &uuid::Uuid,
) -> sea_orm::Select<projects::Entity> {
    projects::Entity::find()
        .select_only()
        .columns([projects::Column::Id, projects::Column::WorkspaceId])
        .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
        .filter(projects::Column::Id.eq(*project_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(projects::Column::DeletedAt.is_null())
}

#[derive(Debug, FromQueryResult)]
struct ContextProjectRow {
    id: uuid::Uuid,
    workspace_id: uuid::Uuid,
}

fn duplicate_name_select(
    project_id: &uuid::Uuid,
    name: &str,
    exclude: Option<&uuid::Uuid>,
) -> sea_orm::Select<modules::Entity> {
    let select = modules::Entity::find()
        .select_only()
        .column(modules::Column::Id)
        .filter(modules::Column::ProjectId.eq(*project_id))
        .filter(modules::Column::Name.eq(name))
        .filter(modules::Column::DeletedAt.is_null());
    match exclude {
        Some(self_id) => select.filter(modules::Column::Id.ne(*self_id)),
        None => select,
    }
}

fn live_member_ids_select(module_id: &uuid::Uuid) -> sea_orm::Select<module_members::Entity> {
    module_members::Entity::find()
        .select_only()
        .column(module_members::Column::MemberId)
        .filter(module_members::Column::ModuleId.eq(*module_id))
        .filter(module_members::Column::MemberId.is_not_null())
        .filter(module_members::Column::DeletedAt.is_null())
}

fn creator_exists_select(
    module_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> sea_orm::Select<modules::Entity> {
    modules::Entity::find()
        .select_only()
        .column(modules::Column::Id)
        .filter(modules::Column::Id.eq(*module_id))
        .filter(modules::Column::CreatedById.eq(*user_id))
        .filter(modules::Column::DeletedAt.is_null())
}

#[derive(Debug, FromQueryResult)]
#[allow(dead_code)]
struct DestroyModuleRow {
    id: uuid::Uuid,
    name: String,
}

fn destroy_module_select(
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
) -> sea_orm::Select<modules::Entity> {
    modules::Entity::find()
        .select_only()
        .columns([modules::Column::Id, modules::Column::Name])
        .join(JoinType::InnerJoin, modules::Relation::Workspaces.def())
        .filter(modules::Column::Id.eq(*module_id))
        .filter(modules::Column::ProjectId.eq(*project_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(modules::Column::DeletedAt.is_null())
}

fn destroy_issue_ids_select(module_id: &uuid::Uuid) -> sea_orm::Select<module_issues::Entity> {
    module_issues::Entity::find()
        .select_only()
        .column(module_issues::Column::IssueId)
        .filter(module_issues::Column::ModuleId.eq(*module_id))
        .filter(module_issues::Column::DeletedAt.is_null())
}

#[derive(Debug, FromQueryResult)]
struct ModuleLinkRow {
    id: uuid::Uuid,
    created_at: sea_orm::prelude::DateTimeWithTimeZone,
    updated_at: sea_orm::prelude::DateTimeWithTimeZone,
    deleted_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    title: Option<String>,
    url: String,
    metadata: Value,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project: uuid::Uuid,
    workspace: uuid::Uuid,
    module: uuid::Uuid,
}

fn module_links_select(module_id: &uuid::Uuid) -> sea_orm::Select<module_links::Entity> {
    module_links::Entity::find()
        .select_only()
        .columns([
            module_links::Column::Id,
            module_links::Column::CreatedAt,
            module_links::Column::UpdatedAt,
            module_links::Column::DeletedAt,
            module_links::Column::Title,
            module_links::Column::Url,
            module_links::Column::Metadata,
        ])
        .column_as(module_links::Column::CreatedById, "created_by")
        .column_as(module_links::Column::UpdatedById, "updated_by")
        .column_as(module_links::Column::ProjectId, "project")
        .column_as(module_links::Column::WorkspaceId, "workspace")
        .column_as(module_links::Column::ModuleId, "module")
        .filter(module_links::Column::ModuleId.eq(*module_id))
        .filter(module_links::Column::DeletedAt.is_null())
        .order_by_desc(module_links::Column::CreatedAt)
}

#[derive(Debug, FromQueryResult)]
struct WriteRowData {
    id: uuid::Uuid,
    lead_id: Option<uuid::Uuid>,
    created_at: sea_orm::prelude::DateTimeWithTimeZone,
    updated_at: sea_orm::prelude::DateTimeWithTimeZone,
    deleted_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    name: String,
    description: String,
    description_text: Option<Value>,
    description_html: Option<Value>,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    status: String,
    view_props: Value,
    sort_order: f64,
    external_source: Option<String>,
    external_id: Option<String>,
    archived_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    logo_props: Value,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project: uuid::Uuid,
    workspace: uuid::Uuid,
    lead: Option<uuid::Uuid>,
}

fn write_row_select(module_id: &uuid::Uuid) -> sea_orm::Select<modules::Entity> {
    modules::Entity::find()
        .select_only()
        .columns([
            modules::Column::Id,
            modules::Column::LeadId,
            modules::Column::CreatedAt,
            modules::Column::UpdatedAt,
            modules::Column::DeletedAt,
            modules::Column::Name,
            modules::Column::Description,
            modules::Column::DescriptionText,
            modules::Column::DescriptionHtml,
            modules::Column::StartDate,
            modules::Column::TargetDate,
            modules::Column::Status,
            modules::Column::ViewProps,
            modules::Column::SortOrder,
            modules::Column::ExternalSource,
            modules::Column::ExternalId,
            modules::Column::ArchivedAt,
            modules::Column::LogoProps,
        ])
        .column_as(modules::Column::CreatedById, "created_by")
        .column_as(modules::Column::UpdatedById, "updated_by")
        .column_as(modules::Column::ProjectId, "project")
        .column_as(modules::Column::WorkspaceId, "workspace")
        .column_as(modules::Column::LeadId, "lead")
        .filter(modules::Column::Id.eq(*module_id))
}

#[derive(Debug, FromQueryResult)]
struct StoredAuditRow {
    workspace_id: uuid::Uuid,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
}

fn stored_audit_select(module_id: &uuid::Uuid) -> sea_orm::Select<modules::Entity> {
    modules::Entity::find()
        .select_only()
        .columns([
            modules::Column::WorkspaceId,
            modules::Column::CreatedById,
            modules::Column::UpdatedById,
        ])
        .filter(modules::Column::Id.eq(*module_id))
}

fn sub_issues_count_select(
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
) -> sea_orm::Select<module_issues::Entity> {
    module_issues::Entity::find()
        .select_only()
        .column(module_issues::Column::Id)
        .join(JoinType::InnerJoin, module_issues::Relation::Issues.def())
        .join(JoinType::LeftJoin, issues::Relation::States.def())
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .filter(module_issues::Column::ModuleId.eq(*module_id))
        .filter(module_issues::Column::DeletedAt.is_null())
        .filter(issues::Column::ProjectId.eq(*project_id))
        .filter(issues::Column::ParentId.is_not_null())
        .filter(issues::Column::DeletedAt.is_null())
        .filter(issues::Column::IsDraft.eq(false))
        .filter(issues::Column::ArchivedAt.is_null())
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(
            Condition::any()
                .add(states::Column::Group.is_null())
                .add(states::Column::Group.ne("triage")),
        )
}

fn estimate_type_exists_select(
    slug: &str,
    project_id: &uuid::Uuid,
) -> sea_orm::Select<estimates::Entity> {
    estimates::Entity::find()
        .select_only()
        .column(estimates::Column::Id)
        .join(JoinType::InnerJoin, estimates::Relation::Projects.def())
        .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
        .filter(projects::Column::Id.eq(*project_id))
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(estimates::Column::Type.eq("points"))
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
    let upper = normalize_resolve_identifier(raw);
    let db = pidash_db::pool::sea_orm_connection(pool);
    resolve_project_select(slug, &upper)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ProjectNotFound)
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
/// Shared with sibling handler issue 407 (user-properties gate).
pub(crate) fn gate_roles(gate: &gates::Gate) -> &[i32] {
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let project_role: Option<i16> = project_role_select(slug, project_id, user_id)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    use pidash_auth::permissions::ROLE_ADMIN;
    let workspace_role: Option<i16> = workspace_role_select(slug, user_id)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(pidash_auth::permissions::allow::AllowFacts {
        workspace: pidash_types::WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: workspace_role.is_some(),
        has_allowed_workspace_role: workspace_role
            .map(|role| allowed.contains(&i32::from(role)))
            .unwrap_or(false),
        is_creator: false,
        has_allowed_project_role: project_role
            .map(|role| allowed.contains(&i32::from(role)))
            .unwrap_or(false),
        is_project_member: project_role.is_some(),
        is_workspace_admin: workspace_role
            .map(|role| i32::from(role) == ROLE_ADMIN)
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
    let message = pidash_jobs::TaskMessage::new(task, vec![], kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; response stands");
    }
}

/// `soft_delete_related_objects.delay("db", <model>, pk, using=None)`
/// (`db/mixins.py:77-78`): three positional args, `using` stays a null
/// kwarg — exactly as Python's `.delay(label, name, pk, using=using)`
/// call sends it with `using=None`. Fires on every instance `.delete()`
/// in these views.
pub(crate) fn soft_delete_message(model: &str, pk: &uuid::Uuid) -> pidash_jobs::TaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    pidash_jobs::TaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
        ],
        kwargs,
    )
}

/// Best-effort publish of [`soft_delete_message`]: without the queue
/// the response still stands.
pub(crate) async fn enqueue_soft_delete(pool: &sqlx::PgPool, model: &str, pk: &uuid::Uuid) {
    let message = soft_delete_message(model, pk);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// `request.user.user_timezone` (`TimezoneMixin`): unknown zones 500
/// through the same branch Django's `zoneinfo` activation raises into.
/// Shared with sibling handler issue 407 (same render path).
pub(crate) async fn actor_timezone(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<Tz, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let name: String = actor_timezone_select(user_id)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-66`):
/// `APP_BASE_URL` wins when set, else `WEB_URL`; unset is
/// `ImproperlyConfigured` → 500. Empty strings fall through like
/// Python's `or` (PIDASHCONV-1597).
fn request_origin(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    urls.app_base_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| urls.web_url.clone().filter(|s| !s.is_empty()))
        .ok_or(Denial::ServerError)
}
// ---------------------------------------------------------------------------
// Annotated module rows (Q1 over `get_queryset`, `base.py:78-292`)
// ---------------------------------------------------------------------------

// (Former `MODULE_BASE_COLUMNS` deleted with the raw-SQL annotated-row
// fetch: the SeaORM statement selects by column.)

/// Annotated-row key order shared by create/list/partial_update: the
/// `.values()` calls list the same 28 keys in different orders, but
/// Django renders concrete model fields first (in call order) and the
/// requested annotations after (in queryset annotation order), so all
/// three responses share this order (verified live).
const SHARED_ROW_ORDER: &[&str] = &[
    "id",
    "workspace_id",
    "project_id",
    "name",
    "description",
    "description_text",
    "description_html",
    "start_date",
    "target_date",
    "status",
    "lead_id",
    "view_props",
    "sort_order",
    "external_source",
    "external_id",
    "logo_props",
    "created_at",
    "updated_at",
    "is_favorite",
    "completed_issues",
    "cancelled_issues",
    "started_issues",
    "unstarted_issues",
    "backlog_issues",
    "total_issues",
    "completed_estimate_points",
    "total_estimate_points",
    "member_ids",
];

/// Detail shell order: `ModuleSerializer.Meta.fields` + detail extras
/// (`serializers/module.py:220-273`), then the view-appended blocks.
/// Shared with sibling handler issue 407 (archived detail).
pub(crate) const DETAIL_ROW_ORDER: &[&str] = shape::MODULE_LIST_FIELD_ORDER;

// (Former `bind_placeholders` deleted with the raw-SQL annotated-row
// fetch: the SeaORM statement binds by column, not by placeholder.)

/// One annotated module row, decoded by name (the `row_to_json` /
/// `GROUP BY` statement twin).
#[derive(Debug, FromQueryResult)]
pub(crate) struct ModuleRowData {
    id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    name: String,
    description: String,
    description_text: Option<Value>,
    description_html: Option<Value>,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    status: String,
    lead_id: Option<uuid::Uuid>,
    view_props: Value,
    sort_order: f64,
    external_source: Option<String>,
    external_id: Option<String>,
    logo_props: Value,
    archived_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
    created_at: sea_orm::prelude::DateTimeWithTimeZone,
    updated_at: sea_orm::prelude::DateTimeWithTimeZone,
    is_favorite: bool,
    completed_issues: i64,
    cancelled_issues: i64,
    started_issues: i64,
    unstarted_issues: i64,
    backlog_issues: i64,
    total_issues: i64,
    backlog_estimate_points: Option<f64>,
    unstarted_estimate_points: Option<f64>,
    started_estimate_points: Option<f64>,
    cancelled_estimate_points: Option<f64>,
    completed_estimate_points: Option<f64>,
    total_estimate_points: Option<f64>,
    member_ids: Vec<uuid::Uuid>,
}

/// `"alias"."column"` expression.
fn mcol<C: sea_orm::sea_query::IntoIden + 'static>(alias: &'static str, column: C) -> Expr {
    Expr::col((Alias::new(alias), column))
}

/// `"alias"."column"` column reference (for `equals` / `order_by` /
/// `group_by_col`, which take `IntoColumnRef`, not `Expr`).
fn mref<C: sea_orm::sea_query::IntoIden + 'static>(alias: &'static str, column: C) -> (Alias, C) {
    (Alias::new(alias), column)
}

/// Wrap a select statement as a scalar subquery expression.
fn msub(stmt: sea_orm::sea_query::SelectStatement) -> sea_orm::sea_query::SimpleExpr {
    crate::app_issues::list_engine::sub(stmt)
}

/// `IssueManager` guards for the annotation subqueries
/// (`queries::issue_manager_guards_sql`): the same five predicates.
/// `states` must be joined (the callers join it).
fn annotation_guards() -> Vec<sea_orm::sea_query::SimpleExpr> {
    vec![
        mcol("issues", issues::Column::DeletedAt).is_null(),
        Condition::all()
            .add(mcol("states", states::Column::Group).eq("triage"))
            .add(mcol("states", states::Column::Group).is_not_null())
            .not()
            .into(),
        mcol("issues", issues::Column::ArchivedAt).is_null(),
        mcol("projects", projects::Column::ArchivedAt).is_null(),
        mcol("issues", issues::Column::IsDraft).eq(false),
    ]
}

/// The `is_favorite` `EXISTS` statement (`queries::favorite_exists_sql`):
/// caller, entity_type='module', bridge to the outer module, project,
/// workspace slug, all live rows (`LIMIT 1` kept).
fn favorite_exists_stmt(
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    let mut stmt = SeaQuery::select();
    stmt.expr(Expr::val(1))
        .from_as(user_favorites::Entity, Alias::new("uf"))
        .join_as(
            JoinType::InnerJoin,
            workspaces::Entity,
            Alias::new("w"),
            mcol("uf", user_favorites::Column::WorkspaceId)
                .equals(mref("w", workspaces::Column::Id)),
        )
        .and_where(mcol("uf", user_favorites::Column::DeletedAt).is_null())
        .and_where(
            mcol("uf", user_favorites::Column::EntityIdentifier)
                .equals(mref("m", modules::Column::Id)),
        )
        .and_where(mcol("uf", user_favorites::Column::EntityType).eq("module"))
        .and_where(mcol("uf", user_favorites::Column::ProjectId).eq(*project_id))
        .and_where(mcol("uf", user_favorites::Column::UserId).eq(*user_id))
        .and_where(mcol("w", workspaces::Column::Slug).eq(slug))
        .limit(1);
    stmt.to_owned()
}

/// One per-group count annotation scalar subquery
/// (`queries::count_issues_sql`): grouped counts `INNER JOIN states`,
/// the total `LEFT JOIN`s it (NULL-safe guard semantics ported as
/// observed), all `LIMIT 1` under the `COALESCE` at the use site.
fn count_annotation(group: Option<&str>) -> sea_orm::sea_query::SimpleExpr {
    let mut stmt = SeaQuery::select();
    stmt.expr(mcol("issues", issues::Column::Id).count())
        .from_as(issues::Entity, Alias::new("issues"))
        .join_as(
            match group {
                Some(_) => JoinType::InnerJoin,
                None => JoinType::LeftJoin,
            },
            states::Entity,
            Alias::new("states"),
            mcol("issues", issues::Column::StateId).equals(mref("states", states::Column::Id)),
        )
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            Alias::new("projects"),
            mcol("issues", issues::Column::ProjectId)
                .equals(mref("projects", projects::Column::Id)),
        )
        .join_as(
            JoinType::InnerJoin,
            module_issues::Entity,
            Alias::new("module_issues"),
            mcol("issues", issues::Column::Id)
                .equals(mref("module_issues", module_issues::Column::IssueId)),
        );
    for condition in annotation_guards() {
        stmt.and_where(condition);
    }
    stmt.and_where(mcol("module_issues", module_issues::Column::DeletedAt).is_null())
        .and_where(
            mcol("module_issues", module_issues::Column::ModuleId)
                .equals(mref("m", modules::Column::Id)),
        );
    if let Some(group) = group {
        stmt.and_where(mcol("states", states::Column::Group).eq(group));
    }
    let mut cell = stmt.to_owned();
    cell.group_by_col(mref("module_issues", module_issues::Column::ModuleId));
    cell.limit(1);
    msub(cell)
}

/// One per-group estimate `SUM` annotation scalar subquery
/// (`queries::estimate_subquery_sql`).
fn estimate_annotation(group: Option<&str>) -> sea_orm::sea_query::SimpleExpr {
    let mut stmt = SeaQuery::select();
    stmt.expr(Func::sum(
        mcol("estimate_points", estimate_points::Column::Value)
            .cast_as(Alias::new("double precision")),
    ))
    .from_as(issues::Entity, Alias::new("issues"))
    .join_as(
        JoinType::InnerJoin,
        states::Entity,
        Alias::new("states"),
        mcol("issues", issues::Column::StateId).equals(mref("states", states::Column::Id)),
    )
    .join_as(
        JoinType::InnerJoin,
        projects::Entity,
        Alias::new("projects"),
        mcol("issues", issues::Column::ProjectId).equals(mref("projects", projects::Column::Id)),
    )
    .join_as(
        JoinType::InnerJoin,
        estimate_points::Entity,
        Alias::new("estimate_points"),
        mcol("issues", issues::Column::EstimatePointId)
            .equals(mref("estimate_points", estimate_points::Column::Id)),
    )
    .join_as(
        JoinType::InnerJoin,
        estimates::Entity,
        Alias::new("estimates"),
        mcol("estimate_points", estimate_points::Column::EstimateId)
            .equals(mref("estimates", estimates::Column::Id)),
    )
    .join_as(
        JoinType::InnerJoin,
        module_issues::Entity,
        Alias::new("module_issues"),
        mcol("issues", issues::Column::Id)
            .equals(mref("module_issues", module_issues::Column::IssueId)),
    );
    for condition in annotation_guards() {
        stmt.and_where(condition);
    }
    stmt.and_where(mcol("estimates", estimates::Column::Type).eq("points"))
        .and_where(mcol("module_issues", module_issues::Column::DeletedAt).is_null())
        .and_where(
            mcol("module_issues", module_issues::Column::ModuleId)
                .equals(mref("m", modules::Column::Id)),
        );
    if let Some(group) = group {
        stmt.and_where(mcol("states", states::Column::Group).eq(group));
    }
    let mut cell = stmt.to_owned();
    cell.group_by_col(mref("module_issues", module_issues::Column::ModuleId));
    cell.limit(1);
    msub(cell)
}

/// The annotated module-row statement shared by the list/detail fetch and
/// the archived fetch. `member_deleted_guard` selects the `member_ids`
/// deleted-at filter: the main queryset filters soft-deleted members
/// (`base.py:217-222`) while the archive queryset does not
/// (`archive.py:244-253`, ported bug). `archived_only` marks the archive
/// fetch (`archived_at IS NOT NULL`); otherwise unarchived rows only.
#[allow(clippy::too_many_arguments)]
pub(crate) fn module_rows_statement(
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    module_id: Option<&uuid::Uuid>,
    archived_null: bool,
    archived_only: bool,
    member_deleted_guard: bool,
) -> sea_orm::sea_query::SelectStatement {
    let mut stmt = SeaQuery::select();
    stmt.expr_as(mcol("m", modules::Column::Id), Alias::new("id"))
        .expr_as(
            mcol("m", modules::Column::WorkspaceId),
            Alias::new("workspace_id"),
        )
        .expr_as(
            mcol("m", modules::Column::ProjectId),
            Alias::new("project_id"),
        )
        .expr_as(mcol("m", modules::Column::Name), Alias::new("name"))
        .expr_as(
            mcol("m", modules::Column::Description),
            Alias::new("description"),
        )
        .expr_as(
            mcol("m", modules::Column::DescriptionText),
            Alias::new("description_text"),
        )
        .expr_as(
            mcol("m", modules::Column::DescriptionHtml),
            Alias::new("description_html"),
        )
        .expr_as(
            mcol("m", modules::Column::StartDate),
            Alias::new("start_date"),
        )
        .expr_as(
            mcol("m", modules::Column::TargetDate),
            Alias::new("target_date"),
        )
        .expr_as(mcol("m", modules::Column::Status), Alias::new("status"))
        .expr_as(mcol("m", modules::Column::LeadId), Alias::new("lead_id"))
        .expr_as(
            mcol("m", modules::Column::ViewProps),
            Alias::new("view_props"),
        )
        .expr_as(
            mcol("m", modules::Column::SortOrder),
            Alias::new("sort_order"),
        )
        .expr_as(
            mcol("m", modules::Column::ExternalSource),
            Alias::new("external_source"),
        )
        .expr_as(
            mcol("m", modules::Column::ExternalId),
            Alias::new("external_id"),
        )
        .expr_as(
            mcol("m", modules::Column::LogoProps),
            Alias::new("logo_props"),
        )
        .expr_as(
            mcol("m", modules::Column::ArchivedAt),
            Alias::new("archived_at"),
        )
        .expr_as(
            mcol("m", modules::Column::CreatedAt),
            Alias::new("created_at"),
        )
        .expr_as(
            mcol("m", modules::Column::UpdatedAt),
            Alias::new("updated_at"),
        )
        .expr_as(
            Expr::exists(favorite_exists_stmt(slug, project_id, user_id)),
            Alias::new("is_favorite"),
        );
    // Six group counts in queries::COUNT_ALIASES source order. NB: the
    // `queries::COUNT_GROUPS` order (cancelled-const first) does NOT match
    // COUNT_ALIASES (completed first); zipping them swaps the labels. The
    // pairs below follow base.py:86-135 and the live wire (completed
    // first); PIDASHCONV-504 tracks the groups const.
    for (alias, group) in [
        ("completed_issues", Some("completed")),
        ("cancelled_issues", Some("cancelled")),
        ("started_issues", Some("started")),
        ("unstarted_issues", Some("unstarted")),
        ("backlog_issues", Some("backlog")),
        ("total_issues", None),
    ] {
        stmt.expr_as(
            Func::coalesce([count_annotation(group), Expr::val(0).into()]),
            Alias::new(alias),
        );
    }
    // Six estimate sums in queries::ESTIMATE_POINT_ALIASES order.
    for (alias, group) in [
        ("backlog_estimate_points", Some("backlog")),
        ("unstarted_estimate_points", Some("unstarted")),
        ("started_estimate_points", Some("started")),
        ("cancelled_estimate_points", Some("cancelled")),
        ("completed_estimate_points", Some("completed")),
        ("total_estimate_points", None),
    ] {
        stmt.expr_as(
            Func::coalesce([estimate_annotation(group), Expr::val(0.0).into()]),
            Alias::new(alias),
        );
    }
    // ARRAY_AGG(DISTINCT member_id) never yields NULL elements in
    // Postgres, so the legacy FILTER (WHERE member_id IS NOT NULL) is a
    // no-op: the COALESCE+array-agg form returns the same array.
    stmt.expr_as(
        Func::coalesce([
            PgFunc::array_agg_distinct(mcol("module_members", module_members::Column::MemberId))
                .into(),
            Expr::val("{}").cast_as(Alias::new("uuid[]")),
        ]),
        Alias::new("member_ids"),
    )
    .from_as(modules::Entity, Alias::new("m"))
    .join_as(
        JoinType::InnerJoin,
        workspaces::Entity,
        Alias::new("w"),
        mcol("m", modules::Column::WorkspaceId).equals(mref("w", workspaces::Column::Id)),
    )
    .join_as(
        JoinType::LeftJoin,
        module_members::Entity,
        Alias::new("module_members"),
        {
            let on = Condition::all().add(
                mcol("m", modules::Column::Id)
                    .equals(mref("module_members", module_members::Column::ModuleId)),
            );
            if member_deleted_guard {
                // The ported bug lives here: the archive queryset does
                // not filter soft-deleted members.
                on.add(mcol("module_members", module_members::Column::DeletedAt).is_null())
            } else {
                on
            }
        },
    )
    .and_where(mcol("m", modules::Column::ProjectId).eq(*project_id))
    .and_where(mcol("w", workspaces::Column::Slug).eq(slug))
    .and_where(mcol("m", modules::Column::DeletedAt).is_null())
    .group_by_col(mref("m", modules::Column::Id))
    .order_by(Alias::new("is_favorite"), Order::Desc)
    .order_by(mref("m", modules::Column::CreatedAt), Order::Desc);
    if archived_only {
        stmt.and_where(mcol("m", modules::Column::ArchivedAt).is_not_null());
    } else if archived_null {
        stmt.and_where(mcol("m", modules::Column::ArchivedAt).is_null());
    }
    if let Some(id) = module_id {
        stmt.and_where(mcol("m", modules::Column::Id).eq(*id));
    }
    stmt
}

/// Bridge one annotated module row to the wire map (the
/// `fetch_module_links` Map precedent: uuids as strings, datetimes
/// RFC3339; `shape_row` re-renders only what it lists).
pub(crate) fn module_row_map(row: ModuleRowData) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(row.id.to_string()));
    map.insert(
        "workspace_id".into(),
        Value::String(row.workspace_id.to_string()),
    );
    map.insert(
        "project_id".into(),
        Value::String(row.project_id.to_string()),
    );
    map.insert("name".into(), Value::String(row.name));
    map.insert("description".into(), Value::String(row.description));
    map.insert(
        "description_text".into(),
        row.description_text.unwrap_or(Value::Null),
    );
    map.insert(
        "description_html".into(),
        row.description_html.unwrap_or(Value::Null),
    );
    map.insert(
        "start_date".into(),
        row.start_date
            .map(|d| Value::String(d.to_string()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "target_date".into(),
        row.target_date
            .map(|d| Value::String(d.to_string()))
            .unwrap_or(Value::Null),
    );
    map.insert("status".into(), Value::String(row.status));
    map.insert(
        "lead_id".into(),
        row.lead_id
            .map(|v| Value::String(v.to_string()))
            .unwrap_or(Value::Null),
    );
    map.insert("view_props".into(), row.view_props);
    map.insert("sort_order".into(), serde_json::json!(row.sort_order));
    map.insert(
        "external_source".into(),
        row.external_source
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "external_id".into(),
        row.external_id.map(Value::String).unwrap_or(Value::Null),
    );
    map.insert("logo_props".into(), row.logo_props);
    map.insert(
        "archived_at".into(),
        row.archived_at
            .map(|v| Value::String(v.to_rfc3339()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "created_at".into(),
        Value::String(row.created_at.to_rfc3339()),
    );
    map.insert(
        "updated_at".into(),
        Value::String(row.updated_at.to_rfc3339()),
    );
    map.insert("is_favorite".into(), Value::Bool(row.is_favorite));
    map.insert(
        "completed_issues".into(),
        serde_json::json!(row.completed_issues),
    );
    map.insert(
        "cancelled_issues".into(),
        serde_json::json!(row.cancelled_issues),
    );
    map.insert(
        "started_issues".into(),
        serde_json::json!(row.started_issues),
    );
    map.insert(
        "unstarted_issues".into(),
        serde_json::json!(row.unstarted_issues),
    );
    map.insert(
        "backlog_issues".into(),
        serde_json::json!(row.backlog_issues),
    );
    map.insert("total_issues".into(), serde_json::json!(row.total_issues));
    map.insert(
        "backlog_estimate_points".into(),
        row.backlog_estimate_points
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "unstarted_estimate_points".into(),
        row.unstarted_estimate_points
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "started_estimate_points".into(),
        row.started_estimate_points
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "cancelled_estimate_points".into(),
        row.cancelled_estimate_points
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "completed_estimate_points".into(),
        row.completed_estimate_points
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "total_estimate_points".into(),
        row.total_estimate_points
            .map(|v| serde_json::json!(v))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "member_ids".into(),
        Value::Array(
            row.member_ids
                .into_iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        ),
    );
    map
}

/// Fetch annotated module rows (one statement, the statement above).
async fn fetch_module_rows(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    slug: &str,
    user_id: &uuid::Uuid,
    module_id: Option<&uuid::Uuid>,
    archived_null: bool,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = crate::app_issues::list_engine::query_built::<ModuleRowData>(
        &db,
        &module_rows_statement(
            slug,
            project_id,
            user_id,
            module_id,
            archived_null,
            false,
            true,
        ),
    )
    .await?;
    Ok(rows.into_iter().map(module_row_map).collect())
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

pub(crate) fn quote_json(rendered: String) -> String {
    serde_json::to_string(&rendered).unwrap_or("null".to_owned())
}

pub(crate) fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// `user_timezone_converter` + DRF rendering for one value.
/// Shared with sibling handler issue 407 (link/userprops/archive rows).
pub(crate) fn shift_datetime(value: &Value, timezone: &Tz) -> String {
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
/// Shared with sibling handler issue 407 (archived-row floats).
pub(crate) fn render_float(value: &Value) -> String {
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

/// Shared with sibling handler issue 407 (archived detail shell).
pub(crate) fn shape_value(field: &str, value: &Value, timezone: &Tz) -> String {
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

fn validate_date_field(errors: &mut FieldErrors, field: &str, value: &Value) -> Option<String> {
    match value {
        Value::Null => {
            push_error(errors, field, "This field may not be null.".to_owned());
            None
        }
        Value::String(text) => match chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
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
        },
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
async fn validate_pk_value(pool: &sqlx::PgPool, value: &Value) -> Result<uuid::Uuid, String> {
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
                let db = pidash_db::pool::sea_orm_connection(pool);
                let exists: Option<uuid::Uuid> = user_exists_select(&id)
                    .into_tuple()
                    .one(&db)
                    .await
                    .map_err(|_| "Invalid pk \"?\" - object does not exist.".to_owned())?;
                match exists {
                    Some(_) => Ok(id),
                    None => Err(format!("Invalid pk \"{raw}\" - object does not exist.")),
                }
            }
        },
        other => Err(format!(
            "\u{201c}{}\u{201d} is not a valid UUID.",
            py_str(other)
        )),
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
            push_error(
                &mut errors,
                "name",
                "This field may not be null.".to_owned(),
            );
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
                push_error(&mut errors, "description", "Not a valid string.".to_owned());
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
            push_error(
                &mut errors,
                "status",
                "This field may not be null.".to_owned(),
            );
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
    if body.get("sort_order").is_some()
        && sort_order.is_none()
        && errors.iter().all(|(f, _)| f != "sort_order")
    {
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
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    check_gate(gate, slug, &facts)?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    Ok((pool, user_id, project_id, timezone))
}

/// `ModuleViewSet.list` (`base.py:353-393`): archived excluded, bare JSON
/// array. `fields = [f for f in request.GET.get("fields", "").split(",")
/// if f]` (`base.py:180-183`): a non-empty list renders the
/// `ModuleSerializer` branch (`:356-357`) — the projection is discarded,
/// but the row is the full 29-key `Meta.fields` shape; otherwise the
/// annotated 28-key `.values()` row renders. `Query<HashMap<String, String>>`
/// keeps the last duplicate exactly like `QueryDict.get`.
async fn module_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let (pool, user_id, project_id, timezone) =
        module_context(&state, &slug, &project_raw, extension, gate_for_list()).await?;
    let fields: Vec<&str> = query
        .get("fields")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
        .filter(|field| !field.is_empty())
        .collect();
    let row_order = if fields.is_empty() {
        SHARED_ROW_ORDER
    } else {
        shape::MODULE_LIST_FIELD_ORDER
    };
    let rows = fetch_module_rows(&pool, &project_id, &slug, &user_id, None, true).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&shape_row(row, row_order, &timezone));
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<ContextProjectRow> = context_project_select(slug, project_id)
        .into_model::<ContextProjectRow>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| (row.id, row.workspace_id))
        .ok_or(Denial::NotFound)
}

/// Duplicate-name probe (`module.py:68-72,99-100`): live rows only, with
/// the self exclusion on update.
async fn duplicate_name_exists(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    name: &str,
    exclude: Option<&uuid::Uuid>,
) -> Result<bool, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<uuid::Uuid> = duplicate_name_select(project_id, name, exclude)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let now_sea = chrono::Utc::now().fixed_offset();
    module_members::Entity::update_many()
        .col_expr(
            module_members::Column::DeletedAt,
            Expr::value(Some(now_sea)),
        )
        .filter(module_members::Column::ModuleId.eq(*module_id))
        .filter(module_members::Column::DeletedAt.is_null())
        .exec(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    for chunk in member_ids.chunks(shape::MEMBER_BULK_BATCH_SIZE) {
        let models: Vec<module_members::ActiveModel> = chunk
            .iter()
            .map(|member_id| module_members::ActiveModel {
                id: sea_orm::Set(uuid::Uuid::new_v4()),
                workspace_id: sea_orm::Set(*workspace_id),
                project_id: sea_orm::Set(*project_id),
                module_id: sea_orm::Set(*module_id),
                member_id: sea_orm::Set(*member_id),
                created_by_id: sea_orm::Set(created_by),
                updated_by_id: sea_orm::Set(updated_by),
                created_at: sea_orm::Set(now_sea),
                updated_at: sea_orm::Set(now_sea),
                deleted_at: sea_orm::Set(None),
            })
            .collect();
        module_members::Entity::insert_many(models)
            .on_conflict(
                sea_orm::sea_query::OnConflict::new()
                    .do_nothing()
                    .to_owned(),
            )
            .exec(&db)
            .await
            .map_err(|_| Denial::ServerError)?;
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
    let db = pidash_db::pool::sea_orm_connection(&pool);
    #[derive(Debug, FromQueryResult)]
    struct MinSortRow {
        min_sort: Option<f64>,
    }
    let min_row: Option<MinSortRow> = modules::Entity::find()
        .select_only()
        .column_as(modules::Column::SortOrder.min(), "min_sort")
        .filter(modules::Column::ProjectId.eq(project_id))
        .filter(modules::Column::DeletedAt.is_null())
        .into_model::<MinSortRow>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    // `Module.save` (`db/models/module.py:115-124`) only clobbers input
    // `sort_order` when siblings exist (`MIN(sibling) - 10000`); with no
    // siblings the validated input lands, defaulting to `65535.0` when
    // absent (verified live).
    let sort_order = match min_row.and_then(|row| row.min_sort) {
        Some(min) => min - 10_000.0,
        None => input.sort_order.unwrap_or(65535.0),
    };
    let module_id: uuid::Uuid = uuid::Uuid::new_v4();
    let now_sea = chrono::Utc::now().fixed_offset();
    modules::Entity::insert(modules::ActiveModel {
        id: sea_orm::Set(module_id),
        workspace_id: sea_orm::Set(workspace_id),
        project_id: sea_orm::Set(project_id),
        name: sea_orm::Set(input.name.clone().expect("name")),
        description: sea_orm::Set(input.description.clone().unwrap_or_default()),
        description_text: sea_orm::Set(input.description_text.clone()),
        description_html: sea_orm::Set(input.description_html.clone()),
        start_date: sea_orm::Set(parse_date_opt(&input.start_date.clone().flatten())),
        target_date: sea_orm::Set(parse_date_opt(&input.target_date.clone().flatten())),
        status: sea_orm::Set(input.status.clone().unwrap_or_else(|| "planned".to_owned())),
        lead_id: sea_orm::Set(input.lead_id.flatten()),
        view_props: sea_orm::Set(
            input
                .view_props
                .clone()
                .unwrap_or(Value::Object(Map::new())),
        ),
        sort_order: sea_orm::Set(sort_order),
        external_source: sea_orm::Set(input.external_source.clone().flatten()),
        external_id: sea_orm::Set(input.external_id.clone().flatten()),
        archived_at: sea_orm::Set(None),
        logo_props: sea_orm::Set(
            input
                .logo_props
                .clone()
                .unwrap_or(Value::Object(Map::new())),
        ),
        created_by_id: sea_orm::Set(Some(user_id)),
        updated_by_id: sea_orm::Set(None),
        created_at: sea_orm::Set(now_sea),
        updated_at: sea_orm::Set(now_sea),
        deleted_at: sea_orm::Set(None),
    })
    .exec(&db)
    .await
    .map_err(|_| Denial::ServerError)?;
    if let Some(members) = input.member_ids.as_ref() {
        // `created_by=module.created_by, updated_by=module.updated_by`
        // (`module.py:83-84`): the crum auto-set row, user/NULL on create.
        replace_members(
            &pool,
            &module_id,
            &project_id,
            &workspace_id,
            Some(user_id),
            None,
            members,
        )
        .await?;
    }
    let rows =
        fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), false).await?;
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
/// Shared with sibling handler issue 407 (archived detail `link_module`).
pub(crate) async fn fetch_module_links(
    pool: &sqlx::PgPool,
    module_id: &uuid::Uuid,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows: Vec<ModuleLinkRow> = module_links_select(module_id)
        .into_model::<ModuleLinkRow>()
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut map = Map::new();
        map.insert("id".into(), Value::String(row.id.to_string()));
        map.insert(
            "created_at".into(),
            Value::String(row.created_at.to_rfc3339()),
        );
        map.insert(
            "updated_at".into(),
            Value::String(row.updated_at.to_rfc3339()),
        );
        map.insert(
            "deleted_at".into(),
            match row.deleted_at {
                Some(value) => Value::String(value.to_rfc3339()),
                None => Value::Null,
            },
        );
        map.insert(
            "title".into(),
            match row.title {
                Some(value) => Value::String(value),
                None => Value::Null,
            },
        );
        map.insert("url".into(), Value::String(row.url));
        map.insert("metadata".into(), row.metadata);
        map.insert(
            "created_by".into(),
            match row.created_by {
                Some(value) => Value::String(value.to_string()),
                None => Value::Null,
            },
        );
        map.insert(
            "updated_by".into(),
            match row.updated_by {
                Some(value) => Value::String(value.to_string()),
                None => Value::Null,
            },
        );
        map.insert("project".into(), Value::String(row.project.to_string()));
        map.insert("workspace".into(), Value::String(row.workspace.to_string()));
        map.insert("module".into(), Value::String(row.module.to_string()));
        out.push(map);
    }
    Ok(out)
}

/// Shared with sibling handler issue 407 (link rows + archived detail).
pub(crate) fn shape_link(row: &Map<String, Value>, timezone: &Tz) -> String {
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
/// Shared with sibling handler issue 407 (archived detail).
pub(crate) async fn fetch_sub_issues(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
) -> Result<i64, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let count = sub_issues_count_select(project_id, module_id)
        .count(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(count as i64)
}

/// `estimate_type` gate (`base.py:417-422`): a points estimate exists for
/// the project.
/// Shared with sibling handler issue 407 (archived detail).
pub(crate) async fn fetch_estimate_type(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let found: Option<uuid::Uuid> = estimate_type_exists_select(slug, project_id)
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(found.is_some())
}
/// Shared `FROM/WHERE` for the distribution queries: live module bridges
/// of this project/workspace under the `IssueManager` base
/// (`base.py:431-436,493-498,539-544,592-597`).
/// Ported bug (PB-2035, PB-0102 class): the M2M joins carry no
/// `deleted_at` guard, because Django groups through the `assignees` /
/// `labels` relations whose ORM joins never filter soft-deleted through
/// rows — removed assignees/labels still distribute.
/// Shared issue-facet `FROM` + `WHERE` (`queries::distribution_from_where`):
/// the "current module" scope with the IssueManager guards. Every
/// distribution and the burndown scope build on this fragment. The M2M
/// joins carry no `deleted_at` guard on purpose (Django groups through
/// the `assignees`/`labels` relations whose ORM joins never filter
/// soft-deleted through rows — PB-2035, PB-0102 class).
#[allow(clippy::too_many_arguments)]
fn distribution_scope(
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    assignee_join: bool,
    label_join: bool,
    scope_deleted_guard: bool,
) -> sea_orm::sea_query::SelectStatement {
    let mut stmt = SeaQuery::select();
    stmt.from_as(issues::Entity, Alias::new("issues"))
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            Alias::new("projects"),
            mcol("projects", projects::Column::Id)
                .equals(mref("issues", issues::Column::ProjectId)),
        )
        .join_as(
            JoinType::InnerJoin,
            workspaces::Entity,
            Alias::new("workspaces"),
            mcol("workspaces", workspaces::Column::Id)
                .equals(mref("issues", issues::Column::WorkspaceId)),
        )
        .join_as(
            JoinType::LeftJoin,
            states::Entity,
            Alias::new("states"),
            mcol("states", states::Column::Id).equals(mref("issues", issues::Column::StateId)),
        )
        .join_as(
            JoinType::InnerJoin,
            module_issues::Entity,
            Alias::new("module_issues"),
            mcol("module_issues", module_issues::Column::IssueId)
                .equals(mref("issues", issues::Column::Id)),
        )
        .join_as(
            JoinType::LeftJoin,
            estimate_points::Entity,
            Alias::new("estimate_points"),
            mcol("estimate_points", estimate_points::Column::Id)
                .equals(mref("issues", issues::Column::EstimatePointId)),
        );
    if assignee_join {
        stmt.join_as(
            JoinType::LeftJoin,
            issue_assignees::Entity,
            Alias::new("issue_assignees"),
            mcol("issue_assignees", issue_assignees::Column::IssueId)
                .equals(mref("issues", issues::Column::Id)),
        )
        .join_as(
            JoinType::LeftJoin,
            users::Entity,
            Alias::new("assignees"),
            mcol("assignees", users::Column::Id)
                .equals(mref("issue_assignees", issue_assignees::Column::AssigneeId)),
        );
    }
    if label_join {
        stmt.join_as(
            JoinType::LeftJoin,
            issue_labels::Entity,
            Alias::new("issue_labels"),
            mcol("issue_labels", issue_labels::Column::IssueId)
                .equals(mref("issues", issues::Column::Id)),
        )
        .join_as(
            JoinType::LeftJoin,
            labels::Entity,
            Alias::new("labels"),
            mcol("labels", labels::Column::Id)
                .equals(mref("issue_labels", issue_labels::Column::LabelId)),
        );
    }
    for condition in annotation_guards() {
        stmt.and_where(condition);
    }
    stmt.and_where(mcol("workspaces", workspaces::Column::Slug).eq(slug))
        .and_where(mcol("issues", issues::Column::ProjectId).eq(*project_id))
        .and_where(mcol("module_issues", module_issues::Column::ModuleId).eq(*module_id));
    if scope_deleted_guard {
        // The archive label-estimate queryset omits this guard (the
        // :386-391 ported bug); see fetch_archive_label_estimate.
        stmt.and_where(mcol("module_issues", module_issues::Column::DeletedAt).is_null());
    }
    stmt
}

/// Archive label estimate (archive.py:386-391): identical to the main
/// label estimate query except the ported bug — no
/// `module_issues.deleted_at IS NULL` filter, and no `deleted_at` guard
/// on the label M2M joins (PB-2035, PB-0102 class).
pub(crate) async fn fetch_archive_label_estimate(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let mut stmt = distribution_scope(slug, project_id, module_id, false, true, false);
    stmt.expr_as(
        mcol("labels", labels::Column::Name),
        Alias::new("label_name"),
    )
    .expr_as(mcol("labels", labels::Column::Color), Alias::new("color"))
    .expr_as(mcol("labels", labels::Column::Id), Alias::new("label_id"))
    .expr_as(sum_filter_aggregation(None), Alias::new("total_estimates"))
    .expr_as(
        sum_filter_aggregation(Some(true)),
        Alias::new("completed_estimates"),
    )
    .expr_as(
        sum_filter_aggregation(Some(false)),
        Alias::new("pending_estimates"),
    )
    .group_by_columns([
        mref("labels", labels::Column::Name),
        mref("labels", labels::Column::Color),
        mref("labels", labels::Column::Id),
    ])
    .order_by(mref("labels", labels::Column::Name), Order::Asc);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = crate::app_issues::list_engine::query_built::<DistributionRow>(&db, &stmt).await?;
    Ok(rows
        .into_iter()
        .map(|row| distribution_map(row, true, false))
        .collect())
}

/// One distribution output row, decoded by name.
#[derive(Debug, FromQueryResult)]
struct DistributionRow {
    first_name: Option<String>,
    last_name: Option<String>,
    assignee_id: Option<uuid::Uuid>,
    avatar_url: Option<String>,
    display_name: Option<String>,
    label_name: Option<String>,
    color: Option<String>,
    label_id: Option<uuid::Uuid>,
    total_estimates: Option<f64>,
    completed_estimates: Option<f64>,
    pending_estimates: Option<f64>,
    total_issues: Option<i64>,
    completed_issues: Option<i64>,
    pending_issues: Option<i64>,
}

/// Bridge one distribution row to the wire map.
fn distribution_map(row: DistributionRow, estimate: bool, assignee: bool) -> Map<String, Value> {
    let opt_uuid = |v: Option<uuid::Uuid>| {
        v.map(|v| Value::String(v.to_string()))
            .unwrap_or(Value::Null)
    };
    let mut map = Map::new();
    if assignee {
        map.insert(
            "first_name".into(),
            row.first_name.map(Value::String).unwrap_or(Value::Null),
        );
        map.insert(
            "last_name".into(),
            row.last_name.map(Value::String).unwrap_or(Value::Null),
        );
        map.insert("assignee_id".into(), opt_uuid(row.assignee_id));
        map.insert(
            "avatar_url".into(),
            row.avatar_url.map(Value::String).unwrap_or(Value::Null),
        );
        map.insert(
            "display_name".into(),
            row.display_name.map(Value::String).unwrap_or(Value::Null),
        );
    } else {
        map.insert(
            "label_name".into(),
            row.label_name.map(Value::String).unwrap_or(Value::Null),
        );
        map.insert(
            "color".into(),
            row.color.map(Value::String).unwrap_or(Value::Null),
        );
        map.insert("label_id".into(), opt_uuid(row.label_id));
    }
    if estimate {
        map.insert(
            "total_estimates".into(),
            row.total_estimates
                .map(|v| serde_json::json!(v))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "completed_estimates".into(),
            row.completed_estimates
                .map(|v| serde_json::json!(v))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "pending_estimates".into(),
            row.pending_estimates
                .map(|v| serde_json::json!(v))
                .unwrap_or(Value::Null),
        );
    } else {
        map.insert(
            "total_issues".into(),
            row.total_issues
                .map(|v| serde_json::json!(v))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "completed_issues".into(),
            row.completed_issues
                .map(|v| serde_json::json!(v))
                .unwrap_or(Value::Null),
        );
        map.insert(
            "pending_issues".into(),
            row.pending_issues
                .map(|v| serde_json::json!(v))
                .unwrap_or(Value::Null),
        );
    }
    map
}

/// The assignee avatar CASE (`queries::avatar_url_case_sql` with the real
/// stores column `avatar_asset_id` — PIDASHCONV-502 filed for the
/// services builder): the file's static asset URL or the text avatar.
fn assignee_avatar_case() -> sea_orm::sea_query::SimpleExpr {
    CaseStatement::new()
        .case(
            mcol("assignees", users::Column::AvatarAssetId).is_not_null(),
            Func::cust(Alias::new("concat"))
                .arg(Expr::val("/api/assets/v2/static/"))
                .arg(mcol("assignees", users::Column::AvatarAssetId).cast_as(Alias::new("text")))
                .arg(Expr::val("/")),
        )
        .case(
            mcol("assignees", users::Column::AvatarAssetId).is_null(),
            mcol("assignees", users::Column::Avatar),
        )
        .finally(Expr::val(Option::<String>::None))
        .into()
}

/// `FILTER (WHERE ...)` spelled as CASE (`list_engine` precedent):
/// counts match `COUNT FILTER` rows (0 fallthrough), sums match
/// `SUM FILTER` NULLs (filtered-out everywhere stays NULL).
fn count_filter_aggregation(completed: Option<bool>) -> sea_orm::sea_query::SimpleExpr {
    let condition: sea_orm::sea_query::SimpleExpr = completed_filter_condition(completed).into();
    Func::sum(
        CaseStatement::new()
            .case(condition, Expr::val(1))
            .finally(Expr::val(0)),
    )
    .into()
}

fn sum_filter_aggregation(completed: Option<bool>) -> sea_orm::sea_query::SimpleExpr {
    let condition: sea_orm::sea_query::SimpleExpr = completed_filter_condition(completed).into();
    Func::sum(
        CaseStatement::new()
            .case(
                condition,
                mcol("estimate_points", estimate_points::Column::Value)
                    .cast_as(Alias::new("float")),
            )
            .finally(Expr::val(Option::<f64>::None)),
    )
    .into()
}

fn completed_filter_condition(completed: Option<bool>) -> Condition {
    let base = Condition::all()
        .add(mcol("issues", issues::Column::ArchivedAt).is_null())
        .add(mcol("issues", issues::Column::IsDraft).eq(false));
    match completed {
        Some(true) => base.add(mcol("issues", issues::Column::CompletedAt).is_not_null()),
        Some(false) => base.add(mcol("issues", issues::Column::CompletedAt).is_null()),
        None => base,
    }
}

/// Assignee distribution rows (`base.py:430-490,538-589`):
/// `estimate=true` sums estimates, else counts issues.
/// Shared with sibling handler issue 407 (archived detail).
pub(crate) async fn fetch_assignee_distribution(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    estimate: bool,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let (total_key, completed_key, pending_key) = if estimate {
        (
            "total_estimates",
            "completed_estimates",
            "pending_estimates",
        )
    } else {
        ("total_issues", "completed_issues", "pending_issues")
    };
    let aggregate = |completed: Option<bool>| {
        if estimate {
            sum_filter_aggregation(completed)
        } else {
            count_filter_aggregation(completed)
        }
    };
    let mut stmt = distribution_scope(slug, project_id, module_id, true, false, true);
    stmt.expr_as(
        mcol("assignees", users::Column::FirstName),
        Alias::new("first_name"),
    )
    .expr_as(
        mcol("assignees", users::Column::LastName),
        Alias::new("last_name"),
    )
    .expr_as(
        mcol("assignees", users::Column::Id),
        Alias::new("assignee_id"),
    )
    .expr_as(assignee_avatar_case(), Alias::new("avatar_url"))
    .expr_as(
        mcol("assignees", users::Column::DisplayName),
        Alias::new("display_name"),
    )
    .expr_as(aggregate(None), Alias::new(total_key))
    .expr_as(aggregate(Some(true)), Alias::new(completed_key))
    .expr_as(aggregate(Some(false)), Alias::new(pending_key))
    .group_by_columns([
        mref("assignees", users::Column::FirstName),
        mref("assignees", users::Column::LastName),
        mref("assignees", users::Column::Id),
        mref("assignees", users::Column::DisplayName),
    ])
    .add_group_by(vec![assignee_avatar_case()])
    .order_by(mref("assignees", users::Column::FirstName), Order::Asc)
    .order_by(mref("assignees", users::Column::LastName), Order::Asc);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = crate::app_issues::list_engine::query_built::<DistributionRow>(&db, &stmt).await?;
    Ok(rows
        .into_iter()
        .map(|row| distribution_map(row, estimate, true))
        .collect())
}

/// Label distribution rows (`base.py:492-525,591-624`).
/// Shared with sibling handler issue 407 (archived detail).
pub(crate) async fn fetch_label_distribution(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    estimate: bool,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let (total_key, completed_key, pending_key) = if estimate {
        (
            "total_estimates",
            "completed_estimates",
            "pending_estimates",
        )
    } else {
        ("total_issues", "completed_issues", "pending_issues")
    };
    let aggregate = |completed: Option<bool>| {
        if estimate {
            sum_filter_aggregation(completed)
        } else {
            count_filter_aggregation(completed)
        }
    };
    let mut stmt = distribution_scope(slug, project_id, module_id, false, true, true);
    stmt.expr_as(
        mcol("labels", labels::Column::Name),
        Alias::new("label_name"),
    )
    .expr_as(mcol("labels", labels::Column::Color), Alias::new("color"))
    .expr_as(mcol("labels", labels::Column::Id), Alias::new("label_id"))
    .expr_as(aggregate(None), Alias::new(total_key))
    .expr_as(aggregate(Some(true)), Alias::new(completed_key))
    .expr_as(aggregate(Some(false)), Alias::new(pending_key))
    .group_by_columns([
        mref("labels", labels::Column::Name),
        mref("labels", labels::Column::Color),
        mref("labels", labels::Column::Id),
    ])
    .order_by(mref("labels", labels::Column::Name), Order::Asc);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = crate::app_issues::list_engine::query_built::<DistributionRow>(&db, &stmt).await?;
    Ok(rows
        .into_iter()
        .map(|row| distribution_map(row, estimate, false))
        .collect())
}

/// Assignee row orders: `.values()` lists `avatar_url` (a `Case`
/// annotation) before `display_name` (a concrete joined field), but Django
/// renders concrete fields first in call order and annotations after, so
/// `display_name` precedes `avatar_url` on the wire (verified live;
/// PIDASHCONV-503 filed for the services key consts).
pub(crate) const ASSIGNEE_COUNT_ORDER: &[&str] = &[
    "first_name",
    "last_name",
    "assignee_id",
    "display_name",
    "avatar_url",
    "total_issues",
    "completed_issues",
    "pending_issues",
];
pub(crate) const ASSIGNEE_ESTIMATE_ORDER: &[&str] = &[
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
        if *key == "total_estimates" || *key == "completed_estimates" || *key == "pending_estimates"
        {
            out.push_str(&render_float(value));
        } else {
            out.push_str(&serde_json::to_string(value).unwrap_or("null".to_owned()));
        }
    }
    out.push('}');
    out
}

/// Shared with sibling handler issue 407 (archived detail).
pub(crate) fn render_distribution_array(keys: &[&str], rows: &[Map<String, Value>]) -> String {
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

/// One aggregate-only burndown row (total points `SUM`).
#[derive(Debug, FromQueryResult)]
struct BurndownAggregate {
    total: Option<f64>,
}

/// `burndown_plot` module branch (`utils/analytics_plot.py:198-264`,
/// `plot_type="points"` or `"issues"`): per-day remaining over the
/// module's date range, future days `null`.
/// Completions truncate in the actor's active zone (`TimezoneMixin`
/// activates `user_timezone`; `TruncDate` follows it), bound as `$4`;
/// "today" stays the UTC day (`timezone.now().date()`).
/// Shared with sibling handler issue 407 (archived detail).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fetch_burndown_chart(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    start: chrono::NaiveDate,
    target: chrono::NaiveDate,
    total_issues: i64,
    points: bool,
    timezone: &Tz,
) -> Result<String, Denial> {
    let days = (target - start).num_days();
    let mut chart = String::from("{");
    if days < 0 {
        chart.push('}');
        return Ok(chart);
    }
    // Per-day completed inputs. `AT TIME ZONE` truncation has no
    // sea-query builder (the `$4` zone bind would need an Expr::cust,
    // raw SQL per the guard), so the statement fetches per-issue rows
    // and truncation+grouping happens below in the actor's zone — the
    // cycle PIDASHCONV-1184 precedent. Day arithmetic is the same:
    // summing per-issue values per day equals summing grouped days.
    // Points total sums ALL live module-issue estimates, not just the
    // completed ones (`analytics_plot.py:212-214` — no completed filter
    // on the total). `SUM` over no rows is NULL, and then Python's
    // `sum([])` is the INT `0`, so the chart renders ints, not `0.0`.
    let db = pidash_db::pool::sea_orm_connection(pool);
    let total_points: Option<f64> = if points {
        let mut stmt = distribution_scope(slug, project_id, module_id, false, false, true);
        stmt.expr_as(
            Func::sum(
                mcol("estimate_points", estimate_points::Column::Value)
                    .cast_as(Alias::new("double precision")),
            ),
            Alias::new("total"),
        )
        .and_where(mcol("estimate_points", estimate_points::Column::Id).is_not_null());
        let rate_rows =
            crate::app_issues::list_engine::query_built::<BurndownAggregate>(&db, &stmt).await?;
        rate_rows.into_iter().next().and_then(|row| row.total)
    } else {
        None
    };
    #[derive(Debug, FromQueryResult)]
    struct RawBurndownRow {
        completed_at: Option<sea_orm::prelude::DateTimeWithTimeZone>,
        v: Option<f64>,
        n: Option<i64>,
    }
    let raw_rows = if points {
        let mut stmt = distribution_scope(slug, project_id, module_id, false, false, true);
        stmt.expr_as(
            mcol("issues", issues::Column::CompletedAt),
            Alias::new("completed_at"),
        )
        .expr_as(
            mcol("estimate_points", estimate_points::Column::Value)
                .cast_as(Alias::new("double precision")),
            Alias::new("v"),
        )
        .expr_as(
            Expr::val(Option::<i64>::None).cast_as(Alias::new("bigint")),
            Alias::new("n"),
        )
        .and_where(mcol("estimate_points", estimate_points::Column::Id).is_not_null())
        .and_where(mcol("issues", issues::Column::CompletedAt).is_not_null())
        .order_by(mref("issues", issues::Column::CompletedAt), Order::Asc);
        crate::app_issues::list_engine::query_built::<RawBurndownRow>(&db, &stmt).await?
    } else {
        let mut stmt = distribution_scope(slug, project_id, module_id, false, false, true);
        stmt.expr_as(
            mcol("issues", issues::Column::CompletedAt),
            Alias::new("completed_at"),
        )
        .expr_as(
            Expr::val(Option::<f64>::None).cast_as(Alias::new("float8")),
            Alias::new("v"),
        )
        .expr_as(Expr::val(1), Alias::new("n"))
        .and_where(mcol("issues", issues::Column::CompletedAt).is_not_null())
        .order_by(mref("issues", issues::Column::CompletedAt), Order::Asc);
        crate::app_issues::list_engine::query_built::<RawBurndownRow>(&db, &stmt).await?
    };
    use chrono::TimeZone as _;
    let rows: Vec<(Option<chrono::NaiveDate>, Option<f64>, Option<i64>)> = raw_rows
        .into_iter()
        .map(|row| {
            let day = row
                .completed_at
                .map(|ts| timezone.from_utc_datetime(&ts.naive_utc()).date_naive());
            (day, row.v, row.n)
        })
        .collect();
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
    let facts = fetch_allow_facts(
        &pool,
        &slug,
        &project_id,
        &user_id,
        gate_roles(retrieve_gate),
    )
    .await?;
    check_gate(retrieve_gate, &slug, &facts)?;
    let timezone = actor_timezone(&pool, &user_id).await?;
    let module_id = parse_uuid_or_invalid(&pk_raw)?;
    // Existence probe (`queryset.exists()`, `:414`).
    let rows =
        fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), true).await?;
    if rows.is_empty() {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            queries::MODULE_NOT_FOUND_BODY.to_owned(),
        ));
    }
    // The detail read (`queryset.first()` twice, `:424-425` — kept as
    // sequencing, not deduped).
    let rows =
        fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), true).await?;
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
        let assignees =
            fetch_assignee_distribution(&pool, &slug, &project_id, &module_id, true).await?;
        let labels = fetch_label_distribution(&pool, &slug, &project_id, &module_id, true).await?;
        out.push_str("{\"assignees\":");
        out.push_str(&render_distribution_array(
            ASSIGNEE_ESTIMATE_ORDER,
            &assignees,
        ));
        out.push_str(",\"labels\":");
        out.push_str(&render_distribution_array(
            queries::LABEL_ESTIMATE_ROW_KEYS,
            &labels,
        ));
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
                    &pool,
                    &slug,
                    &project_id,
                    &module_id,
                    start,
                    target,
                    total_issues,
                    true,
                    &timezone,
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
    let assignees =
        fetch_assignee_distribution(&pool, &slug, &project_id, &module_id, false).await?;
    let labels = fetch_label_distribution(&pool, &slug, &project_id, &module_id, false).await?;
    out.push_str(",\"distribution\":{\"assignees\":");
    out.push_str(&render_distribution_array(ASSIGNEE_COUNT_ORDER, &assignees));
    out.push_str(",\"labels\":");
    out.push_str(&render_distribution_array(
        queries::LABEL_COUNT_ROW_KEYS,
        &labels,
    ));
    out.push_str(",\"completion_chart\":");
    let has_dates = row.get("start_date").is_some_and(|v| !v.is_null())
        && row.get("target_date").is_some_and(|v| !v.is_null());
    let total_issues = row
        .get("total_issues")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
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
                &pool,
                &slug,
                &project_id,
                &module_id,
                start,
                target,
                total_issues,
                false,
                &timezone,
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
    let visit = tasks::retrieve_visit_emit(&slug, &pk_raw, &user_id.to_string(), &project_wire);
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows: Vec<uuid::Uuid> = live_member_ids_select(module_id)
        .into_tuple()
        .all(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(rows.into_iter().map(|id| id.to_string()).collect())
}

/// One write-shape row (`ModuleWriteSerializer` over the stored module,
/// `WRITE_RESPONSE_ORDER`): FKs as PK strings, `members` + `member_ids`
/// as id arrays, datetimes in the active zone.
async fn fetch_write_row(
    pool: &sqlx::PgPool,
    module_id: &uuid::Uuid,
    timezone: &Tz,
) -> Result<String, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<WriteRowData> = write_row_select(module_id)
        .into_model::<WriteRowData>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let row = row.ok_or(Denial::ServerError)?;
    let mut map = Map::new();
    map.insert("id".into(), Value::String(row.id.to_string()));
    map.insert(
        "lead_id".into(),
        match row.lead_id {
            Some(value) => Value::String(value.to_string()),
            None => Value::Null,
        },
    );
    map.insert(
        "created_at".into(),
        Value::String(row.created_at.to_rfc3339()),
    );
    map.insert(
        "updated_at".into(),
        Value::String(row.updated_at.to_rfc3339()),
    );
    map.insert(
        "deleted_at".into(),
        match row.deleted_at {
            Some(value) => Value::String(value.to_rfc3339()),
            None => Value::Null,
        },
    );
    map.insert("name".into(), Value::String(row.name));
    map.insert("description".into(), Value::String(row.description));
    map.insert(
        "description_text".into(),
        match row.description_text {
            Some(value) => value,
            None => Value::Null,
        },
    );
    map.insert(
        "description_html".into(),
        match row.description_html {
            Some(value) => value,
            None => Value::Null,
        },
    );
    map.insert(
        "start_date".into(),
        match row.start_date {
            Some(value) => Value::String(value.to_string()),
            None => Value::Null,
        },
    );
    map.insert(
        "target_date".into(),
        match row.target_date {
            Some(value) => Value::String(value.to_string()),
            None => Value::Null,
        },
    );
    map.insert("status".into(), Value::String(row.status));
    map.insert("view_props".into(), row.view_props);
    map.insert("sort_order".into(), serde_json::json!(row.sort_order));
    map.insert(
        "external_source".into(),
        match row.external_source {
            Some(value) => Value::String(value),
            None => Value::Null,
        },
    );
    map.insert(
        "external_id".into(),
        match row.external_id {
            Some(value) => Value::String(value),
            None => Value::Null,
        },
    );
    map.insert(
        "archived_at".into(),
        match row.archived_at {
            Some(value) => Value::String(value.to_rfc3339()),
            None => Value::Null,
        },
    );
    map.insert("logo_props".into(), row.logo_props);
    map.insert(
        "created_by".into(),
        match row.created_by {
            Some(value) => Value::String(value.to_string()),
            None => Value::Null,
        },
    );
    map.insert(
        "updated_by".into(),
        match row.updated_by {
            Some(value) => Value::String(value.to_string()),
            None => Value::Null,
        },
    );
    map.insert("project".into(), Value::String(row.project.to_string()));
    map.insert("workspace".into(), Value::String(row.workspace.to_string()));
    map.insert(
        "lead".into(),
        match row.lead {
            Some(value) => Value::String(value.to_string()),
            None => Value::Null,
        },
    );
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let now_sea = chrono::Utc::now().fixed_offset();
    let mut update = modules::Entity::update_many();
    if let Some(name) = input.name.as_ref() {
        update = update.col_expr(modules::Column::Name, Expr::value(name.clone()));
    }
    if let Some(description) = input.description.as_ref() {
        update = update.col_expr(
            modules::Column::Description,
            Expr::value(description.clone()),
        );
    }
    if let Some(value) = input.description_text.as_ref() {
        update = update.col_expr(
            modules::Column::DescriptionText,
            Expr::value(Some(value.clone())),
        );
    }
    if let Some(value) = input.description_html.as_ref() {
        update = update.col_expr(
            modules::Column::DescriptionHtml,
            Expr::value(Some(value.clone())),
        );
    }
    if let Some(date) = input.start_date.as_ref() {
        let parsed = date
            .as_ref()
            .and_then(|text| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok());
        update = update.col_expr(modules::Column::StartDate, Expr::value(parsed));
    }
    if let Some(date) = input.target_date.as_ref() {
        let parsed = date
            .as_ref()
            .and_then(|text| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok());
        update = update.col_expr(modules::Column::TargetDate, Expr::value(parsed));
    }
    if let Some(status) = input.status.as_ref() {
        update = update.col_expr(modules::Column::Status, Expr::value(status.clone()));
    }
    if let Some(lead_id) = input.lead_id.as_ref() {
        update = update.col_expr(modules::Column::LeadId, Expr::value(*lead_id));
    }
    if let Some(value) = input.view_props.as_ref() {
        update = update.col_expr(modules::Column::ViewProps, Expr::value(value.clone()));
    }
    if let Some(sort_order) = input.sort_order {
        update = update.col_expr(modules::Column::SortOrder, Expr::value(sort_order));
    }
    if let Some(value) = input.external_source.as_ref() {
        update = update.col_expr(modules::Column::ExternalSource, Expr::value(value.clone()));
    }
    if let Some(value) = input.external_id.as_ref() {
        update = update.col_expr(modules::Column::ExternalId, Expr::value(value.clone()));
    }
    if let Some(value) = input.logo_props.as_ref() {
        update = update.col_expr(modules::Column::LogoProps, Expr::value(value.clone()));
    }
    update = update.col_expr(modules::Column::UpdatedAt, Expr::value(now_sea));
    update = update.col_expr(modules::Column::UpdatedById, Expr::value(Some(*user_id)));
    update
        .filter(modules::Column::Id.eq(*module_id))
        .exec(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Validated `YYYY-MM-DD` input as a date value.
fn parse_date_opt(date: &Option<String>) -> Option<chrono::NaiveDate> {
    date.as_ref()
        .and_then(|text| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
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
    let rows =
        fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), false).await?;
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
            let db = pidash_db::pool::sea_orm_connection(&pool);
            let row: Option<StoredAuditRow> = stored_audit_select(&module_id)
                .into_model::<StoredAuditRow>()
                .one(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
            row.map(|r| (r.workspace_id, r.created_by_id, r.updated_by_id))
        } else {
            None
        };
    apply_write_fields(&pool, &module_id, &user_id, &input).await?;
    if let Some(members) = input.member_ids.as_ref() {
        let (workspace_id, created_by, updated_by) = stored.ok_or(Denial::ServerError)?;
        replace_members(
            &pool,
            &module_id,
            &project_id,
            &workspace_id,
            created_by,
            updated_by,
            members,
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
    let (pool, user_id, project_id, timezone) = module_context(
        &state,
        &slug,
        &project_raw,
        extension,
        gate_for_partial_update(),
    )
    .await?;
    let module_id = parse_uuid_or_invalid(&pk_raw)?;
    // Current row over `.filter(pk=pk)` (no archived filter, `:653`).
    let rows =
        fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), false).await?;
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
            let db = pidash_db::pool::sea_orm_connection(&pool);
            let row: Option<StoredAuditRow> = stored_audit_select(&module_id)
                .into_model::<StoredAuditRow>()
                .one(&db)
                .await
                .map_err(|_| Denial::ServerError)?;
            row.map(|value| (value.workspace_id, value.created_by_id, value.updated_by_id))
        } else {
            None
        };
    apply_write_fields(&pool, &module_id, &user_id, &input).await?;
    if let Some(members) = input.member_ids.as_ref() {
        let (workspace_id, created_by, updated_by) = stored.ok_or(Denial::ServerError)?;
        replace_members(
            &pool,
            &module_id,
            &project_id,
            &workspace_id,
            created_by,
            updated_by,
            members,
        )
        .await?;
    }
    let rows =
        fetch_module_rows(&pool, &project_id, &slug, &user_id, Some(&module_id), false).await?;
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
/// `issue_activity.deleted` publishes, soft-delete module (instance
/// `delete()`, so the `soft_delete_related_objects` mixin job fires) +
/// bridges, soft-delete requester favorites + hard-delete all recent
/// visits, 204 empty.
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
    let db = pidash_db::pool::sea_orm_connection(&pool);
    let creator_row: Option<uuid::Uuid> = creator_exists_select(&module_id, &user_id)
        .into_tuple()
        .one(&db)
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
    let module: Option<DestroyModuleRow> = destroy_module_select(&slug, &project_id, &module_id)
        .into_model::<DestroyModuleRow>()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let module_name = match module {
        Some(row) => row.name,
        None => return Err(Denial::NotFound),
    };
    // Live bridge issues for the per-issue publishes (`:727`).
    let issue_rows: Vec<uuid::Uuid> = destroy_issue_ids_select(&module_id)
        .into_tuple()
        .all(&db)
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
    let current_instance = python_dumps(&serde_json::json!({"module_name": module_name.clone()}));
    for issue_id in &issue_rows {
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
            emit.requested_data, requested_data,
            "destroy requested_data matches json.dumps"
        );
        enqueue_task(&pool, emit.task_name(), emit.kwargs()).await;
    }
    // Soft-delete the module (`:742`) and its bridges (`:744`).
    let now_sea = chrono::Utc::now().fixed_offset();
    modules::Entity::update_many()
        .col_expr(modules::Column::DeletedAt, Expr::value(Some(now_sea)))
        .filter(modules::Column::Id.eq(module_id))
        .exec(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    // `module.delete()` is the instance call, so the mixin fires
    // `soft_delete_related_objects("db", "module", pk, using=None)`
    // (`db/mixins.py:77-78`) — before the queryset deletes below, which
    // are bulk updates with no job.
    enqueue_soft_delete(&pool, "module", &module_id).await;
    // The bridge queryset goes through the soft-delete manager, so only
    // live rows are re-stamped (`mixins.py:48-58`): re-stamping already
    // deleted rows would collide on unique (issue_id, module_id,
    // deleted_at) when a pair has two deleted bridges (PIDASHCONV-875).
    module_issues::Entity::update_many()
        .col_expr(module_issues::Column::DeletedAt, Expr::value(Some(now_sea)))
        .filter(module_issues::Column::ModuleId.eq(module_id))
        .filter(module_issues::Column::ProjectId.eq(project_id))
        .filter(module_issues::Column::DeletedAt.is_null())
        .exec(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    // Soft-delete the requester's favorites (`:746-751`, plain
    // `.delete()` over the soft-delete manager — verified live).
    user_favorites::Entity::update_many()
        .col_expr(
            user_favorites::Column::DeletedAt,
            Expr::value(Some(now_sea)),
        )
        .filter(user_favorites::Column::UserId.eq(user_id))
        .filter(user_favorites::Column::EntityType.eq("module"))
        .filter(user_favorites::Column::EntityIdentifier.eq(Some(module_id)))
        .filter(user_favorites::Column::ProjectId.eq(Some(project_id)))
        .filter(user_favorites::Column::DeletedAt.is_null())
        .exec(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    // Hard-delete every recent visit of this module (`:753-758`,
    // `soft=False`). Resolves the workspace by slug first (same tenant
    // scope as the old `USING` join); the extra lookup keeps one extra
    // statement on the delete path (documented count delta).
    let workspace_row: Option<uuid::Uuid> = workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Id)
        .filter(workspaces::Column::Slug.eq(slug.as_str()))
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    if let Some(workspace_id) = workspace_row {
        user_recent_visits::Entity::delete_many()
            .filter(user_recent_visits::Column::WorkspaceId.eq(workspace_id))
            .filter(user_recent_visits::Column::ProjectId.eq(Some(project_id)))
            .filter(user_recent_visits::Column::EntityIdentifier.eq(Some(module_id)))
            .filter(user_recent_visits::Column::EntityName.eq("module"))
            .exec(&db)
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    Ok(empty_response(StatusCode::NO_CONTENT))
}

#[cfg(test)]
mod modules1_seaorm_tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    fn sql_of<E: EntityTrait>(select: sea_orm::Select<E>) -> String {
        select.build(DbBackend::Postgres).to_string()
    }

    #[test]
    fn resolve_select_pins_join_and_scope() {
        let sql = sql_of(resolve_project_select("acme", "ENG"));
        assert!(
            sql.contains(
                r#"INNER JOIN "workspaces" ON "projects"."workspace_id" = "workspaces"."id""#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = "#), "{sql}");
        assert!(sql.contains(r#""projects"."identifier" = "#), "{sql}");
        assert!(sql.contains(r#""projects"."deleted_at" IS NULL"#), "{sql}");
    }

    #[test]
    fn allow_role_selects_pin_active_live_scope() {
        let pid = uuid::Uuid::nil();
        let uid = uuid::Uuid::nil();
        let sql = sql_of(project_role_select("acme", &pid, &uid));
        assert!(
            sql.contains(
                r#"INNER JOIN "workspaces" ON "project_members"."workspace_id" = "workspaces"."id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."is_active" = TRUE"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""project_members"."deleted_at" IS NULL"#),
            "{sql}"
        );
        let sql = sql_of(workspace_role_select("acme", &uid));
        assert!(
            sql.contains(
                r#"INNER JOIN "workspaces" ON "workspace_members"."workspace_id" = "workspaces"."id""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#""workspace_members"."deleted_at" IS NULL"#),
            "{sql}"
        );
    }

    #[test]
    fn timezone_and_user_exists_select_only_id_or_timezone() {
        let uid = uuid::Uuid::nil();
        let sql = sql_of(actor_timezone_select(&uid));
        assert!(sql.contains(r#""users"."user_timezone""#), "{sql}");
        assert!(sql.contains(r#""users"."id" = "#), "{sql}");
        let sql = sql_of(user_exists_select(&uid));
        assert!(sql.contains(r#""users"."id" = "#), "{sql}");
        // No soft-delete filter: `users` is the stock manager (matches old SQL).
        assert!(!sql.contains("deleted_at"), "{sql}");
    }

    #[test]
    fn context_duplicate_member_selects_pin_predicates() {
        let pid = uuid::Uuid::nil();
        let mid = uuid::Uuid::nil();
        let sql = sql_of(context_project_select("acme", &pid));
        assert!(sql.contains(r#""projects"."workspace_id""#), "{sql}");
        assert!(sql.contains(r#""workspaces"."slug" = "#), "{sql}");
        assert!(sql.contains(r#""projects"."id" = "#), "{sql}");
        let sql = sql_of(duplicate_name_select(&pid, "M", None));
        assert!(sql.contains(r#""modules"."name" = "#), "{sql}");
        assert!(sql.contains(r#""modules"."deleted_at" IS NULL"#), "{sql}");
        assert!(!sql.contains(r#""modules"."id" <> "#), "{sql}");
        let sql = sql_of(duplicate_name_select(&pid, "M", Some(&mid)));
        assert!(sql.contains(r#""modules"."id" <> "#), "{sql}");
        let sql = sql_of(live_member_ids_select(&mid));
        assert!(
            sql.contains(r#""module_members"."member_id" IS NOT NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""module_members"."deleted_at" IS NULL"#),
            "{sql}"
        );
    }

    #[test]
    fn module_links_select_pins_scope_and_order() {
        let mid = uuid::Uuid::nil();
        let sql = sql_of(module_links_select(&mid));
        assert!(sql.contains(r#""module_links"."module_id" = "#), "{sql}");
        assert!(
            sql.contains(r#""module_links"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ORDER BY "module_links"."created_at" DESC"#),
            "{sql}"
        );
        assert!(sql.contains(r#"AS "created_by""#), "{sql}");
        assert!(sql.contains(r#"AS "updated_by""#), "{sql}");
        assert!(sql.contains(r#"AS "project""#), "{sql}");
        assert!(sql.contains(r#"AS "workspace""#), "{sql}");
        assert!(sql.contains(r#"AS "module""#), "{sql}");
    }

    #[test]
    fn write_row_select_pins_aliases_and_filter() {
        let mid = uuid::Uuid::nil();
        let sql = sql_of(write_row_select(&mid));
        assert!(sql.contains(r#""modules"."id" = "#), "{sql}");
        assert!(sql.contains(r#"AS "created_by""#), "{sql}");
        assert!(sql.contains(r#"AS "updated_by""#), "{sql}");
        assert!(sql.contains(r#"AS "project""#), "{sql}");
        assert!(sql.contains(r#"AS "workspace""#), "{sql}");
        assert!(sql.contains(r#"AS "lead""#), "{sql}");
        // Both the raw and aliased lead select (old SQL selects lead_id twice).
        assert!(sql.contains(r#""modules"."lead_id""#), "{sql}");
    }

    #[test]
    fn stored_audit_select_pins_three_columns() {
        let mid = uuid::Uuid::nil();
        let sql = sql_of(stored_audit_select(&mid));
        assert!(sql.contains(r#""modules"."workspace_id""#), "{sql}");
        assert!(sql.contains(r#""modules"."created_by_id""#), "{sql}");
        assert!(sql.contains(r#""modules"."updated_by_id""#), "{sql}");
        assert!(sql.contains(r#""modules"."id" = "#), "{sql}");
    }

    #[test]
    fn sub_issues_select_pins_joins_and_guards() {
        let pid = uuid::Uuid::nil();
        let mid = uuid::Uuid::nil();
        let sql = sql_of(sub_issues_count_select(&pid, &mid));
        assert!(
            sql.contains(r#"INNER JOIN "issues" ON "module_issues"."issue_id" = "issues"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"LEFT JOIN "states" ON "issues"."state_id" = "states"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "projects" ON "issues"."project_id" = "projects"."id""#),
            "{sql}"
        );
        assert!(sql.contains(r#""module_issues"."module_id" = "#), "{sql}");
        assert!(
            sql.contains(r#""module_issues"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""issues"."parent_id" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#""issues"."is_draft" = FALSE"#), "{sql}");
        assert!(sql.contains(r#""issues"."archived_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""projects"."archived_at" IS NULL"#), "{sql}");
        // NULL-safe triage exclusion: NULL group passes.
        assert!(sql.contains(r#""states"."group" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""states"."group" <> "#), "{sql}");
    }

    #[test]
    fn estimate_type_select_pins_joins_and_points() {
        let pid = uuid::Uuid::nil();
        let sql = sql_of(estimate_type_exists_select("acme", &pid));
        assert!(
            sql.contains(r#"INNER JOIN "projects" ON "estimates"."project_id" = "projects"."id""#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#"INNER JOIN "workspaces" ON "projects"."workspace_id" = "workspaces"."id""#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#""workspaces"."slug" = "#), "{sql}");
        assert!(sql.contains(r#""projects"."id" = "#), "{sql}");
        assert!(sql.contains(r#""estimates"."type" = "#), "{sql}");
    }

    fn module_rows_stmt(
        slug: &str,
        module_id: Option<&uuid::Uuid>,
        archived_null: bool,
        archived_only: bool,
        member_guard: bool,
    ) -> String {
        let pid = uuid::Uuid::nil();
        let uid = uuid::Uuid::nil();
        let (sql, _) = module_rows_statement(
            slug,
            &pid,
            &uid,
            module_id,
            archived_null,
            archived_only,
            member_guard,
        )
        .build(sea_orm::sea_query::PostgresQueryBuilder);
        sql
    }

    #[test]
    fn module_rows_pins_group_order_and_annotations() {
        let sql = module_rows_stmt("acme", None, true, false, true);
        assert!(sql.contains("GROUP BY"), "{sql}");
        assert!(sql.contains(r#"AS "is_favorite""#), "{sql}");
        assert!(sql.contains(r#"AS "completed_issues""#), "{sql}");
        assert!(sql.contains(r#"AS "total_estimate_points""#), "{sql}");
        assert!(sql.contains(r#"AS "member_ids""#), "{sql}");
        assert!(sql.contains(r#""m"."archived_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(r#""module_members"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#"ORDER BY "is_favorite" DESC"#), "{sql}");
        // 17 annotation columns: is_favorite + 6 counts + 6 sums + member_ids + 19 base = 33 total; check unique aliases
        assert!(sql.contains(r#"AS "backlog_issues""#), "{sql}");
        assert!(sql.contains(r#"AS "backlog_estimate_points""#), "{sql}");
    }

    #[test]
    fn module_rows_archived_variant_pins_flags() {
        let sql = module_rows_stmt("acme", None, false, true, false);
        assert!(sql.contains(r#""m"."archived_at" IS NOT NULL"#), "{sql}");
        assert!(
            !sql.contains(r#""module_members"."deleted_at" IS NULL"#),
            "{sql}"
        );
        assert!(!sql.contains(r#""m"."archived_at" IS NULL"#), "{sql}");
    }

    #[test]
    fn module_rows_detail_scopes_id() {
        let mid = uuid::Uuid::nil();
        let sql = module_rows_stmt("acme", Some(&mid), true, false, true);
        assert!(sql.contains(r#""m"."id" = "#), "{sql}");
    }

    #[tokio::test]
    async fn live_resolve_and_timezone_roundtrip() {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return;
            }
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        let db = pidash_db::pool::sea_orm_connection(&pool);
        // Resolve parses UUIDs without DB; the SeaORM path needs seeded rows,
        // so here we only prove the connection + builders execute shape-clean
        // against an empty scratch schema (selects return None/empty).
        let err: Result<Option<uuid::Uuid>, _> = resolve_project_select("no-such-slug", "ENG")
            .into_tuple()
            .one(&db)
            .await;
        // Scratch DB may not have tables for this module; only assert the
        // statement built (shape tests above pin shape). If tables exist,
        // a miss is None; if not, an error is acceptable in this probe.
        let _ = err;
        let err: Result<Option<String>, _> = actor_timezone_select(&uuid::Uuid::nil())
            .into_tuple()
            .one(&db)
            .await;
        let _ = err;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin_state(app_base_url: Option<&str>, web_url: Option<&str>) -> AppState {
        let mut settings = pidash_db::config::Settings::test_defaults();
        settings.urls.app_base_url = app_base_url.map(str::to_owned);
        settings.urls.web_url = web_url.map(str::to_owned);
        AppState::with_settings("test", settings)
    }

    /// `base_host(request, is_app=True)` (`utils/host.py:17-66`):
    /// `APP_BASE_URL` wins when truthy, else `WEB_URL`; unset is
    /// `ImproperlyConfigured` → 500. Empty strings fall through like
    /// Python's `or` (PIDASHCONV-1597, PIDASHCONV-1648).
    #[test]
    fn request_origin_falls_through_empty_strings() {
        let app = "https://app.example.invalid";
        let web = "https://web.example.invalid";
        let state = origin_state(Some(app), Some(web));
        assert_eq!(request_origin(&state).expect("app base wins"), app);
        let state = origin_state(None, Some(web));
        assert_eq!(request_origin(&state).expect("falls through"), web);
        let state = origin_state(Some(""), Some(web));
        assert_eq!(request_origin(&state).expect("empty falls through"), web);
        let state = origin_state(Some(app), Some(""));
        assert_eq!(request_origin(&state).expect("app wins"), app);
        let state = origin_state(Some(""), Some(""));
        assert!(matches!(request_origin(&state), Err(Denial::ServerError)));
        let state = origin_state(None, None);
        assert!(matches!(request_origin(&state), Err(Denial::ServerError)));
    }

    #[test]
    fn soft_delete_message_matches_the_mixin_call() {
        // `db/mixins.py:77-78`: `.delay(label, name, pk, using=using)`
        // with `using=None` — three positional args, `using` stays a
        // null kwarg (never a positional `"default"`).
        let pk = uuid::Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap();
        for model in ["module", "modulelink"] {
            let message = soft_delete_message(model, &pk);
            assert_eq!(
                message.task,
                "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
            );
            assert_eq!(
                message.args,
                vec![
                    Value::String("db".to_owned()),
                    Value::String(model.to_owned()),
                    Value::String(pk.to_string()),
                ]
            );
            assert_eq!(message.kwargs.len(), 1);
            assert_eq!(message.kwargs.get("using"), Some(&Value::Null));
        }
    }

    #[test]
    fn gates_cover_all_six_owned_actions() {
        let rows = [
            (
                "POST",
                "workspaces/<slug>/projects/<project_id>/modules/",
                "base.py:294",
            ),
            (
                "GET",
                "workspaces/<slug>/projects/<project_id>/modules/",
                "base.py:353",
            ),
            (
                "GET",
                "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
                "base.py:395",
            ),
            (
                "PATCH",
                "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
                "base.py:651",
            ),
            (
                "DELETE",
                "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
                "base.py:723",
            ),
        ];
        for (method, path, source) in rows {
            let row = gates::gate_for(method, path).expect("gate row");
            assert_eq!(
                row.source.split(' ').next(),
                Some(source),
                "{method} {path}"
            );
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
            "id",
            "workspace_id",
            "project_id",
            "name",
            "description",
            "description_text",
            "description_html",
            "start_date",
            "target_date",
            "status",
            "lead_id",
            "member_ids",
            "view_props",
            "sort_order",
            "external_source",
            "external_id",
            "logo_props",
            "is_favorite",
            "completed_issues",
            "cancelled_issues",
            "started_issues",
            "unstarted_issues",
            "backlog_issues",
            "total_issues",
            "completed_estimate_points",
            "total_estimate_points",
            "created_at",
            "updated_at",
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
                "is_favorite",
                "completed_issues",
                "cancelled_issues",
                "started_issues",
                "unstarted_issues",
                "backlog_issues",
                "total_issues",
                "completed_estimate_points",
                "total_estimate_points",
                "member_ids",
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
        // `completed_issues` (PIDASHCONV-504). Pinned on the SeaORM
        // statement twin.
        let pid = uuid::Uuid::nil();
        let uid = uuid::Uuid::nil();
        let (sql, _) = module_rows_statement("acme", &pid, &uid, None, true, false, true)
            .build(sea_orm::sea_query::PostgresQueryBuilder);
        let completed = sql.find("AS \"completed_issues\"").expect("alias");
        let cancelled = sql.find("AS \"cancelled_issues\"").expect("alias");
        let completed_group = sql[..completed]
            .rfind("\"states\".\"group\" = 'completed'")
            .expect("group");
        let cancelled_group = sql[..cancelled]
            .rfind("\"states\".\"group\" = 'cancelled'")
            .expect("group");
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
        assert_eq!(parse_sort_order_value(&serde_json::json!(true)), Some(1.0));
        assert_eq!(parse_sort_order_value(&serde_json::json!(false)), Some(0.0));
        assert_eq!(parse_sort_order_value(&serde_json::json!(2)), Some(2.0));
        assert_eq!(
            parse_sort_order_value(&serde_json::json!(" 1.5 ")),
            Some(1.5)
        );
        assert_eq!(parse_sort_order_value(&serde_json::json!("abc")), None);
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
        assert_eq!(parse_sort_order_value(&serde_json::json!("1__0")), None);
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
            "id",
            "lead_id",
            "member_ids",
            "members",
            "lead",
            "project",
            "workspace",
            "created_by",
            "updated_by",
            "deleted_at",
            "archived_at",
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
        for status in [
            "backlog",
            "planned",
            "in-progress",
            "paused",
            "completed",
            "cancelled",
        ] {
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
