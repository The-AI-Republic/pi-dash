#![forbid(unsafe_code)]

//! Issue archive handlers (D-26, stage 5, PIDASHCONV-656).
//!
//! Ports `app/views/issue/archive.py` (2 units, 344 lines):
//!
//! - `GET archived-issues/` (`IssueArchiveViewSet.list`, `:106-219`):
//!   the pilot-2 list machinery over the archive scope (non-epic,
//!   archived, this project/slug) with the archive annotations.
//! - `GET issues/<pk>/archive/` (`retrieve`, `:221-255`): the 30-key
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
//! * FX-ISS-19 says archive-retrieve returns the 37-key shape; it is 30
//!   keys (only `is_subscribed` annotated — the 639 finding, pinned by
//!   `ARCHIVE_DETAIL_KEYS` in `test_archive.py`).
//! * FX-ISS-19 says `current_instance` is the 31-key shape; the enqueue
//!   serializes a bare instance, so DRF omits the 7 annotation keys (24
//!   keys — the pilot-2 `SERIALIZER_FIELDS` rule, verified live).
//! * FX-ISS-19 omits the `save()` recomputes (`completed_at`,
//!   `updated_at`, `description_stripped`, NULL-state default).
//!
//! # Known gaps (precedent: pilot-2 records the same)
//!
//! * `?expand=` on archive retrieve only *adds* nested keys; the nested
//!   expansion fleet is out of scope, so the base 30-key shape renders
//!   (the contract suite never sends `expand` here).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use chrono_tz::Tz;
use pidash_db::entity::{issues, projects, states, workspaces};
use pidash_services::app_issues::serializers_engage;
use sea_orm::{
    ColumnTrait, Condition, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait,
};
use serde_json::{Map, Value};

use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_services::app_issues::agent_runs::{self, RenderedAgentRun};
use pidash_services::app_issues::{
    issue_detail_base_to_representation, issue_detail_to_representation, issue_is_actively_synced,
    on_results_fields, order_sql, raw_group_mismatch, serialize_drf_datetime, AgentTickerInput,
    IssueDetailBaseRow, IssueDetailRow, ListParams, ParseOptions,
};
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues as domain_issues;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{self, WriteActor, WriteCtx, WriteScope};
use pidash_services::orchestration::clock::ProjectClockPolicy;
use pidash_types::orchestration::StateRef;

use super::queries_core::archive_retrieve_select;
use super::{
    actor_user_id, denial_from_param, flat_paginated_response, grouped_response, json_response,
    multi_map, query_last, resolve_project_id, Denial, Gate, HandlerResult, ListContext, QueryMap,
};
use crate::domain_hooks::AppTaskEnqueueHook;
use crate::issue_transition::AppTransitionHook;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json::{
    parse_request_bytes_spans, JObject, JStr, JVal, JsonFail, JSON_PARSE_PREFIX,
};
use pidash_compat::response::{empty_response, json_response as status_response};

/// Archived collection path in `app/urls/issue.py:247-249` form.
pub const ARCHIVED_ISSUES_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/archived-issues/";
/// Archive detail path in `app/urls/issue.py:252-254` form.
pub const ISSUE_ARCHIVE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/archive/";
/// Bulk path in `app/urls/issue.py:102-104` form.
pub const BULK_ARCHIVE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/bulk-archive-issues/";

