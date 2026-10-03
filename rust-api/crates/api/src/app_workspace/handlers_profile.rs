//! Workspace user-profile handler family (D-24, stage 5, PIDASHCONV-618).
//!
//! Ports `apps/api/pi_dash/app/views/workspace/user.py`:
//!
//! - `WorkspaceUserProfileEndpoint.get` (`:281-369`, W24): `{"project_data",
//!   "user_data"}` — the 8-key inline `user_data` dict plus `project_data`
//!   (4 annotated counts) only when the requester's workspace role is `>=
//!   15`.
//! - `WorkspaceUserProfileIssuesEndpoint.get` (`:99-251`, W25): the
//!   viewer-gated issue list (legacy filters, `ComplexFilterBackend` +
//!   `IssueFilterSet`, annotations, ordering, grouper, grouped /
//!   sub-grouped / flat paginators with `issue_on_results` shaping).
//! - `WorkspaceUserProfileStatsEndpoint.get` (`:397-522`, W21): the 9-key
//!   stats envelope (state/priority distributions + 5 counts + present /
//!   upcoming cycles).
//! - `WorkspaceUserActivityEndpoint.get` (`:371-395`, W22): the
//!   entity-gated paginated activity list via `IssueActivitySerializer`.
//! - `UserActivityEndpoint.get` (`app/views/user/base.py:392-404`, U12):
//!   the actor-scoped `users/me/activities/` list through the same
//!   serializer.
//!
//! Fixture ids: F-W24-15 (these routes) + consumed F-W24-11 (profile rows)
//! and F-W24-13 (gates). Profile/stats envelopes are inline dicts rendered
//! here; issue shapes come from merged `services::app_issues`
//! ordering/params/shape; activity shapes from D-26's
//! `issue_activity_to_representation` (PIDASHCONV-642, merged).
//!
//! Layering: SQL builders live in
//! [`pidash_services::app_workspace::queries_profile`] (QRY-C,
//! PIDASHCONV-610); gates in [`super::gates`] (PIDASHCONV-613) over the
//! F-06 kernel; the issues pipeline composes the shared kernels —
//! `pidash_db::{filter, filterset, issue_filters}`, the D-26
//! `crate::app_issues` list helpers, `pidash_services::app_issues`
//! ordering/params/shape, and `crate::paginator`. Endpoint-specific SQL
//! (scope preamble, join selection, annotation/group glue) lives here
//! because it names this endpoint's scope; the merged helpers are
//! referenced, never re-ported.
//!
//! Alias contract: the outer issues query uses the pilot `issue` / `state`
//! aliases so every compiled filter/order fragment splices verbatim; the
//! QRY-C base (`profile_issues_from_where`, `i`-aliased) is consumed as
//! the `id IN (...)` scope subquery, and its annotation fragment is
//! adapted `= i.id` → `= issue.id` ([`profile_annotations_for_issue`]).
//! The stats legacy splice instead rewrites `issue.` → `i.` / `state.` →
//! `s.` ([`rewrite_stats_aliases`]) because the QRY-C stats statements are
//! complete selects.
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * No `permission_classes` on the profile/stats endpoints (`user.py:281,
//!   397`): any authenticated caller reaches the body; the profile 404 for
//!   non-members comes from the `WorkspaceMember.objects.get` miss, not a
//!   gate.
//! * The R2 id-set keeps the viewer's OWN project scope (`:146-147`), not
//!   the target's.
//! * `role >= 15` is a literal (`:289`); guests get `project_data = []`.
//! * `pending_issues` uses the literal trio
//!   `('backlog','unstarted','started')`, excluding review/test.
//! * Q2 `HAVING COUNT(*) >= 1` is always true — emitted verbatim.
//! * R5 cycle queries take no legacy filters and no requester scope; the
//!   Q9 variable is singular while the key is plural `present_cycles`.
//! * The three R1 counts are plain `COUNT` (0 when empty, never NULL):
//!   `Func(F("id"), function="Count")` is not an `Aggregate`, so Django
//!   emits no `GROUP BY` — unlike the pilot's `Count()` (NULLIF) shape,
//!   which belongs to a different view.
//! * `?project=` values validate at lookup time; non-UUIDs 400.
//! * Non-UUID `user_id` path segments: Django's `<uuid:>` converter
//!   rejects them at URL resolve (HTML 404, unreachable through the JSON
//!   edge); here they answer the `ValidationError` 400, the same body the
//!   kernels render for badly-formed UUIDs inside filters (cycle-issues
//!   precedent).
//!
//! # Deliberate pilot divergences (faithful to the Python source)
//!
//! * The m2m group joins carry the `GROUP_FILTER_MAPPER` deleted guard
//!   (`grouper.py:41-49`); the pilot/cycle ports force the INNER join but
//!   drop the guard.
//! * The `module_ids` array omits the `m.deleted_at IS NULL` guard the
//!   pilot adds (its own docs disclose that addition as a divergence);
//!   `grouper.py:63-72` filters only `module__archived_at`.
//! * Sub-group m2m levels project their raw key (the grouper swaps the
//!   array for the key at both levels, `grouper.py:107-136`); the pilot
//!   projects the outer level only.
//! * The `target_date` / `start_date` / `created_by` group values keep
//!   NULLs (as `"None"`) and duplicates (QRY-C `group_values_distinct_sql`
//!   quirk); the pilot filters NULLs.
//!
//! # Residuals (pathological, beyond review scope)
//!
//! * Avatar/cover/logo assets whose `entity_type` is not one of the four
//!   static types resolve to `None` ([`static_asset_url`]); live upload
//!   paths always use the static types, and the attachment/description URL
//!   shapes would need workspace/project joins for data that cannot occur.
//! * Activity `order_by` relation traversals (`issue__name`) answer 500;
//!   single-level fields and FK names cover the exercised surface (pilot
//!   `order_key` precedent).

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::Router;
use serde_json::{Map, Value};
use sqlx::Row;

use crate::app_issues::{
    complex_filter, fetch_count, fetch_json_rows, legacy_sql, multi_map, order_key, page_denial,
    query_last, query_values, render_condition, shape_row, Binder, Denial, FilteredSet, QueryMap,
};
use crate::app_workspace::gates;
use crate::state::AppState;
use pidash_db::issue_filters::FilterValue;
use pidash_services::app_issues::params::{ListParams, ParseOptions};
use pidash_services::app_issues::shape::{envelope, on_results_fields};
use pidash_services::app_issues::{
    app_issue_flat_to_representation, issue_activity_to_representation, AppIssueFlatRow,
    IssueActivityRow,
};
use pidash_services::app_project::ser_member::{
    project_lite_to_representation, resolve_project_cover_image_url, resolve_workspace_logo_url,
    workspace_lite_to_representation, ProjectLiteRow, WorkspaceLiteRow,
};
use pidash_services::app_project::ser_shared::{user_lite_to_representation, UserLiteRow};
use pidash_services::app_workspace::models_user::user as user_model;
use pidash_services::app_workspace::queries_profile as qp;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `GET` on the five owned paths; every other method falls through to
/// Django (the views define `get` only, so Django answers 405 itself).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            USER_STATS_PATH,
            axum::routing::get(user_profile_stats)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            USER_ACTIVITY_PATH,
            axum::routing::get(user_activity)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            USER_PROFILE_PATH,
            axum::routing::get(user_profile)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            USER_ISSUES_PATH,
            axum::routing::get(user_profile_issues)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            ME_ACTIVITIES_PATH,
            axum::routing::get(me_activities)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

/// W21 (`app/urls/workspace.py:158-162`).
pub const USER_STATS_PATH: &str = "/api/workspaces/{slug}/user-stats/{user_id}/";
/// W22 (`app/urls/workspace.py:163-167`).
pub const USER_ACTIVITY_PATH: &str = "/api/workspaces/{slug}/user-activity/{user_id}/";
/// W24 (`app/urls/workspace.py:173-177`).
pub const USER_PROFILE_PATH: &str = "/api/workspaces/{slug}/user-profile/{user_id}/";
/// W25 (`app/urls/workspace.py:178-182`).
pub const USER_ISSUES_PATH: &str = "/api/workspaces/{slug}/user-issues/{user_id}/";
/// U12 (`app/urls/user.py:65`).
pub const ME_ACTIVITIES_PATH: &str = "/api/users/me/activities/";

// ---------------------------------------------------------------------------
// Shared request plumbing (cycle-issues precedent)
// ---------------------------------------------------------------------------

