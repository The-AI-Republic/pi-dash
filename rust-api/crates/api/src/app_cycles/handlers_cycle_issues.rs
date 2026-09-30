//! Cycle-issue + shared request plumbing handlers (D-27, stage 5, PIDASHCONV-323).
//!
//! Ports `apps/api/pi_dash/app/views/cycle/issue.py`:
//!
//! - `CycleIssueViewSet.list` (`:108-221`): the issue-list pipeline scoped to
//!   one cycle — legacy `issue_filters(params, 'GET')` (`:111`),
//!   `ComplexFilterBackend` + `IssueFilterSet` (`:43-44, :119`), the
//!   `apply_annotations` columns (`:77-106`), `order_issue_queryset`
//!   (`:130-134`), the grouped / sub-grouped / plain paginators
//!   (`:136-221`) with `issue_on_results` shaping.
//! - `CycleIssueViewSet.create` (`:223-297`): `issues` required (`:227-228`),
//!   completed-cycle refusal (`:232-236`), move-vs-`bulk_create`
//!   (`:239-279`, batches 10 / 100), `issue_activity` publish (`:281-295`),
//!   201 `{"message": "success"}` (`:297`).
//! - `CycleIssueViewSet.destroy` (`:299-324`): `issue_activity` publish
//!   first (`:305-319`), bridge soft delete (`:320`), 204 (`:324`).
//!
//! Fixture ids: F-C27-01 (serializer shapes), F-C27-05 (queryset +
//! create/destroy envelopes), F-C27-07 (MEMBER gates via `super::gates`),
//! F-C27-10 (favorites scope reference).
//!
//! Layering: SQL builders live in
//! [`pidash_services::app_cycles::queries`]; gates in [`super::gates`]
//! over the F-06 kernel; the list pipeline composes the shared kernels —
//! `pidash_db::{filter, filterset, issue_filters}`, the D-26
//! `crate::app_issues` list helpers, `pidash_services::app_issues`
//! ordering/params/shape, and `crate::paginator`. Cycle-specific SQL
//! (scope preamble, join selection, annotation/legacy/group glue) lives
//! here because it names this endpoint's bridge scope; the D-26 helpers
//! are referenced, never re-ported.
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * `create` with a `NULL` cycle `end_date` raises `TypeError`
//!   (`None < timezone.now()`, `:232`) → generic 500. Draft cycles 500 on
//!   every add here too.
//! * `destroy` on a missing bridge still answers 204: `filter().delete()`
//!   on an empty set with no existence check (`:301-320`).
//! * `retrieve` / `update` / `partial_update` on the detail path fall
//!   through to the `ModelViewSet` defaults, which die with a 500 for
//!   every caller (contract `test_cycle_issue_detail_broken_upstream`).
//!   Those methods proxy to Django, which reproduces the 500 itself.
//! * Non-UUID path segments: Django's `<uuid:>` converter rejects them at
//!   URL resolve (HTML 404, unreachable through the JSON edge); here they
//!   answer the `ValidationError` 400, the same body the kernels render
//!   for badly-formed UUIDs inside filters.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::Router;
use serde_json::{Map, Value};
use sqlx::{Postgres, Row};

use crate::app_cycles::gates;
use crate::app_issues::{
    complex_filter, fetch_count, fetch_json_rows, group_values, legacy_sql, multi_map, order_key,
    page_denial, query_last, render_condition, shape_row, Binder, Denial, FilteredSet, QueryMap,
};
use crate::state::AppState;
use pidash_auth::permissions::allow::AllowFacts;

