#![forbid(unsafe_code)]

//! Module-issue handlers (D-28, stage 5, PIDASHCONV-397).
//!
//! Ports `ModuleIssueViewSet` from
//! `apps/api/pi_dash/app/views/module/issue.py:45-337`:
//!
//! - `list` (`:94-207`): the issue-list pipeline scoped to one module —
//!   legacy `issue_filters(params, 'GET')` (`:97`),
//!   `ComplexFilterBackend` + `IssueFilterSet` (`:50-51, :101`), the
//!   `apply_annotations` columns (`:53-82`), `order_issue_queryset`
//!   (`:112-117`), the grouped / sub-grouped / plain paginators
//!   (`:126-207`) with `issue_on_results` shaping.
//! - `create_module_issues` (`:209-246`): `issues` required (`:212-214`),
//!   `bulk_create(ignore_conflicts)` (`:216-230`, batches of 10),
//!   one `issue_activity` publish per input issue (`:232-245`),
//!   201 `{"message": "success"}` (`:246`).
//! - `create_issue_modules` (`:248-315`): `modules` adds (`:255-285`) +
//!   `removed_modules` removals with per-removal `activity.deleted`
//!   publish then delete (`:287-313`), 201 `{"message": "success"}`
//!   (`:315`).
//! - `destroy` (`:317-337`): the `activity.deleted` publish fires before
//!   the delete and unconditionally (`:325-335`); the bridge
//!   `filter().delete()` is a soft delete (`:336`); 204 (`:337`).
//!
//! Fixture ids: FX-MOD-06
//! (`rust-api/fixtures/app_modules/handlers/module_issues_links.golden.json`);
//! FX-MOD-05 enqueues (`tasks/enqueue_payloads.golden.json`) via
//! `super::handlers_modules::enqueue_task`; gates via `super::gates`
//! (FX-MOD-04, PIDASHCONV-379).
//!
//! Layering: the list pipeline composes the shared kernels —
//! `pidash_db::{filter, filterset, issue_filters}`, the D-26
//! `crate::app_issues` list helpers, `pidash_services::app_issues`
//! ordering/params/shape, and `crate::paginator` — exactly like the
//! cycle-issue port (`app_cycles::handlers_cycle_issues`, PIDASHCONV-323),
//! whose `apply_annotations` is line-identical to this view's (`:53-82`)
//! once the `SoftDeletionManager` (`db/mixins.py:56-58`, automatic
//! `deleted_at IS NULL` on the `IssueLink` / `FileAsset` `.objects`
//! managers) is accounted for. Module-specific SQL (bridge scope, the
//! create/remove/destroy writes) lives here; the D-26 helpers are
//! referenced, never re-ported.
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * `retrieve` / `update` / `partial_update` on the detail route fall
//!   through to the undecorated `ModelViewSet` defaults, which die with a
//!   500 for every caller (the route names its kwargs
//!   `module_id`/`issue_id` but the defaults look for `pk`; contract
//!   `test_detail_get_put_patch_are_server_errors`). Those methods proxy
//!   to Django, which reproduces the 500 itself (cycle-issue precedent).
//! * `destroy` of a missing bridge answers 500: `None.module`
//!   (`AttributeError`) at `:331` (no `None` guard there — unlike the
//!   removal path at `:304`, which falls back to a null module name and
//!   answers 201).
//! * Detaching from a soft-deleted module still publishes its name: the
//!   forward-FK fetch sees soft-deleted rows (verified live).
//! * UUID prep per item (`UUIDField.to_python` + the FK save-prep
//!   `""→None` rule): `create_module_issues` `str()`-coerces items, so a
//!   non-UUID spelling is `ValidationError` → 400 detail and `""` inserts
//!   `NULL` → 400 payload (unlike the cycle create, whose `__in` filter
//!   renders 400 detail for every bad spelling). `create_issue_modules`
//!   passes items raw: `null`/ints/`""` die at the `NOT NULL` column or
//!   the FK → 400 payload, while bad strings/floats/containers/out-of-range
//!   ints are `ValidationError` → 400 detail; on the removal (lookup) path
//!   there is no `""→None` rule, so `null`/in-range ints simply match no
//!   bridge → 201 with a null name.
//! * `create_issue_modules` serializes the *raw* `modules` item into
//!   `requested_data` (`json.dumps({"module_id": module})` at `:275` —
//!   non-string items render unstringified), while the removal payload
//!   uses `str(module_id)` (`:296`) and the destroy payload `str()` of the
//!   URL kwarg (canonical).
//! * Non-UUID path segments: Django's `<uuid:>` converter rejects them at
//!   URL resolve (HTML 404, unreachable through the JSON edge); here they
//!   answer the `ValidationError` 400, the same body the kernels render
//!   for badly-formed UUIDs inside filters (cycle-issue precedent).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::Router;
use serde_json::{Map, Value};
use sqlx::{Postgres, Row};

use pidash_services::app_modules::tasks;

use super::gates;
use crate::app_issues::{
    complex_filter, fetch_count, fetch_json_rows, group_values, legacy_sql, multi_map, order_key,
    page_denial, query_last, render_condition, shape_row, Binder, Denial, FilteredSet, QueryMap,
};
use crate::state::AppState;
use pidash_auth::permissions::allow::AllowFacts;

/// `GET` + `POST` on the module-issues collection path, `POST` on the
/// issue-modules path, `DELETE` on the detail path. Every other method
/// falls through to Django: the detail retrieve/update/partial_update are
/// undecorated `ModelViewSet` defaults (500 upstream), which the proxy
/// reproduces byte-identically.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/",
            axum::routing::get(module_issue_list)
                .post(create_module_issues)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/modules/",
            axum::routing::post(create_issue_modules)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/{issue_id}/",
            axum::routing::delete(module_issue_destroy)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .post(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

/// Collection path in `app/urls/module.py:41-45` form.
pub const MODULE_ISSUES_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/";
/// Reverse path in `app/urls/module.py:36-40` form.
pub const ISSUE_MODULES_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/modules/";
/// Detail path in `app/urls/module.py:46-57` form.
pub const MODULE_ISSUE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/{issue_id}/";

/// 400 `{"error": "Issues are required"}` (`issue.py:213-214`).
pub const ISSUES_REQUIRED_BODY: &str = r#"{"error":"Issues are required"}"#;
/// 201 success envelope (`issue.py:246,315`).
pub const MODULE_ISSUE_CREATE_BODY: &str = r#"{"message":"success"}"#;
/// Group-clash message (`issue.py:129-133`).
pub const GROUP_CLASH_MESSAGE: &str = "Group by and sub group by cannot have same parameters";

// ---------------------------------------------------------------------------
// Shared request plumbing (cycle-issue / handlers_modules precedent)
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

#[cfg(test)]
fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// `request.user` from the Django session (`app_issues` actor rule):
/// missing session, missing key, or a non-UUID id is anonymous → 401.
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
/// callers only (anonymous already 401'd); UUIDs pass through unchecked;
/// other identifiers resolve `UPPER(identifier)` in the workspace, else
/// `Http404` (detail 404).
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
    row.map(|row| row.0).ok_or(Denial::ProjectNotFound)
}

/// Badly-formed UUIDs in paths render the `ValidationError` branch
/// (`app/views/base.py:126-130`): 400 `{"error": "Please provide valid
/// detail"}`.
pub(crate) const INVALID_DETAIL_MSG: &str = "Please provide valid detail";

pub(crate) fn parse_uuid_or_invalid(raw: &str) -> Result<uuid::Uuid, Denial> {
    raw.parse::<uuid::Uuid>()
        .map_err(|_| Denial::BadError(INVALID_DETAIL_MSG.to_owned()))
}

