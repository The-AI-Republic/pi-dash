#![forbid(unsafe_code)]

//! Issue archive handlers (D-26, stage 5, PIDASHCONV-656).
//!
//! Ports `app/views/issue/archive.py` (2 units, 344 lines):
//!
//! - `GET archived-issues/` (`IssueArchiveViewSet.list`, `:106-219`):
//!   the pilot-2 list machinery over the archive scope (non-epic,
//!   archived, this project/slug) with the archive annotations.
//! - `GET issues/<pk>/archive/` (`retrieve`, `:221-255`): the 28-key
//!   [`ISSUE_DETAIL_ARCHIVE_FIELDS`] detail (only `is_subscribed`
//!   annotated).
//! - `POST issues/<pk>/archive/` (`archive`, `:257-279`): the
//!   completed/cancelled gate, `issue_activity` enqueue, then `save()`
//!   with its recomputes.
//! - `DELETE issues/<pk>/archive/` (`unarchive`, `:281-303`): enqueue,
//!   clear, `save()`, 204.
//! - `POST bulk-archive-issues/` (`BulkArchiveIssuesEndpoint.post`,
//!   `:306-344`): per-issue gate + enqueue, one `bulk_update`.
//!
//! Every other method on those paths proxies to Django (route table in
//! `super::routes`); `HEAD` rides axum's `get` handling on the two
//! GET-owned paths like Django's GET-backed `HEAD`.
//!
//! Reuse (call, don't copy): the `super::` list kernels (filter stacks,
//! order, paginators, shaping), `queries_core` archive SQL, the 639
//! detail rendering, the 594 signal entries, the 561 blocker lists,
//! `sync_description_stripped`, and the jobs Celery enqueue.
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * The grouped `count_filter` counts *non*-archived rows
//!   (`archived_at__isnull=True` inside the archived list).
//! * `group_by`/`sub_group_by` of `target_date`, `start_date` or
//!   `created_by` always 500s (archive passes no `queryset` to
//!   `issue_group_values`, so `None.values_list` raises).
//! * Bulk-archive enqueues for the issues *before* the first bad one,
//!   then answers 400 (partial side effects on failure).
//! * Re-archiving answers 404 (`issue_objects` excludes archived rows).
//! * Archiving a NULL-state issue 500s (`None.group`); unarchiving one
//!   assigns the default state instead.
//! * `save()` recomputes `completed_at` from the state group on every
//!   archive/unarchive (clobbering it on completed issues) and always
//!   rewrites `updated_at` + `description_stripped`; `bulk_update`
//!   touches `archived_at` only.
//! * The bulk response date is recomputed after the loop (this port
//!   stamps one date; the two differ only across a midnight rollover).
//!
//! # Fixture deltas (fixtures untouched — outside this issue's paths)
//!
//! * FX-ISS-19 says archive-retrieve returns the 35-key shape; it is 28
//!   keys (only `is_subscribed` annotated — the 639 finding, pinned by
//!   `ARCHIVE_DETAIL_KEYS` in `test_archive.py`).
//! * FX-ISS-19 says `current_instance` is the 29-key shape; the enqueue
//!   serializes a bare instance, so DRF omits the 7 annotation keys (22
//!   keys — the pilot-2 `SERIALIZER_FIELDS` rule, verified live).
//! * FX-ISS-19 omits the `save()` recomputes (`completed_at`,
//!   `updated_at`, `description_stripped`, NULL-state default).
//!
//! # Known gaps (precedent: pilot-2 records the same)
//!
//! * `?expand=` on archive retrieve only *adds* nested keys; the nested
//!   expansion fleet is out of scope, so the base 28-key shape renders
//!   (the contract suite never sends `expand` here).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;
use std::io::Write;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use chrono_tz::Tz;
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::Row;

use pidash_auth::permissions::membership::ProjectRoleFacts;
use pidash_db::app_pages::strip::sync_description_stripped;
use pidash_db::tasks_ticker::models::issue_agent_ticker::{
    IssueAgentTicker, COLUMNS as TICKER_COLUMNS, TABLE as TICKER_TABLE,
};
use pidash_services::app_issues::{
    issue_detail_base_to_representation, issue_detail_to_representation, issue_is_actively_synced,
    on_results_fields, order_sql, raw_group_mismatch, serialize_drf_datetime,
    serialize_iso_datetime, AgentLiveStateRow, AgentRunDetailRow, AgentTickerInput,
    IssueDetailBaseRow, IssueDetailRow, ListParams, ParseOptions, GITHUB_ISSUE_SYNC_PROBE_SQL,
    GIT_ISSUE_SYNC_PROBE_SQL,
};
use pidash_services::dispatch::policy::UserFlags;
use pidash_services::orchestration::blockers::{
    has_open_blockers_sql, relations_summary, summary_sql, BlockerRow,
};
use pidash_services::orchestration::clock::ProjectClockPolicy;
use pidash_services::orchestration::creation::{
    AdmissionError, ExecutionFields, IssueView, LockedIssue, NewAgentRun, PodView, ProjectView,
    RenderBundle, RenderedTurn, RunView, RunnerView, StateView,
};
use pidash_services::orchestration::creation::{
    CreationError, CreationSeam, ExecutionError, ExecutionRequest, FinalizeAgentRunSeam,
    STATE_SELECT_SQL,
};
use pidash_services::orchestration::entries::{
    capture_prior_state, fire_state_transition, EntriesSeam, FireOutcome, FireRequest,
    PreflightSeam, CLOCK_POLICY_SQL, PRIOR_STATE_SELECT_SQL,
};
use pidash_services::prompting::composer::OverrideRow;
use pidash_types::dispatch::AgentExecutorKind;
use pidash_types::orchestration::StateRef;

use super::queries_core::{archive_retrieve_sql, archive_scope_where, ARCHIVE_JOINS};
use super::{
    actor_user_id, alias_nullable_only, annotation_selects, complex_filter, denial_from_param,
    fetch_json_rows, flat_paginated_response, group_join_alias, grouped_response, json_response,
    legacy_sql, multi_map, order_key, query_last, render_condition, resolve_project_id, Binder,
    Denial, FilteredSet, Gate, HandlerResult, ListContext, QueryMap, RELATION_JOINS,
};
use crate::state::AppState;
use crate::v1_cycles_modules::json_cpython::{
    parse_request_bytes, JVal, JsonFail, JSON_PARSE_PREFIX,
};

/// Archived collection path in `app/urls/issue.py:247-249` form.
pub const ARCHIVED_ISSUES_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/archived-issues/";
/// Archive detail path in `app/urls/issue.py:252-254` form.
pub const ISSUE_ARCHIVE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/archive/";
/// Bulk path in `app/urls/issue.py:102-104` form.
pub const BULK_ARCHIVE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/bulk-archive-issues/";

/// `issue_activity` Celery wire name (bare `@shared_task` default).
pub const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";

/// `archive()` state-gate body (`archive.py:262`).
const ARCHIVE_STATE_MESSAGE: &str = "Can only archive completed or cancelled state group issue";
/// `BulkArchiveIssuesEndpoint` empty-list body (`archive.py:314`).
const IDS_REQUIRED_MESSAGE: &str = "Issue IDs are required";
/// `ERROR_CODES["INVALID_ARCHIVE_STATE_GROUP"]` (`utils/error_codes.py:7`).
const INVALID_ARCHIVE_STATE_GROUP_CODE: i64 = 4091;
/// DRF's default `PermissionDenied` body (the bulk `ProjectEntityPermission`
/// denial carries no custom message).
const ENTITY_DENIED_BODY: &str =
    r#"{"detail":"You do not have permission to perform this action."}"#;
/// `handle_exception`'s `ValidationError` message (bad UUID in `issue_ids`).
const INVALID_DETAIL_MESSAGE: &str = "Please provide valid detail";
/// `issue.save()` on archive: the `archived_at` write plus every
/// recompute — `completed_at` from the state group, `updated_at`, the
/// stripped description, and `updated_by_id` from crum
/// (`BaseModel.save` sets it to the requesting user on every update).
const ARCHIVE_UPDATE_SQL: &str = "UPDATE issues SET archived_at = $1, completed_at = $2, \
    updated_at = $3, description_stripped = $4, updated_by_id = $5 WHERE id = $6";
/// `issue.save()` on unarchive: the same recomputes plus the `state_id`
/// write (the NULL-state default assignment rides the same statement).
const UNARCHIVE_UPDATE_SQL: &str = "UPDATE issues SET archived_at = NULL, completed_at = $1, \
    updated_at = $2, description_stripped = $3, state_id = $4, updated_by_id = $5 WHERE id = $6";