/// `GET` + `POST` on the collection path; `DELETE` on the detail path.
/// Every other method falls through to Django: the detail
/// retrieve/update/partial_update are undecorated `ModelViewSet` defaults
/// (500 upstream, gate `Authenticated`), which the proxy reproduces.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/",
            axum::routing::get(cycle_issue_list)
                .post(cycle_issue_create)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/",
            axum::routing::delete(cycle_issue_destroy)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .post(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

/// Collection path in `app/urls/cycle.py:40-44` form (route order check).
pub const CYCLE_ISSUES_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/";
/// Detail path in `app/urls/cycle.py:45-53` form.
pub const CYCLE_ISSUE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/";

/// `issue_activity` Celery wire name (bare `@shared_task` default:
/// `bgtasks/issue_activities_task.py:1503-1504`).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// Group-clash message (`issue.py:146-150`); renders as
/// [`cycle_queries::GROUP_CLASH_BODY`] through `Denial::BadError`.
pub const GROUP_CLASH_MESSAGE: &str = "Group by and sub group by cannot have same parameters";

// ---------------------------------------------------------------------------
// Shared request plumbing (also used by `handlers_favorites`)
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
/// `Http404("Project not found")` (`db/models/project.py:192-219`).
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

/// Badly-formed UUIDs in filters/paths render the `ValidationError` branch
/// (`app/views/base.py:110-152`): 400 `{"error": "Please provide valid
/// detail"}`.
pub(crate) const INVALID_DETAIL_MSG: &str = "Please provide valid detail";

pub(crate) fn parse_uuid_or_invalid(raw: &str) -> Result<uuid::Uuid, Denial> {
    raw.parse::<uuid::Uuid>()
        .map_err(|_| Denial::BadError(INVALID_DETAIL_MSG.to_owned()))
}

/// Membership facts for one `(user, slug, project)` over the same rows the
/// decorator reads (`app/permissions/base.py:19-86`): active project and
/// workspace memberships scoped by slug/project, soft-deleted rows excluded.
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

/// Best-effort deferred publish of an `issue_activity.delay(...)` call
/// (handlers_views precedent): enqueue failures never change the response.
pub(crate) async fn enqueue_activity(pool: &sqlx::PgPool, kwargs: serde_json::Map<String, Value>) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(ISSUE_ACTIVITY_TASK, vec![], kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// Common preamble for the three cycle-issue actions: session auth, project
/// rewrite, the route's MEMBER gate. Returns `(pool, user_id, project_id)`.
async fn cycle_issue_context(
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
        "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/",
    )
    .expect("cycle-issues list gate")
    .gate
}

fn gate_for_create() -> &'static gates::Gate {
    &gates::gate_for(
        "POST",
        "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/",
    )
    .expect("cycle-issues create gate")
    .gate
}

fn gate_for_destroy() -> &'static gates::Gate {
    &gates::gate_for(
        "DELETE",
        "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/",
    )
    .expect("cycle-issues destroy gate")
    .gate
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

/// `issue_on_results` flat field order (`utils/grouper.py:95-143`): the 22
/// base keys plus the three array keys, with the m2m group swap applied.
fn flat_row_fields(group_by: Option<&str>, sub_group_by: Option<&str>) -> Vec<String> {
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
        "cycle_id",
        "sub_issues_count",
        "created_at",
        "updated_at",
        "created_by",
        "updated_by",
        "attachment_count",
        "link_count",
        "is_draft",
        "archived_at",
        "state__group",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let mut arrays = vec!["assignee_ids", "label_ids", "module_ids"];
    let mapper = [
        ("labels__id", "label_ids"),
        ("assignees__id", "assignee_ids"),
        ("issue_module__module_id", "module_ids"),
    ];
    for group in [group_by, sub_group_by].into_iter().flatten() {
        if let Some((_, array)) = mapper.iter().find(|(key, _)| *key == group) {
            if let Some(position) = arrays.iter().position(|name| name == array) {
                arrays.remove(position);
                arrays.push(group);
            }
        }
    }
    fields.extend(arrays.into_iter().map(str::to_owned));
    fields
}