/// Membership facts for one `(user, slug, project)` over the same rows the
/// decorator reads (`app/permissions/base.py:19-86`): active project and
/// workspace memberships scoped by slug/project, soft-deleted rows excluded.
/// The module-issue gates are all `ADMIN, MEMBER`.
pub(crate) async fn fetch_allow_facts(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<AllowFacts, Denial> {
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
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
    let allowed = [ROLE_ADMIN, ROLE_MEMBER];
    Ok(AllowFacts {
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

/// Run one `super::gates` row: anonymous already 401'd; `Allow` runs the
/// body, `Deny` answers the decorator 403.
pub(crate) fn check_gate(gate: &gates::Gate, slug: &str, facts: &AllowFacts) -> Result<(), Denial> {
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

/// `origin=base_host(request, is_app=True)`: `WEB_URL or APP_BASE_URL`
/// (`handlers_modules::request_origin`, same settings seam).
fn request_origin(state: &AppState) -> Result<String, Denial> {
    state
        .settings()
        .urls
        .app_base_url
        .clone()
        .ok_or(Denial::ServerError)
}

/// Common preamble for the module-issue actions: session auth, project
/// rewrite, the route's ADMIN/MEMBER gate. Returns `(pool, user_id,
/// project_id)`.
async fn module_issue_context(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    gate: &gates::Gate,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<(sqlx::PgPool, uuid::Uuid, uuid::Uuid), Denial> {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    Ok((pool, user_id, project_id))
}

fn gate_for_list() -> &'static gates::Gate {
    &gates::gate_for(
        "GET",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/",
    )
    .expect("module-issues list gate")
    .gate
}

fn gate_for_create_module_issues() -> &'static gates::Gate {
    &gates::gate_for(
        "POST",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/",
    )
    .expect("module-issues create gate")
    .gate
}

fn gate_for_create_issue_modules() -> &'static gates::Gate {
    &gates::gate_for(
        "POST",
        "workspaces/<slug>/projects/<project_id>/issues/<issue_id>/modules/",
    )
    .expect("issue-modules create gate")
    .gate
}

fn gate_for_destroy() -> &'static gates::Gate {
    &gates::gate_for(
        "DELETE",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
    )
    .expect("module-issue destroy gate")
    .gate
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

/// The m2m group field to its array annotation.
const M2M_ARRAYS: &[(&str, &str)] = &[
    ("labels__id", "label_ids"),
    ("assignees__id", "assignee_ids"),
    ("issue_module__module_id", "module_ids"),
];

/// The array annotation an m2m group field swaps out, if any.
fn skip_array_for(group_by: &str) -> Option<&'static str> {
    M2M_ARRAYS
        .iter()
        .find(|(key, _)| *key == group_by)
        .map(|(_, array)| *array)
}

/// Row key order on the wire, pinned live across flat/grouped/sub-grouped
/// responses: the 18 concrete columns in `required_fields` order, then
/// `state__group`, then the m2m group/sub keys (group first), then the
/// four scalar annotations in annotation order, then the surviving arrays
/// in grouper order with the swapped ones re-appended by the grouping
/// kernels (group first, then sub — the kernels `insert` them, which lands
/// at the end of the insertion-ordered map, exactly like Django's
/// processors assigning new dict keys).
///
/// So this vec carries the swapped arrays REMOVED on grouped paths (the
/// kernels re-add them); the flat path keeps all three.
fn wire_row_fields(group_by: Option<&str>, sub_group_by: Option<&str>) -> Vec<String> {
    let mut fields = vec![
        "id",
        "name",
        "state_id",
        "sort_order",
        "completed_at",
        "estimate_point",
        "priority",
        "start_date",
        "target_date",
        "sequence_id",
        "project_id",
        "parent_id",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
        "is_draft",
        "archived_at",
        "state__group",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    for group in [group_by, sub_group_by].into_iter().flatten() {
        if M2M_ARRAYS.iter().any(|(key, _)| *key == group) {
            fields.push(group.to_owned());
        }
    }
    fields.extend(
        [
            "cycle_id",
            "link_count",
            "attachment_count",
            "sub_issues_count",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    let swapped: Vec<&str> = [group_by, sub_group_by]
        .into_iter()
        .flatten()
        .filter_map(skip_array_for)
        .collect();
    for array in ["assignee_ids", "label_ids", "module_ids"] {
        if !swapped.contains(&array) {
            fields.push(array.to_owned());
        }
    }
    fields
}

/// Scalar selects for the list rows: `apply_annotations` (`issue.py:53-82`)
/// as re-annotated by `issue_queryset_grouper` (`utils/grouper.py:30-92`,
/// which runs after and owns the three array columns).
///
/// - The count subqueries are bare `COUNT`s with no grouping: they always
///   return one row, so an empty set renders `0`, never `null` (no
///   `NULLIF`). The `IssueLink` / `FileAsset` `.objects` managers are
///   `SoftDeletionManager`s (`db/mixins.py:56-58`), so their implicit
///   `deleted_at IS NULL` is spelled out here.
/// - The array columns are the grouper's `Coalesce(ArrayAgg…)` subqueries:
///   assignees/labels guarded by bridge `deleted_at IS NULL` only;
///   modules guarded by bridge `deleted_at IS NULL` plus
///   `module__archived_at IS NULL` — with NO module `deleted_at` guard, so
///   soft-deleted (but unarchived) modules still list (pinned live).
///
/// `skip_arrays` drops the swapped arrays for the m2m group/sub paths
/// (the grouper skips both; the grouping kernels re-add them at the end).
fn module_row_selects(skip_arrays: &[&str]) -> String {
    let mut selects = String::from(
        r#"issue.id, issue.name, issue.state_id, issue.sort_order, issue.completed_at,
        issue.estimate_point_id AS estimate_point, issue.priority, issue.start_date,
        issue.target_date, issue.sequence_id, issue.project_id, issue.parent_id,
        (SELECT ci.cycle_id FROM cycle_issues ci
          WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1) AS cycle_id,
        (SELECT COUNT(*) FROM issue_links il
          WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS link_count,
        (SELECT COUNT(*) FROM file_assets fa
          WHERE fa.issue_id = issue.id AND fa.entity_type = 'ISSUE_ATTACHMENT'
            AND fa.deleted_at IS NULL) AS attachment_count,
        (SELECT COUNT(*) FROM issues c
           LEFT JOIN states cs ON cs.id = c.state_id AND cs.deleted_at IS NULL
           JOIN projects cp ON cp.id = c.project_id
          WHERE c.parent_id = issue.id AND c.deleted_at IS NULL
            AND (cs."group" IS NULL OR NOT (cs."group" = 'triage'))
            AND c.archived_at IS NULL AND cp.archived_at IS NULL AND c.is_draft = FALSE
        ) AS sub_issues_count,
        issue.created_at, issue.updated_at, issue.created_by_id AS created_by,
        issue.updated_by_id AS updated_by, issue.is_draft, issue.archived_at,
        state."group" AS "state__group""#,
    );
    if !skip_arrays.contains(&"label_ids") {
        selects.push_str(
            r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT il.label_id), '{}'::uuid[])
           FROM issue_labels il
          WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS label_ids"#,
        );
    }
    if !skip_arrays.contains(&"assignee_ids") {
        selects.push_str(
            r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id), '{}'::uuid[])
           FROM issue_assignees ia
          WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL) AS assignee_ids"#,
        );
    }
    if !skip_arrays.contains(&"module_ids") {
        selects.push_str(
            r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT mi.module_id), '{}'::uuid[])
           FROM module_issues mi JOIN modules m ON m.id = mi.module_id
          WHERE mi.issue_id = issue.id AND mi.deleted_at IS NULL
            AND m.archived_at IS NULL) AS module_ids"#,
        );
    }
    selects
}

/// Relation joins the module list can use: same alias contract as the D-26
/// list (`issue_assignee`, `issue_cycle`, `issue_module`, `issue_mention`,
/// `label_issue`, `issue_subscribers`, `issue_intake`).
const RELATION_JOINS: &[(&str, &str)] = &[
    ("issue_assignees", "issue_assignee"),
    ("cycle_issues", "issue_cycle"),
    ("module_issues", "issue_module"),
    ("issue_mentions", "issue_mention"),
    ("issue_labels", "label_issue"),
    ("issue_subscribers", "issue_subscribers"),
];

/// True when every `"alias".` reference in the filter SQL is an `IS NULL` /
/// `IS NOT NULL` test (the `__isnull` predicates take the LEFT join).
fn alias_nullable_only(where_sql: &str, alias: &str) -> bool {
    let marker = format!("\"{alias}\".");
    let mut rest = where_sql;
    while let Some(start) = rest.find(&marker) {
        let after = &rest[start + marker.len()..];
        let ident_len = after
            .chars()
            .take_while(|char| char.is_alphanumeric() || *char == '_' || *char == '"')
            .map(|char| char.len_utf8())
            .sum::<usize>();
        let tail = after[ident_len..].trim_start();
        if !(tail.starts_with("IS NULL") || tail.starts_with("IS NOT NULL")) {
            return false;
        }
        rest = &after[ident_len..];
    }
    true
}

/// M2M group filters force an INNER join on that relation with the deleted
/// guard (`GROUP_FILTER_MAPPER` in `utils/grouper.py:41-49`).
fn group_join_alias(group_by: Option<&str>, sub_group_by: Option<&str>, alias: &str) -> bool {
    let raw = match alias {
        "issue_assignee" => "assignees__id",
        "label_issue" => "labels__id",
        "issue_module" => "issue_module__module_id",
        _ => return false,
    };
    group_by == Some(raw) || sub_group_by == Some(raw)
}