/// Session auth + `_rewrite_project_kwarg` + `@allow_permission([ADMIN,
/// MEMBER])`, in Django's `initial()` order (rewrite, then the permission
/// checks, then `TimezoneMixin` activation). None of the archive endpoints
/// checks project existence (`Project.objects.get` never runs here), so
/// unlike [`super::resolve_gate`] there is no 404 for a missing project
/// row — only the membership 403 — and no guest scoping (guests are
/// denied). The tenant facts resolve after the allow check, so an unknown
/// slug answers 403, never 500.
async fn archive_context(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<ArchiveContext, Denial> {
    let base = archive_base(state, slug, project_raw, extension).await?;
    archive_allow(&base.pool, &base.slug, &base.project_id, &base.user_id).await?;
    archive_tenant(base).await
}

/// Session auth + rewrite without the role check, so the bulk endpoint
/// can run `ProjectEntityPermission` first (Django checks
/// `permission_classes` before the view body's decorator).
async fn archive_base(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<ArchiveBase, Denial> {
    let pool = state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    Ok(ArchiveBase {
        pool,
        slug: slug.to_owned(),
        project_id,
        user_id,
    })
}

/// The tenant facts for an allowed request: the render timezone plus the
/// workspace row (see [`workspace_id`]).
async fn archive_tenant(base: ArchiveBase) -> Result<ArchiveContext, Denial> {
    let timezone = user_timezone(&base.pool, &base.user_id).await?;
    let workspace_id = workspace_id(&base.pool, &base.slug).await?;
    Ok(ArchiveContext {
        pool: base.pool,
        gate: Gate {
            user_id: base.user_id,
            timezone,
            workspace_id,
            project_id: base.project_id,
            guest_scoped: false,
        },
        slug: base.slug,
    })
}

/// Session auth + the project-kwarg rewrite, before any permission or
/// tenant lookup.
struct ArchiveBase {
    pool: sqlx::PgPool,
    slug: String,
    project_id: uuid::Uuid,
    user_id: uuid::Uuid,
}

/// The authenticated archive request: pool, gate facts, tenant slug.
struct ArchiveContext {
    pool: sqlx::PgPool,
    gate: Gate,
    slug: String,
}

/// `@allow_permission([ADMIN, MEMBER])` at `PROJECT` level: an active
/// project membership with role 20/15, or any active project membership
/// plus an active workspace ADMIN membership — else the allow-style 403.
async fn archive_allow(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
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
    match role {
        Some((20,)) | Some((15,)) => Ok(()),
        _ => {
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
    }
}

/// `ProjectEntityPermission` for the bulk POST (non-safe method): an
/// active project membership with role 20/15 — no workspace-admin
/// fallback.
async fn entity_allowed(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let allowed: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.role IN (20, 15) AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(allowed.is_some())
}

/// The actor's render timezone (`TimezoneMixin` activation; a bad zone or
/// a missing user row is a 500).
async fn user_timezone(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// The tenant workspace id. The passing gate implies the row exists (the
/// membership check joins it), so a miss is a 500, never a 404.
async fn workspace_id(pool: &sqlx::PgPool, slug: &str) -> Result<uuid::Uuid, Denial> {
    let row: Option<(uuid::Uuid,)> = sqlx::query_as("SELECT id FROM workspaces WHERE slug = $1")
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// A non-200 JSON response with the exact body.
fn status_response(status: StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("status response")
}

/// JSON-escape one string value.
fn json_detail(value: &str) -> String {
    serde_json::to_string(value).expect("detail string")
}

/// The 204 with no body (`DELETE` unarchive).
fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(Vec::new()))
        .expect("empty response")
}

// ---------------------------------------------------------------------------
// Archived list (`archive.py:106-219`)
// ---------------------------------------------------------------------------

/// `GET .../archived-issues/`: the pilot-2 list flow over the archive
/// scope — strict `per_page`, the group mismatch guard, rich + legacy
/// filters, `order_issue_queryset`, then the flat or grouped paginator.
pub async fn archived_list(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let context = archive_context(&state, &slug, &project_id, extension).await?;
    // Main-list preamble order: the raw mismatch precedes `per_page`
    // parsing, and the parsed mismatch follows it.
    let multi = multi_map(&query);
    let group_by = multi
        .get("group_by")
        .and_then(|values| values.last().map(String::as_str));
    let sub_group_by = multi
        .get("sub_group_by")
        .and_then(|values| values.last().map(String::as_str));
    if let Some(mismatch) = raw_group_mismatch(group_by, sub_group_by) {
        return Err(denial_from_param(mismatch));
    }
    let options = ParseOptions {
        require_issues: false,
        strict_per_page: true,
    };
    let params = ListParams::parse_with(&multi_map(&query), options).map_err(denial_from_param)?;
    if let Some(mismatch) = params.group_mismatch() {
        return Err(Denial::BadError(mismatch.message));
    }
    let show_sub_issues = query_last(&query, "show_sub_issues").as_deref() == Some("true")
        || query_last(&query, "show_sub_issues").is_none();
    let filtered = archive_filtered_set(
        &context.gate,
        &context.slug,
        &query,
        params.group_by.as_deref(),
        params.sub_group_by.as_deref(),
        show_sub_issues,
    )?;
    let order_spec = order_sql(&params.order_by, "state.\"group\"", |name| {
        format!("min_{}", name.replace("__", "_"))
    });
    let (key_expr, descending) = order_key(&order_spec.out_param, &params.order_by)?;
    let direction = if descending { "DESC" } else { "ASC" };
    let per_page = params.per_page;
    let cursor = crate::paginator::Cursor::from_string(&params.cursor_raw)
        .map_err(|error| Denial::BadDetail(error.detail()))?;
    let list_context = ListContext {
        gate: context.gate,
        params,
        pool: context.pool,
        slug: context.slug,
    };
    let group_by = list_context.params.group_by.clone();
    let sub_group_by = list_context.params.sub_group_by.clone();
    // Ported crash: archive passes no `queryset` to `issue_group_values`,
    // so these three group fields raise on `None.values_list` (500).
    for group in [&group_by, &sub_group_by].into_iter().flatten() {
        if matches!(group.as_str(), "target_date" | "start_date" | "created_by") {
            return Err(Denial::ServerError);
        }
    }
    if let Some(group) = group_by.clone() {
        return grouped_response(
            &list_context,
            &filtered,
            &group,
            sub_group_by.clone(),
            &key_expr,
            direction,
            per_page,
            cursor,
        )
        .await;
    }
    let selects = annotation_selects(true, None, false, true, false);
    let fields = on_results_fields(None, None);
    flat_paginated_response(
        &list_context,
        &filtered,
        &key_expr,
        direction,
        per_page,
        cursor,
        selects,
        fields,
        false,
    )
    .await
}

/// The filtered archived set: [`archive_scope_where`] plus the
/// `show_sub_issues` top-level filter, then the same rich + legacy
/// stacks and relation joins as [`super::filtered_set`]. The `states`
/// join serves `state__group` grouping/ordering with Django's auto-join
/// semantics (no soft-delete guard); there is no `projects` join (the
/// archive scope never filters on it, and no filter leaf references it).
fn archive_filtered_set(
    gate: &Gate,
    slug: &str,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
    show_sub_issues: bool,
) -> Result<FilteredSet, Denial> {
    let mut binder = Binder::new();
    let mut preamble = archive_scope_where(&mut binder, slug, gate.project_id);
    if !show_sub_issues {
        preamble.push_str(" AND issue.parent_id IS NULL");
    }
    let mut fragments: Vec<String> = vec![preamble];
    let mut complex_sql = String::new();
    if let Some(cond) = complex_filter(query)? {
        let (fragment, values) = render_condition(&cond);
        complex_sql = binder.splice(&fragment, values);
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
    if !complex_sql.is_empty() {
        fragments.push(complex_sql);
    }
    if !legacy_sql_text.is_empty() {
        fragments.push(legacy_sql_text);
    }
    let where_sql = fragments.join(" AND ");
    let mut joins = String::new();
    let mut referenced = Vec::new();
    for (table, alias, key_column) in RELATION_JOINS {
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
        let _ = key_column;
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
        "{ARCHIVE_JOINS} \
        LEFT JOIN states AS state ON state.id = issue.state_id \
        {joins} \
        WHERE {where_sql}"
    );
    Ok(FilteredSet {
        from_where,
        values: binder.values(),
        referenced,
    })
}

// ---------------------------------------------------------------------------
// Archive retrieve (`archive.py:221-255`)
// ---------------------------------------------------------------------------

/// `GET .../issues/<pk>/archive/`: the 28-key detail. The `expand` param
/// parses (comma-split, empties dropped) but only ever *adds* nested keys,
/// so the base shape below is what every request renders; the nested
/// expansion fleet is the documented gap. The reaction/link prefetches
/// run no queries here: no detail field reads them.
pub async fn archive_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    // Non-UUID tails never match Django's `<uuid:pk>` converter: proxy
    // (Django 404s before auth — the labels-sibling pattern).
    let Ok(issue_id) = pk_raw.parse::<uuid::Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let context = archive_context(&state, &slug, &project_raw, extension).await?;
    let _expand: Option<Vec<String>> = query_last(&query, "expand").map(|raw| {
        raw.split(',')
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect()
    });
    let mut binder = Binder::new();
    let sql = archive_retrieve_sql(
        &mut binder,
        &slug,
        context.gate.project_id,
        issue_id,
        context.gate.user_id,
    );
    let rows = fetch_json_rows(&context.pool, &sql, binder.values()).await?;
    let Some(row) = rows.into_iter().next() else {
        return Err(Denial::NotFound);
    };
    let owned = DetailOwned::fetch(&context.pool, &row).await?;
    let input = owned.view();
    let view = issue_detail_to_representation(&input);
    let body = serde_json::to_string(&view).map_err(|_| Denial::ServerError)?;
    Ok(json_response(body))
}

/// Read a nullable text column out of a `row_to_json` map.
fn json_str<'m>(row: &'m Map<String, Value>, key: &str) -> Option<&'m str> {
    row.get(key).and_then(Value::as_str)
}

/// Read a required text column (a miss is a 500 — the column is selected).
fn json_str_required(row: &Map<String, Value>, key: &str) -> Result<String, Denial> {
    json_str(row, key)
        .map(str::to_owned)
        .ok_or(Denial::ServerError)
}

/// Read an optional text column.
fn json_str_opt(row: &Map<String, Value>, key: &str) -> Option<String> {
    json_str(row, key).map(str::to_owned)
}

/// Read a required float column.
fn json_f64(row: &Map<String, Value>, key: &str) -> Result<f64, Denial> {
    row.get(key)
        .and_then(Value::as_f64)
        .ok_or(Denial::ServerError)
}

/// Read a required int column.
fn json_i32(row: &Map<String, Value>, key: &str) -> Result<i32, Denial> {
    let raw = row
        .get(key)
        .and_then(Value::as_i64)
        .ok_or(Denial::ServerError)?;
    i32::try_from(raw).map_err(|_| Denial::ServerError)
}

/// Read a required bool column.
fn json_bool(row: &Map<String, Value>, key: &str) -> Result<bool, Denial> {
    row.get(key)
        .and_then(Value::as_bool)
        .ok_or(Denial::ServerError)
}

/// Render an optional DRF datetime column (`row_to_json` RFC 3339 in,
/// `Z`-suffixed DRF out).
fn json_drf_opt(row: &Map<String, Value>, key: &str) -> Result<Option<String>, Denial> {
    json_str(row, key)
        .map(|text| {
            chrono::DateTime::parse_from_rfc3339(text)
                .map(|aware| serialize_drf_datetime(aware.with_timezone(&chrono::Utc)))
                .map_err(|_| Denial::ServerError)
        })
        .transpose()
}

/// Render a required DRF datetime column.
fn json_drf(row: &Map<String, Value>, key: &str) -> Result<String, Denial> {
    json_drf_opt(row, key)?.ok_or(Denial::ServerError)
}

/// Everything the 28-key render borrows: the base scalars plus the
/// ticker/policy/state/run/blocker inputs, all owned.
struct DetailOwned {
    id: String,
    name: String,
    state_id: Option<String>,
    sort_order: f64,
    completed_at: Option<String>,
    estimate_point: Option<String>,
    priority: String,
    complexity_score: i32,
    start_date: Option<String>,
    target_date: Option<String>,
    sequence_id: i32,
    project_id: String,
    parent_id: Option<String>,
    assigned_pod_id: Option<String>,
    agent_executor: Option<String>,
    created_at: String,
    updated_at: String,
    created_by: Option<String>,
    updated_by: Option<String>,
    is_draft: bool,
    archived_at: Option<String>,
    is_synced: bool,
    description_html: String,
    is_subscribed: bool,
    ticker: Option<IssueAgentTicker>,
    policy: ProjectClockPolicy,
    state_name: Option<String>,
    state_group: Option<String>,
    latest_run: Option<OwnedRun>,
    active_run: Option<OwnedRun>,
    run_count: i64,
    blockers: pidash_services::orchestration::blockers::RelationsSummary,
}

/// An agent-run row with its joined runner/live facts, all owned.
struct OwnedRun {
    id: String,
    status: String,
    executor_kind: String,
    queue_position: Option<i16>,
    runner_id: Option<String>,
    runner_name: Option<String>,
    created_at: String,
    assigned_at: Option<String>,
    started_at: Option<String>,
    ended_at: Option<String>,
    done_payload: Option<Value>,
    error: String,
    error_code: String,
    llm_model: String,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    live_state: Option<OwnedLiveState>,
}

/// A joined live-state row, all owned.
struct OwnedLiveState {
    observed_run_id: Option<String>,
    last_event_at: Option<String>,
    last_event_kind: Option<String>,
    last_event_summary: Option<String>,
    agent_pid: Option<i32>,
    agent_subprocess_alive: Option<bool>,
    approvals_pending: Option<i32>,
    usage: Value,
    llm_model: Option<String>,
    turn_count: Option<i32>,
    updated_at: String,
}

impl DetailOwned {
    /// Fetch every detail input for one archived-issue row map: the base
    /// scalars inline, then the ticker row, clock policy, state, runs and
    /// blocker lists.
    async fn fetch(pool: &sqlx::PgPool, row: &Map<String, Value>) -> Result<Self, Denial> {
        let id = json_str_required(row, "id")?;
        let issue_id = id.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
        let state_id = json_str_opt(row, "state_id");
        let is_synced = fetch_is_synced(pool, issue_id, json_str(row, "external_source")).await?;
        let ticker = fetch_ticker(pool, issue_id).await?;
        let project_id = json_str_required(row, "project_id")?;
        let project_uuid = project_id
            .parse::<uuid::Uuid>()
            .map_err(|_| Denial::ServerError)?;
        let policy = fetch_clock_policy(pool, project_uuid).await?;
        let (state_name, state_group) = match state_id.as_deref() {
            None => (None, None),
            Some(raw) => {
                let state_uuid = raw.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
                let found = fetch_state(pool, state_uuid).await?;
                match found {
                    None => return Err(Denial::ServerError),
                    Some((name, group)) => (Some(name), Some(group)),
                }
            }
        };
        let latest_run = fetch_agent_run(pool, true, issue_id).await?;
        let active_run = fetch_agent_run(pool, false, issue_id).await?;
        let run_count = fetch_run_count(pool, issue_id).await?;
        let blockers = fetch_blockers(pool, issue_id).await?;
        Ok(Self {
            id,
            name: json_str_required(row, "name")?,
            state_id,
            sort_order: json_f64(row, "sort_order")?,
            completed_at: json_drf_opt(row, "completed_at")?,
            estimate_point: json_str_opt(row, "estimate_point_id"),
            priority: json_str_required(row, "priority")?,
            complexity_score: json_i32(row, "complexity_score")?,
            start_date: json_str_opt(row, "start_date"),
            target_date: json_str_opt(row, "target_date"),
            sequence_id: json_i32(row, "sequence_id")?,
            project_id,
            parent_id: json_str_opt(row, "parent_id"),
            assigned_pod_id: json_str_opt(row, "assigned_pod_id"),
            agent_executor: json_str_opt(row, "agent_executor"),
            created_at: json_drf(row, "created_at")?,
            updated_at: json_drf(row, "updated_at")?,
            created_by: json_str_opt(row, "created_by_id"),
            updated_by: json_str_opt(row, "updated_by_id"),
            is_draft: json_bool(row, "is_draft")?,
            archived_at: json_str_opt(row, "archived_at"),
            is_synced,
            description_html: json_str_required(row, "description_html")?,
            is_subscribed: json_bool(row, "is_subscribed")?,
            ticker,
            policy,
            state_name,
            state_group,
            latest_run,
            active_run,
            run_count,
            blockers,
        })
    }

    /// Borrow the render input: the seven annotation fields stay `None`
    /// (unannotated → omitted) and `is_intake` stays `None`, which is the
    /// 28-key archive shape.
    fn view(&self) -> IssueDetailRow<'_> {
        let base = IssueDetailBaseRow {
            id: &self.id,
            name: &self.name,
            state_id: self.state_id.as_deref(),
            sort_order: self.sort_order,
            completed_at: self.completed_at.as_deref(),
            estimate_point: self.estimate_point.as_deref(),
            priority: &self.priority,
            complexity_score: self.complexity_score,
            start_date: self.start_date.as_deref(),
            target_date: self.target_date.as_deref(),
            sequence_id: self.sequence_id,
            project_id: &self.project_id,
            parent_id: self.parent_id.as_deref(),
            cycle_id: None,
            assigned_pod_id: self.assigned_pod_id.as_deref(),
            agent_executor: self.agent_executor.as_deref(),
            module_ids: None,
            label_ids: None,
            assignee_ids: None,
            sub_issues_count: None,
            created_at: &self.created_at,
            updated_at: &self.updated_at,
            created_by: self.created_by.as_deref(),
            updated_by: self.updated_by.as_deref(),
            attachment_count: None,
            link_count: None,
            is_draft: self.is_draft,
            archived_at: self.archived_at.as_deref(),
            is_synced: self.is_synced,
        };
        let state = match (self.state_group.as_deref(), self.state_name.as_deref()) {
            (Some(group), Some(name)) => Some(StateRef { group, name }),
            _ => None,
        };
        IssueDetailRow {
            base,
            description_html: &self.description_html,
            is_subscribed: self.is_subscribed,
            is_intake: None,
            ticker: self.ticker.as_ref().map(|ticker| AgentTickerInput {
                ticker,
                policy: &self.policy,
                state,
            }),
            latest_run: self.latest_run.as_ref().map(OwnedRun::view),
            active_run: self.active_run.as_ref().map(OwnedRun::view),
            run_count: self.run_count,
            blockers: &self.blockers,
        }
    }
}