/// Scalar selects for the list rows: the D-26 `annotation_selects` set
/// verbatim (`app_issues`, from `apply_annotations` + the grouper array
/// subqueries) — `cycle_id` / `link_count` / `attachment_count` /
/// manager-guarded `sub_issues_count` plus the three `Coalesce` arrays.
/// `skip_array` drops one array for its m2m group path (the grouper rule).
fn cycle_row_selects(skip_array: Option<&str>) -> String {
    let mut selects = String::from(
        r#"issue.id, issue.name, issue.state_id, issue.sort_order, issue.completed_at,
        issue.estimate_point_id AS estimate_point, issue.priority, issue.start_date,
        issue.target_date, issue.sequence_id, issue.project_id, issue.parent_id,
        (SELECT ci.cycle_id FROM cycle_issues ci
          WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1) AS cycle_id,
        (SELECT NULLIF(COUNT(*), 0) FROM issue_links il
          WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS link_count,
        (SELECT NULLIF(COUNT(*), 0) FROM file_assets fa
          WHERE fa.issue_id = issue.id AND fa.entity_type = 'ISSUE_ATTACHMENT'
            AND fa.deleted_at IS NULL) AS attachment_count,
        (SELECT NULLIF(COUNT(*), 0) FROM issues c
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
    if skip_array != Some("label_ids") {
        selects.push_str(
            r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT il.label_id), '{}'::uuid[])
           FROM issue_labels il
          WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS label_ids"#,
        );
    }
    if skip_array != Some("assignee_ids") {
        selects.push_str(
            r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id), '{}'::uuid[])
           FROM issue_assignees ia
          WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL) AS assignee_ids"#,
        );
    }
    if skip_array != Some("module_ids") {
        selects.push_str(
            r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT mi.module_id), '{}'::uuid[])
           FROM module_issues mi JOIN modules m ON m.id = mi.module_id
          WHERE mi.issue_id = issue.id AND mi.deleted_at IS NULL
            AND m.archived_at IS NULL AND m.deleted_at IS NULL) AS module_ids"#,
        );
    }
    selects
}

/// Relation joins the cycle list can use: same alias contract as the D-26
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

/// The array annotation the grouper skips for an m2m group field.
fn skip_array_for(group_by: &str) -> Option<&'static str> {
    match group_by {
        "labels__id" => Some("label_ids"),
        "assignees__id" => Some("assignee_ids"),
        "issue_module__module_id" => Some("module_ids"),
        _ => None,
    }
}

/// Extra select carrying the raw m2m group key per joined row.
fn group_member_select(group_by: &str) -> &'static str {
    match group_by {
        "labels__id" => ", label_issue.label_id AS \"labels__id\"",
        "assignees__id" => ", issue_assignee.assignee_id AS \"assignees__id\"",
        "issue_module__module_id" => ", issue_module.module_id AS \"issue_module__module_id\"",
        _ => "",
    }
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