/// The SQL expression a group field partitions by (D-26 `group_expression`
/// over the shared `issue` / `state` aliases).
fn group_expression(field: &str) -> Result<String, Denial> {
    Ok(match field {
        "labels__id" => "label_issue.label_id".to_owned(),
        "assignees__id" => "issue_assignee.assignee_id".to_owned(),
        "issue_module__module_id" => "issue_module.module_id".to_owned(),
        "state_id" => "issue.state_id".to_owned(),
        "priority" => "issue.priority".to_owned(),
        "cycle_id" => "(SELECT ci.cycle_id FROM cycle_issues ci WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1)".to_owned(),
        "project_id" => "issue.project_id".to_owned(),
        "state__group" => "state.\"group\"".to_owned(),
        "target_date" => "issue.target_date".to_owned(),
        "start_date" => "issue.start_date".to_owned(),
        "created_by" => "issue.created_by_id".to_owned(),
        _ => return Err(Denial::ServerError),
    })
}

/// Extra selects carrying the raw m2m group/sub keys per joined row (the
/// window partitions by them, and the rows must carry both).
fn group_member_selects(group_by: Option<&str>, sub_group_by: Option<&str>) -> String {
    let mut out = String::new();
    for group in [group_by, sub_group_by].into_iter().flatten() {
        out.push_str(match group {
            "labels__id" => ", label_issue.label_id AS \"labels__id\"",
            "assignees__id" => ", issue_assignee.assignee_id AS \"assignees__id\"",
            "issue_module__module_id" => ", issue_module.module_id AS \"issue_module__module_id\"",
            _ => "",
        });
    }
    out
}

/// Bind a `sea_query::Value` vec to raw SQL (same mapping as the D-26
/// `bind_all`, over this endpoint's placeholders).
fn bind_values<'a>(
    sql: &'a str,
    values: Vec<sea_query::Value>,
) -> Result<sqlx::query::Query<'a, Postgres, sqlx::postgres::PgArguments>, Denial> {
    use sea_query::Value as SeaValue;
    let mut query = sqlx::query(sql);
    for value in &values {
        query = match value {
            SeaValue::Bool(value) => query.bind(*value),
            SeaValue::TinyInt(value) => query.bind(*value),
            SeaValue::SmallInt(value) => query.bind(*value),
            SeaValue::Int(value) => query.bind(*value),
            SeaValue::BigInt(value) => query.bind(*value),
            SeaValue::TinyUnsigned(value) => query.bind(value.map(i16::from)),
            SeaValue::SmallUnsigned(value) => query.bind(value.map(i32::from)),
            SeaValue::Unsigned(value) => query.bind(value.map(|v| v as i64)),
            SeaValue::BigUnsigned(value) => query.bind(value.map(|v| v as i64)),
            SeaValue::Float(value) => query.bind(*value),
            SeaValue::Double(value) => query.bind(*value),
            SeaValue::String(value) => query.bind(value.clone().map(|text| *text)),
            SeaValue::Char(value) => query.bind(value.map(|char| char.to_string())),
            SeaValue::Bytes(value) => query.bind(value.clone().map(|bytes| (*bytes).clone())),
            SeaValue::Uuid(value) => query.bind(value.clone().map(|id| *id)),
        };
    }
    Ok(query)
}

/// Filtered `FROM … WHERE` for the module-issue list (`issue.py:97-107`):
/// the `Issue.issue_objects` manager (`deleted_at`, triage-state,
/// archived, project-archived, draft exclusions) + the module bridge scope
/// (`issue_module__module_id`, bridge live, `get_queryset` `:84-92`) +
/// tenant (workspace slug, project, active membership) + the
/// `ComplexFilterBackend` tree + the legacy `issue_filters(params, 'GET')`
/// predicates.
fn module_filtered_set(
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
) -> Result<FilteredSet, Denial> {
    let mut binder = Binder::new();
    let slug_holder = binder.bind_string(slug.to_owned());
    let project_holder = binder.bind_uuid(*project_id);
    let user_holder = binder.bind_uuid(*user_id);
    let module_holder = binder.bind_uuid(*module_id);
    let mut fragments = vec![format!(
        r#"workspaces.slug = {slug_holder} AND issue.project_id = {project_holder}
        AND issue.deleted_at IS NULL
        AND (state."group" IS NULL OR NOT (state."group" = 'triage'))
        AND issue.archived_at IS NULL AND project.archived_at IS NULL
        AND issue.is_draft = FALSE
        AND bridge.module_id = {module_holder} AND bridge.deleted_at IS NULL
        AND EXISTS (SELECT 1 FROM project_members pm
                     WHERE pm.member_id = {user_holder} AND pm.project_id = {project_holder}
                       AND pm.is_active AND pm.deleted_at IS NULL)"#
    )];
    if let Some(cond) = complex_filter(query)? {
        let (fragment, values) = render_condition(&cond);
        fragments.push(binder.splice(&fragment, values));
    }
    let flat: HashMap<String, String> = query
        .keys()
        .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let legacy = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
        .map_err(|_| Denial::ServerError)?;
    let mut legacy_sql_text = String::new();
    for (name, value) in legacy.predicates() {
        let fragment = legacy_sql(&mut binder, name, value)?;
        if legacy_sql_text.is_empty() {
            legacy_sql_text = fragment;
        } else {
            legacy_sql_text = format!("{legacy_sql_text} AND {fragment}");
        }
    }
    if !legacy_sql_text.is_empty() {
        fragments.push(legacy_sql_text);
    }
    // M2M group filters scope their relation to live bridges
    // (`GROUP_FILTER_MAPPER` in `utils/grouper.py:41-49`, applied by
    // `issue_queryset_grouper` for the grouped paths).
    for group in [group_by, sub_group_by].into_iter().flatten() {
        let alias = match group {
            "assignees__id" => "issue_assignee",
            "labels__id" => "label_issue",
            "issue_module__module_id" => "issue_module",
            _ => continue,
        };
        fragments.push(format!("{alias}.deleted_at IS NULL"));
    }
    let where_sql = fragments.join(" AND ");
    // The scope bridge always joins (one live row per issue in scope: the
    // partial unique on `(issue_id, module_id)` where `deleted_at IS NULL`).
    // Every other relation joins ONLY when referenced — by a filter or by a
    // group key — exactly like Django's `filter()`/window partition, which
    // never join unreferenced relations. An unreferenced `LEFT JOIN` would
    // fan out (one row per bridge) and defeat the grouped `DISTINCT` (the
    // window `__rn` differs per fanned row), inflating the window.
    let mut joins = String::from(" JOIN module_issues AS bridge ON bridge.issue_id = issue.id");
    let mut referenced = vec!["bridge"];
    for (table, alias) in RELATION_JOINS {
        let marker = format!("\"{alias}\".");
        let mentioned = where_sql.contains(&marker);
        let nullable_only = mentioned && alias_nullable_only(&where_sql, alias);
        let used = mentioned || group_join_alias(group_by, sub_group_by, alias);
        if !used {
            continue;
        }
        referenced.push(*alias);
        let inner = used && !nullable_only || group_join_alias(group_by, sub_group_by, alias);
        let kind = if inner { "INNER JOIN" } else { "LEFT JOIN" };
        joins.push_str(&format!(
            " {kind} {table} AS {alias} ON {alias}.issue_id = issue.id"
        ));
    }
    // `issue_intake` always joins: the grouped totals carry the intake
    // `count_filter` over this same `from_where`, so the alias must resolve
    // there even when no filter names it (Django's totals `filter()` adds
    // the join on demand). Pure `__isnull` predicates take the `LEFT` join,
    // like every other relation. The `deleted_at` guard is the
    // `SoftDeletionManager` on `IntakeIssue.objects`; at most one live row
    // per issue exists in practice, so the join cannot fan out.
    let intake_used = where_sql.contains("\"issue_intake\".");
    let intake_kind = if !intake_used || alias_nullable_only(&where_sql, "issue_intake") {
        "LEFT JOIN"
    } else {
        "INNER JOIN"
    };
    joins.push_str(&format!(
        " {intake_kind} intake_issues AS issue_intake ON issue_intake.issue_id = issue.id AND issue_intake.deleted_at IS NULL"
    ));
    if intake_used {
        referenced.push("issue_intake");
    }
    let from_where = format!(
        r#"FROM issues AS issue
        JOIN projects AS project ON project.id = issue.project_id
        JOIN workspaces ON workspaces.id = issue.workspace_id
        LEFT JOIN states AS state ON state.id = issue.state_id AND state.deleted_at IS NULL
        {joins}
        WHERE {where_sql}"#
    );
    Ok(FilteredSet {
        from_where,
        values: binder.values(),
        referenced,
    })
}