impl OwnedRun {
    fn view(&self) -> AgentRunDetailRow<'_> {
        AgentRunDetailRow {
            id: &self.id,
            status: &self.status,
            executor_kind: &self.executor_kind,
            queue_position: self.queue_position,
            runner_id: self.runner_id.as_deref(),
            runner_name: self.runner_name.as_deref(),
            created_at: &self.created_at,
            assigned_at: self.assigned_at.as_deref(),
            started_at: self.started_at.as_deref(),
            ended_at: self.ended_at.as_deref(),
            done_payload: self.done_payload.as_ref(),
            error: &self.error,
            error_code: &self.error_code,
            llm_model: &self.llm_model,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            total_tokens: self.total_tokens,
            live_state: self.live_state.as_ref().map(|live| AgentLiveStateRow {
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
            }),
        }
    }
}

/// `_issue_is_actively_synced`: no probes without a source; the git probe
/// first, the github probe only on a git miss (Django's short-circuit).
async fn fetch_is_synced(
    pool: &sqlx::PgPool,
    issue_id: uuid::Uuid,
    external_source: Option<&str>,
) -> Result<bool, Denial> {
    let Some(source) = external_source else {
        return Ok(false);
    };
    if source.is_empty() {
        return Ok(false);
    }
    let git_hit: Option<(i32,)> = sqlx::query_as(GIT_ISSUE_SYNC_PROBE_SQL)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let git_hit = git_hit.is_some();
    let mut github_hit = false;
    if !git_hit {
        let found: Option<(i32,)> = sqlx::query_as(GITHUB_ISSUE_SYNC_PROBE_SQL)
            .bind(issue_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        github_hit = found.is_some();
    }
    Ok(issue_is_actively_synced(
        Some(source),
        None,
        || git_hit,
        || github_hit,
    ))
}

/// `obj.agent_ticker` through `_base_manager` (no soft-delete guard):
/// none without a row, the row with one, a 500 on duplicates (Django's
/// `MultipleObjectsReturned` is uncaught in `get_agent_ticker`).
async fn fetch_ticker(
    pool: &sqlx::PgPool,
    issue_id: uuid::Uuid,
) -> Result<Option<IssueAgentTicker>, Denial> {
    let sql = format!(
        "SELECT {} FROM {TICKER_TABLE} WHERE issue_id = $1",
        TICKER_COLUMNS.join(", ")
    );
    let rows = sqlx::query(&sql)
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if rows.len() > 1 {
        return Err(Denial::ServerError);
    }
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(IssueAgentTicker {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
        created_by_id: row
            .try_get("created_by_id")
            .map_err(|_| Denial::ServerError)?,
        updated_by_id: row
            .try_get("updated_by_id")
            .map_err(|_| Denial::ServerError)?,
        deleted_at: row.try_get("deleted_at").map_err(|_| Denial::ServerError)?,
        issue_id: row.try_get("issue_id").map_err(|_| Denial::ServerError)?,
        used: row.try_get("used").map_err(|_| Denial::ServerError)?,
        granted: row.try_get("granted").map_err(|_| Denial::ServerError)?,
        waited: row.try_get("waited").map_err(|_| Denial::ServerError)?,
        user_disabled: row
            .try_get("user_disabled")
            .map_err(|_| Denial::ServerError)?,
        next_run_at: row
            .try_get("next_run_at")
            .map_err(|_| Denial::ServerError)?,
        last_tick_at: row
            .try_get("last_tick_at")
            .map_err(|_| Denial::ServerError)?,
        enabled: row.try_get("enabled").map_err(|_| Denial::ServerError)?,
        disarm_reason: row
            .try_get("disarm_reason")
            .map_err(|_| Denial::ServerError)?,
        pending_entry: row
            .try_get("pending_entry")
            .map_err(|_| Denial::ServerError)?,
        pending_entry_free: row
            .try_get("pending_entry_free")
            .map_err(|_| Denial::ServerError)?,
        pending_entry_actor_id: row
            .try_get("pending_entry_actor_id")
            .map_err(|_| Denial::ServerError)?,
        pending_entry_trigger: row
            .try_get("pending_entry_trigger")
            .map_err(|_| Denial::ServerError)?,
        resume_parent_run_id: row
            .try_get("resume_parent_run_id")
            .map_err(|_| Denial::ServerError)?,
    }))
}

/// One `CLOCK_POLICY_SQL` row: enabled, max ticks, then the three
/// stage intervals.
type PolicyRow = (
    Option<bool>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
);

/// One summary-statement row plus the ignored full-set flag column.
type BlockerTuple = (uuid::Uuid, i32, String, Option<String>, Option<String>, i32);

/// The project clock-policy columns behind `effective_max_ticks` and the
/// stage interval (reached through the cached `issue.project`: no
/// soft-delete guard, and a missing row is a 500).
async fn fetch_clock_policy(
    pool: &sqlx::PgPool,
    project_id: uuid::Uuid,
) -> Result<ProjectClockPolicy, Denial> {
    let row: Option<PolicyRow> = sqlx::query_as(CLOCK_POLICY_SQL)
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some((enabled, max_ticks, interval, review_interval, test_interval)) = row else {
        return Err(Denial::ServerError);
    };
    Ok(ProjectClockPolicy {
        agent_ticking_enabled: enabled,
        agent_default_max_ticks: max_ticks,
        agent_default_interval_seconds: interval.map(i64::from),
        agent_review_default_interval_seconds: review_interval.map(i64::from),
        agent_test_default_interval_seconds: test_interval.map(i64::from),
    })
}

/// One state row through `_base_manager` (no soft-delete guard).
async fn fetch_state(
    pool: &sqlx::PgPool,
    state_id: uuid::Uuid,
) -> Result<Option<(String, String)>, Denial> {
    let row: Option<(uuid::Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
        .bind(state_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(_, name, group)| (name, group)))
}