type HandlerResult = Result<Response, Denial>;

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// `request.user` from the Django session (`app_issues` actor rule):
/// missing session, missing key, or a non-UUID id is anonymous → 401.
fn actor_user_id(
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

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Badly-formed UUIDs in paths render the `ValidationError` branch
/// (`app/views/base.py:126-130`): 400 `{"error": "Please provide valid
/// detail"}`.
const INVALID_DETAIL_MSG: &str = "Please provide valid detail";

fn parse_uuid_or_invalid(raw: &str) -> Result<uuid::Uuid, Denial> {
    raw.parse::<uuid::Uuid>()
        .map_err(|_| Denial::BadError(INVALID_DETAIL_MSG.to_owned()))
}

/// The actor's `user_timezone` (`users.user_timezone`), parsed for DRF
/// serializer rendering (`enforce_timezone` against the activated actor
/// zone). A missing row or an unparseable zone is a 500 (the pilot reads
/// the same column through its gate context).
async fn actor_timezone(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<chrono_tz::Tz, Denial> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// Workspace role facts for one `(user, slug)` over the same rows the
/// permission classes read (`WorkspaceMember.objects`, soft-deletion
/// scoped, `workspace__slug` + `member` + `is_active`).
async fn fetch_workspace_role(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<Option<i16>, Denial> {
    let row: Option<(i16,)> = sqlx::query_as(
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
    Ok(row.map(|row| row.0))
}

/// Run one class gate (`WorkspaceEntityPermission` /
/// `WorkspaceViewerPermission`): anonymous already 401'd; `Allow`
/// yields `None` (the body runs), anything else yields the denial
/// response. Unlike the decorator gates, class denials render
/// [`gates::CLASS_DENIED_BODY`], not `Denial::Forbidden` — so the
/// denial is returned as a response here instead of flowing through
/// [`Denial`].
async fn check_class_gate(
    pool: &sqlx::PgPool,
    gate: &gates::Gate,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<Option<Response>, Denial> {
    use pidash_auth::permissions::workspace::WorkspaceFacts;
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
    let deny = || {
        Response::builder()
            .status(StatusCode::FORBIDDEN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(gates::CLASS_DENIED_BODY))
            .expect("class denial response")
    };
    let role = fetch_workspace_role(pool, slug, user_id).await?;
    let facts = WorkspaceFacts {
        workspace: pidash_types::WorkspaceId::from(slug),
        authenticated: true,
        has_admin_or_member_role: role
            .map(|role| i32::from(role) == ROLE_ADMIN || i32::from(role) == ROLE_MEMBER)
            .unwrap_or(false),
        has_admin_role: role
            .map(|role| i32::from(role) == ROLE_ADMIN)
            .unwrap_or(false),
        is_member: role.is_some(),
        is_admin_unfiltered: role
            .map(|role| i32::from(role) == ROLE_ADMIN)
            .unwrap_or(false),
    };
    let scope = gates::tenant_context(slug);
    let outcome = match gate {
        gates::Gate::ClassEntity => gates::decide_class_entity("GET", &scope, &facts),
        gates::Gate::ClassViewer => gates::decide_class_viewer(&scope, &facts),
        _ => gates::GateOutcome::DenyClass,
    };
    if outcome == gates::GateOutcome::Allow {
        Ok(None)
    } else {
        Ok(Some(deny()))
    }
}

// ---------------------------------------------------------------------------
// Unit 1 — user profile (W24, `user.py:281-369`)
// ---------------------------------------------------------------------------

/// `FileAsset.asset_url` over the joined entity row
/// (`db/models/asset.py:80-100`): the four static types render
/// `/api/assets/v2/static/<id>/`; anything else maps to `None`
/// (pathological residual — live upload paths always use the static
/// types for avatar/cover/logo assets).
fn static_asset_url(entity_type: Option<&str>, asset_id: &str) -> Option<String> {
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Some(format!("/api/assets/v2/static/{asset_id}/"))
        }
        _ => None,
    }
}

/// One `project_data` row (`user.py:343-351`), pre-rendered.
struct ProfileProject {
    id: String,
    /// Raw JSON (`logo_props` is a JSON object column).
    logo_props: String,
    created_issues: i64,
    assigned_issues: i64,
    completed_issues: i64,
    pending_issues: i64,
}

/// [`qp::profile_user_sql`] row: the 8 `user_data` columns plus the
/// `avatar` / `cover_image` source columns (email and cover are
/// nullable; `date_joined` is `timestamptz`).
type ProfileUserRow = (
    Option<String>,
    String,
    String,
    String,
    Option<uuid::Uuid>,
    Option<String>,
    Option<uuid::Uuid>,
    chrono::DateTime<chrono::Utc>,
    String,
    String,
);

/// The 8-key inline `user_data` dict (`user.py:356-365`), pre-rendered.
struct ProfileUserData {
    email: Option<String>,
    first_name: String,
    last_name: String,
    avatar_url: Option<String>,
    cover_image_url: Option<String>,
    date_joined: String,
    user_timezone: String,
    display_name: String,
}

/// Render the profile envelope in emission order
/// (`user.py:353-368`): `project_data` first, then `user_data`.
fn render_profile_body(projects: &[ProfileProject], user: &ProfileUserData) -> String {
    fn opt(value: &Option<String>) -> String {
        match value {
            Some(text) => json_string(text),
            None => "null".to_owned(),
        }
    }
    let mut rows = String::from("[");
    for (index, project) in projects.iter().enumerate() {
        if index > 0 {
            rows.push(',');
        }
        rows.push_str(&format!(
            "{{\"id\":{},\"logo_props\":{},\"created_issues\":{},\"assigned_issues\":{},\"completed_issues\":{},\"pending_issues\":{}}}",
            json_string(&project.id),
            project.logo_props,
            project.created_issues,
            project.assigned_issues,
            project.completed_issues,
            project.pending_issues,
        ));
    }
    rows.push(']');
    format!(
        "{{\"project_data\":{rows},\"user_data\":{{\"email\":{},\"first_name\":{},\"last_name\":{},\"avatar_url\":{},\"cover_image_url\":{},\"date_joined\":{},\"user_timezone\":{},\"display_name\":{}}}}}",
        opt(&user.email),
        json_string(&user.first_name),
        json_string(&user.last_name),
        opt(&user.avatar_url),
        opt(&user.cover_image_url),
        json_string(&user.date_joined),
        json_string(&user.user_timezone),
        json_string(&user.display_name),
    )
}

/// `WorkspaceUserProfileEndpoint.get` (`user.py:281-369`).
async fn user_profile(
    State(state): State<AppState>,
    Path((slug, user_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let viewer = actor_user_id(extension)?;
    let user_id = parse_uuid_or_invalid(&user_raw)?;
    // `User.objects.get(pk=user_id)` (`:283`): miss → 404.
    let row: Option<ProfileUserRow> = sqlx::query_as(&qp::profile_user_sql())
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (
        email,
        first_name,
        last_name,
        avatar,
        avatar_asset_id,
        cover_image,
        cover_image_asset_id,
        date_joined,
        user_timezone,
        display_name,
    ) = row.ok_or(Denial::NotFound)?;
    // `WorkspaceMember.objects.get(...)` (`:285-287`): miss → 404 (this,
    // not a gate, is what 404s non-members — the view has no
    // `permission_classes`).
    let role: Option<(i16,)> = sqlx::query_as(&qp::requester_role_sql())
        .bind(&slug)
        .bind(viewer)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let role = role.map(|row| row.0).ok_or(Denial::NotFound)?;
    // Asset entity rows for the `avatar_url` / `cover_image_url`
    // properties (`db/models/user.py:143-165`, FK-first, text fallback).
    let mut avatar_entity: Option<Option<String>> = None;
    let mut cover_entity: Option<Option<String>> = None;
    if avatar_asset_id.is_some() || cover_image_asset_id.is_some() {
        let rows: Vec<(uuid::Uuid, Option<String>)> =
            sqlx::query_as("SELECT id, entity_type FROM file_assets WHERE id = ANY($1)")
                .bind(
                    [avatar_asset_id, cover_image_asset_id]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>(),
                )
                .fetch_all(&pool)
                .await
                .map_err(|_| Denial::ServerError)?;
        for (id, entity) in &rows {
            if Some(*id) == avatar_asset_id {
                avatar_entity = Some(entity.clone());
            }
            if Some(*id) == cover_image_asset_id {
                cover_entity = Some(entity.clone());
            }
        }
    }
    let avatar_entity_type: Option<String> = avatar_entity.flatten();
    let avatar_url = match avatar_asset_id {
        Some(id) => {
            let url = static_asset_url(avatar_entity_type.as_deref(), &id.to_string());
            user_model::avatar_url(true, url.as_deref(), &avatar).map(str::to_owned)
        }
        None => user_model::avatar_url(false, None, &avatar).map(str::to_owned),
    };
    let cover_entity_type: Option<String> = cover_entity.flatten();
    let cover_image_url = match cover_image_asset_id {
        Some(id) => {
            let url = static_asset_url(cover_entity_type.as_deref(), &id.to_string());
            user_model::cover_image_url(true, url.as_deref(), cover_image.as_deref())
                .map(str::to_owned)
        }
        None => user_model::cover_image_url(false, None, cover_image.as_deref()).map(str::to_owned),
    };
    // `role >= 15` branch (`:289`, the literal — guests keep `[]`).
    let mut projects = Vec::new();
    if qp::requester_sees_projects(i32::from(role)) {
        let rows = sqlx::query(&qp::profile_projects_sql())
            .bind(user_id)
            .bind(&slug)
            .bind(viewer)
            .fetch_all(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        for row in rows {
            let id: uuid::Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
            let logo_props: Value = row.try_get("logo_props").map_err(|_| Denial::ServerError)?;
            let created: i64 = row
                .try_get("created_issues")
                .map_err(|_| Denial::ServerError)?;
            let assigned: i64 = row
                .try_get("assigned_issues")
                .map_err(|_| Denial::ServerError)?;
            let completed: i64 = row
                .try_get("completed_issues")
                .map_err(|_| Denial::ServerError)?;
            let pending: i64 = row
                .try_get("pending_issues")
                .map_err(|_| Denial::ServerError)?;
            projects.push(ProfileProject {
                id: id.to_string(),
                logo_props: serde_json::to_string(&logo_props).map_err(|_| Denial::ServerError)?,
                created_issues: created,
                assigned_issues: assigned,
                completed_issues: completed,
                pending_issues: pending,
            });
        }
    }
    Ok(json_response(
        StatusCode::OK,
        render_profile_body(
            &projects,
            &ProfileUserData {
                email,
                first_name,
                last_name,
                avatar_url,
                cover_image_url,
                date_joined: crate::serializer::render_datetime(&date_joined),
                user_timezone,
                display_name,
            },
        ),
    ))
}

// ---------------------------------------------------------------------------
// Unit 2 — user issues (W25, `user.py:99-251`)
// ---------------------------------------------------------------------------

/// The four R1 annotation selects over the outer `issue` alias:
/// [`qp::profile_issues_annotations_sql`] with the correlation adapted
/// `= i.id` → `= issue.id`. The replacement count is pinned (4) so a
/// QRY-C reshape fails loudly instead of silently dropping a column.
fn profile_annotations_for_issue() -> String {
    let adapted = qp::profile_issues_annotations_sql().replace("= i.id", "= issue.id");
    debug_assert_eq!(adapted.matches("= issue.id").count(), 4);
    adapted
}

/// Scalar selects for the profile-issues rows: the base `issue_on_results`
/// columns plus the QRY-C R1 annotations plus the three grouper `Coalesce`
/// arrays. `skip_arrays` drops one array per m2m group level (the grouper
/// swaps the array for the raw key at BOTH levels — `grouper.py:107-136`).
/// The `module_ids` array carries NO `m.deleted_at` guard (`grouper.py:63-72`
/// filters only `module__archived_at` — the pilot's extra guard is its own
/// disclosed divergence).
fn profile_row_selects(skip_arrays: &[&str]) -> String {
    let mut selects = format!(
        r#"issue.id, issue.name, issue.state_id, issue.sort_order, issue.completed_at,
        issue.estimate_point_id AS estimate_point, issue.priority, issue.start_date,
        issue.target_date, issue.sequence_id, issue.project_id, issue.parent_id,
        {annotations},
        issue.created_at, issue.updated_at, issue.created_by_id AS created_by,
        issue.updated_by_id AS updated_by, issue.is_draft, issue.archived_at,
        state."group" AS "state__group""#,
        annotations = profile_annotations_for_issue(),
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

/// Array names skipped for the active m2m group levels.
fn skip_arrays_for(group_by: Option<&str>, sub_group_by: Option<&str>) -> Vec<&'static str> {
    [group_by, sub_group_by]
        .into_iter()
        .flatten()
        .filter_map(skip_array_for)
        .collect()
}

/// Raw m2m key selects for the active group levels (the grouper buckets
/// each joined row by its raw key).
fn group_member_selects(group_by: Option<&str>, sub_group_by: Option<&str>) -> String {
    let mut out = String::new();
    for group in [group_by, sub_group_by].into_iter().flatten() {
        out.push_str(group_member_select(group));
    }
    out
}

/// Relation joins the profile-issues list can use: same alias contract as
/// the D-26 list (`issue_assignee`, `issue_cycle`, `issue_module`,
/// `issue_mention`, `label_issue`, `issue_subscribers`, `issue_intake`).
const RELATION_JOINS: &[(&str, &str)] = &[
    ("issue_assignees", "issue_assignee"),
    ("cycle_issues", "issue_cycle"),
    ("module_issues", "issue_module"),
    ("issue_mentions", "issue_mention"),
    ("issue_labels", "label_issue"),
    ("issue_subscribers", "issue_subscribers"),
];

/// True when every `"alias".` / `alias.` reference in the filter SQL is an
/// `IS NULL` / `IS NOT NULL` test (the `__isnull` predicates take the
/// LEFT join). Both spellings are scanned: sea-query renders quoted
/// identifiers, the legacy compiler unquoted columns.
fn alias_nullable_only(where_sql: &str, alias: &str) -> bool {
    for marker in [format!("\"{alias}\"."), format!("{alias}.")] {
        let mut rest = where_sql;
        while let Some(start) = rest.find(&marker) {
            // Skip matches that are actually a longer identifier (e.g.
            // `issue.` inside `issue_assignee.`): the char before the
            // marker must not be an identifier char. `issue.` never
            // appears inside `issue_*` aliases (no dot follows `issue`
            // there), so only the boundary before matters.
            let before_ok = rest[..start]
                .chars()
                .next_back()
                .map(|char| !(char.is_alphanumeric() || char == '_' || char == '"'))
                .unwrap_or(true);
            let after = &rest[start + marker.len()..];
            let ident_len = after
                .chars()
                .take_while(|char| char.is_alphanumeric() || *char == '_' || *char == '"')
                .map(|char| char.len_utf8())
                .sum::<usize>();
            let tail = after[ident_len..].trim_start();
            if before_ok && !(tail.starts_with("IS NULL") || tail.starts_with("IS NOT NULL")) {
                return false;
            }
            rest = &after[ident_len..];
        }
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

/// The `GROUP_FILTER_MAPPER` deleted guards for the active m2m groups
/// (`grouper.py:41-49`): `Q(<relation>__deleted_at__isnull=True)`.
fn group_deleted_guards(group_by: Option<&str>, sub_group_by: Option<&str>) -> Vec<String> {
    let mut guards = Vec::new();
    for group in [group_by, sub_group_by].into_iter().flatten() {
        let alias = match group {
            "assignees__id" => "issue_assignee",
            "labels__id" => "label_issue",
            "issue_module__module_id" => "issue_module",
            _ => continue,
        };
        let guard = format!("{alias}.deleted_at IS NULL");
        if !guards.contains(&guard) {
            guards.push(guard);
        }
    }
    guards
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

/// Filtered `FROM … WHERE` for the profile-issues list (`user.py:140-154`):
/// the QRY-C R2 scope as the `id IN (...)` id-set (viewer-owned project
/// scope, manager scope applied twice) plus the `ComplexFilterBackend`
/// tree plus the legacy `issue_filters(params, "GET")` predicates, over
/// the pilot `issue` / `state` aliases so every compiled fragment splices
/// verbatim. Downstream consumers are all `DISTINCT issue.id`-based, so
/// the id-set wrap is equivalent to Django's single queryset.
fn profile_filtered_set(
    user_id: &uuid::Uuid,
    slug: &str,
    viewer: &uuid::Uuid,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
) -> Result<FilteredSet, Denial> {
    let mut binder = Binder::new();
    // Bind order is the QRY-C contract: `$1` target, `$2` slug, `$3`
    // viewer. The base fragment is pushed verbatim (no splice — its
    // `$1/$2/$3` already name these binds); later fragments renumber
    // from `$4` on.
    binder.bind_uuid(*user_id);
    binder.bind_string(slug.to_owned());
    binder.bind_uuid(*viewer);
    let mut fragments = vec![format!(
        "issue.id IN (SELECT i.id {})",
        qp::profile_issues_from_where()
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
    fragments.extend(group_deleted_guards(group_by, sub_group_by));
    let where_sql = fragments.join(" AND ");
    let mut joins = String::new();
    let mut referenced = Vec::new();
    for (table, alias) in RELATION_JOINS {
        let quoted = format!("\"{alias}\".");
        let bare = format!("{alias}.");
        let mentioned = where_sql.contains(&quoted) || where_sql.contains(&bare);
        let nullable_only = mentioned && alias_nullable_only(&where_sql, alias);
        let used = mentioned || group_join_alias(group_by, sub_group_by, alias);
        let inner = used && !nullable_only || group_join_alias(group_by, sub_group_by, alias);
        if used {
            referenced.push(*alias);
            let kind = if inner { "INNER JOIN" } else { "LEFT JOIN" };
            joins.push_str(&format!(
                " {kind} {table} AS {alias} ON {alias}.issue_id = issue.id"
            ));
        }
    }
    if where_sql.contains("\"issue_intake\".") || where_sql.contains("issue_intake.") {
        referenced.push("issue_intake");
        let kind = if alias_nullable_only(&where_sql, "issue_intake") {
            "LEFT JOIN"
        } else {
            "INNER JOIN"
        };
        joins.push_str(&format!(
            " {kind} intake_issues AS issue_intake ON issue_intake.issue_id = issue.id"
        ));
    }
    // The grouped-totals `count_filter` leg (QRY-C `ii` alias). Present
    // only on grouped paths; the pilot likewise keeps an always-LEFT
    // intake leg on its list queries.
    if group_by.is_some() {
        joins.push_str(&format!(" {}", qp::grouped_count_filter_join("issue")));
    }
    let from_where = format!(
        r#"FROM issues AS issue
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

/// `WorkspaceUserProfileIssuesEndpoint.get` (`user.py:99-251`).
async fn user_profile_issues(
    State(state): State<AppState>,
    Path((slug, user_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let viewer = actor_user_id(extension)?;
    // Class gates run in `initial()`, before the body parses `user_id`
    // (a non-member with a garbage id 403s, never 400s).
    let gate = gates::gate_for("GET", "workspaces/<slug>/user-issues/<user_id>/")
        .expect("user-issues gate")
        .gate;
    if let Some(denied) = check_class_gate(&pool, &gate, &slug, &viewer).await? {
        return Ok(denied);
    }
    let user_id = parse_uuid_or_invalid(&user_raw)?;
    // Group clash is checked in-view before `paginate` parses per_page
    // (`user.py:174-182`).
    if let Some(mismatch) = qp::profile_group_mismatch(
        query_last(&query, "group_by").as_deref(),
        query_last(&query, "sub_group_by").as_deref(),
    ) {
        return Err(Denial::BadError(mismatch.message));
    }
    let params = ListParams::parse_with(
        &multi_map(&query),
        ParseOptions {
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
    let filtered = profile_filtered_set(&user_id, &slug, &viewer, &query, group_by, sub_group_by)?;
    // Total count over the pre-annotation set (the `deepcopy` at `:157`).
    let total_sql = format!("SELECT COUNT(DISTINCT issue.id) {}", filtered.from_where);
    let total_count = fetch_count(&pool, &total_sql, filtered.values.clone()).await?;
    // `order_issue_queryset` (`:162-165`), default `-created_at`.
    let order_spec = qp::profile_issues_order(Some(&params.order_by), "state.\"group\"", |name| {
        format!("min_{}", name.replace("__", "_"))
    });
    let (key_expr, descending) = order_key(&order_spec.out_param, &params.order_by)?;
    let direction = if descending { "DESC" } else { "ASC" };
    let per_page = params.per_page;
    let cursor = crate::paginator::Cursor::from_string(&params.cursor_raw).map_err(page_denial)?;
    if group_by.is_some() {
        return profile_grouped_response(
            &pool,
            &slug,
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
    profile_flat_response(&pool, &filtered, &key_expr, direction, per_page, cursor).await
}

/// Plain `paginate` branch (`user.py:243-250`): distinct rows, the
/// rewritten order key `NULLS LAST` plus `-created_at`, offset window.
async fn profile_flat_response(
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
    let limit = per_page.min(1000);
    let window = offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    let selects = profile_row_selects(&[]);
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
    let fields = on_results_fields(None, None);
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
/// (`GroupedOffsetPaginator.__get_total_queryset`, `paginator.py:297-303`),
/// via [`fetch_json_rows`] (no local binder needed).
async fn profile_group_total_pairs(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    group_expr: &str,
    count_filter: &str,
) -> Result<Vec<(String, i64)>, Denial> {
    let sql = format!(
        "SELECT COALESCE(({group_expr})::text, 'None') AS bucket, COUNT(DISTINCT issue.id) FILTER (WHERE {count_filter}) AS n {}",
        filtered.from_where,
    );
    let rows = fetch_json_rows(pool, &sql, filtered.values.clone()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let bucket = row
            .get("bucket")
            .and_then(Value::as_str)
            .ok_or(Denial::ServerError)?
            .to_owned();
        let count = row
            .get("n")
            .and_then(Value::as_i64)
            .ok_or(Denial::ServerError)?;
        out.push((bucket, count));
    }
    Ok(out)
}

/// `(group, sub, count)` pairs for the nested sub-totals.
async fn profile_sub_total_pairs(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    group_expr: &str,
    sub_expr: &str,
    count_filter: &str,
) -> Result<Vec<(String, String, i64)>, Denial> {
    let sql = format!(
        "SELECT COALESCE(({group_expr})::text, 'None') AS bucket, COALESCE(({sub_expr})::text, 'None') AS sub, COUNT(DISTINCT issue.id) FILTER (WHERE {count_filter}) AS n {}",
        filtered.from_where,
    );
    let rows = fetch_json_rows(pool, &sql, filtered.values.clone()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let bucket = row
            .get("bucket")
            .and_then(Value::as_str)
            .ok_or(Denial::ServerError)?
            .to_owned();
        let sub = row
            .get("sub")
            .and_then(Value::as_str)
            .ok_or(Denial::ServerError)?
            .to_owned();
        let count = row
            .get("n")
            .and_then(Value::as_i64)
            .ok_or(Denial::ServerError)?;
        out.push((bucket, sub, count));
    }
    Ok(out)
}

/// Shape one row into an ordered map for the groupers (datetimes as
/// stored — no `user_timezone_converter` on the grouped path).
fn profile_shape_map(row: &Map<String, Value>, fields: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    for field in fields {
        let value = row.get(field).unwrap_or(&Value::Null);
        out.insert(field.clone(), profile_shape_json_value(field, value));
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

fn profile_shape_json_value(field: &str, value: &Value) -> Value {
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

/// Grouped / sub-grouped branch (`user.py:174-242`): window-function
/// pagination over the group partition, totals with the intake /
/// archived / draft `count_filter`, buckets from QRY-C
/// [`qp::profile_group_values`].
#[allow(clippy::too_many_arguments)]
async fn profile_grouped_response(
    pool: &sqlx::PgPool,
    slug: &str,
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
    let limit = per_page.min(1000);
    let window = grouped_window(limit, cursor.offset, cursor.value, None).map_err(page_denial)?;
    let group_expr = group_expression(group_by)?;
    let skipped = skip_arrays_for(Some(group_by), sub_group_by.as_deref());
    let selects = profile_row_selects(&skipped);
    let member_select = group_member_selects(Some(group_by), sub_group_by.as_deref());
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
    // `count_filter` for grouped pagination (`user.py:207-214,234-241`),
    // wired via QRY-C (the `ii` leg rides in the filtered set).
    let count_filter = qp::grouped_count_filter_sql("issue");
    let group_totals =
        profile_group_total_pairs(pool, filtered, &group_expr, &count_filter).await?;
    if !window_empty && group_totals.is_empty() {
        // `...order_by("-count")[0]` on an empty group list: IndexError.
        return Err(Denial::ServerError);
    }
    let totals = total_dict(&group_totals);
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let fields = on_results_fields(Some(group_by), sub_group_by.as_deref());
    let shaped: Vec<Map<String, Value>> = page
        .iter()
        .map(|row| profile_shape_map(row, &fields))
        .collect();
    let group_fields = profile_group_values_for(pool, group_by, slug, filtered).await?;
    let results_value = match &sub_expr {
        Some(sub) => {
            let sub_pairs =
                profile_sub_total_pairs(pool, filtered, &group_expr, sub, &count_filter).await?;
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
    let top = group_totals
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
            grouped_max_hits(window_empty, top, limit).map_err(page_denial)?,
            total_count,
            &results_json,
        ),
    ))
}

/// `issue_group_values(field, slug, filters, queryset)` for the profile
/// path (`grouper.py:146-224`, no `project_id`), wired via QRY-C
/// [`qp::profile_group_values`]. The `Sql` branches bind `$1 = slug` and
/// append `"None"` per [`qp::group_values_appends_none`]; the `Static`
/// branches are the merged kernels; the `DistinctOverFilteredSet`
/// branches run over the filtered set with the Django
/// `get_extra_select` quirk (values duplicate, NULLs kept as `"None"`),
/// duplicates collapsing like the downstream dict-comp.
async fn profile_group_values_for(
    pool: &sqlx::PgPool,
    field: &str,
    slug: &str,
    filtered: &FilteredSet,
) -> Result<Vec<String>, Denial> {
    let Some(source) = qp::profile_group_values(field) else {
        return Ok(Vec::new());
    };
    match source {
        qp::GroupValuesSource::Static(values) => {
            Ok(values.iter().map(|value| (*value).to_owned()).collect())
        }
        qp::GroupValuesSource::Sql(sql) => {
            let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(&sql)
                .bind(slug)
                .fetch_all(pool)
                .await
                .map_err(|_| Denial::ServerError)?;
            let mut out: Vec<String> = rows.into_iter().map(|row| row.0.to_string()).collect();
            if qp::group_values_appends_none(field) {
                out.push("None".to_owned());
            }
            Ok(out)
        }
        qp::GroupValuesSource::DistinctOverFilteredSet(_) => {
            // Same quirk as `group_values_distinct_sql` (extra
            // `created_at` select pairing `DISTINCT`, NULLs kept), over
            // the outer `issue` alias. `::text` keeps str() parity
            // (dates `YYYY-MM-DD`, UUIDs hyphenated).
            let column = match field {
                "target_date" => "issue.target_date",
                "start_date" => "issue.start_date",
                "created_by" => "issue.created_by_id",
                _ => return Err(Denial::ServerError),
            };
            let sql = format!(
                "SELECT DISTINCT ({column})::text AS bucket, issue.created_at AS ordering_created_at {} ORDER BY issue.created_at DESC",
                filtered.from_where,
            );
            let rows = fetch_json_rows(pool, &sql, filtered.values.clone()).await?;
            let mut out = Vec::new();
            for row in rows {
                let bucket = match row.get("bucket") {
                    Some(Value::String(text)) => text.clone(),
                    _ => "None".to_owned(),
                };
                if !out.contains(&bucket) {
                    out.push(bucket);
                }
            }
            Ok(out)
        }
    }
}

// ---------------------------------------------------------------------------
// Unit 3 — user stats (W21, `user.py:397-522`)
// ---------------------------------------------------------------------------

/// A compiled legacy `issue_filters` fragment plus its binds, numbered
/// from `$1` (the handler splices it into each stats statement).
struct CompiledLegacy {
    text: String,
    values: Vec<sea_query::Value>,
}

/// Rewrite a pilot-compiled legacy fragment (`issue.` / `state.` alias
/// contract) onto the QRY-C stats aliases (`i.` / `s.`). Relation
/// aliases are kept (`label_issue.`, …) and joined by
/// [`stats_relation_joins`]; `issue_assignee.` reuses the preamble's
/// `ia` join (Django reuses the same join for scope + filter
/// conditions).
/// Replace `needle` with `replacement` only at identifier boundaries:
/// the char before the match must not be alphanumeric, `_` or `"`, so
/// relation aliases containing the needle (`label_issue.`) survive.
fn replace_at_boundary(text: &str, needle: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(needle) {
        let boundary = rest[..start]
            .chars()
            .next_back()
            .map(|char| !(char.is_alphanumeric() || char == '_' || char == '"'))
            .unwrap_or(true);
        out.push_str(&rest[..start]);
        if boundary {
            out.push_str(replacement);
        } else {
            out.push_str(needle);
        }
        rest = &rest[start + needle.len()..];
    }
    out.push_str(rest);
    out
}

fn rewrite_stats_aliases(fragment: &str) -> String {
    // Specific aliases first: `issue_assignee.` must become `ia.` before
    // the general `issue.` → `i.` pass. The general pass is
    // boundary-aware so `label_issue.` (which contains `issue.`)
    // survives; the `issue_*` relation aliases spell `issue_` (never
    // `issue.`) and are untouched either way.
    let fragment = fragment
        .replace("\"issue_assignee\".", "\"ia\".")
        .replace("issue_assignee.", "ia.")
        .replace("\"issue\".", "\"i\".")
        .replace("\"state\".", "\"s\".");
    let fragment = replace_at_boundary(&fragment, "issue.", "i.");
    fragment.replace("state.", "s.")
}

/// Relation legs a rewritten stats fragment references, over the stats
/// `i` alias. Django joins value predicates INNER and `__isnull`-only
/// relations LEFT; `ia` reuses the preamble join when the query has one
/// (Q1/Q2/Q4-Q6) and is added for Q3.
fn stats_relation_joins(rewritten: &str, has_assignee_join: bool) -> String {
    const LEGS: &[(&str, &str)] = &[
        ("label_issue", "issue_labels"),
        ("issue_cycle", "cycle_issues"),
        ("issue_module", "module_issues"),
        ("issue_mention", "issue_mentions"),
        ("issue_subscribers", "issue_subscribers"),
        ("issue_intake", "intake_issues"),
    ];
    let mut joins = String::new();
    let referenced = |alias: &str| {
        rewritten.contains(&format!("\"{alias}\".")) || rewritten.contains(&format!("{alias}."))
    };
    for (alias, table) in LEGS {
        if !referenced(alias) {
            continue;
        }
        let kind = if alias_nullable_only(rewritten, alias) {
            "LEFT JOIN"
        } else {
            "INNER JOIN"
        };
        joins.push_str(&format!(
            " {kind} {table} AS {alias} ON {alias}.issue_id = i.id"
        ));
    }
    if referenced("ia") && !has_assignee_join {
        let kind = if alias_nullable_only(rewritten, "ia") {
            "LEFT JOIN"
        } else {
            "INNER JOIN"
        };
        joins.push_str(&format!(" {kind} issue_assignees ia ON ia.issue_id = i.id"));
    }
    joins
}

/// Insert relation joins at the end of the FROM chain (before WHERE).
/// Q1-Q6 statements carry no subqueries, so the first ` WHERE ` is the
/// clause. INNER legs commute; LEFT legs preserve the left rows from any
/// position — equivalent to Django's join order.
fn insert_stats_joins(statement: String, joins: &str) -> Result<String, Denial> {
    if joins.is_empty() {
        return Ok(statement);
    }
    match statement.split_once(" WHERE ") {
        Some((head, tail)) => Ok(format!("{head}{joins} WHERE {tail}")),
        None => Err(Denial::ServerError),
    }
}

/// Compile the legacy predicates for the Q7 subscriber scope
/// (`user.py:484-494`): the SAME `**filters` dict, resolved against
/// `IssueSubscriber`. Only `created_by`, `project` and `created_at`
/// lookups resolve there; every other name is a Django `FieldError` →
/// generic 500. Date validation mirrors the pilot compiler (garbage
/// bounds are `ValidationError` → 400 invalid detail).
fn subscriber_legacy_sql(
    binder: &mut Binder,
    name: &str,
    value: &FilterValue,
) -> Result<String, Denial> {
    const INVALID: &str = "Please provide valid detail";
    if let Some(path) = name.strip_suffix("__isnull") {
        if path != "created_by" {
            return Err(Denial::ServerError);
        }
        let flag = match value {
            FilterValue::Flag(flag) => *flag,
            _ => return Err(Denial::ServerError),
        };
        return Ok(if flag {
            "sub.created_by_id IS NULL".to_owned()
        } else {
            "sub.created_by_id IS NOT NULL".to_owned()
        });
    }
    match value {
        FilterValue::Uuids(ids) => {
            let column = match name {
                "created_by__in" => "sub.created_by_id",
                "project__in" => "sub.project_id",
                _ => return Err(Denial::ServerError),
            };
            if ids.is_empty() {
                return Ok("FALSE".to_owned());
            }
            let mut holders = Vec::with_capacity(ids.len());
            for id in ids {
                holders.push(binder.bind_uuid(*id));
            }
            Ok(format!("{column} IN ({})", holders.join(",")))
        }
        FilterValue::Strings(_) => Err(Denial::ServerError),
        FilterValue::Text(text) => {
            let Some((term, operator)) = name.rsplit_once("__") else {
                return Err(Denial::ServerError);
            };
            if term != "created_at__date" {
                return Err(Denial::ServerError);
            }
            if text.parse::<chrono::NaiveDate>().is_err()
                && chrono::DateTime::parse_from_rfc3339(text).is_err()
            {
                return Err(Denial::BadError(INVALID.to_owned()));
            }
            match operator {
                "contains" => {
                    let escaped = text
                        .replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_");
                    let holder = binder.bind_string(format!("%{escaped}%"));
                    Ok(format!("sub.created_at::text LIKE {holder}"))
                }
                "gte" | "lte" => {
                    let holder = binder.bind_string(text.clone());
                    let operator = if operator == "gte" { ">=" } else { "<=" };
                    Ok(format!("sub.created_at::date {operator} {holder}::date"))
                }
                _ => Err(Denial::ServerError),
            }
        }
        FilterValue::Flag(_) => Err(Denial::ServerError),
        FilterValue::Day(day) => {
            let Some((term, operator)) = name.rsplit_once("__") else {
                return Err(Denial::ServerError);
            };
            if term != "created_at__date" {
                return Err(Denial::ServerError);
            }
            let operator = match operator {
                "gte" => ">=",
                "lte" => "<=",
                _ => return Err(Denial::ServerError),
            };
            let holder = binder.bind_string(day.to_string());
            Ok(format!("sub.created_at::date {operator} {holder}::date"))
        }
        FilterValue::Null => {
            if name != "created_by" {
                return Err(Denial::ServerError);
            }
            Ok("sub.created_by_id IS NULL".to_owned())
        }
    }
}

/// Compile the request's legacy `issue_filters(params, "GET")` once per
/// scope: the pilot compiler for issue-rooted queries (Q1-Q6),
/// [`subscriber_legacy_sql`] for Q7. Returns the `(issue, subscriber)`
/// fragments; empty text means unfiltered.
fn compile_stats_legacy(query: &QueryMap) -> Result<(CompiledLegacy, CompiledLegacy), Denial> {
    let flat: HashMap<String, String> = query
        .keys()
        .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let legacy = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
        .map_err(|_| Denial::ServerError)?;
    let mut issue_binder = Binder::new();
    let mut issue_text = String::new();
    let mut sub_binder = Binder::new();
    let mut sub_text = String::new();
    for (name, value) in legacy.predicates() {
        let fragment = legacy_sql(&mut issue_binder, name, value)?;
        if issue_text.is_empty() {
            issue_text = fragment;
        } else {
            issue_text = format!("{issue_text} AND {fragment}");
        }
        let fragment = subscriber_legacy_sql(&mut sub_binder, name, value)?;
        if sub_text.is_empty() {
            sub_text = fragment;
        } else {
            sub_text = format!("{sub_text} AND {fragment}");
        }
    }
    Ok((
        CompiledLegacy {
            text: issue_text,
            values: issue_binder.values(),
        },
        CompiledLegacy {
            text: sub_text,
            values: sub_binder.values(),
        },
    ))
}

/// Run one Q1-Q6 statement: fresh binder (`$1` target, `$2` slug, `$3`
/// viewer), legacy splice with alias rewrite, relation joins inserted
/// before WHERE.
async fn stats_issue_rows(
    pool: &sqlx::PgPool,
    build: fn(Option<&str>) -> String,
    user_id: &uuid::Uuid,
    slug: &str,
    viewer: &uuid::Uuid,
    legacy: &CompiledLegacy,
    has_assignee_join: bool,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let mut binder = Binder::new();
    binder.bind_uuid(*user_id);
    binder.bind_string(slug.to_owned());
    binder.bind_uuid(*viewer);
    let statement = if legacy.text.is_empty() {
        build(None)
    } else {
        let rewritten = rewrite_stats_aliases(&legacy.text);
        let spliced = binder.splice(&rewritten, legacy.values.clone());
        let joins = stats_relation_joins(&rewritten, has_assignee_join);
        insert_stats_joins(build(Some(&spliced)), &joins)?
    };
    fetch_json_rows(pool, &statement, binder.values()).await
}

/// Count variant of [`stats_issue_rows`].
async fn stats_issue_count(
    pool: &sqlx::PgPool,
    build: fn(Option<&str>) -> String,
    user_id: &uuid::Uuid,
    slug: &str,
    viewer: &uuid::Uuid,
    legacy: &CompiledLegacy,
    has_assignee_join: bool,
) -> Result<i64, Denial> {
    let mut binder = Binder::new();
    binder.bind_uuid(*user_id);
    binder.bind_string(slug.to_owned());
    binder.bind_uuid(*viewer);
    let statement = if legacy.text.is_empty() {
        build(None)
    } else {
        let rewritten = rewrite_stats_aliases(&legacy.text);
        let spliced = binder.splice(&rewritten, legacy.values.clone());
        let joins = stats_relation_joins(&rewritten, has_assignee_join);
        insert_stats_joins(build(Some(&spliced)), &joins)?
    };
    fetch_count(pool, &statement, binder.values()).await
}

/// Run the Q7 subscriber statement (subscriber-scope legacy, join-free).
async fn stats_subscriber_count(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
    slug: &str,
    viewer: &uuid::Uuid,
    legacy: &CompiledLegacy,
) -> Result<i64, Denial> {
    let mut binder = Binder::new();
    binder.bind_uuid(*user_id);
    binder.bind_string(slug.to_owned());
    binder.bind_uuid(*viewer);
    let statement = if legacy.text.is_empty() {
        qp::stats_subscribed_count_sql(None)
    } else {
        let spliced = binder.splice(&legacy.text, legacy.values.clone());
        qp::stats_subscribed_count_sql(Some(&spliced))
    };
    fetch_count(pool, &statement, binder.values()).await
}

/// One cycle row (`user.py:500,507`).
struct StatsCycle {
    name: String,
    id: String,
    project_id: String,
}

/// The 9-query stats bundle, pre-rendered.
struct StatsBundle {
    state_distribution: String,
    priority_distribution: String,
    created_issues: i64,
    assigned_issues: i64,
    completed_issues: i64,
    pending_issues: i64,
    subscribed_issues: i64,
    present_cycles: Vec<StatsCycle>,
    upcoming_cycles: Vec<StatsCycle>,
}

/// Render the 9-key stats envelope in emission order (`user.py:509-521`).
fn render_stats_body(bundle: &StatsBundle) -> String {
    fn cycles(rows: &[StatsCycle]) -> String {
        let mut out = String::from("[");
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"cycle__name\":{},\"cycle__id\":{},\"cycle__project_id\":{}}}",
                json_string(&row.name),
                json_string(&row.id),
                json_string(&row.project_id),
            ));
        }
        out.push(']');
        out
    }
    format!(
        "{{\"state_distribution\":{},\"priority_distribution\":{},\"created_issues\":{},\"assigned_issues\":{},\"completed_issues\":{},\"pending_issues\":{},\"subscribed_issues\":{},\"present_cycles\":{},\"upcoming_cycles\":{}}}",
        bundle.state_distribution,
        bundle.priority_distribution,
        bundle.created_issues,
        bundle.assigned_issues,
        bundle.completed_issues,
        bundle.pending_issues,
        bundle.subscribed_issues,
        cycles(&bundle.present_cycles),
        cycles(&bundle.upcoming_cycles),
    )
}

/// `WorkspaceUserProfileStatsEndpoint.get` (`user.py:397-522`).
async fn user_profile_stats(
    State(state): State<AppState>,
    Path((slug, user_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let viewer = actor_user_id(extension)?;
    let user_id = parse_uuid_or_invalid(&user_raw)?;
    // No permission_classes (`:397`): any authenticated caller runs the
    // bundle; the queries scope by requester.
    let (legacy, sub_legacy) = compile_stats_legacy(&query)?;
    let state_rows = stats_issue_rows(
        &pool,
        qp::stats_state_distribution_sql,
        &user_id,
        &slug,
        &viewer,
        &legacy,
        true,
    )
    .await?;
    let mut state_distribution = String::from("[");
    for (index, row) in state_rows.iter().enumerate() {
        if index > 0 {
            state_distribution.push(',');
        }
        let group = match row.get("state_group") {
            Some(Value::String(text)) => json_string(text),
            _ => "null".to_owned(),
        };
        let count = row
            .get("state_count")
            .and_then(Value::as_i64)
            .ok_or(Denial::ServerError)?;
        state_distribution.push_str(&format!(
            "{{\"state_group\":{group},\"state_count\":{count}}}"
        ));
    }
    state_distribution.push(']');
    let priority_rows = stats_issue_rows(
        &pool,
        qp::stats_priority_distribution_sql,
        &user_id,
        &slug,
        &viewer,
        &legacy,
        true,
    )
    .await?;
    let mut priority_distribution = String::from("[");
    for (index, row) in priority_rows.iter().enumerate() {
        if index > 0 {
            priority_distribution.push(',');
        }
        let priority = match row.get("priority") {
            Some(Value::String(text)) => json_string(text),
            _ => "null".to_owned(),
        };
        let count = row
            .get("priority_count")
            .and_then(Value::as_i64)
            .ok_or(Denial::ServerError)?;
        let order = row
            .get("priority_order")
            .and_then(Value::as_i64)
            .ok_or(Denial::ServerError)?;
        priority_distribution.push_str(&format!(
            "{{\"priority\":{priority},\"priority_count\":{count},\"priority_order\":{order}}}"
        ));
    }
    priority_distribution.push(']');
    let created_issues = stats_issue_count(
        &pool,
        qp::stats_created_count_sql,
        &user_id,
        &slug,
        &viewer,
        &legacy,
        false,
    )
    .await?;
    let assigned_issues = stats_issue_count(
        &pool,
        qp::stats_assigned_count_sql,
        &user_id,
        &slug,
        &viewer,
        &legacy,
        true,
    )
    .await?;
    let pending_issues = stats_issue_count(
        &pool,
        qp::stats_pending_count_sql,
        &user_id,
        &slug,
        &viewer,
        &legacy,
        true,
    )
    .await?;
    let completed_issues = stats_issue_count(
        &pool,
        qp::stats_completed_count_sql,
        &user_id,
        &slug,
        &viewer,
        &legacy,
        true,
    )
    .await?;
    let subscribed_issues =
        stats_subscriber_count(&pool, &user_id, &slug, &viewer, &sub_legacy).await?;
    // Q8/Q9 take no legacy filters and no requester scope (`:496-507`).
    let now = chrono::Utc::now();
    let upcoming: Vec<(String, uuid::Uuid, uuid::Uuid)> =
        sqlx::query_as(&qp::stats_upcoming_cycles_sql())
            .bind(&slug)
            .bind(now)
            .bind(user_id)
            .fetch_all(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let present: Vec<(String, uuid::Uuid, uuid::Uuid)> =
        sqlx::query_as(&qp::stats_present_cycles_sql())
            .bind(&slug)
            .bind(now)
            .bind(user_id)
            .fetch_all(&pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let cycles = |rows: Vec<(String, uuid::Uuid, uuid::Uuid)>| {
        rows.into_iter()
            .map(|(name, id, project_id)| StatsCycle {
                name,
                id: id.to_string(),
                project_id: project_id.to_string(),
            })
            .collect::<Vec<_>>()
    };
    Ok(json_response(
        StatusCode::OK,
        render_stats_body(&StatsBundle {
            state_distribution,
            priority_distribution,
            created_issues,
            assigned_issues,
            completed_issues,
            pending_issues,
            subscribed_issues,
            present_cycles: cycles(present),
            upcoming_cycles: cycles(upcoming),
        }),
    ))
}

// ---------------------------------------------------------------------------
// Units 4-5 — user activity + me activities (W22, U12)
// ---------------------------------------------------------------------------

/// Resolve the `OffsetPaginator` order key for the activity lists
/// (`user.py:390`, `user/base.py:399`): the raw `order_by` value names a
/// single-level `IssueActivity` field, `-` prefix for descending.
/// Relation traversals answer 500 (residual — the pilot `order_key`
/// precedent); FK names order by their id column.
fn activity_order_expr(order_by: &str) -> Result<(String, bool), Denial> {
    let descending = order_by.starts_with('-');
    let key = order_by.trim_start_matches('-');
    let column = match key {
        "id" | "created_at" | "updated_at" | "deleted_at" | "verb" | "field" | "old_value"
        | "new_value" | "comment" | "attachments" | "old_identifier" | "new_identifier"
        | "epoch" => format!("a.\"{key}\""),
        "issue" | "issue_id" => "a.issue_id".to_owned(),
        "project" | "project_id" => "a.project_id".to_owned(),
        "workspace" | "workspace_id" => "a.workspace_id".to_owned(),
        "issue_comment" | "issue_comment_id" => "a.issue_comment_id".to_owned(),
        "actor" | "actor_id" => "a.actor_id".to_owned(),
        "created_by" | "created_by_id" => "a.created_by_id".to_owned(),
        "updated_by" | "updated_by_id" => "a.updated_by_id".to_owned(),
        _ => return Err(Denial::ServerError),
    };
    Ok((column, descending))
}

/// Wide row selects for the activity lists over the QRY-C `a` / `w` /
/// `p` / `actor_u` / `i` aliases: every `IssueActivitySerializer` input
/// plus the nested lite columns (actor, issue flat, project lite,
/// workspace lite). Asset entity types resolve in a follow-up batched
/// query ([`fetch_asset_entities`]).
fn activity_row_selects() -> String {
    String::from(
        r#"a.id AS aid, a.created_at AS a_created_at, a.updated_at AS a_updated_at,
        a.deleted_at AS a_deleted_at, a.verb AS verb, a.field AS field,
        a.old_value AS old_value, a.new_value AS new_value, a.comment AS comment,
        a.attachments AS attachments, a.old_identifier AS old_identifier,
        a.new_identifier AS new_identifier, a.epoch AS epoch,
        a.created_by_id AS created_by, a.updated_by_id AS updated_by,
        a.project_id AS project, a.workspace_id AS workspace, a.issue_id AS issue,
        a.issue_comment_id AS issue_comment, a.actor_id AS actor,
        actor_u.id AS actor_id, actor_u.first_name AS actor_first_name,
        actor_u.last_name AS actor_last_name, actor_u.avatar AS actor_avatar,
        actor_u.avatar_asset_id AS actor_avatar_asset_id, actor_u.is_bot AS actor_is_bot,
        actor_u.display_name AS actor_display_name,
        i.id AS issue_id, i.name AS issue_name,
        i.description_json AS issue_description_json,
        i.description_html AS issue_description_html, i.priority AS issue_priority,
        i.complexity_score AS issue_complexity_score, i.start_date AS issue_start_date,
        i.target_date AS issue_target_date, i.sequence_id AS issue_sequence_id,
        i.sort_order AS issue_sort_order, i.is_draft AS issue_is_draft,
        p.id AS project_id, p.identifier AS project_identifier, p.name AS project_name,
        p.cover_image AS project_cover_image,
        p.cover_image_asset_id AS project_cover_image_asset_id,
        p.logo_props AS project_logo_props, p.description AS project_description,
        p.is_default AS project_is_default,
        w.name AS workspace_name, w.slug AS workspace_slug, w.id AS workspace_id,
        w.logo AS workspace_logo, w.logo_asset_id AS workspace_logo_asset_id"#,
    )
}

/// Batch-resolve `entity_type` for a page's asset ids (avatar/cover/logo
/// `asset_url` inputs). Returns `id → entity_type`.
async fn fetch_asset_entities(
    pool: &sqlx::PgPool,
    asset_ids: &[uuid::Uuid],
) -> Result<HashMap<uuid::Uuid, Option<String>>, Denial> {
    let mut out = HashMap::new();
    if asset_ids.is_empty() {
        return Ok(out);
    }
    let rows: Vec<(uuid::Uuid, Option<String>)> =
        sqlx::query_as("SELECT id, entity_type FROM file_assets WHERE id = ANY($1)")
            .bind(asset_ids)
            .fetch_all(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    for (id, entity) in rows {
        out.insert(id, entity);
    }
    Ok(out)
}

/// Render one activity row through the merged D-26 serializer views.
/// Datetimes render in the actor's zone (DRF `enforce_timezone`);
/// `source_data` is always `None` here (no `to_attr` prefetch on these
/// endpoints — `issue/activity.py:66-75` sets it only for
/// `activity_type=issue-property`).
fn render_activity_row(
    row: &Map<String, Value>,
    assets: &HashMap<uuid::Uuid, Option<String>>,
    timezone: &chrono_tz::Tz,
) -> Result<String, Denial> {
    fn text<'a>(row: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
        row.get(key).and_then(Value::as_str)
    }
    fn required<'a>(row: &'a Map<String, Value>, key: &str) -> Result<&'a str, Denial> {
        text(row, key).ok_or(Denial::ServerError)
    }
    fn uuid_of(row: &Map<String, Value>, key: &str) -> Result<Option<uuid::Uuid>, Denial> {
        match text(row, key) {
            Some(raw) => raw.parse().map(Some).map_err(|_| Denial::ServerError),
            None => Ok(None),
        }
    }
    fn instant(
        row: &Map<String, Value>,
        key: &str,
        timezone: &chrono_tz::Tz,
    ) -> Result<String, Denial> {
        let raw = required(row, key)?;
        let aware = chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| Denial::ServerError)?;
        Ok(crate::serializer::render_datetime_in(&aware, timezone))
    }
    fn instant_opt(
        row: &Map<String, Value>,
        key: &str,
        timezone: &chrono_tz::Tz,
    ) -> Result<Option<String>, Denial> {
        match text(row, key) {
            Some(raw) => {
                let aware =
                    chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| Denial::ServerError)?;
                Ok(Some(crate::serializer::render_datetime_in(
                    &aware, timezone,
                )))
            }
            None => Ok(None),
        }
    }
    fn asset_url(
        assets: &HashMap<uuid::Uuid, Option<String>>,
        id: Option<uuid::Uuid>,
    ) -> (bool, Option<String>) {
        match id {
            Some(id) => {
                let entity = assets.get(&id).and_then(|entity| entity.clone());
                (true, static_asset_url(entity.as_deref(), &id.to_string()))
            }
            None => (false, None),
        }
    }
    // Owned render buffer: datetimes and asset URLs borrow from here.
    let created_at = instant(row, "a_created_at", timezone)?;
    let updated_at = instant(row, "a_updated_at", timezone)?;
    let deleted_at = instant_opt(row, "a_deleted_at", timezone)?;
    let (avatar_attached, avatar_asset_url) =
        asset_url(assets, uuid_of(row, "actor_avatar_asset_id")?);
    let (cover_attached, cover_asset_url) =
        asset_url(assets, uuid_of(row, "project_cover_image_asset_id")?);
    let (logo_attached, logo_asset_url) =
        asset_url(assets, uuid_of(row, "workspace_logo_asset_id")?);
    let attachments: Vec<&str> = match row.get("attachments") {
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    // Row structs are bound (not temporary) so the views borrowing
    // them live until serialization.
    let actor_avatar: Option<&str> = text(row, "actor_avatar");
    let actor_row: Option<UserLiteRow> = if text(row, "actor").is_some() {
        let avatar = actor_avatar.ok_or(Denial::ServerError)?;
        Some(UserLiteRow {
            id: required(row, "actor_id")?,
            first_name: required(row, "actor_first_name")?,
            last_name: required(row, "actor_last_name")?,
            avatar,
            avatar_url: user_model::avatar_url(
                avatar_attached,
                avatar_asset_url.as_deref(),
                avatar,
            ),
            is_bot: row
                .get("actor_is_bot")
                .and_then(Value::as_bool)
                .ok_or(Denial::ServerError)?,
            display_name: required(row, "actor_display_name")?,
        })
    } else {
        None
    };
    let actor_detail = actor_row.as_ref().map(user_lite_to_representation);
    let issue_row: Option<AppIssueFlatRow> = if text(row, "issue").is_some() {
        let complexity_score = row
            .get("issue_complexity_score")
            .and_then(Value::as_i64)
            .ok_or(Denial::ServerError)?;
        let sequence_id = row
            .get("issue_sequence_id")
            .and_then(Value::as_i64)
            .ok_or(Denial::ServerError)?;
        let sort_order = row
            .get("issue_sort_order")
            .and_then(Value::as_f64)
            .ok_or(Denial::ServerError)?;
        Some(AppIssueFlatRow {
            id: required(row, "issue_id")?,
            name: required(row, "issue_name")?,
            description_json: row
                .get("issue_description_json")
                .ok_or(Denial::ServerError)?,
            description_html: required(row, "issue_description_html")?,
            priority: required(row, "issue_priority")?,
            complexity_score: i32::try_from(complexity_score).map_err(|_| Denial::ServerError)?,
            start_date: text(row, "issue_start_date"),
            target_date: text(row, "issue_target_date"),
            sequence_id: i32::try_from(sequence_id).map_err(|_| Denial::ServerError)?,
            sort_order,
            is_draft: row
                .get("issue_is_draft")
                .and_then(Value::as_bool)
                .ok_or(Denial::ServerError)?,
        })
    } else {
        None
    };
    let issue_detail = issue_row.as_ref().map(app_issue_flat_to_representation);
    let project_row = ProjectLiteRow {
        id: required(row, "project_id")?,
        identifier: required(row, "project_identifier")?,
        name: required(row, "project_name")?,
        cover_image: text(row, "project_cover_image"),
        cover_image_url: resolve_project_cover_image_url(
            cover_attached,
            cover_asset_url.as_deref(),
            text(row, "project_cover_image"),
        ),
        logo_props: row.get("project_logo_props").ok_or(Denial::ServerError)?,
        description: required(row, "project_description")?,
        is_default: row
            .get("project_is_default")
            .and_then(Value::as_bool)
            .ok_or(Denial::ServerError)?,
    };
    let project_detail = project_lite_to_representation(&project_row);
    let workspace_row = WorkspaceLiteRow {
        name: required(row, "workspace_name")?,
        slug: required(row, "workspace_slug")?,
        id: required(row, "workspace_id")?,
        logo_url: resolve_workspace_logo_url(
            logo_attached,
            logo_asset_url.as_deref(),
            text(row, "workspace_logo"),
        ),
    };
    let workspace_detail = workspace_lite_to_representation(&workspace_row);
    let epoch = match row.get("epoch") {
        Some(Value::Null) | None => None,
        Some(value) => Some(value.as_f64().ok_or(Denial::ServerError)?),
    };
    let activity_row = IssueActivityRow {
        id: required(row, "aid")?,
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        source_data: None,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        verb: required(row, "verb")?,
        field: text(row, "field"),
        old_value: text(row, "old_value"),
        new_value: text(row, "new_value"),
        comment: required(row, "comment")?,
        attachments,
        old_identifier: text(row, "old_identifier"),
        new_identifier: text(row, "new_identifier"),
        epoch,
        created_by: text(row, "created_by"),
        updated_by: text(row, "updated_by"),
        project: required(row, "project")?,
        workspace: required(row, "workspace")?,
        issue: text(row, "issue"),
        issue_comment: text(row, "issue_comment"),
        actor: text(row, "actor"),
    };
    let view = issue_activity_to_representation(&activity_row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

/// Flat activity page (`OffsetPaginator` + `on_results` serializer):
/// the order key `NULLS LAST` plus `-created_at`, offset window, 12-key
/// envelope.
#[allow(clippy::too_many_arguments)]
async fn activity_flat_response(
    pool: &sqlx::PgPool,
    from_where: &str,
    values: Vec<sea_query::Value>,
    key_expr: &str,
    direction: &str,
    per_page: i64,
    cursor: crate::paginator::Cursor,
    timezone: &chrono_tz::Tz,
) -> HandlerResult {
    use crate::paginator::{
        apply_offset_window, max_hits, next_cursor, offset_window, prev_cursor,
    };
    let limit = per_page.min(1000);
    let window = offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    let selects = activity_row_selects();
    let inner = format!(
        "SELECT {selects} {from_where} ORDER BY {key_expr} {direction} NULLS LAST, a.created_at DESC LIMIT {} OFFSET {}",
        window.stop - window.offset,
        window.offset
    );
    let rows = fetch_json_rows(pool, &inner, values.clone()).await?;
    let has_more = rows.len() as i64 > limit;
    let page: Vec<Map<String, Value>> = apply_offset_window(&rows, limit)
        .map_err(page_denial)?
        .into_iter()
        .collect();
    let total_count = {
        let sql = format!("SELECT COUNT(*) {from_where}");
        fetch_count(pool, &sql, values).await?
    };
    // One batched asset-entity lookup for the page's avatar/cover/logo
    // resolutions.
    let mut asset_ids = Vec::new();
    for row in &page {
        for key in [
            "actor_avatar_asset_id",
            "project_cover_image_asset_id",
            "workspace_logo_asset_id",
        ] {
            if let Some(Value::String(raw)) = row.get(key) {
                if let Ok(id) = raw.parse::<uuid::Uuid>() {
                    if !asset_ids.contains(&id) {
                        asset_ids.push(id);
                    }
                }
            }
        }
    }
    let assets = fetch_asset_entities(pool, &asset_ids).await?;
    let mut shaped = Vec::with_capacity(page.len());
    for row in &page {
        shaped.push(render_activity_row(row, &assets, timezone)?);
    }
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
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

/// Parse the shared activity-list query params: strict `per_page` +
/// raw `order_by` (default `-created_at`). Group params are unread on
/// these endpoints, so no mismatch guard applies.
fn activity_params(query: &QueryMap) -> Result<(ListParams, String), Denial> {
    let params = ListParams::parse_with(
        &multi_map(query),
        ParseOptions {
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
    let order_by = query_last(query, "order_by").unwrap_or_else(|| "-created_at".to_owned());
    Ok((params, order_by))
}

/// `WorkspaceUserActivityEndpoint.get` (`user.py:371-395`).
async fn user_activity(
    State(state): State<AppState>,
    Path((slug, user_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let viewer = actor_user_id(extension)?;
    // Class gates run in `initial()`, before the body parses `user_id`
    // (a non-member with a garbage id 403s, never 400s).
    let gate = gates::gate_for("GET", "workspaces/<slug>/user-activity/<user_id>/")
        .expect("user-activity gate")
        .gate;
    if let Some(denied) = check_class_gate(&pool, &gate, &slug, &viewer).await? {
        return Ok(denied);
    }
    let user_id = parse_uuid_or_invalid(&user_raw)?;
    let (params, order_by) = activity_params(&query)?;
    let cursor = crate::paginator::Cursor::from_string(&params.cursor_raw).map_err(page_denial)?;
    // `?project=` getlist (`:375,386-387`): raw strings, UUID-validated
    // at lookup time (non-UUIDs 400).
    let projects_raw: Vec<String> = query_values(&query, "project").unwrap_or_default();
    // Scope binds are `$1..$3` (target, slug, viewer); the project list
    // follows at `$4..`.
    let projects_in: Option<(String, Vec<uuid::Uuid>)> = if projects_raw.is_empty() {
        None
    } else {
        let refs: Vec<&str> = projects_raw.iter().map(String::as_str).collect();
        let ids = qp::validate_project_uuids(&refs)
            .map_err(|_| Denial::BadError(INVALID_DETAIL_MSG.to_owned()))?;
        let holders = (0..ids.len())
            .map(|index| format!("${}", 4 + index))
            .collect::<Vec<_>>();
        Some((format!("p.id IN ({})", holders.join(",")), ids))
    };
    let from_where = qp::user_activity_from_where(projects_in.as_ref().map(|pair| pair.0.as_str()));
    let mut binder = Binder::new();
    binder.bind_uuid(user_id);
    binder.bind_string(slug);
    binder.bind_uuid(viewer);
    if let Some((_, ids)) = &projects_in {
        for id in ids {
            binder.bind_uuid(*id);
        }
    }
    let values = binder.values();
    let (key_expr, descending) = activity_order_expr(&order_by)?;
    let direction = if descending { "DESC" } else { "ASC" };
    let timezone = actor_timezone(&pool, &viewer).await?;
    activity_flat_response(
        &pool,
        &from_where,
        values,
        &key_expr,
        direction,
        params.per_page,
        cursor,
        &timezone,
    )
    .await
}

/// `UserActivityEndpoint.get` (`app/views/user/base.py:392-404`).
async fn me_activities(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let viewer = actor_user_id(extension)?;
    let (params, order_by) = activity_params(&query)?;
    let cursor = crate::paginator::Cursor::from_string(&params.cursor_raw).map_err(page_denial)?;
    let (key_expr, descending) = activity_order_expr(&order_by)?;
    let direction = if descending { "DESC" } else { "ASC" };
    let timezone = actor_timezone(&pool, &viewer).await?;
    let mut binder = Binder::new();
    binder.bind_uuid(viewer);
    activity_flat_response(
        &pool,
        &qp::me_activities_from_where(),
        binder.values(),
        &key_expr,
        direction,
        params.per_page,
        cursor,
        &timezone,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::app_issues::ISSUE_ACTIVITY_ALL_FIELDS;

    #[test]
    fn paths_match_the_url_conf() {
        assert_eq!(
            USER_STATS_PATH,
            "/api/workspaces/{slug}/user-stats/{user_id}/"
        );
        assert_eq!(
            USER_ACTIVITY_PATH,
            "/api/workspaces/{slug}/user-activity/{user_id}/"
        );
        assert_eq!(
            USER_PROFILE_PATH,
            "/api/workspaces/{slug}/user-profile/{user_id}/"
        );
        assert_eq!(
            USER_ISSUES_PATH,
            "/api/workspaces/{slug}/user-issues/{user_id}/"
        );
        assert_eq!(ME_ACTIVITIES_PATH, "/api/users/me/activities/");
    }

    #[test]
    fn gates_cover_the_five_owned_actions() {
        let stats =
            gates::gate_for("GET", "workspaces/<slug>/user-stats/<user_id>/").expect("stats gate");
        assert!(matches!(stats.gate, gates::Gate::Authenticated));
        let activity = gates::gate_for("GET", "workspaces/<slug>/user-activity/<user_id>/")
            .expect("activity gate");
        assert!(matches!(activity.gate, gates::Gate::ClassEntity));
        let profile = gates::gate_for("GET", "workspaces/<slug>/user-profile/<user_id>/")
            .expect("profile gate");
        assert!(matches!(profile.gate, gates::Gate::Authenticated));
        let issues = gates::gate_for("GET", "workspaces/<slug>/user-issues/<user_id>/")
            .expect("issues gate");
        assert!(matches!(issues.gate, gates::Gate::ClassViewer));
        let me = gates::gate_for("GET", "users/me/activities/").expect("me gate");
        assert!(matches!(me.gate, gates::Gate::Authenticated));
    }

    #[test]
    fn profile_body_is_byte_exact() {
        // F-W24-11 R3 rows, in emission order (`user.py:353-368`).
        let body = render_profile_body(
            &[ProfileProject {
                id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_owned(),
                logo_props: "{}".to_owned(),
                created_issues: 5,
                assigned_issues: 7,
                completed_issues: 3,
                pending_issues: 4,
            }],
            &ProfileUserData {
                email: Some("ada@x.io".to_owned()),
                first_name: "Ada".to_owned(),
                last_name: "L".to_owned(),
                avatar_url: Some("https://x/y.png".to_owned()),
                cover_image_url: None,
                date_joined: "2025-01-01T00:00:00Z".to_owned(),
                user_timezone: "UTC".to_owned(),
                display_name: "Ada".to_owned(),
            },
        );
        assert_eq!(
            body,
            r#"{"project_data":[{"id":"bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb","logo_props":{},"created_issues":5,"assigned_issues":7,"completed_issues":3,"pending_issues":4}],"user_data":{"email":"ada@x.io","first_name":"Ada","last_name":"L","avatar_url":"https://x/y.png","cover_image_url":null,"date_joined":"2025-01-01T00:00:00Z","user_timezone":"UTC","display_name":"Ada"}}"#
        );
        // Guest branch: `project_data` is `[]`, email may be null.
        let guest = render_profile_body(
            &[],
            &ProfileUserData {
                email: None,
                first_name: "".to_owned(),
                last_name: "".to_owned(),
                avatar_url: None,
                cover_image_url: None,
                date_joined: "2025-01-01T00:00:00Z".to_owned(),
                user_timezone: "UTC".to_owned(),
                display_name: "x".to_owned(),
            },
        );
        assert!(guest.starts_with(r#"{"project_data":[],"user_data":{"email":null,"#));
        assert_eq!(qp::ROLE_MEMBER_MIN, 15);
        assert!(qp::requester_sees_projects(15));
        assert!(!qp::requester_sees_projects(5));
    }

    #[test]
    fn stats_body_key_order_matches_the_view() {
        let body = render_stats_body(&StatsBundle {
            state_distribution: "[]".to_owned(),
            priority_distribution: "[]".to_owned(),
            created_issues: 0,
            assigned_issues: 0,
            completed_issues: 0,
            pending_issues: 0,
            subscribed_issues: 0,
            present_cycles: Vec::new(),
            upcoming_cycles: Vec::new(),
        });
        assert_eq!(
            body,
            r#"{"state_distribution":[],"priority_distribution":[],"created_issues":0,"assigned_issues":0,"completed_issues":0,"pending_issues":0,"subscribed_issues":0,"present_cycles":[],"upcoming_cycles":[]}"#
        );
        assert_eq!(
            qp::STATS_RESPONSE_KEYS,
            &[
                "state_distribution",
                "priority_distribution",
                "created_issues",
                "assigned_issues",
                "completed_issues",
                "pending_issues",
                "subscribed_issues",
                "present_cycles",
                "upcoming_cycles",
            ]
        );
    }

    #[test]
    fn annotations_adapt_all_four_correlations() {
        let adapted = profile_annotations_for_issue();
        assert_eq!(adapted.matches("= issue.id").count(), 4, "{adapted}");
        assert!(!adapted.contains("= i.id"), "{adapted}");
        // Plain `COUNT` (0 when empty), never the pilot NULLIF shape.
        assert!(!adapted.contains("NULLIF"), "{adapted}");
    }

    #[test]
    fn row_selects_cover_every_on_results_key() {
        let selects = profile_row_selects(&[]);
        for key in on_results_fields(None, None) {
            let found = match key.as_str() {
                "estimate_point" => selects.contains("AS estimate_point"),
                "created_by" => selects.contains("AS created_by"),
                "updated_by" => selects.contains("AS updated_by"),
                "state__group" => selects.contains("AS \"state__group\""),
                "cycle_id" | "link_count" | "attachment_count" | "sub_issues_count" => {
                    selects.contains(&format!("AS {key}"))
                }
                "label_ids" | "assignee_ids" | "module_ids" => {
                    selects.contains(&format!("AS {key}"))
                }
                _ => selects.contains(&format!("issue.{key}")),
            };
            assert!(found, "missing select for {key}:\n{selects}");
        }
        // One array skipped per m2m group level; raw keys projected
        // for both (the grouper buckets each joined row by its raw key).
        let selects =
            profile_row_selects(&skip_arrays_for(Some("assignees__id"), Some("labels__id")));
        assert!(!selects.contains("AS assignee_ids"));
        assert!(!selects.contains("AS label_ids"));
        assert!(selects.contains("AS module_ids"));
        let members = group_member_selects(Some("assignees__id"), Some("labels__id"));
        assert!(members.contains("AS \"assignees__id\""), "{members}");
        assert!(members.contains("AS \"labels__id\""), "{members}");
        assert_eq!(group_member_selects(Some("priority"), None), "");
    }

    #[test]
    fn module_array_has_no_deleted_guard() {
        // `grouper.py:63-72` filters only `module__archived_at` (the
        // pilot's extra `m.deleted_at` guard is its own divergence).
        let selects = profile_row_selects(&[]);
        assert!(selects.contains("m.archived_at IS NULL"), "{selects}");
        assert!(!selects.contains("m.deleted_at"), "{selects}");
    }

    #[test]
    fn on_results_field_order_matches_python() {
        // `grouper.py:95-141`: 23 base keys + 3 arrays.
        let fields = on_results_fields(None, None);
        let expected = [
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
        ];
        assert_eq!(fields, expected.map(str::to_owned));
    }

    #[test]
    fn group_guards_cover_m2m_only() {
        assert_eq!(
            group_deleted_guards(Some("assignees__id"), Some("priority")),
            ["issue_assignee.deleted_at IS NULL"]
        );
        // Group == sub is rejected upstream, but dedupe anyway.
        assert_eq!(
            group_deleted_guards(Some("labels__id"), Some("labels__id")),
            ["label_issue.deleted_at IS NULL"]
        );
        assert!(group_deleted_guards(Some("priority"), None).is_empty());
        assert!(group_deleted_guards(None, None).is_empty());
    }

    #[test]
    fn mismatch_body_matches_the_shared_const() {
        let mismatch =
            qp::profile_group_mismatch(Some("priority"), Some("priority")).expect("clash");
        assert_eq!(
            mismatch.body(),
            r#"{"error":"Group by and sub group by cannot have same parameters"}"#
        );
        assert!(qp::profile_group_mismatch(Some("priority"), Some("")).is_none());
        assert!(qp::profile_group_mismatch(None, None).is_none());
    }

    #[test]
    fn rewrite_maps_issue_and_state_keep_relations() {
        let fragment = "issue.priority IN ($1) AND state.\"group\" IN ($2) \
            AND issue_assignee.assignee_id = $3 AND label_issue.label_id = $4 \
            AND issue.created_at::date >= $5::date";
        assert_eq!(
            rewrite_stats_aliases(fragment),
            "i.priority IN ($1) AND s.\"group\" IN ($2) \
            AND ia.assignee_id = $3 AND label_issue.label_id = $4 \
            AND i.created_at::date >= $5::date"
        );
        // Every other relation alias survives verbatim.
        let fragment = "issue_cycle.cycle_id IN ($1) AND issue_module.module_id IN ($2) \
            AND issue_mention.mention_id IN ($3) \
            AND issue_subscribers.subscriber_id IN ($4) AND issue_intake.status IN ($5)";
        assert_eq!(rewrite_stats_aliases(fragment), fragment);
    }

    #[test]
    fn stats_joins_reuse_or_add_assignee_leg() {
        // Q1/Q2/Q4-Q6 reuse the preamble `ia` join; Q3 adds it.
        assert_eq!(stats_relation_joins("ia.assignee_id = $1", true), "");
        let added = stats_relation_joins("ia.assignee_id = $1", false);
        assert!(
            added.contains("INNER JOIN issue_assignees ia ON ia.issue_id = i.id"),
            "{added}"
        );
        // Value predicates INNER, `__isnull`-only LEFT.
        let inner = stats_relation_joins("label_issue.label_id IN ($1)", true);
        assert!(inner.contains("INNER JOIN issue_labels"), "{inner}");
        let left = stats_relation_joins("label_issue.label_id IS NULL", true);
        assert!(left.contains("LEFT JOIN issue_labels"), "{left}");
        assert_eq!(stats_relation_joins("i.priority IN ($1)", true), "");
    }

    #[test]
    fn stats_builders_accept_join_insertion() {
        // Precondition of `insert_stats_joins`: one ` WHERE ` each.
        for build in [
            qp::stats_state_distribution_sql,
            qp::stats_priority_distribution_sql,
            qp::stats_created_count_sql,
            qp::stats_assigned_count_sql,
            qp::stats_pending_count_sql,
            qp::stats_completed_count_sql,
        ] {
            let statement = build(Some("$99"));
            assert_eq!(statement.matches(" WHERE ").count(), 1, "{statement}");
            let with_joins =
                insert_stats_joins(statement, " INNER JOIN issue_labels label_issue ON true")
                    .expect("insert");
            assert!(with_joins.contains("ON true WHERE"), "{with_joins}");
        }
        assert!(insert_stats_joins("SELECT 1".to_owned(), " JOIN x").is_err());
        assert_eq!(
            insert_stats_joins("SELECT 1".to_owned(), "").unwrap(),
            "SELECT 1"
        );
    }

    #[test]
    fn subscriber_scope_resolves_three_lookups_only() {
        let id = uuid::Uuid::nil();
        let mut binder = Binder::new();
        let sql =
            subscriber_legacy_sql(&mut binder, "created_by__in", &FilterValue::Uuids(vec![id]))
                .expect("created_by");
        assert_eq!(sql, "sub.created_by_id IN ($1)");
        let sql = subscriber_legacy_sql(&mut binder, "project__in", &FilterValue::Uuids(vec![id]))
            .expect("project");
        assert_eq!(sql, "sub.project_id IN ($2)");
        let day = chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let sql =
            subscriber_legacy_sql(&mut binder, "created_at__date__gte", &FilterValue::Day(day))
                .expect("created_at");
        assert!(sql.contains("sub.created_at::date >="), "{sql}");
        // Everything else is a Django `FieldError` → 500.
        for (name, value) in [
            ("state__in", FilterValue::Uuids(vec![id])),
            (
                "priority__in",
                FilterValue::Strings(vec!["high".to_owned()]),
            ),
            ("labels__in", FilterValue::Uuids(vec![id])),
            ("assignees__in", FilterValue::Uuids(vec![id])),
            (
                "issue_subscribers__subscriber_id__in",
                FilterValue::Uuids(vec![id]),
            ),
            ("target_date__isnull", FilterValue::Flag(true)),
            ("name__icontains", FilterValue::Text("x".to_owned())),
        ] {
            assert!(
                matches!(
                    subscriber_legacy_sql(&mut Binder::new(), name, &value),
                    Err(Denial::ServerError)
                ),
                "name {name} should 500"
            );
        }
        // Garbage date bounds are `ValidationError` → 400.
        assert!(matches!(
            subscriber_legacy_sql(
                &mut Binder::new(),
                "created_at__date__gte",
                &FilterValue::Text("not-a-date".to_owned()),
            ),
            Err(Denial::BadError(_))
        ));
    }

    /// Every `$n` holder in `sql` names a bound value, and every bound
    /// value is referenced (binds are positional, so a gap or overhang
    /// is a runtime error).
    fn assert_holders_continuous(sql: &str, values: &[sea_query::Value]) {
        let mut referenced = vec![false; values.len()];
        let bytes = sql.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'$' {
                let mut end = index + 1;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                if end > index + 1 {
                    let number: usize = sql[index + 1..end].parse().expect("holder");
                    assert!(
                        number >= 1 && number <= values.len(),
                        "holder ${number} out of range in:\n{sql}"
                    );
                    referenced[number - 1] = true;
                    index = end;
                    continue;
                }
            }
            index += 1;
        }
        for (position, seen) in referenced.iter().enumerate() {
            assert!(seen, "bind ${} unreferenced in:\n{sql}", position + 1);
        }
    }

    fn assert_parens_balanced(sql: &str) {
        let mut depth = 0i32;
        for char in sql.chars() {
            if char == '(' {
                depth += 1;
            } else if char == ')' {
                depth -= 1;
            }
            assert!(depth >= 0, "unbalanced parens in:\n{sql}");
        }
        assert_eq!(depth, 0, "unbalanced parens in:\n{sql}");
    }

    #[test]
    fn filtered_set_assembles_valid_sql() {
        use crate::app_issues::OneOrMany;
        let uid = uuid::Uuid::nil();
        let viewer = uuid::Uuid::max();
        let mut query = QueryMap::new();
        query.insert(
            "labels".to_owned(),
            OneOrMany::One(
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa,bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
                    .to_owned(),
            ),
        );
        query.insert("priority".to_owned(), OneOrMany::One("high".to_owned()));
        let filtered = profile_filtered_set(&uid, "acme", &viewer, &query, Some("priority"), None)
            .expect("filtered set");
        // Scope binds first, then the compiled legacy binds.
        assert_eq!(filtered.values.len(), 3 + 2 + 1);
        assert_holders_continuous(&filtered.from_where, &filtered.values);
        assert_parens_balanced(&filtered.from_where);
        // The labels predicate forces its INNER leg over the outer alias.
        assert!(
            filtered.from_where.contains(
                "INNER JOIN issue_labels AS label_issue ON label_issue.issue_id = issue.id"
            ),
            "{}",
            filtered.from_where
        );
        assert!(filtered.referenced.contains(&"label_issue"));
        // The QRY-C scope rides as the id-set subquery, verbatim.
        assert!(
            filtered
                .from_where
                .contains("issue.id IN (SELECT i.id FROM issues i"),
            "{}",
            filtered.from_where
        );
    }

    #[test]
    fn stats_statements_assemble_valid_sql() {
        let uid = uuid::Uuid::nil();
        // A representative compiled legacy fragment (pilot aliases).
        let mut scratch = Binder::new();
        let label = uuid::Uuid::nil();
        let fragment = crate::app_issues::legacy_sql(
            &mut scratch,
            "labels__in",
            &FilterValue::Uuids(vec![label]),
        )
        .expect("compile");
        let legacy = CompiledLegacy {
            text: fragment,
            values: scratch.values(),
        };
        for (build, has_ia) in [
            (
                qp::stats_state_distribution_sql as fn(Option<&str>) -> String,
                true,
            ),
            (qp::stats_priority_distribution_sql, true),
            (qp::stats_created_count_sql, false),
            (qp::stats_assigned_count_sql, true),
            (qp::stats_pending_count_sql, true),
            (qp::stats_completed_count_sql, true),
        ] {
            let mut binder = Binder::new();
            binder.bind_uuid(uid);
            binder.bind_string("acme".to_owned());
            binder.bind_uuid(uid);
            let rewritten = rewrite_stats_aliases(&legacy.text);
            assert!(
                rewritten.contains("label_issue.label_id"),
                "relation refs survive the rewrite: {rewritten}"
            );
            assert!(!rewritten.contains("issue.priority"), "{rewritten}");
            let spliced = binder.splice(&rewritten, legacy.values.clone());
            let joins = stats_relation_joins(&rewritten, has_ia);
            assert!(
                joins.contains("INNER JOIN issue_labels AS label_issue"),
                "{joins}"
            );
            let statement = insert_stats_joins(build(Some(&spliced)), &joins).expect("insert");
            assert_holders_continuous(&statement, &binder.values());
            assert_parens_balanced(&statement);
        }
    }

    #[test]
    fn activity_order_maps_single_level_fields() {
        assert_eq!(
            activity_order_expr("-created_at").unwrap(),
            ("a.\"created_at\"".to_owned(), true)
        );
        assert_eq!(
            activity_order_expr("actor").unwrap(),
            ("a.actor_id".to_owned(), false)
        );
        assert_eq!(
            activity_order_expr("issue_comment_id").unwrap(),
            ("a.issue_comment_id".to_owned(), false)
        );
        assert!(matches!(
            activity_order_expr("issue__name"),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            activity_order_expr("bogus"),
            Err(Denial::ServerError)
        ));
    }

    #[test]
    fn static_asset_url_covers_four_types() {
        assert_eq!(
            static_asset_url(Some("USER_AVATAR"), "abc"),
            Some("/api/assets/v2/static/abc/".to_owned())
        );
        assert_eq!(
            static_asset_url(Some("WORKSPACE_LOGO"), "abc"),
            Some("/api/assets/v2/static/abc/".to_owned())
        );
        assert_eq!(static_asset_url(Some("ISSUE_ATTACHMENT"), "abc"), None);
        assert_eq!(static_asset_url(None, "abc"), None);
    }

    /// Minimal row_to_json-shaped activity row: top-level columns only,
    /// null actor/issue, no assets.
    fn sparse_activity_row() -> Map<String, Value> {
        let mut row = Map::new();
        row.insert(
            "aid".to_owned(),
            Value::from("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
        );
        row.insert(
            "a_created_at".to_owned(),
            Value::from("2026-01-15T10:30:00+00:00"),
        );
        row.insert(
            "a_updated_at".to_owned(),
            Value::from("2026-01-15T10:30:00+00:00"),
        );
        row.insert("a_deleted_at".to_owned(), Value::Null);
        row.insert("verb".to_owned(), Value::from("created"));
        row.insert("field".to_owned(), Value::Null);
        row.insert("old_value".to_owned(), Value::Null);
        row.insert("new_value".to_owned(), Value::Null);
        row.insert("comment".to_owned(), Value::from(""));
        row.insert("attachments".to_owned(), Value::Array(Vec::new()));
        row.insert("old_identifier".to_owned(), Value::Null);
        row.insert("new_identifier".to_owned(), Value::Null);
        row.insert("epoch".to_owned(), Value::Null);
        row.insert("created_by".to_owned(), Value::Null);
        row.insert("updated_by".to_owned(), Value::Null);
        row.insert(
            "project".to_owned(),
            Value::from("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"),
        );
        row.insert(
            "workspace".to_owned(),
            Value::from("cccccccc-cccc-cccc-cccc-cccccccccccc"),
        );
        row.insert("issue".to_owned(), Value::Null);
        row.insert("issue_comment".to_owned(), Value::Null);
        row.insert("actor".to_owned(), Value::Null);
        row.insert(
            "project_id".to_owned(),
            Value::from("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"),
        );
        row.insert("project_identifier".to_owned(), Value::from("ENG"));
        row.insert("project_name".to_owned(), Value::from("Eng"));
        row.insert("project_cover_image".to_owned(), Value::Null);
        row.insert("project_cover_image_asset_id".to_owned(), Value::Null);
        row.insert("project_logo_props".to_owned(), serde_json::json!({}));
        row.insert("project_description".to_owned(), Value::from("d"));
        row.insert("project_is_default".to_owned(), Value::from(false));
        row.insert("workspace_name".to_owned(), Value::from("W"));
        row.insert("workspace_slug".to_owned(), Value::from("w"));
        row.insert(
            "workspace_id".to_owned(),
            Value::from("cccccccc-cccc-cccc-cccc-cccccccccccc"),
        );
        row.insert("workspace_logo".to_owned(), Value::Null);
        row.insert("workspace_logo_asset_id".to_owned(), Value::Null);
        row
    }

    #[test]
    fn activity_row_renders_25_keys_in_wire_order() {
        let row = sparse_activity_row();
        let assets = HashMap::new();
        let rendered = render_activity_row(&row, &assets, &chrono_tz::UTC).expect("render");
        let value: Value = serde_json::from_str(&rendered).expect("json");
        let object = value.as_object().expect("object");
        let keys: Vec<&str> = object.keys().map(String::as_str).collect();
        assert_eq!(keys, ISSUE_ACTIVITY_ALL_FIELDS);
        assert_eq!(object["verb"], Value::from("created"));
        assert_eq!(object["created_at"], Value::from("2026-01-15T10:30:00Z"));
        assert_eq!(object["actor_detail"], Value::Null);
        assert_eq!(object["issue_detail"], Value::Null);
        assert_eq!(object["source_data"], Value::Null);
        assert_eq!(object["project_detail"]["identifier"], Value::from("ENG"));
        assert_eq!(object["workspace_detail"]["slug"], Value::from("w"));
    }

    #[test]
    fn activity_datetimes_shift_to_actor_zone() {
        let row = sparse_activity_row();
        let assets = HashMap::new();
        let rendered =
            render_activity_row(&row, &assets, &chrono_tz::America::New_York).expect("render");
        let value: Value = serde_json::from_str(&rendered).expect("json");
        // January in New York is EST (-05:00), kept verbatim like DRF.
        assert_eq!(
            value["created_at"],
            Value::from("2026-01-15T05:30:00-05:00")
        );
    }
}