/// `ModuleIssueViewSet.list` (`issue.py:94-207`).
async fn module_issue_list(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let (pool, user_id, project_id) =
        module_issue_context(&state, &slug, &project_raw, gate_for_list(), extension).await?;
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    // Group clash is checked in-view before `paginate` parses per_page
    // (`issue.py:126-133`): 400 `{"error": "Group by and sub group by
    // cannot have same parameters"}`.
    if let (Some(group), Some(sub)) = (
        query_last(&query, "group_by"),
        query_last(&query, "sub_group_by"),
    ) {
        if !group.is_empty() && group == sub {
            return Err(Denial::BadError(GROUP_CLASH_MESSAGE.to_owned()));
        }
    }
    let params = pidash_services::app_issues::params::ListParams::parse_with(
        &multi_map(&query),
        pidash_services::app_issues::params::ParseOptions {
            require_issues: false,
            strict_per_page: true,
        },
    )
    .map_err(|error| {
        if error.key == "detail" {
            Denial::BadDetail(error.message)
        } else {
            Denial::BadError(error.message)
        }
    })?;
    if let Some(mismatch) = params.group_mismatch() {
        return Err(Denial::BadError(mismatch.message));
    }
    let group_by = params.group_by.as_deref();
    let sub_group_by = params.sub_group_by.as_deref();
    let filtered = module_filtered_set(
        &slug,
        &project_id,
        &user_id,
        &module_id,
        &query,
        group_by,
        sub_group_by,
    )?;
    // Total count over the pre-annotation set (the `deepcopy` at `:107`).
    let total_sql = format!("SELECT COUNT(DISTINCT issue.id) {}", filtered.from_where);
    let total_count = fetch_count(&pool, &total_sql, filtered.values.clone()).await?;
    // `order_issue_queryset` (`:112-117`), default `created_at`.
    let order_spec = pidash_services::app_issues::ordering::order_sql(
        &params.order_by,
        "state.\"group\"",
        |name| format!("min_{}", name.replace("__", "_")),
    );
    let (key_expr, descending) = order_key(&order_spec.out_param, &params.order_by)?;
    let direction = if descending { "DESC" } else { "ASC" };
    let per_page = params.per_page;
    let cursor = crate::paginator::Cursor::from_string(&params.cursor_raw).map_err(page_denial)?;
    if group_by.is_some() {
        return module_grouped_response(
            &pool,
            &slug,
            &project_id,
            &filtered,
            group_by.unwrap_or_default(),
            sub_group_by.map(str::to_owned),
            &key_expr,
            direction,
            per_page,
            cursor,
            total_count,
        )
        .await;
    }
    module_flat_response(&pool, &filtered, &key_expr, direction, per_page, cursor).await
}

/// Plain `paginate` branch (`issue.py:199-207`): distinct rows, the
/// rewritten order key `NULLS LAST` plus `-created_at`, offset window.
async fn module_flat_response(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    key_expr: &str,
    direction: &str,
    per_page: i64,
    cursor: crate::paginator::Cursor,
) -> HandlerResult {
    use crate::paginator::{
        apply_offset_window, max_hits, next_cursor, offset_window, prev_cursor,
    };
    use pidash_services::app_issues::shape::envelope;
    let limit = per_page.min(1000);
    let window = offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    let selects = module_row_selects(&[]);
    let inner = format!(
        "SELECT DISTINCT {selects}, ({key_expr}) AS __order_key {} ORDER BY __order_key {direction} NULLS LAST, issue.created_at DESC LIMIT {} OFFSET {}",
        filtered.from_where,
        window.stop - window.offset,
        window.offset
    );
    let rows = fetch_json_rows(pool, &inner, filtered.values.clone()).await?;
    let has_more = rows.len() as i64 > limit;
    let page: Vec<Map<String, Value>> = apply_offset_window(&rows, limit)
        .map_err(page_denial)?
        .into_iter()
        .collect();
    let total_count = {
        let sql = format!("SELECT COUNT(DISTINCT issue.id) {}", filtered.from_where);
        fetch_count(pool, &sql, filtered.values.clone()).await?
    };
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let timezone = chrono_tz::UTC;
    let fields = wire_row_fields(None, None);
    let shaped: Vec<String> = page
        .iter()
        .map(|row| shape_row(row, &fields, &timezone, false))
        .collect();
    Ok(json_response(
        StatusCode::OK,
        envelope(
            None,
            None,
            total_count,
            &next.to_string(),
            &prev.to_string(),
            next.has_results_or_false(),
            prev.has_results_or_false(),
            shaped.len(),
            max_hits(total_count, limit).map_err(page_denial)?,
            total_count,
            &format!("[{}]", shaped.join(",")),
        ),
    ))
}

/// `(group, filtered count)` pairs for the totals dict
/// (`GroupedOffsetPaginator.__get_total_queryset`, `paginator.py:299-303`).
async fn module_group_total_pairs(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    group_expr: &str,
    count_filter: &str,
) -> Result<Vec<(String, i64)>, Denial> {
    let sql = format!(
        "SELECT COALESCE(({group_expr})::text, 'None') AS bucket, COUNT(DISTINCT issue.id) FILTER (WHERE {strip_and}) AS n {} GROUP BY 1",
        filtered.from_where,
        strip_and = count_filter.strip_prefix("AND ").unwrap_or(count_filter),
    );
    let rows = bind_values(&sql, filtered.values.clone())?
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::new();
    for row in rows {
        let bucket: String = row.try_get("bucket").map_err(|_| Denial::ServerError)?;
        let count: i64 = row.try_get("n").map_err(|_| Denial::ServerError)?;
        out.push((bucket, count));
    }
    Ok(out)
}

/// `(group, sub, count)` pairs for the nested sub-totals.
async fn module_sub_total_pairs(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    group_expr: &str,
    sub_expr: &str,
    count_filter: &str,
) -> Result<Vec<(String, String, i64)>, Denial> {
    let sql = format!(
        "SELECT COALESCE(({group_expr})::text, 'None') AS bucket, COALESCE(({sub_expr})::text, 'None') AS sub, COUNT(DISTINCT issue.id) FILTER (WHERE {strip_and}) AS n {} GROUP BY 1, 2",
        filtered.from_where,
        strip_and = count_filter.strip_prefix("AND ").unwrap_or(count_filter),
    );
    let rows = bind_values(&sql, filtered.values.clone())?
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::new();
    for row in rows {
        let bucket: String = row.try_get("bucket").map_err(|_| Denial::ServerError)?;
        let sub: String = row.try_get("sub").map_err(|_| Denial::ServerError)?;
        out.push((
            bucket,
            sub,
            row.try_get("n").map_err(|_| Denial::ServerError)?,
        ));
    }
    Ok(out)
}

/// Shape one row into an ordered map for the groupers. `fields` is the
/// exact wire order (`wire_row_fields`): the m2m group/sub keys ride in
/// their pinned positions, and the swapped arrays stay out (the grouping
/// kernels re-append them at the end, like Django's processors assigning
/// new dict keys).
fn module_shape_map(row: &Map<String, Value>, fields: &[String]) -> Map<String, Value> {
    let timezone = chrono_tz::UTC;
    let mut out = Map::new();
    for field in fields {
        let value = row.get(field).unwrap_or(&Value::Null);
        out.insert(
            field.clone(),
            module_shape_json_value(field, value, &timezone),
        );
    }
    out
}