/// `LATEST_AGENT_RUN_SQL` / `ACTIVE_AGENT_RUN_SQL`, mapped positionally:
/// both `llm_model` columns share one name, so name lookup cannot tell
/// the run's from the live row's. Positions follow the consts' select
/// order (28 columns; anything else is a 500). A NULL live `updated_at`
/// means the join missed, so there is no live row.
async fn fetch_agent_run(
    pool: &sqlx::PgPool,
    latest: bool,
    issue_id: uuid::Uuid,
) -> Result<Option<OwnedRun>, Denial> {
    use pidash_services::app_issues::{ACTIVE_AGENT_RUN_SQL, LATEST_AGENT_RUN_SQL};
    let sql = if latest {
        LATEST_AGENT_RUN_SQL
    } else {
        ACTIVE_AGENT_RUN_SQL
    };
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(sql)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.len() != 28 {
        return Err(Denial::ServerError);
    }
    let id: uuid::Uuid = row.try_get(0).map_err(|_| Denial::ServerError)?;
    let status: String = row.try_get(1).map_err(|_| Denial::ServerError)?;
    let executor_kind: String = row.try_get(2).map_err(|_| Denial::ServerError)?;
    let queue_position: Option<i16> = row.try_get(3).map_err(|_| Denial::ServerError)?;
    let runner_id: Option<uuid::Uuid> = row.try_get(4).map_err(|_| Denial::ServerError)?;
    let created_at: chrono::DateTime<chrono::Utc> =
        row.try_get(5).map_err(|_| Denial::ServerError)?;
    let assigned_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get(6).map_err(|_| Denial::ServerError)?;
    let started_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get(7).map_err(|_| Denial::ServerError)?;
    let ended_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get(8).map_err(|_| Denial::ServerError)?;
    let done_payload: Option<Value> = row.try_get(9).map_err(|_| Denial::ServerError)?;
    let error: String = row.try_get(10).map_err(|_| Denial::ServerError)?;
    let error_code: String = row.try_get(11).map_err(|_| Denial::ServerError)?;
    let llm_model: String = row.try_get(12).map_err(|_| Denial::ServerError)?;
    let input_tokens: Option<i64> = row.try_get(13).map_err(|_| Denial::ServerError)?;
    let output_tokens: Option<i64> = row.try_get(14).map_err(|_| Denial::ServerError)?;
    let total_tokens: Option<i64> = row.try_get(15).map_err(|_| Denial::ServerError)?;
    let runner_name: Option<String> = row.try_get(16).map_err(|_| Denial::ServerError)?;
    let observed_run_id: Option<uuid::Uuid> = row.try_get(17).map_err(|_| Denial::ServerError)?;
    let last_event_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get(18).map_err(|_| Denial::ServerError)?;
    let last_event_kind: Option<String> = row.try_get(19).map_err(|_| Denial::ServerError)?;
    let last_event_summary: Option<String> = row.try_get(20).map_err(|_| Denial::ServerError)?;
    let agent_pid: Option<i32> = row.try_get(21).map_err(|_| Denial::ServerError)?;
    let agent_subprocess_alive: Option<bool> = row.try_get(22).map_err(|_| Denial::ServerError)?;
    let approvals_pending: Option<i32> = row.try_get(23).map_err(|_| Denial::ServerError)?;
    let usage: Option<Value> = row.try_get(24).map_err(|_| Denial::ServerError)?;
    let live_llm_model: Option<String> = row.try_get(25).map_err(|_| Denial::ServerError)?;
    let turn_count: Option<i32> = row.try_get(26).map_err(|_| Denial::ServerError)?;
    let live_updated_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get(27).map_err(|_| Denial::ServerError)?;
    Ok(Some(OwnedRun {
        id: id.to_string(),
        status,
        executor_kind,
        queue_position,
        runner_id: runner_id.map(|id| id.to_string()),
        runner_name,
        created_at: serialize_iso_datetime(created_at),
        assigned_at: assigned_at.map(serialize_iso_datetime),
        started_at: started_at.map(serialize_iso_datetime),
        ended_at: ended_at.map(serialize_iso_datetime),
        done_payload,
        error,
        error_code,
        llm_model,
        input_tokens,
        output_tokens,
        total_tokens,
        live_state: live_updated_at.map(|updated_at| OwnedLiveState {
            observed_run_id: observed_run_id.map(|id| id.to_string()),
            last_event_at: last_event_at.map(serialize_iso_datetime),
            last_event_kind,
            last_event_summary,
            agent_pid,
            agent_subprocess_alive,
            approvals_pending,
            usage: usage.unwrap_or(Value::Null),
            llm_model: live_llm_model,
            turn_count,
            updated_at: serialize_iso_datetime(updated_at),
        }),
    }))
}

/// `AGENT_RUN_COUNT_SQL`.
async fn fetch_run_count(pool: &sqlx::PgPool, issue_id: uuid::Uuid) -> Result<i64, Denial> {
    use pidash_services::app_issues::AGENT_RUN_COUNT_SQL;
    let row: Option<(i64,)> = sqlx::query_as(AGENT_RUN_COUNT_SQL)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// One cached `relations_summary` over the two summary statements plus
/// the full-set open check (the fixture's 3 statements).
async fn fetch_blockers(
    pool: &sqlx::PgPool,
    issue_id: uuid::Uuid,
) -> Result<pidash_services::orchestration::blockers::RelationsSummary, Denial> {
    let blocked_sql = summary_sql(false).replace(":issue_id", "$1");
    let blocking_sql = summary_sql(true).replace(":issue_id", "$1");
    let open_sql = has_open_blockers_sql().replace(":issue_id", "$1");
    let blocked: Vec<BlockerTuple> = sqlx::query_as(&blocked_sql)
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let blocking: Vec<BlockerTuple> = sqlx::query_as(&blocking_sql)
        .bind(issue_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let open: Option<(i32,)> = sqlx::query_as(&open_sql)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let blocked: Vec<BlockerRow> = blocked
        .into_iter()
        .map(
            |(issue_id, sequence_id, project_identifier, state_name, state_group, _)| BlockerRow {
                issue_id,
                sequence_id,
                project_identifier,
                state_name,
                state_group,
            },
        )
        .collect();
    let blocking: Vec<BlockerRow> = blocking
        .into_iter()
        .map(
            |(issue_id, sequence_id, project_identifier, state_name, state_group, _)| BlockerRow {
                issue_id,
                sequence_id,
                project_identifier,
                state_name,
                state_group,
            },
        )
        .collect();
    Ok(relations_summary(&blocked, &blocking, open.is_some()))
}

// ---------------------------------------------------------------------------
// Archive / unarchive (`archive.py:257-303`)
// ---------------------------------------------------------------------------

/// One issue row for the write paths: the base scalars plus the state
/// group for the gate and the save() recomputes.
struct WriteRow {
    id: uuid::Uuid,
    name: String,
    state_id: Option<uuid::Uuid>,
    group: Option<String>,
    sort_order: f64,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    estimate_point: Option<String>,
    priority: String,
    complexity_score: i32,
    start_date: Option<String>,
    target_date: Option<String>,
    sequence_id: i32,
    project_id: uuid::Uuid,
    parent_id: Option<String>,
    assigned_pod_id: Option<String>,
    agent_executor: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    created_by: Option<String>,
    updated_by: Option<String>,
    is_draft: bool,
    archived_at: Option<String>,
    external_source: Option<String>,
    description_html: String,
}

/// `POST .../issues/<pk>/archive/`: the gate, the enqueue (before the
/// save, over the unsaved row), the `save()` write, the explicit signal
/// pair, then `{"archived_at": str(...)}`.
pub async fn archive_issue(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    // Non-UUID tails never match Django's `<uuid:pk>` converter: proxy
    // (Django 404s before auth — the labels-sibling pattern).
    let Ok(issue_id) = pk_raw.parse::<uuid::Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let context = archive_context(&state, &slug, &project_raw, extension).await?;
    let row =
        fetch_archive_issue(&context.pool, &slug, &context.gate.project_id, &issue_id).await?;
    // `issue.state.group`: NULL state is `AttributeError` (500); a set id
    // with no row is `DoesNotExist` (404).
    let group = match (&row.state_id, &row.group) {
        (None, _) => return Err(Denial::ServerError),
        (Some(_), None) => return Err(Denial::NotFound),
        (Some(_), Some(group)) => group.clone(),
    };
    if group != "completed" && group != "cancelled" {
        return Err(Denial::BadError(ARCHIVE_STATE_MESSAGE.to_owned()));
    }
    let today = chrono::Utc::now().date_naive();
    let origin = activity_origin(&state)?;
    let current_instance = current_instance_json(&context.pool, &row).await?;
    let mut kwargs = Map::new();
    kwargs.insert(
        "type".to_owned(),
        Value::String("issue.activity.updated".to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(format!(
            "{{\"archived_at\": \"{today}\", \"automation\": false}}"
        )),
    );
    kwargs.insert(
        "actor_id".to_owned(),
        Value::String(context.gate.user_id.to_string()),
    );
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(context.gate.project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        Value::String(current_instance),
    );
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(chrono::Utc::now().timestamp().into()),
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin));
    enqueue_activity(&context.pool, kwargs).await;
    // `save()`: the pre-save snapshot, the write with its recomputes,
    // then the explicit post-save fire.
    let mut seam = ArchiveSignalSeam {
        pool: &context.pool,
    };
    let prev = capture_prior_state(&mut seam, Some(issue_id))
        .await
        .map_err(|_| Denial::ServerError)?;
    let now = chrono::Utc::now();
    let completed_at = if group == "completed" {
        Some(now)
    } else {
        None
    };
    let stripped = sync_description_stripped(Some(&row.description_html));
    sqlx::query(ARCHIVE_UPDATE_SQL)
        .bind(today)
        .bind(completed_at)
        .bind(now)
        .bind(stripped)
        .bind(context.gate.user_id)
        .bind(issue_id)
        .execute(&context.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    fire_after_save(
        &context.pool,
        issue_id,
        prev,
        row.state_id,
        Some(context.gate.user_id),
        now,
    )
    .await?;
    Ok(json_response(format!("{{\"archived_at\":\"{today}\"}}")))
}

/// `DELETE .../issues/<pk>/archive/`: the enqueue, the `save()` write
/// (with the NULL-state default assignment), the explicit signal pair,
/// then the empty 204.
pub async fn unarchive_issue(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    // Non-UUID tails never match Django's `<uuid:pk>` converter: proxy
    // (Django 404s before auth — the labels-sibling pattern).
    let Ok(issue_id) = pk_raw.parse::<uuid::Uuid>() else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let context = archive_context(&state, &slug, &project_raw, extension).await?;
    let row =
        fetch_unarchive_issue(&context.pool, &slug, &context.gate.project_id, &issue_id).await?;
    let origin = activity_origin(&state)?;
    let current_instance = current_instance_json(&context.pool, &row).await?;
    let mut kwargs = Map::new();
    kwargs.insert(
        "type".to_owned(),
        Value::String("issue.activity.updated".to_owned()),
    );
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String("{\"archived_at\": null}".to_owned()),
    );
    kwargs.insert(
        "actor_id".to_owned(),
        Value::String(context.gate.user_id.to_string()),
    );
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(context.gate.project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        Value::String(current_instance),
    );
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(chrono::Utc::now().timestamp().into()),
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin));
    enqueue_activity(&context.pool, kwargs).await;
    // `save()` state half: NULL takes the default-state assignment (and
    // skips the `completed_at` recompute); a set id with no row raises
    // `DoesNotExist` (404) — after the enqueue, like the `save()` raise.
    let mut seam = ArchiveSignalSeam {
        pool: &context.pool,
    };
    let prev = capture_prior_state(&mut seam, Some(issue_id))
        .await
        .map_err(|_| Denial::ServerError)?;
    let now = chrono::Utc::now();
    let (state_id, completed_at) = match (&row.state_id, &row.group) {
        (None, _) => {
            let assigned =
                default_state_for_project(&context.pool, &context.gate.project_id).await?;
            (assigned, row.completed_at)
        }
        (Some(_), None) => return Err(Denial::NotFound),
        (Some(id), Some(group)) => {
            let completed_at = if group == "completed" {
                Some(now)
            } else {
                None
            };
            (Some(*id), completed_at)
        }
    };
    let stripped = sync_description_stripped(Some(&row.description_html));
    sqlx::query(UNARCHIVE_UPDATE_SQL)
        .bind(completed_at)
        .bind(now)
        .bind(stripped)
        .bind(state_id)
        .bind(context.gate.user_id)
        .bind(issue_id)
        .execute(&context.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    fire_after_save(
        &context.pool,
        issue_id,
        prev,
        state_id,
        Some(context.gate.user_id),
        now,
    )
    .await?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

/// `Issue.issue_objects.get(workspace, project, pk)`: soft-deleted rows,
/// triage states (NULL states kept), archived rows, archived projects and
/// drafts are all invisible — a miss is the 404. The state group rides a
/// descriptor-parity join (no soft-delete guard).
async fn fetch_archive_issue(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
) -> Result<WriteRow, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT issue.id, issue.name, issue.state_id, s.\"group\" AS state_group, \
         issue.sort_order, issue.completed_at, issue.estimate_point_id, issue.priority, \
         issue.complexity_score, issue.start_date, issue.target_date, issue.sequence_id, \
         issue.project_id, issue.parent_id, issue.assigned_pod_id, issue.agent_executor, \
         issue.created_at, issue.updated_at, issue.created_by_id, issue.updated_by_id, \
         issue.is_draft, issue.archived_at, issue.external_source, issue.description_html \
         FROM issues AS issue \
         JOIN workspaces ON workspaces.id = issue.workspace_id \
         JOIN projects AS project ON project.id = issue.project_id \
         LEFT JOIN states AS s ON s.id = issue.state_id \
         WHERE issue.deleted_at IS NULL \
         AND (s.\"group\" IS NULL OR NOT (s.\"group\" = 'triage')) \
         AND issue.archived_at IS NULL AND project.archived_at IS NULL \
         AND issue.is_draft = FALSE \
         AND issue.project_id = $1 AND workspaces.slug = $2 AND issue.id = $3",
    )
    .bind(project_id)
    .bind(slug)
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(write_row).transpose()?.ok_or(Denial::NotFound)
}