/// Re-export of the canonical [`pidash_services::domain::activity::ISSUE_ACTIVITY_TASK`].
pub use pidash_services::domain::activity::ISSUE_ACTIVITY_TASK;

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
pub(crate) async fn archive_allow(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<(), Denial> {
    let allows = super::access::allows(
        &pidash_db::pool::sea_orm_connection(pool),
        slug,
        *project_id,
        *user_id,
        &[20, 15],
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    if allows {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// `ProjectEntityPermission` for the bulk POST (non-safe method): an
/// active project membership with role 20/15 — no workspace-admin
/// fallback.
pub(crate) async fn entity_allowed(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let role = super::access::project_role(
        &pidash_db::pool::sea_orm_connection(pool),
        slug,
        *project_id,
        *user_id,
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(matches!(role, Some(20 | 15)))
}

/// The actor's render timezone (`TimezoneMixin` activation; a bad zone or
/// a missing user row is a 500).
async fn user_timezone(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Tz, Denial> {
    let name = super::access::user_timezone(&pidash_db::pool::sea_orm_connection(pool), *user_id)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// The tenant workspace id. The passing gate implies the row exists (the
/// membership check joins it), so a miss is a 500, never a 404.
async fn workspace_id(pool: &sqlx::PgPool, slug: &str) -> Result<uuid::Uuid, Denial> {
    let id: Option<uuid::Uuid> = workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Id)
        .filter(workspaces::Column::Slug.eq(slug))
        .into_tuple()
        .one(&pidash_db::pool::sea_orm_connection(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    id.ok_or(Denial::ServerError)
}

/// JSON-escape one string value.
fn json_detail(value: &str) -> String {
    serde_json::to_string(value).expect("detail string")
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
    let set = super::list_engine::archive_issue_set(
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
    // The archive list annotates the grouped `Subquery` count form
    // (empty counts render `NULL`), so its count keys order `NULL`-style.
    // Its `FROM` always carries the unfiltered `issue_cycle` bridge join,
    // which the cycle-name traversal reuses like Django.
    let key = super::list_engine::order_expr(
        &order_spec.out_param,
        &params.order_by,
        Some("issue_cycle"),
        true,
    )?;
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
            &set,
            &group,
            sub_group_by.clone(),
            &key,
            per_page,
            cursor,
        )
        .await;
    }
    let fields = on_results_fields(None, None);
    flat_paginated_response(
        &list_context,
        &set,
        &key,
        per_page,
        cursor,
        super::LIST_ANNOTATIONS,
        fields,
        false,
        None,
    )
    .await
}

// ---------------------------------------------------------------------------
// Archive retrieve (`archive.py:221-255`)
// ---------------------------------------------------------------------------

/// `GET .../issues/<pk>/archive/`: the 30-key detail. Django serializes
/// through the same `IssueDetailSerializer(issue, expand=self.expand)`, so
/// `?expand=` runs the shared `DynamicBaseSerializer` kernel — mapper names
/// nest, declared names clobber, `issue_attachments` appends, and a mapped
/// name that is not an `Issue` attribute (`sub_issues`, …) 500s. The
/// reaction/link prefetches run no queries here: no detail field reads them.
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
    let expands: Option<Vec<String>> =
        super::expand::split_expand(query_last(&query, "expand").as_deref());
    let stmt = archive_retrieve_select(
        &slug,
        context.gate.project_id,
        issue_id,
        context.gate.user_id,
    );
    let rows = super::list_engine::fetch_rows(&context.pool, stmt).await?;
    let Some(row) = rows.into_iter().next() else {
        return Err(Denial::NotFound);
    };
    let owned = DetailOwned::fetch(&context.pool, &row).await?;
    let input = owned.view();
    let view = issue_detail_to_representation(&input);
    let value = serde_json::to_value(&view).map_err(|_| Denial::ServerError)?;
    let Value::Object(mut body) = value else {
        return Err(Denial::ServerError);
    };
    if let Some(expands) = expands {
        let anchor = super::expand::ExpandAnchor {
            issue_id,
            workspace_id: anchor_uuid(&row, "workspace_id")?,
            project_id: anchor_uuid(&row, "project_id")?,
            created_by_id: anchor_opt_uuid(&row, "created_by_id")?,
            updated_by_id: anchor_opt_uuid(&row, "updated_by_id")?,
            state_id: anchor_opt_uuid(&row, "state_id")?,
            parent_id: anchor_opt_uuid(&row, "parent_id")?,
            estimate_point_id: anchor_opt_uuid(&row, "estimate_point_id")?,
        };
        super::expand::apply_dynamic_expand(
            &context.pool,
            &anchor,
            &context.gate.timezone,
            super::expand::DETAIL_EXPAND_FIELDS,
            &expands,
            &mut body,
        )
        .await?;
    }
    Ok(json_response(
        serde_json::to_string(&Value::Object(body)).map_err(|_| Denial::ServerError)?,
    ))
}

/// Read a required UUID column out of a `row_to_json` map.
fn anchor_uuid(row: &Map<String, Value>, key: &str) -> Result<uuid::Uuid, Denial> {
    json_str(row, key)
        .and_then(|text| text.parse().ok())
        .ok_or(Denial::ServerError)
}

/// Read a nullable UUID column out of a `row_to_json` map.
fn anchor_opt_uuid(row: &Map<String, Value>, key: &str) -> Result<Option<uuid::Uuid>, Denial> {
    match json_str(row, key) {
        Some(text) => text.parse().map(Some).map_err(|_| Denial::ServerError),
        None => Ok(None),
    }
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

/// Everything the 30-key render borrows: the base scalars plus the
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
    type_id: Option<String>,
    is_epic: bool,
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
    latest_run: Option<RenderedAgentRun>,
    active_run: Option<RenderedAgentRun>,
    run_count: i64,
    blockers: pidash_services::orchestration::blockers::RelationsSummary,
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
        let runs = agent_runs::issue_agent_runs(pool, issue_id)
            .await
            .map_err(|_| Denial::ServerError)?;
        let (latest_run, active_run, run_count) = (runs.latest, runs.active, runs.count);
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
            type_id: json_str_opt(row, "type_id"),
            is_epic: fetch_is_epic(pool, json_str_opt(row, "type_id")).await?,
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
    /// 30-key archive shape.
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
            type_id: self.type_id.as_deref(),
            is_epic: self.is_epic,
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
            latest_run: self.latest_run.as_ref().map(RenderedAgentRun::detail_row),
            active_run: self.active_run.as_ref().map(RenderedAgentRun::detail_row),
            run_count: self.run_count,
            blockers: &self.blockers,
        }
    }
}

/// `_issue_is_actively_synced`: no probes without a source; git-only probe
/// (PDASHOSS01-354, legacy tables sunset).
/// `get_is_epic` (`issue.py:1109-1112`): false when the row is untyped,
/// without a query; otherwise the type row's flag. A missing row is
/// Django's `DoesNotExist` → the generic 500 (unreachable — the FK
/// holds). No soft-delete guard: `IssueType` rides the plain manager.
async fn fetch_is_epic(pool: &sqlx::PgPool, type_id: Option<String>) -> Result<bool, Denial> {
    let Some(raw) = type_id else {
        return Ok(false);
    };
    let id = raw.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?;
    super::access::issue_type_is_epic(&pidash_db::pool::sea_orm_connection(pool), id)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)
}

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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let git_hit = serializers_engage::git_issue_synced(&db, issue_id)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(issue_is_actively_synced(Some(source), None, || git_hit))
}

/// `obj.agent_ticker` through `_base_manager` (no soft-delete guard):
/// none without a row, the row with one, a 500 on duplicates (Django's
/// `MultipleObjectsReturned` is uncaught in `get_agent_ticker`).
async fn fetch_ticker(
    pool: &sqlx::PgPool,
    issue_id: uuid::Uuid,
) -> Result<Option<IssueAgentTicker>, Denial> {
    let mut rows = pidash_db::tasks_ticker::models::issue_agent_ticker::tickers_for_issue(
        &pidash_db::pool::sea_orm_connection(pool),
        issue_id,
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(rows.pop()),
        _ => Err(Denial::ServerError),
    }
}

/// The project clock-policy columns behind `effective_max_ticks` and the
/// stage interval (reached through the cached `issue.project`: no
/// soft-delete guard, and a missing row is a 500).
async fn fetch_clock_policy(
    pool: &sqlx::PgPool,
    project_id: uuid::Uuid,
) -> Result<ProjectClockPolicy, Denial> {
    let row = pidash_services::orchestration::entries::clock_policy(
        &pidash_db::pool::sea_orm_connection(pool),
        project_id,
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((enabled, max_ticks, interval, review_interval, test_interval)) = row else {
        return Err(Denial::ServerError);
    };
    Ok(ProjectClockPolicy {
        agent_ticking_enabled: Some(enabled),
        agent_default_max_ticks: Some(max_ticks),
        agent_default_interval_seconds: Some(i64::from(interval)),
        agent_review_default_interval_seconds: Some(i64::from(review_interval)),
        agent_test_default_interval_seconds: Some(i64::from(test_interval)),
    })
}

/// One state row through `_base_manager` (no soft-delete guard).
async fn fetch_state(
    pool: &sqlx::PgPool,
    state_id: uuid::Uuid,
) -> Result<Option<(String, String)>, Denial> {
    pidash_services::orchestration::creation::state_name_group(
        &pidash_db::pool::sea_orm_connection(pool),
        state_id,
    )
    .await
    .map_err(|_| Denial::ServerError)
}

/// One cached `relations_summary` over the two summary statements plus
/// the full-set open check (the fixture's 3 statements).
async fn fetch_blockers(
    pool: &sqlx::PgPool,
    issue_id: uuid::Uuid,
) -> Result<pidash_services::orchestration::blockers::RelationsSummary, Denial> {
    pidash_services::orchestration::blockers::fetch_relations_summary(
        &pidash_db::pool::sea_orm_connection(pool),
        issue_id,
    )
    .await
    .map_err(|_| Denial::ServerError)
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
    type_id: Option<String>,
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
    let requested = format!("{{\"archived_at\": \"{today}\", \"automation\": false}}");
    // `save()`: the pre-save snapshot, the write with its recomputes,
    // then the explicit post-save fire — as one domain write + chain.
    let prev = crate::issue_transition::capture_prior_state_pool(&context.pool, Some(issue_id))
        .await
        .map_err(|_| Denial::ServerError)?;
    let now = chrono::Utc::now();
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = write_ctx(&context.gate);
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(&state, &context.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&context.pool));
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = context.pool.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::archive(
                &ctx,
                txn,
                &pool,
                domain_issues::ArchiveIssue {
                    issue_id,
                    project_id: context.gate.project_id,
                    state_id: row.state_id,
                    description_html: row.description_html.clone(),
                    archived_at: today,
                    updated_at: now,
                    prev_state: prev,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(context.gate.user_id),
                        moved_by_run: None,
                        now,
                    },
                    pre_activity: domain_issues::PreSaveActivity {
                        event: "issue.activity.updated".to_owned(),
                        requested,
                        actor: context.gate.user_id,
                        project: context.gate.project_id,
                        current_instance,
                        origin,
                        subscriber: None,
                    },
                },
            )
            .await
        })
    })
    .await
    .map_err(archive_denial)?;
    if outcome.chain.stopped().is_some() {
        return Err(Denial::ServerError);
    }
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
    // `save()` state half: NULL takes the default-state assignment (and
    // skips the `completed_at` recompute); a set id with no row raises
    // `DoesNotExist` (404) — after the enqueue, like the `save()` raise.
    // All three run in the domain write: the enqueue first, then the
    // sparse save, then completion-if-changed and the fire.
    let prev = crate::issue_transition::capture_prior_state_pool(&context.pool, Some(issue_id))
        .await
        .map_err(|_| Denial::ServerError)?;
    let now = chrono::Utc::now();
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = write_ctx(&context.gate);
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(&state, &context.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&context.pool));
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = context.pool.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::unarchive(
                &ctx,
                txn,
                &pool,
                domain_issues::UnarchiveIssue {
                    issue_id,
                    project_id: context.gate.project_id,
                    state_id: row.state_id,
                    description_html: row.description_html.clone(),
                    updated_at: now,
                    prev_state: prev,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(context.gate.user_id),
                        moved_by_run: None,
                        now,
                    },
                    pre_activity: domain_issues::PreSaveActivity {
                        event: "issue.activity.updated".to_owned(),
                        requested: "{\"archived_at\": null}".to_owned(),
                        actor: context.gate.user_id,
                        project: context.gate.project_id,
                        current_instance,
                        origin,
                        subscriber: None,
                    },
                },
            )
            .await
        })
    })
    .await
    .map_err(archive_denial)?;
    if outcome.chain.stopped().is_some() {
        return Err(Denial::ServerError);
    }
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
    // The default `IssueManager` scope (live, non-triage, non-archived,
    // non-draft, live-project) plus workspace / project / pk.
    let row = issue_with_state(slug, *project_id)
        .join(JoinType::InnerJoin, issues::Relation::Projects.def())
        .filter(
            Condition::any()
                .add(states::Column::Group.is_null())
                .add(states::Column::Group.ne("triage")),
        )
        .filter(issues::Column::ArchivedAt.is_null())
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(issues::Column::IsDraft.eq(false))
        .filter(issues::Column::Id.eq(*issue_id))
        .one(&pidash_db::pool::sea_orm_connection(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(write_row).ok_or(Denial::NotFound)
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
    let row = issue_with_state(slug, *project_id)
        .filter(issues::Column::ArchivedAt.is_not_null())
        .filter(issues::Column::Id.eq(*issue_id))
        .one(&pidash_db::pool::sea_orm_connection(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(write_row).ok_or(Denial::NotFound)
}

/// Map one write-path row.
/// Live issues (`SoftDeletionManager`) of the project in this workspace
/// slug, each with its state (`select_related("state")`, LEFT JOIN).
fn issue_with_state(
    slug: &str,
    project_id: uuid::Uuid,
) -> sea_orm::SelectTwo<issues::Entity, states::Entity> {
    issues::Entity::find()
        .find_also_related(states::Entity)
        .join(JoinType::InnerJoin, issues::Relation::Workspaces.def())
        .filter(issues::Column::DeletedAt.is_null())
        .filter(issues::Column::ProjectId.eq(project_id))
        .filter(workspaces::Column::Slug.eq(slug))
}

/// Map one write-path row.
fn write_row((issue, state): (issues::Model, Option<states::Model>)) -> WriteRow {
    let at = |at: sea_orm::prelude::DateTimeWithTimeZone| at.with_timezone(&chrono::Utc);
    let id_text = |id: Option<uuid::Uuid>| id.map(|id| id.to_string());
    WriteRow {
        id: issue.id,
        name: issue.name,
        state_id: issue.state_id,
        group: state.map(|state| state.group),
        sort_order: issue.sort_order,
        completed_at: issue.completed_at.map(at),
        estimate_point: id_text(issue.estimate_point_id),
        priority: issue.priority,
        complexity_score: issue.complexity_score,
        start_date: issue.start_date.map(|date| date.to_string()),
        target_date: issue.target_date.map(|date| date.to_string()),
        sequence_id: issue.sequence_id,
        project_id: issue.project_id,
        parent_id: id_text(issue.parent_id),
        type_id: id_text(issue.type_id),
        assigned_pod_id: id_text(issue.assigned_pod_id),
        agent_executor: issue.agent_executor,
        created_at: at(issue.created_at),
        updated_at: at(issue.updated_at),
        created_by: id_text(issue.created_by_id),
        updated_by: id_text(issue.updated_by_id),
        is_draft: issue.is_draft,
        archived_at: issue.archived_at.map(|date| date.to_string()),
        external_source: issue.external_source,
        description_html: issue.description_html,
    }
}

/// `current_instance`: the 24-key bare-instance shape over the pre-save
/// row, `json.dumps` spacing (`", "` / `": "`).
async fn current_instance_json(pool: &sqlx::PgPool, row: &WriteRow) -> Result<String, Denial> {
    let is_synced = fetch_is_synced(pool, row.id, row.external_source.as_deref()).await?;
    let is_epic = fetch_is_epic(pool, row.type_id.clone()).await?;
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
        type_id: row.type_id.as_deref(),
        is_epic,
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
    domain_issues::to_spaced_json(&view).ok_or(Denial::ServerError)
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

/// The domain-write context for an archive request: the request user as
/// actor plus the gated tenant scope. The origin resolves up front (the
/// archive enqueues run pre-save), not per effect.
fn write_ctx(gate: &Gate) -> WriteCtx {
    WriteCtx::new(WriteActor {
        user_id: gate.user_id,
        timezone: Some(gate.timezone.name().to_owned()),
    })
    .with_scope(WriteScope {
        workspace_id: Some(gate.workspace_id),
        project_id: Some(gate.project_id),
    })
}

/// Map a domain-write failure onto the archive arms: a dangling-row miss
/// 404s (the unarchive NULL-leg save raise); everything else 500s (the
/// archive UPDATEs map every error to `ServerError` — there is no
/// invalid-payload arm here).
fn archive_denial(error: domain::DomainError) -> Denial {
    if error.is_not_found() {
        Denial::NotFound
    } else {
        Denial::ServerError
    }
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
    headers: HeaderMap,
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
    // `request.data` through the shared negotiator: empty (by the
    // Content-Length header, whatever the content-type) short-circuits
    // to `{}` — the required 400; an unsupported or missing content
    // type is the 415; JSON keeps the `JVal` parse (the
    // `to_serde_publish` inf hazard from the D5 review), so the reason
    // text stays byte-exact (`{oops` and friends) and past the depth
    // cap Django's `RecursionError` escapes DRF into the JSON 500.
    let parsed: Option<JVal> =
        match shared_body::negotiate_body(&headers, &body, &ARCHIVE_BODY_SPEC) {
            Ok(shared_body::NegotiatedBody::Empty) => None,
            Ok(shared_body::NegotiatedBody::JsonText { text, surr }) => {
                match parse_request_bytes_spans(text.as_bytes(), &surr) {
                    Ok(value) => Some(value),
                    Err(JsonFail::Message(reason)) => {
                        return Err(Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{reason}")));
                    }
                    Err(JsonFail::Recursion) => return Err(Denial::ServerError),
                }
            }
            Ok(shared_body::NegotiatedBody::Form { map, files, surr }) => {
                Some(form_request_data(&map, &files, &surr)?)
            }
            Err(shared_body::BodyError::UnsupportedMediaType(message)) => {
                return Ok(status_response(
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    unsupported_media_type_body(&message),
                ));
            }
            Err(shared_body::BodyError::ParseDetail(message)) => {
                return Err(Denial::BadDetail(message));
            }
            Err(shared_body::BodyError::ServerError) => return Err(Denial::ServerError),
        };
    let ids = parse_bulk_ids(parsed.as_ref())?;
    let today = chrono::Utc::now().date_naive();
    let origin = activity_origin(&state)?;
    let rows = fetch_bulk_issues(&context.pool, &slug, &context.gate.project_id, &ids).await?;
    let enqueue_hook: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&context.pool));
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
        domain_issues::bulk_archive_activity(
            &enqueue_hook,
            &domain_issues::BulkArchiveRow {
                issue_id: row.id,
                project_id: context.gate.project_id,
                actor_id: context.gate.user_id,
                current_instance,
                origin: origin.clone(),
            },
            today,
        )
        .await;
    }
    // `bulk_update(["archived_at"])`: one UPDATE, no `save()`.
    let hit: Vec<uuid::Uuid> = rows.iter().map(|row| row.id).collect();
    domain_issues::bulk_archive_update(&context.pool, &hit, today)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(json_response(format!("{{\"archived_at\":\"{today}\"}}")))
}

/// Bulk HTML-input shape: the view reads `request.data` raw (no
/// serializer), so no key arrives as a list (`QueryDict.get` reads the
/// last value) and no blank rule applies (form `issue_ids=` is the
/// required-400 through the empty-string arm, not an absence).
const ARCHIVE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// DRF's `UnsupportedMediaType` body: lowercase `detail`
/// (`exception_handler` renders `{'detail': exc.detail}`, the same
/// renderer as `Denial::BadDetail` — the shared `Denial` has no 415
/// variant, so the bulk arm builds it locally).
fn unsupported_media_type_body(message: &str) -> String {
    format!("{{\"detail\":{}}}", json_detail(message))
}

/// `request.data` for an HTML body, as the `JVal` object
/// [`parse_bulk_ids`] reads. DRF merges files into the data
/// (`_full_data`: texts then files per key), and `.get` reads the last
/// value — the last upload when the key carries any file, else the
/// last text value. Only `issue_ids` is ever read, so the object
/// carries just that member (absent when the key never arrived, which
/// is the required-400).
fn form_request_data(
    map: &Map<String, Value>,
    files: &shared_body::FilesMap,
    surr: &BTreeMap<String, Vec<Vec<(usize, u16)>>>,
) -> Result<JVal, Denial> {
    const KEY: &str = "issue_ids";
    let mut object = JObject::new();
    if let Some(parts) = files.get(KEY).filter(|parts| !parts.is_empty()) {
        // An upload: `len()` is the byte size (`File.__len__`). An empty
        // file is the required-400 (fall through with the member
        // absent); a nonzero one iterates byte lines in `pk__in`, and
        // every line raises `TypeError` in `uuid.UUID(hex=...)`, which
        // `to_python` does not catch (only `AttributeError`/
        // `ValueError`) — the JSON 500.
        let last = parts.last().expect("nonempty file list");
        if !last.bytes.is_empty() {
            return Err(Denial::ServerError);
        }
        return Ok(JVal::Object(object));
    }
    if let Some(Value::String(text)) = map.get(KEY) {
        let spans = surr.get(KEY).and_then(|spans| spans.first()).cloned();
        object.insert(
            JStr::from_text(KEY),
            JVal::Str(JStr::from_dirty(text, &spans.unwrap_or_default())),
        );
    }
    Ok(JVal::Object(object))
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
    let rows = issue_with_state(slug, *project_id)
        .filter(issues::Column::Id.is_in(ids.iter().copied()))
        .order_by_desc(issues::Column::CreatedAt)
        .all(&pidash_db::pool::sea_orm_connection(pool))
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(rows.into_iter().map(write_row).collect())
}

// ---------------------------------------------------------------------------
// Explicit signals (594 entries)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1_cycles_modules::json::parse_request_bytes;
    use pidash_services::app_issues::{ISSUE_DETAIL_ARCHIVE_FIELDS, ISSUE_DETAIL_BASE_FIELDS};
    use pidash_services::orchestration::entries::{
        fire_state_transition, FireOutcome, FireRequest,
    };

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }

    async fn denial_body(denial: Denial) -> (StatusCode, String) {
        use axum::response::IntoResponse;
        let response = denial.into_response();
        let status = response.status();
        let body = response.into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.expect("body");
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
        let rendered = domain_issues::to_spaced_json(&value).expect("spaced");
        assert_eq!(
            rendered,
            "{\"archived_at\": \"2026-10-04\", \"automation\": false, \
             \"nested\": {\"a\": [1, 2], \"b\": null}}"
        );
    }

    #[test]
    fn current_instance_is_24_keys_in_meta_order() {
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
            type_id: None,
            is_epic: false,
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
        let rendered = domain_issues::to_spaced_json(&view).expect("spaced");
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
        assert_eq!(parsed.len(), 24);
        let keys: Vec<&str> = parsed.keys().map(String::as_str).collect();
        assert_eq!(keys, expected);
        assert!(rendered.contains("\"sort_order\": 10000.0"));
    }

    #[test]
    fn archive_detail_is_30_keys() {
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
            type_id: None,
            is_epic: false,
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

    #[tokio::test]
    async fn bulk_parse_error_body_is_lowercase_cpython() {
        // A blank-but-nonempty body 400s; the pins that remain are the
        // 400 status and the lowercase envelope (message is serde's).
        let Err(JsonFail::Message(blank)) = parse_request_bytes(b" ") else {
            panic!("blank body must fail");
        };
        assert!(!blank.is_empty(), "serde reason, got {blank:?}"); // Tier 3 since PIDASHCONV-1555: serde text
        let (status, body) =
            denial_body(Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{blank}"))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.starts_with("{\"detail\":\"JSON parse error - "),
            "envelope shape, got {body:?}"
        );
        // The shared parser's reason for `{oops` (serde text).
        let Err(JsonFail::Message(reason)) = parse_request_bytes(b"{oops") else {
            panic!("{{oops must fail");
        };
        assert!(!reason.is_empty(), "serde reason, got {reason:?}");
        let (status, body) =
            denial_body(Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{reason}"))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.starts_with("{\"detail\":\"JSON parse error - "),
            "envelope shape, got {body:?}"
        );
    }

    #[test]
    fn bulk_unsupported_media_type_body_is_lowercase() {
        assert_eq!(
            unsupported_media_type_body("Unsupported media type \"\" in request."),
            "{\"detail\":\"Unsupported media type \\\"\\\" in request.\"}"
        );
        assert_eq!(
            unsupported_media_type_body("Unsupported media type \"text/plain\" in request."),
            "{\"detail\":\"Unsupported media type \\\"text/plain\\\" in request.\"}"
        );
    }

    #[test]
    fn bulk_negotiate_matrix() {
        use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
        fn negotiate(content_type: Option<&str>, body: &[u8]) -> Result<String, String> {
            let mut headers = HeaderMap::new();
            if let Some(content_type) = content_type {
                headers.insert(CONTENT_TYPE, content_type.parse().expect("ct"));
            }
            headers.insert(CONTENT_LENGTH, body.len().into());
            match shared_body::negotiate_body(&headers, body, &ARCHIVE_BODY_SPEC) {
                Ok(shared_body::NegotiatedBody::Empty) => Ok("empty".to_owned()),
                Ok(shared_body::NegotiatedBody::JsonText { text, .. }) => Ok(text),
                Ok(shared_body::NegotiatedBody::Form { map, .. }) => Ok(format!(
                    "form:{}",
                    map.get("issue_ids").cloned().unwrap_or_default()
                )),
                Err(error) => Err(format!("{error:?}")),
            }
        }
        // Missing or foreign content-type + non-empty body is the 415.
        assert!(negotiate(None, b"{}")
            .unwrap_err()
            .contains("Unsupported media type"));
        assert!(negotiate(Some("text/plain"), b"{}")
            .unwrap_err()
            .contains("text/plain"));
        // Empty short-circuits whatever the content-type.
        assert_eq!(negotiate(None, b"").unwrap(), "empty");
        assert_eq!(negotiate(Some("text/plain"), b"").unwrap(), "empty");
        // JSON still decodes; form reads the last value.
        assert_eq!(
            negotiate(Some("application/json"), b"{\"issue_ids\": []}").unwrap(),
            "{\"issue_ids\": []}"
        );
        assert_eq!(
            negotiate(
                Some("application/x-www-form-urlencoded"),
                b"issue_ids=a&issue_ids=b"
            )
            .unwrap(),
            "form:\"b\""
        );
        assert_eq!(
            negotiate(Some("application/x-www-form-urlencoded"), b"issue_ids=").unwrap(),
            "form:\"\""
        );
    }

    #[test]
    fn bulk_form_request_data_last_wins_files_beat_texts() {
        use shared_body::FilePart;
        fn part(bytes: &[u8]) -> FilePart {
            FilePart {
                filename: "f".to_owned(),
                content_type: "text/plain".to_owned(),
                bytes: bytes.to_vec(),
                in_memory: true,
            }
        }
        // Absent key → required-400; blank text → required-400; junk text →
        // invalid-400 (the string arm iterates characters).
        let empty = Map::new();
        let no_files = shared_body::FilesMap::new();
        let no_surr = BTreeMap::new();
        let data = form_request_data(&empty, &no_files, &no_surr).expect("data");
        assert!(matches!(
            parse_bulk_ids(Some(&data)),
            Err(Denial::BadError(message)) if message == IDS_REQUIRED_MESSAGE
        ));
        let mut blank = Map::new();
        blank.insert("issue_ids".to_owned(), Value::String(String::new()));
        let data = form_request_data(&blank, &no_files, &no_surr).expect("data");
        assert!(matches!(
            parse_bulk_ids(Some(&data)),
            Err(Denial::BadError(message)) if message == IDS_REQUIRED_MESSAGE
        ));
        let mut junk = Map::new();
        junk.insert("issue_ids".to_owned(), Value::String("abc".to_owned()));
        let data = form_request_data(&junk, &no_files, &no_surr).expect("data");
        assert!(matches!(
            parse_bulk_ids(Some(&data)),
            Err(Denial::BadError(message)) if message == INVALID_DETAIL_MESSAGE
        ));
        // Even a well-formed UUID string is the invalid-400: `pk__in`
        // iterates the string character by character.
        let one = uid(1);
        let mut single = Map::new();
        single.insert("issue_ids".to_owned(), Value::String(one.to_string()));
        let data = form_request_data(&single, &no_files, &no_surr).expect("data");
        assert!(matches!(
            parse_bulk_ids(Some(&data)),
            Err(Denial::BadError(message)) if message == INVALID_DETAIL_MESSAGE
        ));
        // An empty upload is the required-400; a nonzero one is the 500
        // (`TypeError` escapes `to_python`).
        let mut files = shared_body::FilesMap::new();
        files.insert("issue_ids".to_owned(), vec![part(b"")]);
        let data = form_request_data(&single, &files, &no_surr).expect("data");
        assert!(matches!(
            parse_bulk_ids(Some(&data)),
            Err(Denial::BadError(message)) if message == IDS_REQUIRED_MESSAGE
        ));
        files.insert("issue_ids".to_owned(), vec![part(b"x")]);
        assert!(matches!(
            form_request_data(&single, &files, &no_surr),
            Err(Denial::ServerError)
        ));
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
        let render = |show: bool| {
            let mut stmt =
                super::super::list_engine::archive_issue_set(&gate, "ws", &query, None, None, show)
                    .expect("filtered")
                    .select();
            stmt.expr(sea_query::Expr::val(1));
            stmt.to_string(sea_query::PostgresQueryBuilder)
        };
        let sql = render(true);
        assert!(sql.contains(r#""issue"."archived_at" IS NOT NULL"#));
        assert!(sql.contains(r#""issue_types"."is_epic""#));
        assert!(sql.contains(r#"LEFT JOIN "states" AS "state""#));
        assert!(!sql.contains(r#""project"."archived_at""#));
        assert!(!sql.contains(r#""issue"."parent_id" IS NULL"#));
        assert!(render(false).contains(r#""issue"."parent_id" IS NULL"#));
    }

    /// FX-ISS-21 archive row: the save fires the receivers and they no-op
    /// (equal states) with zero seam I/O — the lazy pool is never touched.
    #[tokio::test]
    async fn fire_no_transition_needs_no_io() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://127.0.0.1:1/archive_probe")
            .expect("lazy pool");
        let mut seam = crate::issue_transition::test_support::LazyStateSeam { pool: &pool };
        let mut preflight = crate::issue_transition::test_support::UnreachedPreflight;
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
        let mut seam = crate::issue_transition::test_support::LazyStateSeam { pool: &pool };
        let mut preflight = crate::issue_transition::test_support::UnreachedPreflight;
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

    /// The archive / unarchive / bulk issue reads against constraint-free
    /// temp tables on a one-connection pool: scopes, the LEFT JOINed state
    /// and the bulk order.
    #[tokio::test]
    async fn live_write_rows() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in [
            "issues",
            "states",
            "workspaces",
            "projects",
            "issue_agent_ticker",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let (ws, project, state, triage) = (
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        );
        let (open, stateless, archived, draft, in_triage) = (
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        );
        let issue = |id: uuid::Uuid,
                     state: Option<uuid::Uuid>,
                     archived: bool,
                     draft: bool,
                     age: i32| {
            let state = state.map_or("NULL".to_owned(), |s| format!("'{s}'"));
            format!(
                "INSERT INTO issues (id, name, workspace_id, project_id, state_id, sort_order, priority, \
                 complexity_score, sequence_id, created_at, updated_at, is_draft, archived_at, description_html, description_json, git_work_branch, workpad) \
                 VALUES ('{id}', 'n', '{ws}', '{project}', {state}, 1, 'none', 0, 1, now() - interval '{age} min', now(), \
                 {draft}, {}, '', '{{}}', '', '')",
                if archived { "current_date" } else { "NULL" }
            )
        };
        for sql in [
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!("INSERT INTO projects (id, archived_at) VALUES ('{project}', NULL)"),
            format!(
                "INSERT INTO states (id, name, \"group\", created_at, updated_at, description, color, slug, project_id, workspace_id, sequence, \"default\", is_triage) VALUES ('{state}', 'Todo', 'unstarted', now(), now(), '', '', 'todo', '{project}', '{ws}', 1, false, false), \
                 ('{triage}', 'Triage', 'triage', now(), now(), '', '', 'triage', '{project}', '{ws}', 2, false, true)"
            ),
            issue(open, Some(state), false, false, 1),
            issue(stateless, None, false, false, 2),
            issue(archived, Some(state), true, false, 3),
            issue(draft, Some(state), false, true, 4),
            issue(in_triage, Some(triage), false, false, 5),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }
        let row = fetch_archive_issue(&pool, "acme", &project, &open)
            .await
            .expect("archive read");
        assert_eq!(
            (row.group.as_deref(), row.archived_at.as_deref()),
            (Some("unstarted"), None)
        );
        let row = fetch_archive_issue(&pool, "acme", &project, &stateless)
            .await
            .expect("stateless");
        assert_eq!(row.group, None);
        for hidden in [archived, draft, in_triage] {
            assert!(matches!(
                fetch_archive_issue(&pool, "acme", &project, &hidden).await,
                Err(Denial::NotFound)
            ));
        }
        assert!(matches!(
            fetch_archive_issue(&pool, "other", &project, &open).await,
            Err(Denial::NotFound)
        ));
        let row = fetch_unarchive_issue(&pool, "acme", &project, &archived)
            .await
            .expect("unarchive");
        assert!(row.archived_at.is_some());
        assert!(matches!(
            fetch_unarchive_issue(&pool, "acme", &project, &open).await,
            Err(Denial::NotFound)
        ));
        let bulk = fetch_bulk_issues(
            &pool,
            "acme",
            &project,
            &[in_triage, open, draft, uuid::Uuid::new_v4()],
        )
        .await
        .expect("bulk");
        // `-created_at`; drafts and triage stay eligible.
        assert_eq!(
            bulk.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![open, draft, in_triage]
        );
        assert_eq!(fetch_ticker(&pool, open).await.expect("ticker"), None);
    }
}