/// Unquote `sort_order` in serialized grouped rows: Django renders the
/// float bare (`65535.0`, CPython `repr`), while the maps must carry it as
/// a string for the grouping kernels (which never do math on it). The
/// `py_float_str` text is always a bare-JSON-number-safe literal (digits,
/// `e`, `+`, `-`, `.`, or `nan`/`inf` like the flat path emits), so
/// splicing the quotes is exact.
fn unquote_sort_order(results_json: &str) -> String {
    let mut out = String::with_capacity(results_json.len());
    let mut rest = results_json;
    let marker = "\"sort_order\":\"";
    while let Some(start) = rest.find(marker) {
        let (head, tail) = rest.split_at(start + marker.len());
        out.push_str(&head[..head.len() - 1]);
        match tail.find('"') {
            Some(end) => {
                out.push_str(&tail[..end]);
                rest = &tail[end + 1..];
            }
            None => {
                out.push_str(tail);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// DRF UTC (`Z`) rendering for one stored datetime string, passthrough
/// otherwise (null stays null).
fn module_render_datetime(value: &Value) -> Value {
    match value {
        Value::String(text) => match chrono::DateTime::parse_from_rfc3339(text) {
            Ok(aware) => Value::String(crate::serializer::render_datetime(
                &aware.with_timezone(&chrono::Utc),
            )),
            Err(_) => value.clone(),
        },
        _ => value.clone(),
    }
}

fn module_shape_json_value(field: &str, value: &Value, timezone: &chrono_tz::Tz) -> Value {
    let _ = timezone;
    match field {
        // Timestamps render DRF-style on every path. Flat rows go through
        // the shared `shape_row`, which renders created/updated/completed
        // itself (archived_at is always null here — the manager excludes
        // archived rows — so its passthrough matches too).
        "completed_at" | "created_at" | "updated_at" | "archived_at" => {
            module_render_datetime(value)
        }
        "sort_order" => match value {
            Value::Number(number) => {
                let float = number.as_f64().unwrap_or(f64::NAN);
                Value::String(crate::paginator::py_float_str(float))
            }
            _ => value.clone(),
        },
        _ => value.clone(),
    }
}

/// Grouped / sub-grouped branch (`issue.py:126-198`): window-function
/// pagination over the group partition, totals with the intake /
/// archived / draft `count_filter`, buckets from `issue_group_values`.
#[allow(clippy::too_many_arguments)]
async fn module_grouped_response(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    filtered: &FilteredSet,
    group_by: &str,
    sub_group_by: Option<String>,
    key_expr: &str,
    direction: &str,
    per_page: i64,
    cursor: crate::paginator::Cursor,
    total_count: i64,
) -> HandlerResult {
    use crate::paginator::{
        grouped_max_hits, grouped_window, next_cursor, prev_cursor, process_grouped_results,
        process_sub_grouped_results, sub_field_dict, sub_total_dicts, total_dict,
    };
    use pidash_services::app_issues::shape::envelope;
    let limit = per_page.min(1000);
    let window = grouped_window(limit, cursor.offset, cursor.value, None).map_err(page_denial)?;
    let group_expr = group_expression(group_by)?;
    // The grouper skips the swapped arrays for the group AND the sub
    // (`utils/grouper.py:86-92`); the grouping kernels re-add them.
    let skip: Vec<&str> = [Some(group_by), sub_group_by.as_deref()]
        .into_iter()
        .flatten()
        .filter_map(skip_array_for)
        .collect();
    let selects = module_row_selects(&skip);
    let member_selects = group_member_selects(Some(group_by), sub_group_by.as_deref());
    let sub_expr = match &sub_group_by {
        Some(sub) => Some(group_expression(sub)?),
        None => None,
    };
    let partition = match &sub_expr {
        Some(sub) => format!("{group_expr}, {sub}"),
        None => group_expr.clone(),
    };
    let inner = format!(
        "SELECT * FROM (SELECT DISTINCT {selects}{member_selects}, ({key_expr}) AS __order_key,
         ROW_NUMBER() OVER (PARTITION BY {partition} ORDER BY ({key_expr}) {direction} NULLS LAST, issue.created_at DESC) AS __rn
         {from_where}) __w
         WHERE __w.__rn > {offset} AND __w.__rn <= {stop}
         ORDER BY __w.__order_key {direction} NULLS LAST, __w.created_at DESC",
        from_where = filtered.from_where,
        offset = window.offset,
        stop = window.stop,
    );
    let rows = fetch_json_rows(pool, &inner, filtered.values.clone()).await?;
    let has_more = rows.iter().any(|row| {
        row.get("__rn")
            .and_then(|value| value.as_i64())
            .map(|rn| rn >= window.stop)
            .unwrap_or(false)
    });
    let page: Vec<Map<String, Value>> = rows
        .into_iter()
        .filter(|row| {
            row.get("__rn")
                .and_then(|value| value.as_i64())
                .map(|rn| rn < window.stop)
                .unwrap_or(false)
        })
        .collect();
    let window_empty = page.is_empty();
    // `count_filter` for grouped pagination (`issue.py:161-168`).
    let count_filter =
        "((issue_intake.status = ANY('{1,-1,2}')) OR issue_intake.id IS NULL) AND issue.archived_at IS NULL AND issue.is_draft = FALSE";
    let group_totals = module_group_total_pairs(pool, filtered, &group_expr, count_filter).await?;
    if !window_empty && group_totals.is_empty() {
        // `...order_by("-count")[0]` on an empty group list: IndexError.
        return Err(Denial::ServerError);
    }
    let totals = total_dict(&group_totals);
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let fields = wire_row_fields(Some(group_by), sub_group_by.as_deref());
    let shaped: Vec<Map<String, Value>> = page
        .iter()
        .map(|row| module_shape_map(row, &fields))
        .collect();
    let group_fields = group_values(pool, group_by, slug, project_id, filtered).await?;
    let results_value = match &sub_expr {
        Some(_) => {
            let sub_pairs = module_sub_total_pairs(
                pool,
                filtered,
                &group_expr,
                sub_expr.as_deref().unwrap_or_default(),
                count_filter,
            )
            .await?;
            let (group_totals_map, sub_totals_map) = sub_total_dicts(&group_totals, &sub_pairs);
            let _ = group_totals_map;
            let seeded = sub_field_dict(&group_fields, &totals, &sub_totals_map)
                .map_err(|_| Denial::ServerError)?;
            let Value::Object(cells) = seeded else {
                return Err(Denial::ServerError);
            };
            process_sub_grouped_results(
                &shaped,
                group_by,
                &sub_group_by.clone().unwrap_or_default(),
                cells,
            )
            .map_err(|_| Denial::ServerError)?
        }
        None => process_grouped_results(&shaped, group_by, &group_fields, &totals)
            .map_err(|_| Denial::ServerError)?,
    };
    let results_json = serde_json::to_string(&results_value).map_err(|_| Denial::ServerError)?;
    let results_json = unquote_sort_order(&results_json);
    let top_group = group_totals
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(0);
    // Grouped `count` is the fanout-collapsed window length: one row per
    // distinct issue on this page, however many (issue, member) fanout rows
    // the window holds (pinned live: a 2-label issue answers count 1 on
    // page 0; a 3-issue list with `per_page=1` answers count 1 on page 0
    // and count 1 on page 1). `total_count`/`total_results` stay the full
    // distinct-issue count.
    let mut seen_issues = std::collections::HashSet::new();
    let page_issue_count = page
        .iter()
        .filter(|row| {
            row.get("id")
                .map(|id| seen_issues.insert(id.to_string()))
                .unwrap_or(false)
        })
        .count();
    Ok(json_response(
        StatusCode::OK,
        envelope(
            Some(group_by),
            sub_group_by.as_deref(),
            total_count,
            &next.to_string(),
            &prev.to_string(),
            next.has_results_or_false(),
            prev.has_results_or_false(),
            page_issue_count,
            grouped_max_hits(window_empty, top_group, limit).map_err(page_denial)?,
            total_count,
            &results_json,
        ),
    ))
}

// ---------------------------------------------------------------------------
// Creates
// ---------------------------------------------------------------------------

/// Python truthiness over a decoded JSON value for `if not issues:` /
/// `if modules:` (`issue.py:212,255`): `null`, `false`, `0`/`0.0`, `""`,
/// `[]` and `{}` are falsy; every other value — non-empty strings/arrays/
/// objects, non-zero numbers, `true` — is truthy.
fn is_falsy_json(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => number.as_f64().map(|n| n == 0.0).unwrap_or(false),
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.is_empty(),
    }
}

/// `str(item)` over one decoded JSON item (`issue_id=str(issue)` at
/// `issue.py:219`, `str(module_id)` at `:296`): strings pass through,
/// `true`/`false` render `True`/`False`, `null` renders `None`, numbers
/// render in shortest form. The text feeds the UUID prep (`prep_coerced_str`
/// / `prep_raw_item`), which decides 400-payload vs 400-detail vs proceed.
fn py_str_item(item: &Value) -> String {
    match item {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(item).expect("item renders"),
    }
}

/// Classify the decoded `issues` member for `create_module_issues`
/// (`issue.py:211-219`).
///
/// Falsy values answer 400 `{"error": "Issues are required"}`. Of the
/// truthy values, arrays yield their `str()` items and non-empty strings
/// iterate their chars and non-empty objects iterate their keys (each
/// `str()`-coerced downstream) — while truthy numbers/bools are not
/// iterable → `TypeError` → generic 500.
fn extract_create_strings(value: &Value) -> Result<Vec<String>, Denial> {
    if is_falsy_json(value) {
        return Err(Denial::BadError("Issues are required".to_owned()));
    }
    match value {
        Value::Array(items) => Ok(items.iter().map(py_str_item).collect()),
        Value::String(text) => Ok(text.chars().map(|c| c.to_string()).collect()),
        Value::Object(fields) => Ok(fields.keys().cloned().collect()),
        _ => Err(Denial::ServerError),
    }
}

/// `Project.objects.get(pk=project_id)` (`issue.py:215,253`): the workspace
/// id for the new bridge rows. A missing project is `DoesNotExist` → 404.
async fn project_workspace_id(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<uuid::Uuid, Denial> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT p.workspace_id FROM projects p WHERE p.id = $1"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::NotFound)
}

/// One `issue_activity.delay(...)` kwargs map for a module membership
/// change: `type="module.activity.created"` (`issue.py:232-245,272-285`).
/// `requested_data` carries the module side verbatim — `str(module_id)`
/// on the `create_module_issues` path (`:234`), the *raw* item on the
/// `create_issue_modules` path (`json.dumps({"module_id": module})`,
/// `:275`).
fn activity_created_kwargs(
    module_wire: Value,
    actor_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    origin: &str,
) -> Map<String, Value> {
    let mut kwargs = Map::new();
    kwargs.insert(
        "type".to_owned(),
        Value::String("module.activity.created".to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(serde_json::json!({"module_id": module_wire}).to_string()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert("current_instance".to_owned(), Value::Null);
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(chrono::Utc::now().timestamp().into()),
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    kwargs
}

/// Outcome of coercing one JSON item through the module/issue UUID field
/// prep (`UUIDField.to_python`, `fields/__init__.py:2684-2695`, plus the
/// FK save-prep `""→None` rule, `related.py:1119-1128`).
enum Prep {
    /// A concrete UUID to insert or look up.
    Uuid(uuid::Uuid),
    /// A null lookup (removals only): `filter(module_id=None)` matches no
    /// live bridge, so the removal answers 201 with a null module name.
    Null,
    /// 400 `{"error": "The payload is not valid"}` (`IntegrityError`).
    Payload,
    /// 400 `{"error": "Please provide valid detail"}` (`ValidationError`).
    Detail,
}

/// Coerce one *raw* JSON item through the UUID prep (`create_issue_modules`
/// adds with `save=true`, removals with `save=false`; `issue.py:257-259`
/// vs `:288-296`):
///
/// - `null` → `None`: on the save path the `NOT NULL` column dies with
///   `IntegrityError` (400 payload); on the lookup path it queries
///   `IS NULL` and matches nothing (201 with a null name).
/// - bools/ints → `uuid.UUID(int=v)`: in-range values are always valid
///   UUIDs (the FK decides: dangling → 400 payload); out-of-range values
///   (`< 0`, `>= 2^128`) die with `ValueError` → 400 detail. Note
///   `isinstance(True, int)`: `int(True) == 1`.
/// - `""` → `None` on the save path only (`related.py:1119-1128`; the
///   lookup path has no such rule) → 400 payload vs 400 detail.
/// - other strings → `uuid.UUID(hex=s)`: valid spellings proceed,
///   anything else is `ValueError` → 400 detail.
/// - floats/arrays/objects → `AttributeError` → 400 detail.
/// - JSON integers above `u64::MAX` already lost precision in `serde_json`
///   (no `arbitrary_precision`): they answer 400 detail here while Django
///   (unbounded ints) answers payload for values `< 2^128`. Absurd edge
///   (40-digit ints as module ids); no suite sends them.
fn prep_raw_item(item: &Value, save: bool) -> Prep {
    match item {
        Value::Null => {
            if save {
                Prep::Payload
            } else {
                Prep::Null
            }
        }
        Value::Bool(flag) => Prep::Uuid(uuid::Uuid::from_u128(*flag as u128)),
        Value::Number(number) => {
            if let Some(signed) = number.as_i64() {
                if signed >= 0 {
                    return Prep::Uuid(uuid::Uuid::from_u128(signed as u128));
                }
                return Prep::Detail;
            }
            if let Some(unsigned) = number.as_u64() {
                return Prep::Uuid(uuid::Uuid::from_u128(unsigned as u128));
            }
            Prep::Detail
        }
        Value::String(text) => {
            if text.is_empty() {
                if save {
                    return Prep::Payload;
                }
                return Prep::Detail;
            }
            match text.parse::<uuid::Uuid>() {
                Ok(id) => Prep::Uuid(id),
                Err(_) => Prep::Detail,
            }
        }
        Value::Array(_) | Value::Object(_) => Prep::Detail,
    }
}

/// Coerce one `str()`-coerced item (`create_module_issues`,
/// `issue_id=str(issue)`, `issue.py:219`): `""` inserts `NULL` (400
/// payload); a parseable spelling proceeds; anything else is
/// `ValidationError` (400 detail).
fn prep_coerced_str(raw: &str) -> Prep {
    if raw.is_empty() {
        return Prep::Payload;
    }
    match raw.parse::<uuid::Uuid>() {
        Ok(id) => Prep::Uuid(id),
        Err(_) => Prep::Detail,
    }
}

fn denial_for_prep(prep: Prep) -> Denial {
    match prep {
        Prep::Payload => Denial::BadError("The payload is not valid".to_owned()),
        Prep::Detail => Denial::BadError(INVALID_DETAIL_MSG.to_owned()),
        Prep::Uuid(_) | Prep::Null => Denial::ServerError,
    }
}

/// `ModuleIssueViewSet.create_module_issues` (`issue.py:209-246`).
async fn create_module_issues(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let (pool, user_id, project_id) = module_issue_context(
        &state,
        &slug,
        &project_raw,
        gate_for_create_module_issues(),
        extension,
    )
    .await?;
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    // `request.data.get("issues", [])` (`:211`): a non-object body has no
    // `.get` (AttributeError → 500); objects fall through to the
    // truthiness check below.
    let issues_value = match body.0.as_object() {
        Some(_) => body.0.get("issues").cloned().unwrap_or(Value::Null),
        None => return Err(Denial::ServerError),
    };
    let issue_strings = extract_create_strings(&issues_value)?;
    let workspace_id = project_workspace_id(&pool, &project_id).await?;
    // UUID coercion happens at the `UUIDField` prep inside `bulk_create`
    // (`:216-230`): `""` inserts `NULL` → 400 payload, an unparseable
    // spelling dies with `ValidationError` → 400 detail (there is no
    // `__in` filter here, unlike the cycle create).
    let mut issue_ids = Vec::with_capacity(issue_strings.len());
    for raw in &issue_strings {
        match prep_coerced_str(raw) {
            Prep::Uuid(id) => issue_ids.push(id),
            prep => return Err(denial_for_prep(prep)),
        }
    }
    // `bulk_create(..., batch_size=10, ignore_conflicts=True)` (`:216-230`);
    // batching is unobservable, one multi-row `INSERT ... ON CONFLICT DO
    // NOTHING` (the partial unique on `(issue_id, module_id)` where
    // `deleted_at IS NULL`). A dangling issue id violates the FK →
    // `IntegrityError` → 400 `The payload is not valid`.
    if !issue_ids.is_empty() {
        let now = chrono::Utc::now();
        sqlx::query(
            r#"INSERT INTO module_issues
                   (id, project_id, workspace_id, created_by_id, updated_by_id, module_id, issue_id, created_at, updated_at)
                   SELECT gen_random_uuid(), $1, $2, $3, $3, $4, unnest($5::uuid[]), $6, $6
                   ON CONFLICT DO NOTHING"#,
        )
        .bind(project_id)
        .bind(workspace_id)
        .bind(user_id)
        .bind(module_id)
        .bind(&issue_ids)
        .bind(now)
        .execute(&pool)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23503") => {
                Denial::BadError("The payload is not valid".to_owned())
            }
            _ => Denial::ServerError,
        })?;
    }
    // Bulk activity: one publish per *input* issue, even for already-linked
    // rows (`:232-245` iterates `issues`, not the created records).
    let origin = request_origin(&state)?;
    for issue_id in &issue_ids {
        super::handlers_modules::enqueue_task(
            &pool,
            tasks::ISSUE_ACTIVITY_TASK,
            activity_created_kwargs(
                Value::String(module_id.to_string()),
                &user_id,
                issue_id,
                &project_id,
                &origin,
            ),
        )
        .await;
    }
    Ok(json_response(
        StatusCode::CREATED,
        MODULE_ISSUE_CREATE_BODY.to_owned(),
    ))
}

/// `ModuleIssueViewSet.create_issue_modules` (`issue.py:248-315`).
async fn create_issue_modules(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let (pool, user_id, project_id) = module_issue_context(
        &state,
        &slug,
        &project_raw,
        gate_for_create_issue_modules(),
        extension,
    )
    .await?;
    let issue_id = parse_uuid_or_invalid(&issue_raw)?;
    // `request.data.get(...)` (`:251-252`): a non-object body has no `.get`
    // (AttributeError → 500).
    let body_map = match body.0.as_object() {
        Some(map) => map.clone(),
        None => return Err(Denial::ServerError),
    };
    let workspace_id = project_workspace_id(&pool, &project_id).await?;
    let origin = request_origin(&state)?;
    // Adds (`:255-285`): only when `modules` is truthy — a missing, null,
    // empty or falsy member skips the block with no 400. Each iteration
    // unit carries its `str()` coercion (for the bridge row) alongside its
    // raw wire value (for `requested_data`, `:275`). Truthy non-arrays
    // iterate too: strings yield chars, objects yield keys (`for module
    // in modules`); truthy numbers/bools are not iterable → `TypeError`
    // → 500.
    if let Some(modules_value) = body_map.get("modules") {
        if !is_falsy_json(modules_value) {
            let raws: Vec<Value> = match modules_value {
                Value::Array(items) => items.clone(),
                Value::String(text) => text.chars().map(|c| Value::String(c.to_string())).collect(),
                Value::Object(fields) => fields
                    .keys()
                    .map(|key| Value::String(key.clone()))
                    .collect(),
                _ => return Err(Denial::ServerError),
            };
            // UUID coercion at the `UUIDField` prep inside `bulk_create`
            // (`:256-270`): `null`/`""`/ints die at the `NOT NULL` column
            // or the FK (400 payload); unparseable spellings and
            // floats/containers die with `ValidationError` (400 detail).
            let mut ids = Vec::with_capacity(raws.len());
            for wire in &raws {
                match prep_raw_item(wire, true) {
                    Prep::Uuid(id) => ids.push(id),
                    prep => return Err(denial_for_prep(prep)),
                }
            }
            insert_module_bridges(&pool, &project_id, &workspace_id, &user_id, &issue_id, &ids)
                .await?;
            for wire in &raws {
                super::handlers_modules::enqueue_task(
                    &pool,
                    tasks::ISSUE_ACTIVITY_TASK,
                    activity_created_kwargs(
                        wire.clone(),
                        &user_id,
                        &issue_id,
                        &project_id,
                        &origin,
                    ),
                )
                .await;
            }
        }
    }
    finish_issue_modules(
        &pool,
        &slug,
        &project_id,
        &user_id,
        &issue_id,
        &body_map,
        &origin,
    )
    .await
}

/// Shared tail of `create_issue_modules`: removals (`:287-313`) + 201.
async fn finish_issue_modules(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    body_map: &Map<String, Value>,
    origin: &str,
) -> HandlerResult {
    // `removed_modules` defaults to `[]` when absent (`:252`); an explicit
    // non-list iterates too (strings yield chars, objects yield keys;
    // truthy numbers/bools/`null` are not iterable → `TypeError` → 500 —
    // note an explicit `null` member *inside* the list is fine, it just
    // matches no bridge).
    let removed_value = body_map
        .get("removed_modules")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let removed_items: Vec<Value> = match &removed_value {
        Value::Array(items) => items.clone(),
        Value::String(text) => text.chars().map(|c| Value::String(c.to_string())).collect(),
        Value::Object(fields) => fields
            .keys()
            .map(|key| Value::String(key.clone()))
            .collect(),
        _ => return Err(Denial::ServerError),
    };
    for item in &removed_items {
        // `ModuleIssue.objects.filter(...)` (`:288-293`): the UUID lookup
        // prep renders `ValidationError` → 400 for unparseable spellings
        // (`""`, bad strings, floats, containers, out-of-range ints);
        // `null`/in-range ints coerce and match nothing (201 with a null
        // name). `requested_data` carries `str()` of the *raw* item
        // (`:296`), not the normalized UUID.
        let wire = py_str_item(item);
        match prep_raw_item(item, false) {
            Prep::Uuid(module_id) => {
                detach_module_bridge(
                    pool,
                    slug,
                    project_id,
                    user_id,
                    issue_id,
                    &wire,
                    Some(module_id),
                    origin,
                )
                .await?;
            }
            Prep::Null => {
                detach_module_bridge(
                    pool, slug, project_id, user_id, issue_id, &wire, None, origin,
                )
                .await?;
            }
            prep => return Err(denial_for_prep(prep)),
        }
    }
    Ok(json_response(
        StatusCode::CREATED,
        MODULE_ISSUE_CREATE_BODY.to_owned(),
    ))
}

/// One `bulk_create(ignore_conflicts)` for the adds path.
async fn insert_module_bridges(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    module_ids: &[uuid::Uuid],
) -> Result<(), Denial> {
    if module_ids.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now();
    sqlx::query(
        r#"INSERT INTO module_issues
               (id, project_id, workspace_id, created_by_id, updated_by_id, module_id, issue_id, created_at, updated_at)
               SELECT gen_random_uuid(), $1, $2, $3, $3, unnest($4::uuid[]), $5, $6, $6
               ON CONFLICT DO NOTHING"#,
    )
    .bind(project_id)
    .bind(workspace_id)
    .bind(user_id)
    .bind(module_ids)
    .bind(issue_id)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|error| match error {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23503") => {
            Denial::BadError("The payload is not valid".to_owned())
        }
        _ => Denial::ServerError,
    })?;
    Ok(())
}