/// `Issue.objects.get(workspace, project, archived, pk)`: the plain
/// soft-deletion scope plus the archived requirement — drafts and triage
/// stay visible.
async fn fetch_unarchive_issue(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
) -> Result<WriteRow, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        "SELECT issue.id, issue.name, issue.state_id, s.\"group\" AS state_group, \
         issue.sort_order, issue.completed_at, issue.estimate_point_id, issue.priority, \
         issue.complexity_score, issue.start_date, issue.target_date, issue.sequence_id, \
         issue.project_id, issue.parent_id, issue.assigned_pod_id, issue.agent_executor, \
         issue.created_at, issue.updated_at, issue.created_by_id, issue.updated_by_id, \
         issue.is_draft, issue.archived_at, issue.external_source, issue.description_html \
         FROM issues AS issue \
         JOIN workspaces ON workspaces.id = issue.workspace_id \
         LEFT JOIN states AS s ON s.id = issue.state_id \
         WHERE issue.deleted_at IS NULL AND issue.archived_at IS NOT NULL \
         AND issue.project_id = $1 AND workspaces.slug = $2 AND issue.id = $3",
    )
    .bind(project_id)
    .bind(slug)
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(write_row).transpose()?.ok_or(Denial::NotFound)
}

/// Map one write-path row.
fn write_row(row: sqlx::postgres::PgRow) -> Result<WriteRow, Denial> {
    let archived_at: Option<chrono::NaiveDate> = row
        .try_get("archived_at")
        .map_err(|_| Denial::ServerError)?;
    let start_date: Option<chrono::NaiveDate> =
        row.try_get("start_date").map_err(|_| Denial::ServerError)?;
    let target_date: Option<chrono::NaiveDate> = row
        .try_get("target_date")
        .map_err(|_| Denial::ServerError)?;
    let estimate_point: Option<uuid::Uuid> = row
        .try_get("estimate_point_id")
        .map_err(|_| Denial::ServerError)?;
    let parent_id: Option<uuid::Uuid> =
        row.try_get("parent_id").map_err(|_| Denial::ServerError)?;
    let assigned_pod_id: Option<uuid::Uuid> = row
        .try_get("assigned_pod_id")
        .map_err(|_| Denial::ServerError)?;
    let created_by: Option<uuid::Uuid> = row
        .try_get("created_by_id")
        .map_err(|_| Denial::ServerError)?;
    let updated_by: Option<uuid::Uuid> = row
        .try_get("updated_by_id")
        .map_err(|_| Denial::ServerError)?;
    Ok(WriteRow {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        name: row.try_get("name").map_err(|_| Denial::ServerError)?,
        state_id: row.try_get("state_id").map_err(|_| Denial::ServerError)?,
        group: row
            .try_get("state_group")
            .map_err(|_| Denial::ServerError)?,
        sort_order: row.try_get("sort_order").map_err(|_| Denial::ServerError)?,
        completed_at: row
            .try_get("completed_at")
            .map_err(|_| Denial::ServerError)?,
        estimate_point: estimate_point.map(|id| id.to_string()),
        priority: row.try_get("priority").map_err(|_| Denial::ServerError)?,
        complexity_score: row
            .try_get("complexity_score")
            .map_err(|_| Denial::ServerError)?,
        start_date: start_date.map(|date| date.to_string()),
        target_date: target_date.map(|date| date.to_string()),
        sequence_id: row
            .try_get("sequence_id")
            .map_err(|_| Denial::ServerError)?,
        project_id: row.try_get("project_id").map_err(|_| Denial::ServerError)?,
        parent_id: parent_id.map(|id| id.to_string()),
        assigned_pod_id: assigned_pod_id.map(|id| id.to_string()),
        agent_executor: row
            .try_get("agent_executor")
            .map_err(|_| Denial::ServerError)?,
        created_at: row.try_get("created_at").map_err(|_| Denial::ServerError)?,
        updated_at: row.try_get("updated_at").map_err(|_| Denial::ServerError)?,
        created_by: created_by.map(|id| id.to_string()),
        updated_by: updated_by.map(|id| id.to_string()),
        is_draft: row.try_get("is_draft").map_err(|_| Denial::ServerError)?,
        archived_at: archived_at.map(|date| date.to_string()),
        external_source: row
            .try_get("external_source")
            .map_err(|_| Denial::ServerError)?,
        description_html: row
            .try_get("description_html")
            .map_err(|_| Denial::ServerError)?,
    })
}