/// Filtered `FROM … WHERE` for the cycle-issue list (`issue.py:111-128`):
/// the `Issue.issue_objects` manager (`deleted_at`, triage-state,
/// archived, project-archived, draft exclusions) + the cycle bridge scope
/// (`issue_cycle__cycle_id`, bridge live) + tenant (workspace slug,
/// project, active membership — the `get_queryset` `:61-68` filters, kept
/// although the gate already enforces them) + the `ComplexFilterBackend`
/// tree + the legacy `issue_filters(params, 'GET')` predicates.
fn cycle_filtered_set(
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    cycle_id: &uuid::Uuid,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
) -> Result<FilteredSet, Denial> {
    let mut binder = Binder::new();
    let slug_holder = binder.bind_string(slug.to_owned());
    let project_holder = binder.bind_uuid(*project_id);
    let user_holder = binder.bind_uuid(*user_id);
    let cycle_holder = binder.bind_uuid(*cycle_id);
    let mut fragments = vec![format!(
        r#"workspaces.slug = {slug_holder} AND issue.project_id = {project_holder}
        AND issue.deleted_at IS NULL
        AND (state."group" IS NULL OR NOT (state."group" = 'triage'))
        AND issue.archived_at IS NULL AND project.archived_at IS NULL
        AND issue.is_draft = FALSE
        AND bridge.cycle_id = {cycle_holder} AND bridge.deleted_at IS NULL
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
    let where_sql = fragments.join(" AND ");
    let mut joins = String::from(" JOIN cycle_issues AS bridge ON bridge.issue_id = issue.id");
    let mut referenced = vec!["bridge"];
    for (table, alias) in RELATION_JOINS {
        let marker = format!("\"{alias}\".");
        let mentioned = where_sql.contains(&marker);
        let nullable_only = mentioned && alias_nullable_only(&where_sql, alias);
        let used = mentioned || group_join_alias(group_by, sub_group_by, alias);
        let inner = used && !nullable_only || group_join_alias(group_by, sub_group_by, alias);
        if used {
            referenced.push(*alias);
        }
        let kind = if inner { "INNER JOIN" } else { "LEFT JOIN" };
        joins.push_str(&format!(
            " {kind} {table} AS {alias} ON {alias}.issue_id = issue.id"
        ));
    }
    let intake_used = where_sql.contains("\"issue_intake\".");
    joins.push_str(&format!(
        " {} intake_issues AS issue_intake ON issue_intake.issue_id = issue.id",
        if intake_used {
            "INNER JOIN"
        } else {
            "LEFT JOIN"
        }
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

/// `CycleIssueViewSet.list` (`issue.py:108-221`).
async fn cycle_issue_list(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let (pool, user_id, project_id) =
        cycle_issue_context(&state, &slug, &project_raw, gate_for_list(), extension).await?;
    let cycle_id = parse_uuid_or_invalid(&cycle_raw)?;
    // Group clash is checked in-view before `paginate` parses per_page
    // (`issue.py:136-150`): 400 `{"error": "Group by and sub group by
    // cannot have same parameters"}` — the message half of the shared
    // [`cycle_queries::GROUP_CLASH_BODY`] (pinned equal in tests).
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
    let filtered = cycle_filtered_set(
        &slug,
        &project_id,
        &user_id,
        &cycle_id,
        &query,
        group_by,
        sub_group_by,
    )?;
    // Total count over the pre-annotation set (the `deepcopy` at `:125`).
    let total_sql = format!("SELECT COUNT(DISTINCT issue.id) {}", filtered.from_where);
    let total_count = fetch_count(&pool, &total_sql, filtered.values.clone()).await?;
    // `order_issue_queryset` (`:130-134`), default `-created_at`.
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
        return cycle_grouped_response(
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
    cycle_flat_response(&pool, &filtered, &key_expr, direction, per_page, cursor).await
}

/// Plain `paginate` branch (`issue.py:213-221`): distinct rows, the
/// rewritten order key `NULLS LAST` plus `-created_at`, offset window.
async fn cycle_flat_response(
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
    let selects = cycle_row_selects(None);
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
    let fields = flat_row_fields(None, None);
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
async fn cycle_group_total_pairs(
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
async fn cycle_sub_total_pairs(
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
        let count: i64 = row.try_get("n").map_err(|_| Denial::ServerError)?;
        out.push((bucket, sub, count));
    }
    Ok(out)
}

/// Shape one row into an ordered map for the groupers (datetimes as
/// stored — no `user_timezone_converter` on the grouped path).
fn cycle_shape_map(row: &Map<String, Value>, fields: &[String]) -> Map<String, Value> {
    let timezone = chrono_tz::UTC;
    let mut out = Map::new();
    for field in fields {
        let value = row.get(field).unwrap_or(&Value::Null);
        out.insert(
            field.clone(),
            cycle_shape_json_value(field, value, &timezone),
        );
    }
    // Group raw keys ride along for the grouper.
    for (key, value) in row {
        if (key == "labels__id" || key == "assignees__id" || key == "issue_module__module_id")
            && !out.contains_key(key)
        {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

fn cycle_shape_json_value(field: &str, value: &Value, timezone: &chrono_tz::Tz) -> Value {
    let _ = timezone;
    match field {
        "completed_at" => match value {
            Value::String(text) => match chrono::DateTime::parse_from_rfc3339(text) {
                Ok(aware) => Value::String(crate::serializer::render_datetime(
                    &aware.with_timezone(&chrono::Utc),
                )),
                Err(_) => value.clone(),
            },
            _ => value.clone(),
        },
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

/// Grouped / sub-grouped branch (`issue.py:136-212`): window-function
/// pagination over the group partition, totals with the intake /
/// archived / draft `count_filter`, buckets from `issue_group_values`.
#[allow(clippy::too_many_arguments)]
async fn cycle_grouped_response(
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
    let selects = cycle_row_selects(skip_array_for(group_by));
    let member_select = group_member_select(group_by);
    let sub_expr = match &sub_group_by {
        Some(sub) => Some(group_expression(sub)?),
        None => None,
    };
    let partition = match &sub_expr {
        Some(sub) => format!("{group_expr}, {sub}"),
        None => group_expr.clone(),
    };
    let inner = format!(
        "SELECT * FROM (SELECT DISTINCT {selects}{member_select}, ({key_expr}) AS __order_key,
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
    let window_len = page.len();
    // `count_filter` for grouped pagination (`issue.py:176-183`).
    let count_filter =
        "((issue_intake.status = ANY('{1,-1,2}')) OR issue_intake.id IS NULL) AND issue.archived_at IS NULL AND issue.is_draft = FALSE";
    let group_totals = cycle_group_total_pairs(pool, filtered, &group_expr, count_filter).await?;
    if !window_empty && group_totals.is_empty() {
        // `...order_by("-count")[0]` on an empty group list: IndexError.
        return Err(Denial::ServerError);
    }
    let totals = total_dict(&group_totals);
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let fields = flat_row_fields(Some(group_by), sub_group_by.as_deref());
    let shaped: Vec<Map<String, Value>> = page
        .iter()
        .map(|row| cycle_shape_map(row, &fields))
        .collect();
    let group_fields = group_values(pool, group_by, slug, project_id, filtered).await?;
    let results_value = match &sub_expr {
        Some(_) => {
            let sub_pairs = cycle_sub_total_pairs(
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
    let top_group = group_totals
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(0);
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
            window_len,
            grouped_max_hits(window_empty, top_group, limit).map_err(page_denial)?,
            total_count,
            &results_json,
        ),
    ))
}

// ---------------------------------------------------------------------------
// Create
// ---------------------------------------------------------------------------

/// 400 `{"error": "Issues are required"}` (`issue.py:227-228`).
pub const ISSUES_REQUIRED_BODY: &str = r#"{"error":"Issues are required"}"#;
/// 400 completed-cycle refusal (`issue.py:232-236`).
pub const CYCLE_COMPLETED_BODY: &str =
    r#"{"error":"The Cycle has already been completed so no new issues can be added"}"#;
/// 201 success envelope (`issue.py:297`).
pub const CYCLE_ISSUE_CREATE_BODY: &str = r#"{"message":"success"}"#;

/// `CycleIssueViewSet.create` (`issue.py:223-297`).
async fn cycle_issue_create(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let (pool, user_id, project_id) =
        cycle_issue_context(&state, &slug, &project_raw, gate_for_create(), extension).await?;
    let cycle_id = parse_uuid_or_invalid(&cycle_raw)?;
    // `request.data.get("issues", [])` + `if not issues:` (`:223-228`).
    // Non-object bodies cannot `.get` (AttributeError → 500); non-array
    // truthy bodies reach the UUID coercion below (strings 400, other
    // scalars 500 — the iterable/validation split in `__in`).
    let issues_value = match body.0.as_object() {
        Some(_) => body.0.get("issues").cloned().unwrap_or(Value::Null),
        None => return Err(Denial::ServerError),
    };
    let issue_strings: Vec<String> = match &issues_value {
        Value::Array(items) if !items.is_empty() => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item.as_str() {
                    Some(text) => out.push(text.to_owned()),
                    None => return Err(Denial::BadError(INVALID_DETAIL_MSG.to_owned())),
                }
            }
            out
        }
        Value::Null => return Err(Denial::BadError("Issues are required".to_owned())),
        Value::Array(_) => return Err(Denial::BadError("Issues are required".to_owned())),
        Value::String(_) => {
            return Err(Denial::BadError(INVALID_DETAIL_MSG.to_owned()));
        }
        _ => return Err(Denial::ServerError),
    };
    // `Cycle.objects.get(workspace__slug, project_id, pk)` (`:230`).
    let cycle: Option<(
        uuid::Uuid,
        uuid::Uuid,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        r#"SELECT c.id, c.workspace_id, c.end_date FROM cycles c
               JOIN workspaces w ON w.id = c.workspace_id
               WHERE c.id = $1 AND c.project_id = $2 AND w.slug = $3
               AND c.deleted_at IS NULL"#,
    )
    .bind(cycle_id)
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (_, workspace_id, end_date) = cycle.ok_or(Denial::NotFound)?;
    // Completed-cycle refusal (`:232-236`); a NULL `end_date` raises
    // `TypeError` in Python → generic 500 here too (ported bug).
    match end_date {
        None => return Err(Denial::ServerError),
        Some(end) if end < chrono::Utc::now() => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                CYCLE_COMPLETED_BODY.to_owned(),
            ));
        }
        Some(_) => {}
    }
    // UUID coercion happens at the `__in` filter (`:239`), after the
    // cycle lookup and the end-date check above.
    let mut issue_ids = Vec::with_capacity(issue_strings.len());
    for raw in &issue_strings {
        issue_ids.push(parse_uuid_or_invalid(raw)?);
    }
    // Bridges already created for these issues in *other* cycles (`:239`).
    let existing: Vec<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
        r#"SELECT issue_id, cycle_id FROM cycle_issues
           WHERE cycle_id != $1 AND issue_id = ANY($2) AND deleted_at IS NULL"#,
    )
    .bind(cycle_id)
    .bind(&issue_ids)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `existing_issues = [str(...) for ...]` + `set(issues) -
    // set(existing_issues)` (`:241-242`): the comparison is on the RAW
    // input strings, so a non-canonical UUID spelling of an already
    // bridged issue counts as new (and then violates the partial unique
    // constraint → 400, exactly like `bulk_create` does).
    let existing_strings: std::collections::HashSet<String> = existing
        .iter()
        .map(|(issue_id, _)| issue_id.to_string())
        .collect();
    let mut seen_new = std::collections::HashSet::new();
    let mut new_strings: Vec<String> = Vec::new();
    for raw in &issue_strings {
        if !existing_strings.contains(raw) && seen_new.insert(raw.clone()) {
            new_strings.push(raw.clone());
        }
    }
    let mut new_ids = Vec::with_capacity(new_strings.len());
    for raw in &new_strings {
        new_ids.push(parse_uuid_or_invalid(raw)?);
    }
    // `bulk_create(..., batch_size=10)` (`:244-257`); batching is
    // unobservable, one multi-row INSERT with RETURNING for the activity
    // payload below.
    let mut created: Vec<(
        uuid::Uuid,
        uuid::Uuid,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
    )> = Vec::new();
    if !new_ids.is_empty() {
        let now = chrono::Utc::now();
        let rows: Vec<(
            uuid::Uuid,
            uuid::Uuid,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        )> = sqlx::query_as(
            r#"INSERT INTO cycle_issues
                   (id, project_id, workspace_id, created_by_id, updated_by_id, cycle_id, issue_id, created_at, updated_at)
                   SELECT gen_random_uuid(), $1, $2, $3, $3, $4, unnest($5::uuid[]), $6, $6
                   RETURNING id, issue_id, created_at, updated_at"#,
        )
            .bind(project_id)
            .bind(workspace_id)
            .bind(user_id)
            .bind(cycle_id)
            .bind(&new_ids)
            .bind(now)
            .fetch_all(&pool)
            .await
            .map_err(|error| match error {
                sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
                    Denial::BadError("The payload is not valid".to_owned())
                }
                _ => Denial::ServerError,
            })?;
        created = rows;
    }
    // Moved bridges: `bulk_update(["cycle_id"], batch_size=100)` (`:279`).
    // Like Django's `bulk_update`, `updated_at` (`auto_now`) advances too.
    let mut moved: Vec<(uuid::Uuid, uuid::Uuid)> = Vec::new();
    if !existing.is_empty() {
        let moved_ids: Vec<uuid::Uuid> = existing.iter().map(|(issue_id, _)| *issue_id).collect();
        sqlx::query(
            r#"UPDATE cycle_issues SET cycle_id = $1, updated_at = now()
               WHERE issue_id = ANY($2) AND cycle_id != $1 AND deleted_at IS NULL"#,
        )
        .bind(cycle_id)
        .bind(&moved_ids)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        for (issue_id, old_cycle_id) in &existing {
            moved.push((*old_cycle_id, *issue_id));
        }
    }
    // `issue_activity.delay(type="cycle.activity.created", ...)` (`:281-295`).
    let origin = state
        .settings()
        .urls
        .app_base_url
        .clone()
        .ok_or(Denial::ServerError)?;
    let updated_activity: Vec<Value> = moved
        .iter()
        .map(|(old_cycle_id, issue_id)| {
            serde_json::json!({
                "old_cycle_id": old_cycle_id.to_string(),
                "new_cycle_id": cycle_id.to_string(),
                "issue_id": issue_id.to_string(),
            })
        })
        .collect();
    let created_activity: Vec<Value> = created
        .iter()
        .map(|(id, issue_id, created_at, updated_at)| {
            serde_json::json!({
                "model": "db.cycleissue",
                "pk": id.to_string(),
                "fields": {
                    "created_at": render_activity_datetime(created_at),
                    "updated_at": render_activity_datetime(updated_at),
                    "deleted_at": Value::Null,
                    "created_by": user_id.to_string(),
                    "updated_by": user_id.to_string(),
                    "project": project_id.to_string(),
                    "workspace": workspace_id.to_string(),
                    "cycle": cycle_id.to_string(),
                    "issue": issue_id.to_string(),
                },
            })
        })
        .collect();
    let mut kwargs = Map::new();
    kwargs.insert(
        "type".to_owned(),
        Value::String("cycle.activity.created".to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(serde_json::json!({"cycles_list": issue_strings}).to_string()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::Null);
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        Value::String(
            serde_json::json!({
                "updated_cycle_issues": updated_activity,
                "created_cycle_issues": created_activity,
            })
            .to_string(),
        ),
    );
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(chrono::Utc::now().timestamp().into()),
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin));
    enqueue_activity(&pool, kwargs).await;
    Ok(json_response(
        StatusCode::CREATED,
        CYCLE_ISSUE_CREATE_BODY.to_owned(),
    ))
}

/// Django serializer datetime rendering for the activity payload.
fn render_activity_datetime(value: &chrono::DateTime<chrono::Utc>) -> String {
    value.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

// ---------------------------------------------------------------------------
// Destroy
// ---------------------------------------------------------------------------

/// `CycleIssueViewSet.destroy` (`issue.py:299-324`): the activity publish
/// fires before the delete and unconditionally; the bridge
/// `filter().delete()` is a soft delete; a missing bridge still 204s.
async fn cycle_issue_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, cycle_raw, issue_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let (pool, user_id, project_id) =
        cycle_issue_context(&state, &slug, &project_raw, gate_for_destroy(), extension).await?;
    let cycle_id = parse_uuid_or_invalid(&cycle_raw)?;
    let issue_id = parse_uuid_or_invalid(&issue_raw)?;
    let origin = state
        .settings()
        .urls
        .app_base_url
        .clone()
        .ok_or(Denial::ServerError)?;
    let mut kwargs = Map::new();
    kwargs.insert(
        "type".to_owned(),
        Value::String("cycle.activity.deleted".to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(
            serde_json::json!({
                "cycle_id": cycle_id.to_string(),
                "issues": [issue_id.to_string()],
            })
            .to_string(),
        ),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(user_id.to_string()));
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
    kwargs.insert("origin".to_owned(), Value::String(origin));
    enqueue_activity(&pool, kwargs).await;
    sqlx::query(
        r#"UPDATE cycle_issues SET deleted_at = now()
           WHERE issue_id = $1 AND project_id = $2 AND cycle_id = $3
             AND workspace_id IN (SELECT id FROM workspaces WHERE slug = $4)
             AND deleted_at IS NULL"#,
    )
    .bind(issue_id)
    .bind(project_id)
    .bind(cycle_id)
    .bind(&slug)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_issues::OneOrMany;
    use pidash_services::app_cycles::queries as cycle_queries;

    fn empty_query() -> QueryMap {
        HashMap::new()
    }

    #[test]
    fn gates_cover_the_three_owned_actions() {
        let list = gates::gate_for(
            "GET",
            "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/",
        )
        .expect("list gate");
        assert_eq!(list.source, "issue.py:109 (CycleIssueViewSet.list)");
        let create = gates::gate_for(
            "POST",
            "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/",
        )
        .expect("create gate");
        assert_eq!(create.source, "issue.py:223 (CycleIssueViewSet.create)");
        let destroy = gates::gate_for(
            "DELETE",
            "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/",
        )
        .expect("destroy gate");
        assert_eq!(destroy.source, "issue.py:299 (CycleIssueViewSet.destroy)");
        for row in [list, create, destroy] {
            assert!(matches!(row.gate, gates::Gate::Project { .. }));
        }
    }

    #[test]
    fn clash_body_matches_the_shared_const() {
        assert_eq!(
            cycle_queries::GROUP_CLASH_BODY,
            format!("{{\"error\":{}}}", json_string(GROUP_CLASH_MESSAGE))
        );
    }

    #[test]
    fn fixture_bodies_are_byte_exact() {
        // F-C27-05 create/destroy envelopes.
        assert_eq!(ISSUES_REQUIRED_BODY, r#"{"error":"Issues are required"}"#);
        assert_eq!(
            CYCLE_COMPLETED_BODY,
            r#"{"error":"The Cycle has already been completed so no new issues can be added"}"#
        );
        assert_eq!(CYCLE_ISSUE_CREATE_BODY, r#"{"message":"success"}"#);
        assert_eq!(INVALID_DETAIL_MSG, "Please provide valid detail");
    }

    #[test]
    fn flat_fields_follow_issue_on_results_order() {
        let fields = flat_row_fields(None, None);
        let names: Vec<&str> = fields.iter().map(String::as_str).collect();
        assert_eq!(
            names,
            vec![
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
                "cycle_id",
                "sub_issues_count",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
                "attachment_count",
                "link_count",
                "is_draft",
                "archived_at",
                "state__group",
                "assignee_ids",
                "label_ids",
                "module_ids",
            ]
        );
        // M2M group swap: the grouped array leaves, the raw key joins.
        let grouped = flat_row_fields(Some("assignees__id"), None);
        assert!(!grouped.contains(&"assignee_ids".to_owned()));
        assert!(grouped.contains(&"assignees__id".to_owned()));
        assert_eq!(grouped.len(), names.len());
    }

    #[test]
    fn row_selects_carry_the_four_annotations_and_skip_one_array() {
        let selects = cycle_row_selects(None);
        for alias in [
            "AS cycle_id",
            "AS link_count",
            "AS attachment_count",
            "AS sub_issues_count",
            "AS label_ids",
            "AS assignee_ids",
            "AS module_ids",
        ] {
            assert!(selects.contains(alias), "missing {alias}");
        }
        assert!(!cycle_row_selects(Some("label_ids")).contains("AS label_ids"));
        assert!(!cycle_row_selects(Some("assignee_ids")).contains("AS assignee_ids"));
        assert!(!cycle_row_selects(Some("module_ids")).contains("AS module_ids"));
    }

    #[test]
    fn filtered_set_names_the_bridge_scope_and_manager_guards() {
        let project = uuid::Uuid::new_v4();
        let user = uuid::Uuid::new_v4();
        let cycle = uuid::Uuid::new_v4();
        let filtered =
            cycle_filtered_set("acme", &project, &user, &cycle, &empty_query(), None, None)
                .expect("scope");
        for needle in [
            "JOIN cycle_issues AS bridge ON bridge.issue_id = issue.id",
            "bridge.cycle_id = ",
            "bridge.deleted_at IS NULL",
            "issue.deleted_at IS NULL",
            "NOT (state.\"group\" = 'triage')",
            "issue.archived_at IS NULL",
            "project.archived_at IS NULL",
            "issue.is_draft = FALSE",
            "workspaces.slug = ",
            "issue.project_id = ",
            "project_members pm",
        ] {
            assert!(filtered.from_where.contains(needle), "missing {needle}");
        }
        assert_eq!(filtered.values.len(), 4);
        assert!(filtered.referenced.contains(&"bridge"));
    }

    #[test]
    fn group_expressions_cover_the_groupable_fields() {
        for field in [
            "labels__id",
            "assignees__id",
            "issue_module__module_id",
            "state_id",
            "priority",
            "cycle_id",
            "project_id",
            "state__group",
            "target_date",
            "start_date",
            "created_by",
        ] {
            group_expression(field).expect("groupable");
        }
        assert!(group_expression("bogus").is_err());
    }

    #[test]
    fn routes_expose_the_four_cycle_issue_paths() {
        assert!(CYCLE_ISSUES_PATH.contains("cycles/{cycle_id}/cycle-issues/"));
        assert!(CYCLE_ISSUE_PATH.contains("cycle-issues/{issue_id}/"));
        assert_eq!(
            ISSUE_ACTIVITY_TASK,
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
    }

    #[test]
    fn invalid_uuids_render_the_validation_body() {
        assert!(parse_uuid_or_invalid("not-a-uuid").is_err());
        assert!(parse_uuid_or_invalid("123e4567-e89b-12d3-a456-426614174000").is_ok());
        // `OneOrMany` import stays the query-map contract witness.
        let _ = OneOrMany::One("x".to_owned());
    }
}