/// The live bridge's module name for a detachment, if the bridge exists.
/// `module_issue.first().module.name` (`:302-307, :331`): the forward-FK
/// fetch sees soft-deleted modules too (verified live: detaching from a
/// soft-deleted module still publishes its name with 204/201). A missing
/// module row is unreachable (the FK), surfaced as 404.
async fn bridge_module_name(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
) -> Result<Option<Value>, Denial> {
    let bridge: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT mi.id FROM module_issues mi
           JOIN workspaces w ON w.id = mi.workspace_id
           WHERE w.slug = $1 AND mi.project_id = $2 AND mi.module_id = $3 AND mi.issue_id = $4
           AND mi.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(module_id)
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if bridge.is_none() {
        return Ok(None);
    }
    let module_row: Option<(Option<String>,)> =
        sqlx::query_as(r#"SELECT m.name FROM modules m WHERE m.id = $1"#)
            .bind(module_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    match module_row {
        Some((name,)) => Ok(Some(name.map(Value::String).unwrap_or(Value::Null))),
        None => Err(Denial::NotFound),
    }
}

/// One detachment: the `activity.deleted` publish fires before the bridge
/// soft delete (`issue.py:287-313` removals, `:325-336` destroy).
/// `module_wire` is `str()` of the raw item (`:296`); `module_id=None`
/// matches no bridge (the `null`-item removal); `module_name=None` is the
/// missing-bridge publish (removals only — `destroy` 500s before this on
/// a missing bridge, `:331` has no `None` guard).
#[allow(clippy::too_many_arguments)]
async fn detach_module_bridge(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    module_wire: &str,
    module_id: Option<uuid::Uuid>,
    origin: &str,
) -> Result<(), Denial> {
    let module_name = match module_id {
        Some(id) => match bridge_module_name(pool, slug, project_id, &id, issue_id).await? {
            Some(name) => name,
            None => Value::Null,
        },
        None => Value::Null,
    };
    let mut kwargs = Map::new();
    kwargs.insert(
        "type".to_owned(),
        Value::String("module.activity.deleted".to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(serde_json::json!({"module_id": module_wire}).to_string()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        Value::String(serde_json::json!({"module_name": module_name}).to_string()),
    );
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(chrono::Utc::now().timestamp().into()),
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    super::handlers_modules::enqueue_task(pool, tasks::ISSUE_ACTIVITY_TASK, kwargs).await;
    sqlx::query(
        r#"UPDATE module_issues SET deleted_at = now()
           WHERE project_id = $1 AND module_id = $2 AND issue_id = $3
             AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $4)
             AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(module_id)
    .bind(issue_id)
    .bind(slug)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Destroy
// ---------------------------------------------------------------------------

/// `ModuleIssueViewSet.destroy` (`issue.py:317-337`): the activity publish
/// fires before the delete and unconditionally; the bridge
/// `filter().delete()` is a soft delete; a missing bridge still 500s on
/// the `.first().module` dereference; 204 on success.
async fn module_issue_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw, issue_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let (pool, user_id, project_id) =
        module_issue_context(&state, &slug, &project_raw, gate_for_destroy(), extension).await?;
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    let issue_id = parse_uuid_or_invalid(&issue_raw)?;
    let origin = request_origin(&state)?;
    // `:331` has no `None` guard: a missing bridge is `None.module`
    // (`AttributeError`) → 500 before anything is published.
    if bridge_module_name(&pool, &slug, &project_id, &module_id, &issue_id)
        .await?
        .is_none()
    {
        return Err(Denial::ServerError);
    }
    detach_module_bridge(
        &pool,
        &slug,
        &project_id,
        &user_id,
        &issue_id,
        &module_id.to_string(),
        Some(module_id),
        &origin,
    )
    .await?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_issues::OneOrMany;

    fn empty_query() -> QueryMap {
        HashMap::new()
    }

    #[test]
    fn gates_cover_the_owned_actions() {
        let list = gates::gate_for(
            "GET",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/",
        )
        .expect("list gate");
        assert_eq!(list.source, "issue.py:95 (ModuleIssueViewSet.list)");
        let create = gates::gate_for(
            "POST",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/",
        )
        .expect("create gate");
        assert_eq!(
            create.source,
            "issue.py:209 (ModuleIssueViewSet.create_module_issues)"
        );
        let reverse = gates::gate_for(
            "POST",
            "workspaces/<slug>/projects/<project_id>/issues/<issue_id>/modules/",
        )
        .expect("reverse create gate");
        assert_eq!(
            reverse.source,
            "issue.py:248 (ModuleIssueViewSet.create_issue_modules)"
        );
        let destroy = gates::gate_for(
            "DELETE",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
        )
        .expect("destroy gate");
        assert_eq!(destroy.source, "issue.py:317 (ModuleIssueViewSet.destroy)");
        for row in [list, create, reverse, destroy] {
            assert!(matches!(row.gate, gates::Gate::Project { .. }));
        }
        // The detail GET/PUT/PATCH fall through to the undecorated
        // `ModelViewSet` defaults (500 upstream): `IsAuthenticated` only.
        for method in ["GET", "PUT", "PATCH"] {
            let row = gates::gate_for(
                method,
                "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
            )
            .expect("detail gate");
            assert!(matches!(row.gate, gates::Gate::Open));
        }
    }

    #[test]
    fn clash_body_matches_the_contract() {
        assert_eq!(
            GROUP_CLASH_MESSAGE,
            "Group by and sub group by cannot have same parameters"
        );
    }

    #[test]
    fn fixture_bodies_are_byte_exact() {
        assert_eq!(ISSUES_REQUIRED_BODY, r#"{"error":"Issues are required"}"#);
        assert_eq!(MODULE_ISSUE_CREATE_BODY, r#"{"message":"success"}"#);
        assert_eq!(INVALID_DETAIL_MSG, "Please provide valid detail");
    }

    #[test]
    fn query_map_helpers_stay_wired() {
        // Keep the `QueryMap` / `OneOrMany` imports live: the list handler
        // reads multi-value params through them.
        let mut query = empty_query();
        query.insert("group_by".to_owned(), OneOrMany::One("priority".to_owned()));
        assert_eq!(query_last(&query, "group_by").as_deref(), Some("priority"));
        assert_eq!(multi_map(&query)["group_by"], vec!["priority".to_owned()]);
    }

    /// Render a classifier outcome as `(status, body)` for exact comparison.
    fn outcome_strings(result: Result<Vec<String>, Denial>) -> (u16, String) {
        match result {
            Ok(strings) => (201, strings.join(",")),
            Err(Denial::BadError(message)) => {
                (400, format!("{{\"error\":{}}}", json_string(&message)))
            }
            Err(Denial::ServerError) => (500, "server-error".to_owned()),
            Err(other) => panic!("unexpected denial: {other:?}"),
        }
    }

    #[test]
    fn falsy_issues_answer_400() {
        // `if not issues:` (`issue.py:212-214`).
        for raw in [
            serde_json::json!(null),
            serde_json::json!(false),
            serde_json::json!(0),
            serde_json::json!(""),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            assert_eq!(
                outcome_strings(extract_create_strings(&raw)),
                (400, r#"{"error":"Issues are required"}"#.to_owned()),
                "input: {raw}"
            );
        }
    }

    #[test]
    fn truthy_non_iterables_answer_500() {
        // Truthy numbers/bools are not iterable (`TypeError` → 500).
        for raw in [
            serde_json::json!(5),
            serde_json::json!(1.5),
            serde_json::json!(true),
        ] {
            assert_eq!(
                outcome_strings(extract_create_strings(&raw)),
                (500, "server-error".to_owned()),
                "input: {raw}"
            );
        }
    }

    #[test]
    fn containers_iterate_their_units() {
        // Arrays yield items, strings yield chars, objects yield keys.
        assert_eq!(
            extract_create_strings(&serde_json::json!(["a", "b"])).expect("array"),
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert_eq!(
            extract_create_strings(&serde_json::json!("ab")).expect("string"),
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert_eq!(
            extract_create_strings(&serde_json::json!({"k": 1})).expect("object"),
            vec!["k".to_owned()]
        );
    }

    #[test]
    fn python_str_coercions_match() {
        assert_eq!(py_str_item(&serde_json::json!(true)), "True");
        assert_eq!(py_str_item(&serde_json::json!(false)), "False");
        assert_eq!(py_str_item(&serde_json::json!(null)), "None");
        assert_eq!(py_str_item(&serde_json::json!(5)), "5");
        assert_eq!(py_str_item(&serde_json::json!("x")), "x");
    }

    fn prep_outcome(prep: Prep) -> &'static str {
        match prep {
            Prep::Uuid(_) => "uuid",
            Prep::Null => "null",
            Prep::Payload => "payload",
            Prep::Detail => "detail",
        }
    }

    #[test]
    fn raw_prep_matches_django_field_prep() {
        // Save path (`bulk_create`): null/""/ints → 400 payload (NOT NULL
        // or FK); bad strings/floats/containers/out-of-range ints → 400
        // detail (ValidationError). All pinned live against Django.
        let cases = [
            (serde_json::json!(null), true, "payload"),
            (serde_json::json!(""), true, "payload"),
            (serde_json::json!(0), true, "uuid"),
            (serde_json::json!(5), true, "uuid"),
            (serde_json::json!(true), true, "uuid"),
            (serde_json::json!(false), true, "uuid"),
            (serde_json::json!(-5), true, "detail"),
            (serde_json::json!(5.5), true, "detail"),
            (serde_json::json!("xyz"), true, "detail"),
            (serde_json::json!(" "), true, "detail"),
            (serde_json::json!({"a": 1}), true, "detail"),
            (serde_json::json!([1]), true, "detail"),
            (
                serde_json::json!("12345678-1234-1234-1234-123456789012"),
                true,
                "uuid",
            ),
        ];
        for (raw, save, want) in cases {
            assert_eq!(
                prep_outcome(prep_raw_item(&raw, save)),
                want,
                "input: {raw}"
            );
        }
        // Lookup path (removals): no ""→None rule — "" is 400 detail and
        // null coerces to a null lookup (201 with a null name).
        let cases = [
            (serde_json::json!(null), false, "null"),
            (serde_json::json!(""), false, "detail"),
            (serde_json::json!(5), false, "uuid"),
            (serde_json::json!(true), false, "uuid"),
            (serde_json::json!(-5), false, "detail"),
            (serde_json::json!("xyz"), false, "detail"),
            (serde_json::json!(5.5), false, "detail"),
        ];
        for (raw, save, want) in cases {
            assert_eq!(
                prep_outcome(prep_raw_item(&raw, save)),
                want,
                "input: {raw}"
            );
        }
    }

    #[test]
    fn coerced_prep_matches_django_save_prep() {
        // `str()`-coerced items (`create_module_issues`): "" → payload,
        // parseable → uuid, everything else → detail.
        assert_eq!(prep_outcome(prep_coerced_str("")), "payload");
        assert_eq!(prep_outcome(prep_coerced_str("None")), "detail");
        assert_eq!(prep_outcome(prep_coerced_str("5")), "detail");
        assert_eq!(
            prep_outcome(prep_coerced_str("12345678-1234-1234-1234-123456789012")),
            "uuid"
        );
        // The denial each prep maps to.
        assert!(matches!(
            denial_for_prep(Prep::Payload),
            Denial::BadError(_)
        ));
        assert!(matches!(denial_for_prep(Prep::Detail), Denial::BadError(_)));
    }

    #[test]
    fn falsy_classifier_matches() {
        assert!(is_falsy_json(&serde_json::json!(null)));
        assert!(is_falsy_json(&serde_json::json!([])));
        assert!(!is_falsy_json(&serde_json::json!([1])));
        assert!(!is_falsy_json(&serde_json::json!("x")));
    }

    #[test]
    fn wire_order_matches_django() {
        // Pinned live: concrete columns, then the m2m keys, then the
        // scalar annotations, then the surviving arrays.
        let base = vec![
            "id",
            "name",
            "state_id",
            "sort_order",
            "completed_at",
            "estimate_point",
            "priority",
            "start_date",
            "target_date",
            "sequence_id",
            "project_id",
            "parent_id",
            "created_at",
            "updated_at",
            "created_by",
            "updated_by",
            "is_draft",
            "archived_at",
            "state__group",
        ];
        let flat = wire_row_fields(None, None);
        let mut want = base.clone();
        want.extend([
            "cycle_id",
            "link_count",
            "attachment_count",
            "sub_issues_count",
            "assignee_ids",
            "label_ids",
            "module_ids",
        ]);
        assert_eq!(flat, want);
        // m2m group: key at 19, swapped array re-added last by the kernel.
        let grouped = wire_row_fields(Some("labels__id"), None);
        assert_eq!(
            &grouped[18..24],
            &[
                "state__group",
                "labels__id",
                "cycle_id",
                "link_count",
                "attachment_count",
                "sub_issues_count"
            ]
        );
        assert_eq!(&grouped[24..], &["assignee_ids", "module_ids"]);
        // group + sub m2m: both keys, both arrays out.
        let sub = wire_row_fields(Some("labels__id"), Some("assignees__id"));
        assert_eq!(
            &sub[18..25],
            &[
                "state__group",
                "labels__id",
                "assignees__id",
                "cycle_id",
                "link_count",
                "attachment_count",
                "sub_issues_count"
            ]
        );
        assert_eq!(&sub[25..], &["module_ids"]);
    }

    #[test]
    fn sort_order_unquotes_exactly() {
        assert_eq!(
            unquote_sort_order(r#"{"a":1,"sort_order":"65535.0","b":[]}"#),
            r#"{"a":1,"sort_order":65535.0,"b":[]}"#
        );
        assert_eq!(
            unquote_sort_order(r#"[{"sort_order":"1e+20"}]"#),
            r#"[{"sort_order":1e+20}]"#
        );
        assert_eq!(unquote_sort_order(r#"{"x":"y"}"#), r#"{"x":"y"}"#);
    }
}