/// `Issue.save()`'s NULL-state branch: the default non-triage state, else
/// any non-triage state (`State.objects`, soft-deleted excluded,
/// `sequence` order) — or `None` when the project has no states.
async fn default_state_for_project(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<Option<uuid::Uuid>, Denial> {
    for default_only in [true, false] {
        let sql = default_state_sql(default_only);
        let row: Option<(uuid::Uuid,)> = sqlx::query_as(&sql)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        if let Some((id,)) = row {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// The default-state lookup: triage is excluded twice over — the
/// `is_triage` flag *and* the `StateManager` `group != 'triage'` rule
/// (real Triage states carry `is_triage = FALSE`, so the flag alone
/// would admit them).
fn default_state_sql(default_only: bool) -> String {
    let extra = if default_only {
        " AND s.\"default\""
    } else {
        ""
    };
    format!(
        "SELECT s.id FROM states AS s WHERE s.project_id = $1 \
         AND s.deleted_at IS NULL AND NOT s.is_triage AND s.\"group\" != 'triage'{extra} \
         ORDER BY s.sequence ASC LIMIT 1"
    )
}

/// `current_instance`: the 22-key bare-instance shape over the pre-save
/// row, `json.dumps` spacing (`", "` / `": "`).
async fn current_instance_json(pool: &sqlx::PgPool, row: &WriteRow) -> Result<String, Denial> {
    let is_synced = fetch_is_synced(pool, row.id, row.external_source.as_deref()).await?;
    let id = row.id.to_string();
    let state_id = row.state_id.map(|id| id.to_string());
    let project_id = row.project_id.to_string();
    let completed_at = row.completed_at.map(serialize_drf_datetime);
    let created_at = serialize_drf_datetime(row.created_at);
    let updated_at = serialize_drf_datetime(row.updated_at);
    let base = IssueDetailBaseRow {
        id: &id,
        name: &row.name,
        state_id: state_id.as_deref(),
        sort_order: row.sort_order,
        completed_at: completed_at.as_deref(),
        estimate_point: row.estimate_point.as_deref(),
        priority: &row.priority,
        complexity_score: row.complexity_score,
        start_date: row.start_date.as_deref(),
        target_date: row.target_date.as_deref(),
        sequence_id: row.sequence_id,
        project_id: &project_id,
        parent_id: row.parent_id.as_deref(),
        cycle_id: None,
        assigned_pod_id: row.assigned_pod_id.as_deref(),
        agent_executor: row.agent_executor.as_deref(),
        module_ids: None,
        label_ids: None,
        assignee_ids: None,
        sub_issues_count: None,
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: row.created_by.as_deref(),
        updated_by: row.updated_by.as_deref(),
        attachment_count: None,
        link_count: None,
        is_draft: row.is_draft,
        archived_at: row.archived_at.as_deref(),
        is_synced,
    };
    let view = issue_detail_base_to_representation(&base);
    to_spaced_json(&view)
}

/// `base_host(request, is_app=True)`: `APP_BASE_URL` when set, else the
/// `WEB_URL or APP_BASE_URL` origin — unset everywhere is a 500
/// (`ImproperlyConfigured`).
fn activity_origin(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    urls.app_base_url
        .clone()
        .or_else(|| urls.web_url.clone())
        .ok_or(Denial::ServerError)
}

/// Best-effort deferred publish of an `issue_activity.delay(...)` call:
/// enqueue failures never change the response.
async fn enqueue_activity(pool: &sqlx::PgPool, kwargs: Map<String, Value>) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(ISSUE_ACTIVITY_TASK, vec![], kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// `json.dumps` separators (`", "` after items, `": "` after keys).
struct SpacedFormatter;

impl serde_json::ser::Formatter for SpacedFormatter {
    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + Write,
    {
        writer.write_all(b": ")
    }

    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }
}

/// Serialize with [`SpacedFormatter`] (non-ASCII stays literal: the queue
/// transport re-encodes anyway, so only the parsed value must match).
fn to_spaced_json<T: Serialize>(value: &T) -> Result<String, Denial> {
    let mut buf = Vec::new();
    let mut ser = serde_json::ser::Serializer::with_formatter(&mut buf, SpacedFormatter);
    value.serialize(&mut ser).map_err(|_| Denial::ServerError)?;
    String::from_utf8(buf).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Bulk archive (`archive.py:306-344`)
// ---------------------------------------------------------------------------

/// `POST .../bulk-archive-issues/`: the entity gate, the id parse, then
/// per issue (model `-created_at` order) the literal gate plus the
/// enqueue — a bad issue answers 400 *after* the earlier enqueues fired
/// — and one `archived_at`-only `bulk_update`.
pub async fn bulk_archive(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::body::Bytes,
) -> HandlerResult {
    let base = archive_base(&state, &slug, &project_raw, extension).await?;
    // The entity check runs before the view body (and its decorator), so
    // its 403 carries DRF's default detail body rather than the allow
    // body.
    if !entity_allowed(&base.pool, &slug, &base.project_id, &base.user_id).await? {
        return Ok(status_response(StatusCode::FORBIDDEN, ENTITY_DENIED_BODY));
    }
    archive_allow(&base.pool, &base.slug, &base.project_id, &base.user_id).await?;
    let context = archive_tenant(base).await?;
    // `request.data`: a zero-length body short-circuits to `{}` (DRF's
    // content-length check, whatever the content-type — the required 400).
    // Anything else parses through the shared CPython-grammar parser, so
    // the reason text is byte-exact (`{oops` and friends); past the depth
    // cap Django's `RecursionError` escapes DRF into the JSON 500.
    let parsed: Option<JVal> = if body.is_empty() {
        None
    } else {
        match parse_request_bytes(&body) {
            Ok(value) => Some(value),
            Err(JsonFail::Message(reason)) => {
                return Ok(status_response(
                    StatusCode::BAD_REQUEST,
                    &parse_error_body(&reason),
                ));
            }
            Err(JsonFail::Recursion) => return Err(Denial::ServerError),
        }
    };
    let ids = parse_bulk_ids(parsed.as_ref())?;
    let today = chrono::Utc::now().date_naive();
    let origin = activity_origin(&state)?;
    let rows = fetch_bulk_issues(&context.pool, &slug, &context.gate.project_id, &ids).await?;
    for row in &rows {
        // `select_related("state")` caches `None` for a NULL *or* dangling
        // id alike, so a missing group is `AttributeError` (500) either
        // way — and the gate below reads the joined row, deleted or not.
        let group = row.group.as_deref().ok_or(Denial::ServerError)?;
        if group != "completed" && group != "cancelled" {
            let body = format!(
                "{{\"error_code\":{INVALID_ARCHIVE_STATE_GROUP_CODE},\
                 \"error_message\":\"INVALID_ARCHIVE_STATE_GROUP\"}}"
            );
            return Ok(status_response(StatusCode::BAD_REQUEST, &body));
        }
        let current_instance = current_instance_json(&context.pool, row).await?;
        let mut kwargs = Map::new();
        kwargs.insert(
            "type".to_owned(),
            Value::String("issue.activity.updated".to_owned()),
        );
        kwargs.insert(
            "requested_data".to_owned(),
            Value::String(format!(
                "{{\"archived_at\": \"{today}\", \"automation\": false}}"
            )),
        );
        kwargs.insert(
            "actor_id".to_owned(),
            Value::String(context.gate.user_id.to_string()),
        );
        kwargs.insert("issue_id".to_owned(), Value::String(row.id.to_string()));
        kwargs.insert(
            "project_id".to_owned(),
            Value::String(context.gate.project_id.to_string()),
        );
        kwargs.insert(
            "current_instance".to_owned(),
            Value::String(current_instance),
        );
        kwargs.insert(
            "epoch".to_owned(),
            Value::Number(chrono::Utc::now().timestamp().into()),
        );
        kwargs.insert("notification".to_owned(), Value::Bool(true));
        kwargs.insert("origin".to_owned(), Value::String(origin.clone()));
        enqueue_activity(&context.pool, kwargs).await;
    }
    if !rows.is_empty() {
        let hit: Vec<uuid::Uuid> = rows.iter().map(|row| row.id).collect();
        sqlx::query("UPDATE issues SET archived_at = $1 WHERE id = ANY($2)")
            .bind(today)
            .bind(&hit)
            .execute(&context.pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    Ok(json_response(format!("{{\"archived_at\":\"{today}\"}}")))
}

/// DRF's `ParseError` body: lowercase `detail`, the `JSON parse error -
/// ` prefix, CPython's reason. (The shared `Denial::BadDetail` renders a
/// capital-D key — out of this diff's paths — so the bulk arm builds its
/// own body.)
fn parse_error_body(reason: &str) -> String {
    format!(
        "{{\"detail\":{}}}",
        json_detail(&format!("{JSON_PARSE_PREFIX}{reason}"))
    )
}

/// `request.data.get("issue_ids", [])`, then `len()` and the `pk__in`
/// lookup. Missing or empty is the required-400; a non-object body has no
/// `.get` (500); `null`, numbers and bools `len()`-raise (500); strings
/// iterate characters (empty is the required-400, anything else fails UUID
/// parsing); objects iterate keys. Array items follow the UUID field:
/// strings parse (else the invalid 400), in-range ints coerce (bools are
/// ints; floats, negatives, huge ints and nested values are the invalid
/// 400 — `ValidationError`, not a crash), `null` matches nothing and is
/// skipped.
fn parse_bulk_ids(body: Option<&JVal>) -> Result<Vec<uuid::Uuid>, Denial> {
    let required = || Denial::BadError(IDS_REQUIRED_MESSAGE.to_owned());
    let invalid = || Denial::BadError(INVALID_DETAIL_MESSAGE.to_owned());
    let Some(body) = body else {
        return Err(required());
    };
    let JVal::Object(map) = body else {
        return Err(Denial::ServerError);
    };
    let raw_ids = map.get("issue_ids").ok_or_else(required)?;
    match raw_ids {
        JVal::Null => Err(Denial::ServerError),
        JVal::Bool(_) | JVal::Num(_) => Err(Denial::ServerError),
        JVal::Str(text) => {
            if text.is_empty() {
                return Err(required());
            }
            Err(invalid())
        }
        JVal::Object(map) => {
            if map.is_empty() {
                return Err(required());
            }
            let mut ids = Vec::with_capacity(map.iter().count());
            for (key, _) in map.iter() {
                let text = key.to_clean_string().ok_or_else(invalid)?;
                ids.push(text.parse::<uuid::Uuid>().map_err(|_| invalid())?);
            }
            Ok(ids)
        }
        JVal::Array(items) => {
            if items.is_empty() {
                return Err(required());
            }
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    JVal::Str(text) => {
                        let text = text.to_clean_string().ok_or_else(invalid)?;
                        ids.push(text.parse::<uuid::Uuid>().map_err(|_| invalid())?);
                    }
                    JVal::Num(number) => {
                        let bits = number.to_u128().or_else(|| {
                            // `-0` is the one int spelling `u128` rejects
                            // that CPython folds to 0 (`UUID(int=0)`).
                            (!number.is_float() && number.is_zero()).then_some(0)
                        });
                        let bits = bits.ok_or_else(invalid)?;
                        ids.push(uuid::Uuid::from_u128(bits));
                    }
                    JVal::Bool(flag) => {
                        ids.push(uuid::Uuid::from_u128(u128::from(*flag)));
                    }
                    JVal::Null => {}
                    JVal::Array(_) | JVal::Object(_) => return Err(invalid()),
                }
            }
            Ok(ids)
        }
    }
}

/// `Issue.objects.filter(workspace, project, pk__in=ids)
/// .select_related("state")`: unknown ids simply miss, drafts/triage/
/// archived stay eligible, model `-created_at` order, the state join
/// unfiltered.
async fn fetch_bulk_issues(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    ids: &[uuid::Uuid],
) -> Result<Vec<WriteRow>, Denial> {
    let rows = sqlx::query(
        "SELECT issue.id, issue.name, issue.state_id, s.\"group\" AS state_group, \
         issue.sort_order, issue.completed_at, issue.estimate_point_id, issue.priority, \
         issue.complexity_score, issue.start_date, issue.target_date, issue.sequence_id, \
         issue.project_id, issue.parent_id, issue.assigned_pod_id, issue.agent_executor, \
         issue.created_at, issue.updated_at, issue.created_by_id, issue.updated_by_id, \
         issue.is_draft, issue.archived_at, issue.external_source, issue.description_html \
         FROM issues AS issue \
         JOIN workspaces ON workspaces.id = issue.workspace_id \
         LEFT JOIN states AS s ON s.id = issue.state_id \
         WHERE issue.deleted_at IS NULL \
         AND issue.project_id = $1 AND workspaces.slug = $2 AND issue.id = ANY($3) \
         ORDER BY issue.created_at DESC",
    )
    .bind(project_id)
    .bind(slug)
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    rows.into_iter().map(write_row).collect()
}

// ---------------------------------------------------------------------------
// Explicit signals (594 entries)
// ---------------------------------------------------------------------------

/// The post-save fire: a non-transition answers `NoTransition` without
/// I/O; a failed handler is logged with the verbatim line (the counter
/// already bumped inside) and the save stands; only a *lookup* storage
/// failure propagates to the 500 — exactly the `try` placement in
/// `fire_state_transition`.
async fn fire_after_save(
    pool: &sqlx::PgPool,
    issue_id: uuid::Uuid,
    prev_state_id: Option<uuid::Uuid>,
    current_state_id: Option<uuid::Uuid>,
    created_by: Option<uuid::Uuid>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), Denial> {
    let mut seam = ArchiveSignalSeam { pool };
    let mut preflight = ArchivePreflight;
    let outcome = fire_state_transition(
        &mut seam,
        &mut preflight,
        &FireRequest {
            issue_id,
            prev_state_id,
            current_state_id,
            dispatch_immediate: true,
            moved_by_run: None,
            now,
            jitter_secs: 0.0,
            created_by,
        },
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    if let FireOutcome::Failed { log_line, .. } = outcome {
        tracing::error!("{log_line}");
    }
    Ok(())
}

/// The live 594 seam for the archive saves. Only two methods can run
/// here: [`EntriesSeam::prior_state_id`] (the pre-save snapshot) and
/// [`CreationSeam::state`] (the transition's state lookups) — the fire
/// short-circuits on equal states before any other seam use, and the one
/// reachable transition (NULL-state unarchive assigning a non-ticking
/// default) early-returns before the drivers run. The remaining arms
/// answer a store error, which the fire swallows like a handler raise
/// (counter + log line, save stands) rather than crashing the request.
struct ArchiveSignalSeam<'a> {
    pool: &'a sqlx::PgPool,
}

/// The archive saves never dispatch, so preflight never runs.
struct ArchivePreflight;

fn seam_unreachable<T>(method: &str) -> Result<T, CreationError> {
    Err(CreationError::Db(format!(
        "archive signal seam: {method} runs only past a state change"
    )))
}

impl CreationSeam for ArchiveSignalSeam<'_> {
    async fn issue(&mut self, _issue_id: uuid::Uuid) -> Result<IssueView, CreationError> {
        seam_unreachable("issue")
    }

    async fn project(&mut self, _project_id: uuid::Uuid) -> Result<ProjectView, CreationError> {
        seam_unreachable("project")
    }

    async fn state(
        &mut self,
        state_id: Option<uuid::Uuid>,
    ) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let row: Option<(uuid::Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
            .bind(state_id)
            .fetch_optional(self.pool)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?;
        match row {
            None => Err(CreationError::MissingRow(format!(
                "states row {state_id} is gone"
            ))),
            Some((id, name, group)) => Ok(Some(StateView { id, name, group })),
        }
    }

    async fn latest_prior_run(
        &mut self,
        _issue_id: uuid::Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("latest_prior_run")
    }

    async fn active_run_for(
        &mut self,
        _issue_id: uuid::Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("active_run_for")
    }

    async fn run(&mut self, _run_id: uuid::Uuid) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("run")
    }

    async fn runner(
        &mut self,
        _runner_id: uuid::Uuid,
    ) -> Result<Option<RunnerView>, CreationError> {
        seam_unreachable("runner")
    }

    async fn assigned_pod(
        &mut self,
        _pod_id: uuid::Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        seam_unreachable("assigned_pod")
    }

    async fn default_pod_for_project(
        &mut self,
        _project_id: uuid::Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        seam_unreachable("default_pod_for_project")
    }

    async fn resume_parent_run_id(
        &mut self,
        _issue_id: uuid::Uuid,
    ) -> Result<Option<uuid::Uuid>, CreationError> {
        seam_unreachable("resume_parent_run_id")
    }

    async fn work_item_id_for_run(
        &mut self,
        _run_id: uuid::Uuid,
    ) -> Result<Option<uuid::Uuid>, CreationError> {
        seam_unreachable("work_item_id_for_run")
    }

    async fn lock_issue_for_handoff(
        &mut self,
        _issue_id: uuid::Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        seam_unreachable("lock_issue_for_handoff")
    }

    async fn lock_run_for_handoff(
        &mut self,
        _run_id: uuid::Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("lock_run_for_handoff")
    }

    async fn user_flags(&mut self, _user_id: uuid::Uuid) -> Result<UserFlags, CreationError> {
        seam_unreachable("user_flags")
    }

    async fn insert_run(&mut self, _row: &NewAgentRun) -> Result<RunView, CreationError> {
        seam_unreachable("insert_run")
    }

    async fn save_prompt(
        &mut self,
        _run_id: uuid::Uuid,
        _prompt: &str,
        _manifest: &Value,
    ) -> Result<(), CreationError> {
        seam_unreachable("save_prompt")
    }

    async fn save_run_config(
        &mut self,
        _run_id: uuid::Uuid,
        _config: &Value,
    ) -> Result<(), CreationError> {
        seam_unreachable("save_run_config")
    }

    async fn execution_fields(
        &mut self,
        _req: &ExecutionRequest,
    ) -> Result<ExecutionFields, ExecutionError> {
        Err(CreationError::Db(
            "archive signal seam: execution_fields runs only past a state change".to_owned(),
        )
        .into())
    }

    async fn lock_cloud_creation_capacity(
        &mut self,
        _workspace_id: uuid::Uuid,
        _executor_kind: AgentExecutorKind,
        _automatic: bool,
    ) -> Result<Option<AdmissionError>, CreationError> {
        seam_unreachable("lock_cloud_creation_capacity")
    }

    fn dispatch_after_commit(&mut self, _run_id: uuid::Uuid) {}

    async fn render_bundle(
        &mut self,
        _issue_id: uuid::Uuid,
        _run_id: uuid::Uuid,
        _parent_run_id: Option<uuid::Uuid>,
        _trigger: &str,
        _created_by_id: uuid::Uuid,
    ) -> Result<RenderBundle, CreationError> {
        seam_unreachable("render_bundle")
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        String::new()
    }
}

impl FinalizeAgentRunSeam for ArchiveSignalSeam<'_> {
    async fn finalize_failed_run(
        &mut self,
        _run_id: uuid::Uuid,
        _error_code: &str,
        _error: &str,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<RunView, CreationError> {
        seam_unreachable("finalize_failed_run")
    }
}

impl EntriesSeam for ArchiveSignalSeam<'_> {
    async fn prior_state_id(
        &mut self,
        issue_id: uuid::Uuid,
    ) -> Result<Option<uuid::Uuid>, CreationError> {
        let row: Option<(uuid::Uuid, Option<uuid::Uuid>)> = sqlx::query_as(PRIOR_STATE_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(self.pool)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?;
        Ok(row.and_then(|(_, state_id)| state_id))
    }

    async fn queued_follow_up(
        &mut self,
        _issue_id: uuid::Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        seam_unreachable("queued_follow_up")
    }

    async fn lock_ticker(
        &mut self,
        _issue_id: uuid::Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        seam_unreachable("lock_ticker")
    }

    async fn save_ticker(
        &mut self,
        _row: &IssueAgentTicker,
        _write: pidash_services::orchestration::clock::ClockWrite,
    ) -> Result<(), CreationError> {
        seam_unreachable("save_ticker")
    }

    fn set_rollback(&mut self) {}

    async fn clock_policy(
        &mut self,
        _project_id: uuid::Uuid,
    ) -> Result<ProjectClockPolicy, CreationError> {
        seam_unreachable("clock_policy")
    }

    async fn binding(
        &mut self,
        _binding_id: uuid::Uuid,
    ) -> Result<pidash_services::orchestration::entries::BindingView, CreationError> {
        seam_unreachable("binding")
    }

    async fn scheduler_override_pod(
        &mut self,
        _pod_id: uuid::Uuid,
        _project_id: Option<uuid::Uuid>,
    ) -> Result<Option<PodView>, CreationError> {
        seam_unreachable("scheduler_override_pod")
    }

    async fn workspace(
        &mut self,
        _workspace_id: uuid::Uuid,
    ) -> Result<pidash_services::orchestration::entries::WorkspaceView, CreationError> {
        seam_unreachable("workspace")
    }

    async fn scheduler_row(
        &mut self,
        _scheduler_id: uuid::Uuid,
    ) -> Result<pidash_services::orchestration::entries::SchedulerView, CreationError> {
        seam_unreachable("scheduler_row")
    }

    async fn scheduler_override_rows(
        &mut self,
        _workspace_id: uuid::Uuid,
    ) -> Result<Vec<OverrideRow>, CreationError> {
        seam_unreachable("scheduler_override_rows")
    }

    async fn project_role_facts(
        &mut self,
        _user_id: uuid::Uuid,
        _workspace_slug: &str,
        _project_id: uuid::Uuid,
    ) -> Result<ProjectRoleFacts, CreationError> {
        seam_unreachable("project_role_facts")
    }

    async fn has_usable_llm_config(&mut self, _user_id: uuid::Uuid) -> Result<bool, CreationError> {
        seam_unreachable("has_usable_llm_config")
    }

    async fn agent_system_user(
        &mut self,
    ) -> Result<
        Result<uuid::Uuid, pidash_db::orchestration::workpad::AgentUserCollisionError>,
        CreationError,
    > {
        seam_unreachable("agent_system_user")
    }

    async fn insert_scheduler_run(
        &mut self,
        _row: &pidash_services::orchestration::entries::NewSchedulerRun,
    ) -> Result<RunView, CreationError> {
        seam_unreachable("insert_scheduler_run")
    }

    fn compose_scheduler_turn(
        &mut self,
        _context: &Value,
        _index: &pidash_services::prompting::composer::OverrideIndex,
        _workspace_id: Option<&str>,
        _executor_kind: Option<&str>,
        _tool_catalog_version: i64,
    ) -> Result<RenderedTurn, String> {
        Err("archive signal seam: compose_scheduler_turn runs only past a state change".to_owned())
    }
}

impl PreflightSeam for ArchivePreflight {
    async fn preflight_eligibility_or_bounce(
        &mut self,
        _issue_id: uuid::Uuid,
        _creator_id: uuid::Uuid,
        _pod_id: uuid::Uuid,
        _triggered_by: &str,
    ) -> Result<bool, CreationError> {
        seam_unreachable("preflight_eligibility_or_bounce")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::app_issues::{ISSUE_DETAIL_ARCHIVE_FIELDS, ISSUE_DETAIL_BASE_FIELDS};

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }

    async fn denial_body(denial: Denial) -> (StatusCode, String) {
        use axum::response::IntoResponse;
        let response = denial.into_response();
        let status = response.status();
        let body = response.into_body();
        let bytes = axum::body::to_bytes(body, 4096).await.expect("body");
        (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
    }

    #[test]
    fn route_paths_match_urls_issue() {
        assert_eq!(
            ARCHIVED_ISSUES_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/archived-issues/"
        );
        assert_eq!(
            ISSUE_ARCHIVE_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/archive/"
        );
        assert_eq!(
            BULK_ARCHIVE_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/bulk-archive-issues/"
        );
    }

    #[tokio::test]
    async fn archive_gate_bodies_are_verbatim() {
        let (status, body) = denial_body(Denial::BadError(ARCHIVE_STATE_MESSAGE.to_owned())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            "{\"error\":\"Can only archive completed or cancelled state group issue\"}"
        );
        let (status, body) = denial_body(Denial::BadError(IDS_REQUIRED_MESSAGE.to_owned())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, "{\"error\":\"Issue IDs are required\"}");
        let (status, body) = denial_body(Denial::NotFound).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, "{\"error\":\"The required object does not exist.\"}");
        assert_eq!(
            ENTITY_DENIED_BODY,
            "{\"detail\":\"You do not have permission to perform this action.\"}"
        );
    }

    #[test]
    fn bulk_gate_body_orders_error_code_first() {
        let body = format!(
            "{{\"error_code\":{INVALID_ARCHIVE_STATE_GROUP_CODE},\
             \"error_message\":\"INVALID_ARCHIVE_STATE_GROUP\"}}"
        );
        assert_eq!(
            body,
            "{\"error_code\":4091,\"error_message\":\"INVALID_ARCHIVE_STATE_GROUP\"}"
        );
    }

    #[test]
    fn requested_data_shapes_match_json_dumps() {
        let today = "2026-10-04";
        let archive = format!("{{\"archived_at\": \"{today}\", \"automation\": false}}");
        assert_eq!(
            archive,
            "{\"archived_at\": \"2026-10-04\", \"automation\": false}"
        );
        assert_eq!("{\"archived_at\": null}", "{\"archived_at\": null}");
    }

    #[test]
    fn spaced_formatter_matches_dumps_separators() {
        let value = serde_json::json!({
            "archived_at": "2026-10-04",
            "automation": false,
            "nested": {"a": [1, 2], "b": None::<()>},
        });
        let rendered = to_spaced_json(&value).expect("spaced");
        assert_eq!(
            rendered,
            "{\"archived_at\": \"2026-10-04\", \"automation\": false, \
             \"nested\": {\"a\": [1, 2], \"b\": null}}"
        );
    }

    #[test]
    fn current_instance_is_22_keys_in_meta_order() {
        let base = IssueDetailBaseRow {
            id: "id",
            name: "name",
            state_id: None,
            sort_order: 10000.0,
            completed_at: None,
            estimate_point: None,
            priority: "high",
            complexity_score: 0,
            start_date: None,
            target_date: None,
            sequence_id: 1,
            project_id: "project",
            parent_id: None,
            cycle_id: None,
            assigned_pod_id: None,
            agent_executor: None,
            module_ids: None,
            label_ids: None,
            assignee_ids: None,
            sub_issues_count: None,
            created_at: "2026-10-04T06:00:00Z",
            updated_at: "2026-10-04T06:00:00Z",
            created_by: None,
            updated_by: None,
            attachment_count: None,
            link_count: None,
            is_draft: false,
            archived_at: None,
            is_synced: false,
        };
        let view = issue_detail_base_to_representation(&base);
        let rendered = to_spaced_json(&view).expect("spaced");
        let parsed: Map<String, Value> = serde_json::from_str(&rendered).expect("parses");
        // The 7 annotation keys omit; the rest keep Meta order.
        let mut expected: Vec<&str> = ISSUE_DETAIL_BASE_FIELDS.to_vec();
        for key in [
            "cycle_id",
            "module_ids",
            "label_ids",
            "assignee_ids",
            "sub_issues_count",
            "attachment_count",
            "link_count",
        ] {
            expected.retain(|kept| *kept != key);
        }
        assert_eq!(parsed.len(), 22);
        let keys: Vec<&str> = parsed.keys().map(String::as_str).collect();
        assert_eq!(keys, expected);
        assert!(rendered.contains("\"sort_order\": 10000.0"));
    }

    #[test]
    fn archive_detail_is_28_keys() {
        use pidash_services::orchestration::blockers::{RelationDirections, RelationsSummary};
        let blockers = RelationsSummary {
            relations_summary: RelationDirections {
                blocked_by: Vec::new(),
                blocking: Vec::new(),
            },
            has_open_blockers: false,
        };
        let base = IssueDetailBaseRow {
            id: "id",
            name: "name",
            state_id: None,
            sort_order: 0.0,
            completed_at: None,
            estimate_point: None,
            priority: "none",
            complexity_score: 0,
            start_date: None,
            target_date: None,
            sequence_id: 1,
            project_id: "project",
            parent_id: None,
            cycle_id: None,
            assigned_pod_id: None,
            agent_executor: None,
            module_ids: None,
            label_ids: None,
            assignee_ids: None,
            sub_issues_count: None,
            created_at: "2026-10-04T06:00:00Z",
            updated_at: "2026-10-04T06:00:00Z",
            created_by: None,
            updated_by: None,
            attachment_count: None,
            link_count: None,
            is_draft: false,
            archived_at: None,
            is_synced: false,
        };
        let row = IssueDetailRow {
            base,
            description_html: "",
            is_subscribed: false,
            is_intake: None,
            ticker: None,
            latest_run: None,
            active_run: None,
            run_count: 0,
            blockers: &blockers,
        };
        let view = issue_detail_to_representation(&row);
        let rendered = serde_json::to_string(&view).expect("json");
        let parsed: Map<String, Value> = serde_json::from_str(&rendered).expect("parses");
        let keys: Vec<&str> = parsed.keys().map(String::as_str).collect();
        assert_eq!(keys, ISSUE_DETAIL_ARCHIVE_FIELDS.to_vec());
    }

    /// Parse one bulk body through the shared CPython-grammar parser,
    /// then the `issue_ids` lookup — the same two steps the handler runs.
    fn ids_of(text: &str) -> Result<Vec<uuid::Uuid>, Denial> {
        let value = parse_request_bytes(text.as_bytes()).expect("test body parses");
        parse_bulk_ids(Some(&value))
    }

    #[test]
    fn bulk_id_matrix() {
        // Missing / empty / null.
        assert!(matches!(
            parse_bulk_ids(None),
            Err(Denial::BadError(message)) if message == IDS_REQUIRED_MESSAGE
        ));
        assert!(matches!(
            ids_of("{}"),
            Err(Denial::BadError(message)) if message == IDS_REQUIRED_MESSAGE
        ));
        assert!(matches!(
            ids_of(r#"{"issue_ids": []}"#),
            Err(Denial::BadError(message)) if message == IDS_REQUIRED_MESSAGE
        ));
        assert!(matches!(
            ids_of(r#"{"issue_ids": null}"#),
            Err(Denial::ServerError)
        ));
        // Scalars: numbers and bools len-raise (500); strings iterate.
        assert!(matches!(
            ids_of(r#"{"issue_ids": 5}"#),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            ids_of(r#"{"issue_ids": true}"#),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            ids_of(r#"{"issue_ids": ""}"#),
            Err(Denial::BadError(message)) if message == IDS_REQUIRED_MESSAGE
        ));
        assert!(matches!(
            ids_of(r#"{"issue_ids": "abc"}"#),
            Err(Denial::BadError(message)) if message == INVALID_DETAIL_MESSAGE
        ));
        // Top-level non-object bodies have no `.get` (500).
        for top in ["[]", "\"x\"", "5", "true", "null"] {
            assert!(matches!(ids_of(top), Err(Denial::ServerError)), "{top}");
        }
        // Arrays: uuid strings pass, garbage 400s, ints coerce, nulls skip.
        let one = "11111111-1111-1111-1111-111111111111";
        let uuid_one = one.parse::<uuid::Uuid>().expect("uuid");
        let ids = ids_of(&format!(r#"{{"issue_ids": ["{one}"]}}"#)).expect("ids");
        assert_eq!(ids, vec![uuid_one]);
        assert!(matches!(
            ids_of(r#"{"issue_ids": ["not-a-uuid"]}"#),
            Err(Denial::BadError(message)) if message == INVALID_DETAIL_MESSAGE
        ));
        let ids = ids_of(&format!(
            r#"{{"issue_ids": [5, true, "{one}", null, -0, 340282366920938463463374607431768211455]}}"#
        ))
        .expect("ids");
        assert_eq!(
            ids,
            vec![
                uuid::Uuid::from_u128(5),
                uuid::Uuid::from_u128(1),
                uuid_one,
                uuid::Uuid::from_u128(0),
                uuid::Uuid::from_u128(u128::MAX),
            ]
        );
        // UUID-field edges: floats, negatives, huge ints and nested values
        // are the invalid 400 (`ValidationError`), not a crash.
        let nested = format!(r#"{{"issue_ids": [["{one}"]]}}"#);
        for edge in [
            r#"{"issue_ids": [5.5]}"#.to_owned(),
            r#"{"issue_ids": [-5]}"#.to_owned(),
            r#"{"issue_ids": [340282366920938463463374607431768211456]}"#.to_owned(),
            nested,
            r#"{"issue_ids": [{"a": 1}]}"#.to_owned(),
        ] {
            assert!(
                matches!(
                    ids_of(&edge),
                    Err(Denial::BadError(message)) if message == INVALID_DETAIL_MESSAGE
                ),
                "{edge}"
            );
        }
        // Objects iterate keys.
        let keyed = format!(r#"{{"issue_ids": {{"{one}": true}}}}"#);
        let ids = ids_of(&keyed).expect("ids");
        assert_eq!(ids, vec![uuid_one]);
        assert!(matches!(
            ids_of(r#"{"issue_ids": {"nope": 1}}"#),
            Err(Denial::BadError(message)) if message == INVALID_DETAIL_MESSAGE
        ));
    }

    #[test]
    fn bulk_parse_error_body_is_lowercase_cpython() {
        // A blank-but-nonempty body carries the CPython position.
        let Err(JsonFail::Message(blank)) = parse_request_bytes(b" ") else {
            panic!("blank body must fail");
        };
        assert_eq!(blank, "Expecting value: line 1 column 2 (char 1)");
        assert_eq!(
            parse_error_body(&blank),
            "{\"detail\":\"JSON parse error - Expecting value: line 1 column 2 (char 1)\"}"
        );
        // The shared parser's reason for `{oops`, verbatim.
        let Err(JsonFail::Message(reason)) = parse_request_bytes(b"{oops") else {
            panic!("{{oops must fail");
        };
        assert_eq!(
            reason,
            "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"
        );
        assert_eq!(
            parse_error_body(&reason),
            "{\"detail\":\"JSON parse error - \
             Expecting property name enclosed in double quotes: line 1 column 2 (char 1)\"}"
        );
    }

    #[test]
    fn save_updates_write_updated_by() {
        for sql in [ARCHIVE_UPDATE_SQL, UNARCHIVE_UPDATE_SQL] {
            assert!(sql.contains("updated_by_id = $5"), "{sql}");
            assert!(sql.contains("WHERE id = $6"), "{sql}");
        }
        assert!(ARCHIVE_UPDATE_SQL.contains("archived_at = $1"));
        assert!(UNARCHIVE_UPDATE_SQL.contains("archived_at = NULL"));
    }

    #[test]
    fn default_state_lookup_excludes_triage_twice() {
        for sql in [default_state_sql(true), default_state_sql(false)] {
            assert!(sql.contains("NOT s.is_triage"), "{sql}");
            assert!(sql.contains("s.\"group\" != 'triage'"), "{sql}");
        }
        assert!(default_state_sql(true).contains("AND s.\"default\""));
        assert!(!default_state_sql(false).contains("default"));
    }

    #[test]
    fn filtered_set_carries_archive_scope() {
        let gate = Gate {
            user_id: uid(1),
            timezone: chrono_tz::UTC,
            workspace_id: uid(2),
            project_id: uid(3),
            guest_scoped: false,
        };
        let query = QueryMap::new();
        let filtered =
            archive_filtered_set(&gate, "ws", &query, None, None, true).expect("filtered");
        assert!(filtered
            .from_where
            .contains("issue.archived_at IS NOT NULL"));
        assert!(filtered.from_where.contains("issue_types.is_epic"));
        assert!(filtered.from_where.contains("LEFT JOIN states AS state"));
        assert!(!filtered.from_where.contains("project.archived_at"));
        assert!(!filtered.from_where.contains("issue.parent_id IS NULL"));
        let filtered =
            archive_filtered_set(&gate, "ws", &query, None, None, false).expect("filtered");
        assert!(filtered.from_where.contains("issue.parent_id IS NULL"));
    }

    /// FX-ISS-21 archive row: the save fires the receivers and they no-op
    /// (equal states) with zero seam I/O — the lazy pool is never touched.
    #[tokio::test]
    async fn fire_no_transition_needs_no_io() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://127.0.0.1:1/archive_probe")
            .expect("lazy pool");
        let mut seam = ArchiveSignalSeam { pool: &pool };
        let mut preflight = ArchivePreflight;
        let now = chrono::Utc::now();
        let outcome = fire_state_transition(
            &mut seam,
            &mut preflight,
            &FireRequest {
                issue_id: uid(9),
                prev_state_id: Some(uid(3)),
                current_state_id: Some(uid(3)),
                dispatch_immediate: true,
                moved_by_run: None,
                now,
                jitter_secs: 0.0,
                created_by: Some(uid(1)),
            },
        )
        .await
        .expect("no-transition never fails");
        assert_eq!(outcome, FireOutcome::NoTransition);
    }

    /// FX-ISS-21 lookup rule: a storage failure outside the handler `try`
    /// propagates (here: the unreachable lazy pool, short acquire budget
    /// so the test fails fast).
    #[tokio::test]
    async fn fire_lookup_failure_propagates() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect_lazy("postgresql://127.0.0.1:1/archive_probe")
            .expect("lazy pool");
        let mut seam = ArchiveSignalSeam { pool: &pool };
        let mut preflight = ArchivePreflight;
        let now = chrono::Utc::now();
        let outcome = fire_state_transition(
            &mut seam,
            &mut preflight,
            &FireRequest {
                issue_id: uid(9),
                prev_state_id: Some(uid(3)),
                current_state_id: Some(uid(4)),
                dispatch_immediate: true,
                moved_by_run: None,
                now,
                jitter_secs: 0.0,
                created_by: Some(uid(1)),
            },
        )
        .await;
        assert!(outcome.is_err());
    }
}
