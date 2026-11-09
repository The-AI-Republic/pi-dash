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
//! QRY-C scope (`profile_issues_from_where`, `i`-aliased) is consumed as
//! the correlated `EXISTS (… WHERE i2.id = issue.id)` scope, and its
//! annotation fragment is adapted `= i.id` → `= issue.id`.
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
//! * The flat list keeps duplicate rows from legacy m2m filters
//!   (`?assignees=A,B`, `?labels=L1,L2`): no `.distinct()` anywhere on
//!   this endpoint, so each joined copy counts and renders
//!   (PIDASHCONV-1372).
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

use crate::app_issues::{
    filters_timezone, multi_map, page_denial, query_last, query_values, render_condition,
    shape_row, Denial, QueryMap,
};
use crate::app_workspace::gates;
use crate::state::AppState;
use pidash_compat::response::json_response;
use pidash_db::entity::{
    cycle_issues, cycles, file_assets, intake_issues, issue_activities, issue_assignees,
    issue_labels, issues, labels, modules, project_members, projects, states, users,
    workspace_members, workspaces,
};
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
use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::sea_query::{
    Alias, Asterisk, BinOper, CaseStatement, Condition, Expr, ExprTrait, Func, IntoIden, JoinType,
    NullOrdering, Order, OverStatement, PostgresQueryBuilder, Query as SeaQuery, SelectStatement,
    SimpleExpr, SubQueryStatement,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult, QueryFilter,
    QuerySelect, RelationTrait,
};

// ---------------------------------------------------------------------------
// SeaORM engine (profile analog of app_issues/list_engine.rs)
// ---------------------------------------------------------------------------

pub(crate) fn p_a(name: &'static str) -> Alias {
    Alias::new(name)
}

/// `alias.column` for the profile aliases.
pub(crate) fn p_col<C: IntoIden + 'static>(alias: &'static str, column: C) -> Expr {
    Expr::col((p_a(alias), column))
}

pub(crate) fn p_issue<C: IntoIden + 'static>(column: C) -> Expr {
    p_col("issue", column)
}

pub(crate) fn p_sub(stmt: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(SubQueryStatement::SelectStatement(stmt)))
}

pub(crate) fn p_text<T: Into<String>>(value: T) -> SimpleExpr {
    Expr::val(value.into()).into()
}

pub(crate) fn p_select_one_from<T: IntoIden + 'static>(
    table: T,
    alias: &'static str,
) -> SelectStatement {
    let mut stmt = SeaQuery::select();
    stmt.expr(Expr::val(1)).from_as(table, p_a(alias));
    stmt
}

/// Run a built statement into typed rows, decoded by name.
pub(crate) async fn p_query_built<T: FromQueryResult>(
    db: &sea_orm::DatabaseConnection,
    stmt: &SelectStatement,
) -> Result<Vec<T>, Denial> {
    let rows = db
        .query_all(DbBackend::Postgres.build(stmt))
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter()
        .map(|row| T::from_query_result(row, "").map_err(|_| Denial::ServerError))
        .collect()
}

/// Every row of a built statement as a JSON map via `row_to_json`.
pub(crate) async fn p_fetch_rows(
    pool: &sqlx::PgPool,
    inner: &SelectStatement,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let mut wrapper = SeaQuery::select();
    wrapper
        .expr_as(
            SimpleExpr::from(Func::cust(p_a("row_to_json")).arg(Expr::col(p_a("__r"))))
                .cast_as(p_a("text")),
            p_a("__row"),
        )
        .from_subquery(inner.clone(), p_a("__r"));
    #[derive(FromQueryResult)]
    struct JsonRow {
        __row: String,
    }
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = p_query_built::<JsonRow>(&db, &wrapper).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        match serde_json::from_str(&row.__row).map_err(|_| Denial::ServerError)? {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

#[derive(Debug, FromQueryResult)]
struct PCountRow {
    n: i64,
}

/// A single `n` count.
pub(crate) async fn p_fetch_n(pool: &sqlx::PgPool, stmt: &SelectStatement) -> Result<i64, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = p_query_built::<PCountRow>(&db, stmt).await?;
    rows.into_iter()
        .next()
        .map(|row| row.n)
        .ok_or(Denial::ServerError)
}

/// `state."group" IS NULL OR NOT (state."group" = 'triage')`.
pub(crate) fn p_not_triage(alias: &'static str) -> SimpleExpr {
    let group = || p_col(alias, states::Column::Group);
    group().is_null().or(group().eq("triage").not())
}

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
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            USER_ACTIVITY_PATH,
            axum::routing::get(user_activity)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            USER_PROFILE_PATH,
            axum::routing::get(user_profile)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            USER_ISSUES_PATH,
            axum::routing::get(user_profile_issues)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            ME_ACTIVITIES_PATH,
            axum::routing::get(me_activities)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
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

/// Parse a `<uuid:user_id>` path segment the way Django's converter routes
/// it: only the canonical lowercase hyphenated spelling reaches the view;
/// anything else is the resolver 404 before auth (`UUIDConverter` matches
/// `[0-9a-f]{8}-…` lowercase-only, PIDASHCONV-1713).
fn parse_uuid_path(raw: &str) -> Result<uuid::Uuid, Denial> {
    match crate::uuid_path::classify_uuid_segment(raw) {
        crate::uuid_path::UuidSegment::Canonical(id) => Ok(id),
        crate::uuid_path::UuidSegment::NonCanonical | crate::uuid_path::UuidSegment::Garbage => {
            Err(Denial::PageNotFound)
        }
    }
}

/// The actor's `user_timezone` (`users.user_timezone`), parsed for DRF
/// serializer rendering (`enforce_timezone` against the activated actor
/// zone). A missing row or an unparseable zone is a 500 (the pilot reads
/// the same column through its gate context).
async fn actor_timezone(
    pool: &sqlx::PgPool,
    user_id: &uuid::Uuid,
) -> Result<chrono_tz::Tz, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let name = crate::app_issues::access::user_timezone(&db, *user_id)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ServerError)?;
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    crate::app_issues::access::workspace_role(&db, slug, *user_id)
        .await
        .map_err(|_| Denial::ServerError)
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

/// `project_data` row decoded by name (`COUNT FILTER` → `COUNT(CASE)`).
#[derive(Debug, FromQueryResult)]
struct ProfileProjectRow {
    id: uuid::Uuid,
    logo_props: Value,
    created_issues: i64,
    assigned_issues: i64,
    completed_issues: i64,
    pending_issues: i64,
}

/// `SELECT p.id, p.logo_props, COUNT(...) FILTER ... FROM projects p ...`
/// with the single-query four-`FILTER` fanout (see
/// [`qp::profile_projects_sql`]): one shared
/// `projects`/`workspaces`/`project_members`/`issues pi`/`issue_assignees
/// ia`/`states s` skeleton, grouped by `p.id, p.logo_props`.
fn profile_projects_select(user_id: uuid::Uuid, slug: &str, viewer: uuid::Uuid) -> SelectStatement {
    // `COUNT(pi.id) FILTER (WHERE c)` == `COUNT(CASE WHEN c THEN pi.id END)`.
    let filtered_count = |cond: Condition| {
        let case = CaseStatement::new().case(cond, p_col("pi", issues::Column::Id));
        Func::count(SimpleExpr::Case(Box::new(case)))
    };
    let mut stmt = SeaQuery::select();
    stmt.from_as(projects::Entity, p_a("p"))
        .join_as(
            JoinType::InnerJoin,
            workspaces::Entity,
            p_a("w"),
            p_col("w", workspaces::Column::Id).equals((p_a("p"), projects::Column::WorkspaceId)),
        )
        .join_as(
            JoinType::InnerJoin,
            project_members::Entity,
            p_a("rpm"),
            p_col("rpm", project_members::Column::ProjectId)
                .equals((p_a("p"), projects::Column::Id)),
        )
        .join_as(
            JoinType::LeftJoin,
            issues::Entity,
            p_a("pi"),
            p_col("pi", issues::Column::ProjectId).equals((p_a("p"), projects::Column::Id)),
        )
        .join_as(
            JoinType::LeftJoin,
            issue_assignees::Entity,
            p_a("ia"),
            p_col("ia", issue_assignees::Column::IssueId).equals((p_a("pi"), issues::Column::Id)),
        )
        .join_as(
            JoinType::LeftJoin,
            states::Entity,
            p_a("s"),
            p_col("s", states::Column::Id).equals((p_a("pi"), issues::Column::StateId)),
        );
    stmt.column((p_a("p"), projects::Column::Id))
        .column((p_a("p"), projects::Column::LogoProps))
        .expr_as(
            filtered_count(
                Condition::all()
                    .add(p_col("pi", issues::Column::CreatedById).eq(user_id))
                    .add(p_col("pi", issues::Column::ArchivedAt).is_null())
                    .add(p_col("pi", issues::Column::IsDraft).eq(false)),
            ),
            p_a("created_issues"),
        )
        .expr_as(
            filtered_count(
                Condition::all()
                    .add(p_col("ia", issue_assignees::Column::AssigneeId).eq(user_id))
                    .add(p_col("pi", issues::Column::ArchivedAt).is_null())
                    .add(p_col("pi", issues::Column::IsDraft).eq(false)),
            ),
            p_a("assigned_issues"),
        )
        .expr_as(
            filtered_count(
                Condition::all()
                    .add(p_col("pi", issues::Column::CompletedAt).is_not_null())
                    .add(p_col("ia", issue_assignees::Column::AssigneeId).eq(user_id))
                    .add(p_col("pi", issues::Column::ArchivedAt).is_null())
                    .add(p_col("pi", issues::Column::IsDraft).eq(false)),
            ),
            p_a("completed_issues"),
        )
        .expr_as(
            filtered_count(
                Condition::all()
                    .add(p_col("s", states::Column::Group).is_in([
                        "backlog",
                        "unstarted",
                        "started",
                    ]))
                    .add(p_col("ia", issue_assignees::Column::AssigneeId).eq(user_id))
                    .add(p_col("pi", issues::Column::ArchivedAt).is_null())
                    .add(p_col("pi", issues::Column::IsDraft).eq(false)),
            ),
            p_a("pending_issues"),
        );
    stmt.and_where(p_col("w", workspaces::Column::Slug).eq(slug))
        .and_where(p_col("rpm", project_members::Column::MemberId).eq(viewer))
        .and_where(p_col("rpm", project_members::Column::IsActive).eq(true))
        .and_where(p_col("p", projects::Column::ArchivedAt).is_null())
        .and_where(p_col("p", projects::Column::DeletedAt).is_null())
        .group_by_col((p_a("p"), projects::Column::Id))
        .group_by_col((p_a("p"), projects::Column::LogoProps));
    stmt
}

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
    // The `<uuid:user_id>` converter runs at URL resolution, before auth.
    let user_id = parse_uuid_path(&user_raw)?;
    let pool = pool_of(&state)?;
    let viewer = actor_user_id(extension)?;
    // `User.objects.get(pk=user_id)` (`:283`): miss → 404.
    let db = pidash_db::pool::sea_orm_connection(&pool);
    let profile_row: Option<users::Model> = users::Entity::find_by_id(user_id)
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    let profile_row = profile_row.ok_or(Denial::NotFound)?;
    let email = profile_row.email;
    let first_name = profile_row.first_name;
    let last_name = profile_row.last_name;
    let avatar = profile_row.avatar;
    let avatar_asset_id = profile_row.avatar_asset_id;
    let cover_image = profile_row.cover_image;
    let cover_image_asset_id = profile_row.cover_image_asset_id;
    let date_joined = profile_row.date_joined;
    let user_timezone = profile_row.user_timezone;
    let display_name = profile_row.display_name;
    // `WorkspaceMember.objects.get(...)` (`:285-287`): miss → 404 (this,
    // not a gate, is what 404s non-members — the view has no
    // `permission_classes`).
    let role = fetch_workspace_role(&pool, &slug, &viewer)
        .await?
        .ok_or(Denial::NotFound)?;
    // Asset entity rows for the `avatar_url` / `cover_image_url`
    // properties (`db/models/user.py:143-165`, FK-first, text fallback).
    let mut avatar_entity: Option<Option<String>> = None;
    let mut cover_entity: Option<Option<String>> = None;
    if avatar_asset_id.is_some() || cover_image_asset_id.is_some() {
        let wanted: Vec<uuid::Uuid> = [avatar_asset_id, cover_image_asset_id]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let rows: Vec<(uuid::Uuid, Option<String>)> = file_assets::Entity::find()
            .select_only()
            .columns([file_assets::Column::Id, file_assets::Column::EntityType])
            .filter(file_assets::Column::Id.is_in(wanted))
            .into_tuple()
            .all(&db)
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
        let stmt = profile_projects_select(user_id, &slug, viewer);
        let rows = p_query_built::<ProfileProjectRow>(&db, &stmt).await?;
        for row in rows {
            projects.push(ProfileProject {
                id: row.id.to_string(),
                logo_props: serde_json::to_string(&row.logo_props)
                    .map_err(|_| Denial::ServerError)?,
                created_issues: row.created_issues,
                assigned_issues: row.assigned_issues,
                completed_issues: row.completed_issues,
                pending_issues: row.pending_issues,
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

/// `(SELECT ci.cycle_id FROM cycle_issues ci WHERE ci.issue_id = issue.id
/// AND ci.deleted_at IS NULL ORDER BY ci.created_at DESC LIMIT 1)`: the
/// QRY-C `cycle_id` annotation (its inner `ORDER BY` makes the pick
/// deterministic over several live links).
fn profile_cycle_id_subquery() -> SimpleExpr {
    let mut stmt = SeaQuery::select();
    stmt.column((p_a("ci"), cycle_issues::Column::CycleId))
        .from_as(cycle_issues::Entity, p_a("ci"))
        .and_where(
            p_col("ci", cycle_issues::Column::IssueId).equals((p_a("issue"), issues::Column::Id)),
        )
        .and_where(p_col("ci", cycle_issues::Column::DeletedAt).is_null())
        .order_by((p_a("ci"), cycle_issues::Column::CreatedAt), Order::Desc)
        .limit(1);
    p_sub(stmt)
}

/// `COALESCE(ARRAY_AGG(DISTINCT x), '{}'::uuid[])` for the grouper arrays.
fn profile_id_array(x: Expr) -> SimpleExpr {
    use sea_orm::sea_query::extension::postgres::PgFunc;
    Func::coalesce([
        PgFunc::array_agg_distinct(x).into(),
        Expr::val("{}").cast_as(p_a("uuid[]")),
    ])
    .into()
}

/// The `module_ids` array: joins `modules m` and filters only
/// `m.archived_at IS NULL` (`grouper.py:63-72` — the pilot's extra
/// `m.deleted_at` guard is its own disclosed divergence).
fn profile_module_ids() -> SimpleExpr {
    let mut stmt = SeaQuery::select();
    stmt.expr(profile_id_array(p_col(
        "mi",
        pidash_db::entity::module_issues::Column::ModuleId,
    )))
    .from_as(pidash_db::entity::module_issues::Entity, p_a("mi"))
    .join_as(
        JoinType::InnerJoin,
        modules::Entity,
        p_a("m"),
        p_col("m", modules::Column::Id).equals((
            p_a("mi"),
            pidash_db::entity::module_issues::Column::ModuleId,
        )),
    )
    .and_where(
        p_col("mi", pidash_db::entity::module_issues::Column::IssueId)
            .equals((p_a("issue"), issues::Column::Id)),
    )
    .and_where(p_col("mi", pidash_db::entity::module_issues::Column::DeletedAt).is_null())
    .and_where(p_col("m", modules::Column::ArchivedAt).is_null());
    p_sub(stmt)
}

/// The labelled `label_ids` array subquery.
fn profile_label_ids() -> SimpleExpr {
    let mut stmt = SeaQuery::select();
    stmt.expr(profile_id_array(p_col("il", issue_labels::Column::LabelId)))
        .from_as(issue_labels::Entity, p_a("il"))
        .and_where(
            p_col("il", issue_labels::Column::IssueId).equals((p_a("issue"), issues::Column::Id)),
        )
        .and_where(p_col("il", issue_labels::Column::DeletedAt).is_null());
    p_sub(stmt)
}

/// The `assignee_ids` array (no active-member filter on this endpoint).
fn profile_assignee_ids() -> SimpleExpr {
    let mut stmt = SeaQuery::select();
    stmt.expr(profile_id_array(p_col(
        "ia",
        issue_assignees::Column::AssigneeId,
    )))
    .from_as(issue_assignees::Entity, p_a("ia"))
    .and_where(
        p_col("ia", issue_assignees::Column::IssueId).equals((p_a("issue"), issues::Column::Id)),
    )
    .and_where(p_col("ia", issue_assignees::Column::DeletedAt).is_null());
    p_sub(stmt)
}

/// The profile flat/list select: the base `issue_on_results` columns plus
/// the QRY-C R1 annotations plus the grouper arrays. `skip_arrays` drops
/// one array per m2m group level (the grouper swaps the array for the raw
/// key at BOTH levels — `grouper.py:107-136`). The three scalar counts
/// are the plain `COUNT` form (0 when empty, never NULL).
fn profile_annotate(stmt: &mut SelectStatement, skip_arrays: &[&str]) {
    stmt.column((p_a("issue"), issues::Column::Id))
        .column((p_a("issue"), issues::Column::Name))
        .column((p_a("issue"), issues::Column::StateId))
        .column((p_a("issue"), issues::Column::SortOrder))
        .column((p_a("issue"), issues::Column::CompletedAt))
        .expr_as(
            p_issue(issues::Column::EstimatePointId),
            p_a("estimate_point"),
        )
        .column((p_a("issue"), issues::Column::Priority))
        .column((p_a("issue"), issues::Column::StartDate))
        .column((p_a("issue"), issues::Column::TargetDate))
        .column((p_a("issue"), issues::Column::SequenceId))
        .column((p_a("issue"), issues::Column::ProjectId))
        .column((p_a("issue"), issues::Column::ParentId))
        .expr_as(profile_cycle_id_subquery(), p_a("cycle_id"))
        .expr_as(
            crate::app_issues::list_engine::link_count(false),
            p_a("link_count"),
        )
        .expr_as(
            crate::app_issues::list_engine::attachment_count(false),
            p_a("attachment_count"),
        )
        .expr_as(
            crate::app_issues::list_engine::sub_issues_count(false, true),
            p_a("sub_issues_count"),
        )
        .column((p_a("issue"), issues::Column::CreatedAt))
        .column((p_a("issue"), issues::Column::UpdatedAt))
        .expr_as(p_issue(issues::Column::CreatedById), p_a("created_by"))
        .expr_as(p_issue(issues::Column::UpdatedById), p_a("updated_by"))
        .column((p_a("issue"), issues::Column::IsDraft))
        .column((p_a("issue"), issues::Column::ArchivedAt))
        .expr_as(p_col("state", states::Column::Group), p_a("state__group"));
    if !skip_arrays.contains(&"label_ids") {
        stmt.expr_as(profile_label_ids(), p_a("label_ids"));
    }
    if !skip_arrays.contains(&"assignee_ids") {
        stmt.expr_as(profile_assignee_ids(), p_a("assignee_ids"));
    }
    if !skip_arrays.contains(&"module_ids") {
        stmt.expr_as(profile_module_ids(), p_a("module_ids"));
    }
}

/// Array names skipped for the active m2m group levels.
fn skip_arrays_for(group_by: Option<&str>, sub_group_by: Option<&str>) -> Vec<&'static str> {
    [group_by, sub_group_by]
        .into_iter()
        .flatten()
        .filter_map(skip_array_for)
        .collect()
}

/// Add the raw m2m group key per joined row (the grouper buckets each
/// joined row by its raw key at both levels — `grouper.py:107-136`).
fn p_group_member(stmt: &mut SelectStatement, group_by: &str) {
    match group_by {
        "labels__id" => {
            stmt.expr_as(
                p_col("label_issue", issue_labels::Column::LabelId),
                p_a("labels__id"),
            );
        }
        "assignees__id" => {
            stmt.expr_as(
                p_col("issue_assignee", issue_assignees::Column::AssigneeId),
                p_a("assignees__id"),
            );
        }
        "issue_module__module_id" => {
            stmt.expr_as(
                p_col(
                    "issue_module",
                    pidash_db::entity::module_issues::Column::ModuleId,
                ),
                p_a("issue_module__module_id"),
            );
        }
        _ => {}
    }
}

fn p_group_members(stmt: &mut SelectStatement, group_by: Option<&str>, sub_group_by: Option<&str>) {
    for group in [group_by, sub_group_by].into_iter().flatten() {
        p_group_member(stmt, group);
    }
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
fn group_deleted_guards(group_by: Option<&str>, sub_group_by: Option<&str>) -> Vec<&'static str> {
    let mut guards = Vec::new();
    for group in [group_by, sub_group_by].into_iter().flatten() {
        let alias = match group {
            "assignees__id" => "issue_assignee",
            "labels__id" => "label_issue",
            "issue_module__module_id" => "issue_module",
            _ => continue,
        };
        if !guards.contains(&alias) {
            guards.push(alias);
        }
    }
    guards
}

/// The expression a group field partitions by (the D-26 `group_expression`
/// over the shared `issue` / `state` aliases; `cycle_id` carries the
/// QRY-C `ORDER BY ci.created_at DESC` subquery).
fn group_expression(field: &str) -> Result<SimpleExpr, Denial> {
    Ok(match field {
        "labels__id" => Expr::col((p_a("label_issue"), issue_labels::Column::LabelId)).into(),
        "assignees__id" => {
            Expr::col((p_a("issue_assignee"), issue_assignees::Column::AssigneeId)).into()
        }
        "issue_module__module_id" => Expr::col((
            p_a("issue_module"),
            pidash_db::entity::module_issues::Column::ModuleId,
        ))
        .into(),
        "state_id" => p_issue(issues::Column::StateId).into(),
        "priority" => p_issue(issues::Column::Priority).into(),
        "cycle_id" => profile_cycle_id_subquery(),
        "project_id" => p_issue(issues::Column::ProjectId).into(),
        "state__group" => p_col("state", states::Column::Group).into(),
        "target_date" => p_issue(issues::Column::TargetDate).into(),
        "start_date" => p_issue(issues::Column::StartDate).into(),
        "created_by" => p_issue(issues::Column::CreatedById).into(),
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

/// The filtered profile-issues set: `FROM`, joins and `WHERE`, no select
/// list. Callers clone the base statement and add columns, order and
/// windows (the `list_engine::IssueSet` analog).
#[derive(Debug, Clone)]
struct ProfileSet {
    base: SelectStatement,
}

impl ProfileSet {
    fn select(&self) -> SelectStatement {
        self.base.clone()
    }
}

/// The R2 scope as correlated `EXISTS` (`qp::profile_issues_from_where`
/// inner leg, `issue.id IN (SELECT ...)` ≡ `EXISTS (… WHERE i2.id =
/// issue.id)`): assignee-OR-creator-OR-subscriber, slug-scoped, the
/// manager scope on the inner alias, over `i2/p2/s2/ia/sub`. The OR legs
/// LEFT-join with no deleted condition (live-or-deleted links match).
fn profile_id_exists(user_id: uuid::Uuid, slug: &str) -> SimpleExpr {
    let mut stmt = p_select_one_from(issues::Entity, "i2");
    stmt.join(
        JoinType::InnerJoin,
        workspaces::Entity,
        Expr::col((workspaces::Entity, workspaces::Column::Id))
            .equals((p_a("i2"), issues::Column::WorkspaceId)),
    )
    .join_as(
        JoinType::InnerJoin,
        projects::Entity,
        p_a("p2"),
        p_col("p2", projects::Column::Id).equals((p_a("i2"), issues::Column::ProjectId)),
    )
    .join_as(
        JoinType::LeftJoin,
        states::Entity,
        p_a("s2"),
        p_col("s2", states::Column::Id).equals((p_a("i2"), issues::Column::StateId)),
    )
    .join_as(
        JoinType::LeftJoin,
        issue_assignees::Entity,
        p_a("ia"),
        p_col("ia", issue_assignees::Column::IssueId).equals((p_a("i2"), issues::Column::Id)),
    )
    .join_as(
        JoinType::LeftJoin,
        pidash_db::entity::issue_subscribers::Entity,
        p_a("sub"),
        p_col("sub", pidash_db::entity::issue_subscribers::Column::IssueId)
            .equals((p_a("i2"), issues::Column::Id)),
    )
    .and_where(p_col("i2", issues::Column::Id).equals((p_a("issue"), issues::Column::Id)))
    .and_where(
        Condition::any()
            .add(p_col("ia", issue_assignees::Column::AssigneeId).eq(user_id))
            .add(p_col("i2", issues::Column::CreatedById).eq(user_id))
            .add(
                p_col(
                    "sub",
                    pidash_db::entity::issue_subscribers::Column::SubscriberId,
                )
                .eq(user_id),
            )
            .into(),
    )
    .and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
    .and_where(p_col("i2", issues::Column::DeletedAt).is_null())
    .and_where(p_not_triage("s2"))
    .and_where(p_col("i2", issues::Column::ArchivedAt).is_null())
    .and_where(p_col("p2", projects::Column::ArchivedAt).is_null())
    .and_where(p_col("i2", issues::Column::IsDraft).eq(false));
    Expr::exists(stmt)
}

/// The rich `filters` param compiled like `complex_filter`, over the
/// `issue` / `state` aliases. Returns the condition plus the relation
/// aliases a leaf filters through (a declared relation leaf that compiled
/// to a predicate, not to the match-all / match-none constants).
fn p_rich_condition(
    query: &QueryMap,
    tz: &chrono_tz::Tz,
) -> Result<Option<(Condition, std::collections::BTreeSet<&'static str>)>, Denial> {
    use pidash_db::filter::FilterTree;
    fn relation_alias(name: &str) -> Option<&'static str> {
        use pidash_db::filterset::{FilterKind, ISSUE_FILTERSET};
        let decl = ISSUE_FILTERSET.iter().find(|decl| decl.name == name)?;
        match decl.kind {
            FilterKind::Relation { join_table, .. } => Some(join_table),
            _ => None,
        }
    }
    fn walk(
        tree: &FilterTree,
        tz: &chrono_tz::Tz,
        used: &mut std::collections::BTreeSet<&'static str>,
    ) -> Result<Condition, pidash_db::filter::FilterError> {
        match tree {
            FilterTree::And(children) => {
                let mut cond = Condition::all();
                for child in children {
                    cond = cond.add(walk(child, tz, used)?);
                }
                Ok(cond)
            }
            FilterTree::Or(children) => {
                let mut cond = Condition::any();
                for child in children {
                    cond = cond.add(walk(child, tz, used)?);
                }
                Ok(cond)
            }
            FilterTree::Not(child) => Ok(walk(child, tz, used)?.not()),
            FilterTree::Leaf(leaves) => {
                let mut combined = Condition::all();
                for leaf in leaves {
                    let name = crate::app_issues::leaf_name(&leaf.field, leaf.lookup);
                    let expr = pidash_db::filterset::compile_leaf(&name, &leaf.value, tz)?;
                    if !matches!(expr, SimpleExpr::Custom(_)) {
                        if let Some(alias) = relation_alias(&name) {
                            used.insert(alias);
                        }
                    }
                    combined = combined.add(expr);
                }
                Ok(combined)
            }
        }
    }
    let raw = query_last(query, "filters").unwrap_or_default();
    if raw.is_empty() {
        return Ok(None);
    }
    match FilterTree::parse_param_opt(&raw).map_err(crate::app_issues::filter_denial)? {
        Some(tree) => {
            let mut used = std::collections::BTreeSet::new();
            let cond = walk(&tree, tz, &mut used).map_err(crate::app_issues::filter_denial)?;
            Ok(Some((cond, used)))
        }
        None => Ok(None),
    }
}

/// The legacy `issue_filters(params, "GET")` predicates over the `issue`
/// alias, truncated in UTC (this slice's current truncation; the
/// actor-zone fix follows the `app_issues` pattern separately).
fn p_legacy_condition(query: &QueryMap) -> Result<Option<Condition>, Denial> {
    let flat: HashMap<String, String> = query
        .keys()
        .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let legacy = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
        .map_err(|_| Denial::ServerError)?;
    let mut cond = Condition::all();
    let mut any = false;
    for (name, value) in legacy.predicates() {
        cond = cond.add(crate::app_issues::list_engine::legacy_expr(
            name,
            value,
            &chrono_tz::UTC,
        )?);
        any = true;
    }
    Ok(any.then_some(cond))
}

/// Filtered `FROM … WHERE` for the profile-issues list (`user.py:140-154`):
/// the R2 scope as correlated `EXISTS` (viewer-owned project scope,
/// manager scope applied twice) plus the `ComplexFilterBackend` tree plus
/// the legacy `issue_filters(params, "GET")` predicates, over the `issue`
/// / `state` aliases. The flat consumer counts and returns every joined
/// row (no `.distinct()` on this endpoint, PIDASHCONV-1372); only the
/// grouped consumers are `DISTINCT`-based.
fn profile_filtered_set(
    user_id: &uuid::Uuid,
    slug: &str,
    viewer: &uuid::Uuid,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
    tz: &chrono_tz::Tz,
) -> Result<ProfileSet, Denial> {
    let mut stmt = SeaQuery::select();
    stmt.from_as(issues::Entity, p_a("issue")).join_as(
        JoinType::LeftJoin,
        states::Entity,
        p_a("state"),
        Condition::all()
            .add(p_col("state", states::Column::Id).equals((p_a("issue"), issues::Column::StateId)))
            .add(p_col("state", states::Column::DeletedAt).is_null()),
    );
    let mut cond = Condition::all().add(profile_id_exists(*user_id, slug));
    // Manager scope on the outer alias (triage, archived, draft) plus the
    // slug and the requester's OWN active project memberships
    // (`:146-147` — ported as-is: the viewer sees only issues in THEIR
    // OWN projects, not the target's).
    cond = cond.add(p_issue(issues::Column::DeletedAt).is_null());
    cond = cond.add(p_not_triage("state"));
    cond = cond.add(p_issue(issues::Column::ArchivedAt).is_null());
    cond = cond.add(p_issue(issues::Column::IsDraft).eq(false));
    // Slug + viewer membership via correlated EXISTS over project_members
    // joined to the issue's project (the `rpm` leg of the QRY-C base).
    {
        let mut member = p_select_one_from(project_members::Entity, "rpm");
        member
            .join_as(
                JoinType::InnerJoin,
                projects::Entity,
                p_a("pp"),
                p_col("pp", projects::Column::Id)
                    .equals((p_a("rpm"), project_members::Column::ProjectId)),
            )
            .join(
                JoinType::InnerJoin,
                workspaces::Entity,
                Expr::col((workspaces::Entity, workspaces::Column::Id))
                    .equals((p_a("pp"), projects::Column::WorkspaceId)),
            )
            .and_where(
                p_col("rpm", project_members::Column::ProjectId)
                    .equals((p_a("issue"), issues::Column::ProjectId)),
            )
            .and_where(p_col("rpm", project_members::Column::MemberId).eq(*viewer))
            .and_where(p_col("rpm", project_members::Column::IsActive).into())
            .and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
            .and_where(p_col("pp", projects::Column::ArchivedAt).is_null());
        cond = cond.add(Expr::exists(member));
    }
    let rich_pair = p_rich_condition(query, tz)?;
    let mut rich_used = std::collections::BTreeSet::new();
    if let Some((rich, used)) = rich_pair.as_ref() {
        cond = cond.add(rich.clone());
        rich_used = used.clone();
    }
    let legacy = p_legacy_condition(query)?;
    // Rendered filter text decides the relation joins (the text engine's
    // scan): rich leaves that filter through a relation take INNER,
    // legacy values keep LEFT+WHERE semantics, `__isnull`-only stays LEFT.
    {
        let mut probe = Condition::all();
        if let Some((rich, _)) = rich_pair.as_ref() {
            probe = probe.add(rich.clone());
        }
        if let Some(legacy_cond) = legacy.clone() {
            probe = probe.add(legacy_cond);
        }
        let (fragment, _) = render_condition(&probe);
        for (table, alias) in RELATION_JOINS {
            let mentioned = fragment.contains(&format!("\"{alias}\"."))
                || fragment.contains(&format!("{alias}."));
            let nullable_only = mentioned && alias_nullable_only(&fragment, alias);
            let grouped = group_join_alias(group_by, sub_group_by, alias);
            let rich_inner = rich_used.contains(alias);
            let used_leg = mentioned || grouped;
            if !used_leg {
                continue;
            }
            let inner = grouped || rich_inner || !nullable_only;
            let kind = if inner {
                JoinType::InnerJoin
            } else {
                JoinType::LeftJoin
            };
            p_add_relation_join(&mut stmt, table, alias, kind);
        }
        if fragment.contains("\"issue_intake\".") || fragment.contains("issue_intake.") {
            let kind = if alias_nullable_only(&fragment, "issue_intake") {
                JoinType::LeftJoin
            } else {
                JoinType::InnerJoin
            };
            stmt.join_as(
                kind,
                intake_issues::Entity,
                p_a("issue_intake"),
                p_col("issue_intake", intake_issues::Column::IssueId)
                    .equals((p_a("issue"), issues::Column::Id)),
            );
        }
    }
    if let Some(legacy_cond) = legacy {
        cond = cond.add(legacy_cond);
    }
    // `GROUP_FILTER_MAPPER` deleted guards as ordinary predicates.
    for alias in group_deleted_guards(group_by, sub_group_by) {
        cond = cond.add(p_col(alias, issue_assignees::Column::DeletedAt).is_null());
    }
    // The grouped-totals `count_filter` leg (`ii` alias). Present only on
    // grouped paths; the pilot likewise keeps an always-LEFT intake leg.
    if group_by.is_some() {
        stmt.join_as(
            JoinType::LeftJoin,
            intake_issues::Entity,
            p_a("ii"),
            p_col("ii", intake_issues::Column::IssueId).equals((p_a("issue"), issues::Column::Id)),
        );
    }
    stmt.cond_where(cond);
    Ok(ProfileSet { base: stmt })
}

/// `JOIN <table> AS <alias> ON <alias>.issue_id = issue.id`.
fn p_add_relation_join(
    stmt: &mut SelectStatement,
    table: &str,
    alias: &'static str,
    kind: JoinType,
) {
    let on = |issue_id: Expr| issue_id.equals((p_a("issue"), issues::Column::Id));
    match table {
        "issue_assignees" => {
            stmt.join_as(
                kind,
                issue_assignees::Entity,
                p_a(alias),
                on(p_col(alias, issue_assignees::Column::IssueId)),
            );
        }
        "cycle_issues" => {
            stmt.join_as(
                kind,
                cycle_issues::Entity,
                p_a(alias),
                on(p_col(alias, cycle_issues::Column::IssueId)),
            );
        }
        "module_issues" => {
            stmt.join_as(
                kind,
                pidash_db::entity::module_issues::Entity,
                p_a(alias),
                on(p_col(
                    alias,
                    pidash_db::entity::module_issues::Column::IssueId,
                )),
            );
        }
        "issue_mentions" => {
            stmt.join_as(
                kind,
                pidash_db::entity::issue_mentions::Entity,
                p_a(alias),
                on(p_col(
                    alias,
                    pidash_db::entity::issue_mentions::Column::IssueId,
                )),
            );
        }
        "issue_labels" => {
            stmt.join_as(
                kind,
                issue_labels::Entity,
                p_a(alias),
                on(p_col(alias, issue_labels::Column::IssueId)),
            );
        }
        "issue_subscribers" => {
            stmt.join_as(
                kind,
                pidash_db::entity::issue_subscribers::Entity,
                p_a(alias),
                on(p_col(
                    alias,
                    pidash_db::entity::issue_subscribers::Column::IssueId,
                )),
            );
        }
        _ => {}
    }
}

/// `WorkspaceUserProfileIssuesEndpoint.get` (`user.py:99-251`).
async fn user_profile_issues(
    State(state): State<AppState>,
    Path((slug, user_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    // The `<uuid:user_id>` converter runs at URL resolution, before auth
    // and before the class gate (a non-member with a garbage id 404s,
    // never 403s).
    let user_id = parse_uuid_path(&user_raw)?;
    let pool = pool_of(&state)?;
    let viewer = actor_user_id(extension)?;
    let gate = gates::gate_for("GET", "workspaces/<slug>/user-issues/<user_id>/")
        .expect("user-issues gate")
        .gate;
    if let Some(denied) = check_class_gate(&pool, &gate, &slug, &viewer).await? {
        return Ok(denied);
    }
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
    // The *viewer's* zone: Django activates `request.user.user_timezone`,
    // not the profile target's.
    let timezone = filters_timezone(&pool, &viewer, &query).await?;
    let filtered = profile_filtered_set(
        &user_id,
        &slug,
        &viewer,
        &query,
        group_by,
        sub_group_by,
        &timezone,
    )?;
    // `order_issue_queryset` (`:162-165`), default `-created_at`. The
    // profile list annotates the plain `Func(Count)` form (empty counts
    // render `0`), so its count keys order `0`-style. A filter that
    // referenced the cycle bridge lends its join to the cycle-name
    // traversal (Django reuses it, scope included, instead of joining
    // again); otherwise the traversal joins fresh.
    let order_spec = qp::profile_issues_order(Some(&params.order_by), "state.\"group\"", |name| {
        format!("min_{}", name.replace("__", "_"))
    });
    let cycle_bridge = {
        let text = filtered.select().to_string(PostgresQueryBuilder);
        text.contains(r#"cycle_issues" AS "issue_cycle""#)
            .then_some("issue_cycle")
    };
    let key = crate::app_issues::list_engine::order_expr(
        &order_spec.out_param,
        &params.order_by,
        cycle_bridge,
        false,
    )?;
    // Total count over the pre-annotation set (the `deepcopy` at `:157`).
    // The flat branch counts every joined row: there is no `.distinct()`
    // on this endpoint, so legacy m2m filters fan out and each copy
    // counts (PIDASHCONV-1372). The grouped branch keeps its shape.
    let total_count = {
        let mut stmt = filtered.select();
        if group_by.is_some() && !key.joins.is_empty() {
            key.joins.apply(&mut stmt);
        }
        if group_by.is_none() || !key.joins.is_empty() {
            stmt.expr_as(Func::count(Expr::col(Asterisk)), p_a("n"));
        } else {
            stmt.expr_as(Func::count_distinct(p_issue(issues::Column::Id)), p_a("n"));
        }
        // `COUNT(*)` fan-out already in the statement; the grouped
        // distinct form matches the pilot's `COUNT(DISTINCT issue.id)`.
        p_fetch_n(&pool, &stmt).await?
    };
    let per_page = params.per_page;
    let cursor = crate::paginator::Cursor::from_string(&params.cursor_raw).map_err(page_denial)?;
    if group_by.is_some() {
        return profile_grouped_response(
            &pool,
            &slug,
            &filtered,
            group_by.unwrap_or_default(),
            sub_group_by.map(str::to_owned),
            &key,
            per_page,
            cursor,
            total_count,
        )
        .await;
    }
    profile_flat_response(&pool, &filtered, &key, per_page, cursor, total_count).await
}

fn p_non_negative(value: i64) -> Result<u64, Denial> {
    u64::try_from(value).map_err(|_| Denial::ServerError)
}

/// Plain `paginate` branch (`user.py:243-250`): every joined row (no
/// `DISTINCT` — duplicates from legacy m2m filters render, like Django),
/// the rewritten order key `NULLS LAST` plus `-created_at`, offset
/// window. The total over the pre-annotation set arrives from the caller
/// (the `deepcopy` at `:157`), like the grouped branch — no second count.
#[allow(clippy::too_many_arguments)]
async fn profile_flat_response(
    pool: &sqlx::PgPool,
    filtered: &ProfileSet,
    key: &crate::app_issues::list_engine::OrderExpr,
    per_page: i64,
    cursor: crate::paginator::Cursor,
    total_count: i64,
) -> HandlerResult {
    use crate::paginator::{
        apply_offset_window, max_hits, next_cursor, offset_window, prev_cursor,
    };
    let limit = per_page.min(1000);
    let window = offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    // The cycle-name traversal joins ride on the row query only; every
    // joined row is returned (Django slices the queryset with no
    // `.distinct()`, so fan-out copies stay).
    let mut inner = filtered.select();
    key.joins.apply(&mut inner);
    profile_annotate(&mut inner, &[]);
    inner
        .expr_as(key.expr.clone(), p_a("__order_key"))
        .order_by_with_nulls(p_a("__order_key"), key.order(), NullOrdering::Last)
        .order_by((p_a("issue"), issues::Column::CreatedAt), Order::Desc)
        .limit(p_non_negative(window.stop - window.offset)?)
        .offset(p_non_negative(window.offset)?);
    let rows = p_fetch_rows(pool, &inner).await?;
    let has_more = rows.len() as i64 > limit;
    let page: Vec<Map<String, Value>> = apply_offset_window(&rows, limit)
        .map_err(page_denial)?
        .into_iter()
        .collect();
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

/// The grouped-totals count filter over the `ii` leg (intake 1/-1/2 or
/// no row, unarchived, not a draft).
fn profile_count_filter() -> SimpleExpr {
    p_col("ii", intake_issues::Column::Status)
        .is_in([1, -1, 2])
        .or(p_col("ii", intake_issues::Column::Id).is_null())
        .and(p_issue(issues::Column::ArchivedAt).is_null())
        .and(p_issue(issues::Column::IsDraft).eq(false))
}

/// `COUNT(DISTINCT CASE WHEN <count filter> THEN issue.id END)`:
/// `COUNT(DISTINCT issue.id) FILTER (WHERE c)` without `FILTER`.
fn profile_filtered_distinct_count() -> SimpleExpr {
    let case = CaseStatement::new().case(profile_count_filter(), p_issue(issues::Column::Id));
    Func::count_distinct(SimpleExpr::Case(Box::new(case))).into()
}

/// `COALESCE(<expr>::text, 'None')`: a group bucket's key.
fn profile_bucket(expr: SimpleExpr) -> SimpleExpr {
    Func::coalesce([expr.cast_as(p_a("text")), p_text("None")]).into()
}

/// `(bucket, n)` per group: the filtered distinct count (pilot
/// `group_total_pairs`).
fn profile_group_totals(set: &ProfileSet, group_by: &str) -> Result<SelectStatement, Denial> {
    let mut stmt = set.select();
    stmt.expr_as(profile_bucket(group_expression(group_by)?), p_a("bucket"))
        .expr_as(profile_filtered_distinct_count(), p_a("n"))
        .group_by_col(p_a("bucket"));
    Ok(stmt)
}

/// `(bucket, sub, n)` per group pair (pilot `sub_total_pairs`).
fn profile_sub_totals(
    set: &ProfileSet,
    group_by: &str,
    sub_group_by: &str,
) -> Result<SelectStatement, Denial> {
    let mut stmt = set.select();
    stmt.expr_as(profile_bucket(group_expression(group_by)?), p_a("bucket"))
        .expr_as(profile_bucket(group_expression(sub_group_by)?), p_a("sub"))
        .expr_as(profile_filtered_distinct_count(), p_a("n"))
        .group_by_col(p_a("bucket"))
        .group_by_col(p_a("sub"));
    Ok(stmt)
}

#[derive(Debug, FromQueryResult)]
struct PTotalRow {
    bucket: String,
    n: i64,
}

#[derive(Debug, FromQueryResult)]
struct PSubTotalRow {
    bucket: String,
    sub: String,
    n: i64,
}

/// `(group, filtered count)` pairs for the totals dict
/// (`GroupedOffsetPaginator.__get_total_queryset`, `paginator.py:297-303`).
async fn profile_group_total_pairs(
    pool: &sqlx::PgPool,
    filtered: &ProfileSet,
    group_by: &str,
) -> Result<Vec<(String, i64)>, Denial> {
    let stmt = profile_group_totals(filtered, group_by)?;
    let db = pidash_db::pool::sea_orm_connection(pool);
    Ok(p_query_built::<PTotalRow>(&db, &stmt)
        .await?
        .into_iter()
        .map(|row| (row.bucket, row.n))
        .collect())
}

/// `(group, sub, filtered count)` triples for the nested sub-totals.
async fn profile_sub_total_pairs(
    pool: &sqlx::PgPool,
    filtered: &ProfileSet,
    group_by: &str,
    sub_group_by: &str,
) -> Result<Vec<(String, String, i64)>, Denial> {
    let stmt = profile_sub_totals(filtered, group_by, sub_group_by)?;
    let db = pidash_db::pool::sea_orm_connection(pool);
    Ok(p_query_built::<PSubTotalRow>(&db, &stmt)
        .await?
        .into_iter()
        .map(|row| (row.bucket, row.sub, row.n))
        .collect())
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
    filtered: &ProfileSet,
    group_by: &str,
    sub_group_by: Option<String>,
    key: &crate::app_issues::list_engine::OrderExpr,
    per_page: i64,
    cursor: crate::paginator::Cursor,
    total_count: i64,
) -> HandlerResult {
    use crate::paginator::{
        grouped_max_hits, grouped_window, next_cursor, prev_cursor, process_grouped_results,
        process_sub_grouped_results, sub_field_dict, sub_total_dicts, total_dict,
    };
    use sea_orm::sea_query::{Asterisk, WindowStatement};
    let limit = per_page.min(1000);
    let window = grouped_window(limit, cursor.offset, cursor.value, None).map_err(page_denial)?;
    let gexpr = group_expression(group_by)?;
    let skipped = skip_arrays_for(Some(group_by), sub_group_by.as_deref());
    let sub_expr = match &sub_group_by {
        Some(sub) => Some(group_expression(sub)?),
        None => None,
    };
    // The cycle-name traversal joins ride on the row query: the window
    // numbers fan-out rows like Django's, and the final order is global.
    let mut window_fn = WindowStatement::new();
    window_fn.add_partition_by(gexpr.clone());
    if let Some(sub) = sub_expr.clone() {
        window_fn.add_partition_by(sub);
    }
    window_fn
        .order_by_expr_with_nulls(key.expr.clone(), key.order(), NullOrdering::Last)
        .order_by((p_a("issue"), issues::Column::CreatedAt), Order::Desc);
    let mut inner = filtered.select();
    key.joins.apply(&mut inner);
    inner.distinct();
    profile_annotate(&mut inner, &skipped);
    p_group_members(&mut inner, Some(group_by), sub_group_by.as_deref());
    inner
        .expr_as(key.expr.clone(), p_a("__order_key"))
        .expr_window_as(Func::cust(p_a("ROW_NUMBER")), window_fn, p_a("__rn"));
    let mut outer = SeaQuery::select();
    outer
        .column(Asterisk)
        .from_subquery(inner, p_a("__w"))
        .and_where(p_col("__w", Alias::new("__rn")).gt(window.offset))
        .and_where(p_col("__w", Alias::new("__rn")).lte(window.stop))
        .order_by_with_nulls(
            (p_a("__w"), p_a("__order_key")),
            key.order(),
            NullOrdering::Last,
        )
        .order_by((p_a("__w"), p_a("created_at")), Order::Desc);
    let rows = p_fetch_rows(pool, &outer).await?;
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
    // `count_filter` for grouped pagination (`user.py:207-214,234-241`);
    // the `ii` leg rides in the filtered set.
    let group_totals = profile_group_total_pairs(pool, filtered, group_by).await?;
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
        Some(_) => {
            let sub_pairs = profile_sub_total_pairs(
                pool,
                filtered,
                group_by,
                sub_group_by.clone().unwrap_or_default().as_str(),
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
/// path (`grouper.py:146-224`, no `project_id`). The workspace-`Sql`
/// branches are SeaORM entity reads; `"None"` is appended per
/// [`qp::group_values_appends_none`]; the `Static` branches are the merged
/// kernels; the `DistinctOverFilteredSet` branches run over the filtered
/// set with the Django `get_extra_select` quirk (values duplicate, NULLs
/// kept as `"None"`), duplicates collapsing like the downstream
/// dict-comp.
async fn profile_group_values_for(
    pool: &sqlx::PgPool,
    field: &str,
    slug: &str,
    filtered: &ProfileSet,
) -> Result<Vec<String>, Denial> {
    use sea_orm::QueryOrder;
    let db = pidash_db::pool::sea_orm_connection(pool);
    let mut out: Vec<String> = match field {
        "state_id" => states::Entity::find()
            .select_only()
            .column(states::Column::Id)
            .join(JoinType::InnerJoin, states::Relation::Workspaces.def())
            .filter(states::Column::IsTriage.eq(false))
            .filter(states::Column::Group.ne("triage"))
            .filter(workspaces::Column::Slug.eq(slug))
            .filter(states::Column::DeletedAt.is_null())
            .order_by_asc(states::Column::Sequence)
            .into_tuple::<uuid::Uuid>()
            .all(&db)
            .await
            .map_err(|_| Denial::ServerError)?
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        "labels__id" => labels::Entity::find()
            .select_only()
            .column(labels::Column::Id)
            .join(JoinType::InnerJoin, labels::Relation::Workspaces.def())
            .filter(workspaces::Column::Slug.eq(slug))
            .filter(labels::Column::DeletedAt.is_null())
            .order_by_desc(labels::Column::CreatedAt)
            .into_tuple::<uuid::Uuid>()
            .all(&db)
            .await
            .map_err(|_| Denial::ServerError)?
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        "assignees__id" => workspace_members::Entity::find()
            .select_only()
            .column(workspace_members::Column::MemberId)
            .join(
                JoinType::InnerJoin,
                workspace_members::Relation::Workspaces.def(),
            )
            .filter(workspaces::Column::Slug.eq(slug))
            .filter(workspace_members::Column::IsActive.eq(true))
            .filter(workspace_members::Column::DeletedAt.is_null())
            .order_by_desc(workspace_members::Column::CreatedAt)
            .into_tuple::<Option<uuid::Uuid>>()
            .all(&db)
            .await
            .map_err(|_| Denial::ServerError)?
            .into_iter()
            .flatten()
            .map(|id| id.to_string())
            .collect(),
        "issue_module__module_id" => modules::Entity::find()
            .select_only()
            .column(modules::Column::Id)
            .join(JoinType::InnerJoin, modules::Relation::Workspaces.def())
            .filter(workspaces::Column::Slug.eq(slug))
            .filter(modules::Column::DeletedAt.is_null())
            .order_by_desc(modules::Column::CreatedAt)
            .into_tuple::<uuid::Uuid>()
            .all(&db)
            .await
            .map_err(|_| Denial::ServerError)?
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        "cycle_id" => cycles::Entity::find()
            .select_only()
            .column(cycles::Column::Id)
            .join(JoinType::InnerJoin, cycles::Relation::Workspaces.def())
            .filter(workspaces::Column::Slug.eq(slug))
            .filter(cycles::Column::DeletedAt.is_null())
            .order_by_desc(cycles::Column::CreatedAt)
            .into_tuple::<uuid::Uuid>()
            .all(&db)
            .await
            .map_err(|_| Denial::ServerError)?
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        "project_id" => projects::Entity::find()
            .select_only()
            .column(projects::Column::Id)
            .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
            .filter(workspaces::Column::Slug.eq(slug))
            .filter(projects::Column::DeletedAt.is_null())
            .order_by_desc(projects::Column::CreatedAt)
            .into_tuple::<uuid::Uuid>()
            .all(&db)
            .await
            .map_err(|_| Denial::ServerError)?
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        "priority" => qp::profile_group_values(field)
            .and_then(|source| match source {
                qp::GroupValuesSource::Static(values) => Some(
                    values
                        .iter()
                        .map(|value| (*value).to_owned())
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_default(),
        "state__group" => qp::profile_group_values(field)
            .and_then(|source| match source {
                qp::GroupValuesSource::Static(values) => Some(
                    values
                        .iter()
                        .map(|value| (*value).to_owned())
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_default(),
        "target_date" | "start_date" | "created_by" => {
            let column = match field {
                "target_date" => issues::Column::TargetDate,
                "start_date" => issues::Column::StartDate,
                _ => issues::Column::CreatedById,
            };
            let mut stmt = filtered.select();
            stmt.distinct()
                .expr_as(p_issue(column).cast_as(p_a("text")), p_a("bucket"))
                .expr_as(
                    p_issue(issues::Column::CreatedAt),
                    p_a("ordering_created_at"),
                )
                .order_by((p_a("issue"), issues::Column::CreatedAt), Order::Desc);
            #[derive(Debug, FromQueryResult)]
            struct BucketRow {
                bucket: Option<String>,
            }
            let rows = p_query_built::<BucketRow>(&db, &stmt).await?;
            let mut seen: Vec<String> = Vec::new();
            for row in rows {
                let bucket = row.bucket.unwrap_or_else(|| "None".to_owned());
                if !seen.contains(&bucket) {
                    seen.push(bucket);
                }
            }
            return Ok(seen);
        }
        _ => return Ok(Vec::new()),
    };
    if qp::group_values_appends_none(field) {
        out.push("None".to_owned());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Unit 3 — user stats (W21, `user.py:397-522`)
// ---------------------------------------------------------------------------

/// `(alias, column)` for `__isnull` predicates over the stats `i` alias.
fn s_isnull_column_i(path: &str) -> Option<Expr> {
    Some(match path {
        "parent" => p_col("i", issues::Column::ParentId),
        "labels" => p_col("label_issue", issue_labels::Column::LabelId),
        "assignees" => p_col("ia", issue_assignees::Column::AssigneeId),
        "created_by" => p_col("i", issues::Column::CreatedById),
        "issue_cycle__cycle_id" => p_col("issue_cycle", cycle_issues::Column::CycleId),
        "issue_module__module_id" => p_col(
            "issue_module",
            pidash_db::entity::module_issues::Column::ModuleId,
        ),
        "label_issue__deleted_at" => p_col("label_issue", issue_labels::Column::DeletedAt),
        "issue_assignee__deleted_at" => p_col("ia", issue_assignees::Column::DeletedAt),
        "issue_cycle__deleted_at" => p_col("issue_cycle", cycle_issues::Column::DeletedAt),
        "issue_module__deleted_at" => p_col(
            "issue_module",
            pidash_db::entity::module_issues::Column::DeletedAt,
        ),
        "issue_subscribers__deleted_at" => p_col(
            "issue_subscribers",
            pidash_db::entity::issue_subscribers::Column::DeletedAt,
        ),
        "target_date" => p_col("i", issues::Column::TargetDate),
        "start_date" => p_col("i", issues::Column::StartDate),
        _ => return None,
    })
}

fn s_uuid_in_column_i(name: &str) -> Option<Expr> {
    Some(match name {
        "state__in" => p_col("i", issues::Column::StateId),
        "parent__in" => p_col("i", issues::Column::ParentId),
        "labels__in" => p_col("label_issue", issue_labels::Column::LabelId),
        "assignees__in" => p_col("ia", issue_assignees::Column::AssigneeId),
        "issue_mention__mention__id__in" => p_col(
            "issue_mention",
            pidash_db::entity::issue_mentions::Column::MentionId,
        ),
        "created_by__in" => p_col("i", issues::Column::CreatedById),
        "project__in" => p_col("i", issues::Column::ProjectId),
        "issue_cycle__cycle_id__in" => p_col("issue_cycle", cycle_issues::Column::CycleId),
        "issue_module__module_id__in" => p_col(
            "issue_module",
            pidash_db::entity::module_issues::Column::ModuleId,
        ),
        "issue_subscribers__subscriber_id__in" => p_col(
            "issue_subscribers",
            pidash_db::entity::issue_subscribers::Column::SubscriberId,
        ),
        _ => return None,
    })
}

fn s_strings_in_column_i(name: &str) -> Option<Expr> {
    Some(match name {
        "state__group__in" => p_col("s", states::Column::Group),
        "estimate_point__in" => p_col("i", issues::Column::EstimatePointId),
        "priority__in" => p_col("i", issues::Column::Priority),
        "issue_intake__status__in" => p_col("issue_intake", intake_issues::Column::Status),
        _ => return None,
    })
}

/// A `__date` term truncated in UTC: `(i.<column> AT TIME ZONE 'UTC')::date`.
fn s_zoned_date_i(column: issues::Column) -> SimpleExpr {
    p_col("i", column)
        .binary(BinOper::Custom("AT TIME ZONE"), p_text("UTC"))
        .cast_as(p_a("date"))
}

fn s_date_term_i(term: &str) -> Option<SimpleExpr> {
    Some(match term {
        "created_at__date" => s_zoned_date_i(issues::Column::CreatedAt),
        "completed_at__date" => s_zoned_date_i(issues::Column::CompletedAt),
        "start_date" => p_col("i", issues::Column::StartDate).into(),
        "target_date" => p_col("i", issues::Column::TargetDate).into(),
        _ => return None,
    })
}

fn s_date_bound_i(left: SimpleExpr, operator: &str, value: String) -> Option<SimpleExpr> {
    let right = Expr::val(value).cast_as(p_a("date"));
    match operator {
        "gte" => Some(left.gte(right)),
        "lte" => Some(left.lte(right)),
        _ => None,
    }
}

fn s_like_pattern(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

/// One legacy predicate over the stats `i` alias (same columns, same
/// errors as the text engine's `legacy_sql` with the `issue.` → `i.`,
/// `state.` → `s.`, `issue_assignee.` → `ia.` rewrite applied at compile
/// time).
fn s_legacy_expr_i(name: &str, value: &FilterValue) -> Result<SimpleExpr, Denial> {
    if let Some(path) = name.strip_suffix("__isnull") {
        let column = s_isnull_column_i(path).ok_or(Denial::ServerError)?;
        let flag = match value {
            FilterValue::Flag(flag) => *flag,
            _ => return Err(Denial::ServerError),
        };
        return Ok(if flag {
            column.is_null()
        } else {
            column.is_not_null()
        });
    }
    match value {
        FilterValue::Uuids(ids) => {
            let column = s_uuid_in_column_i(name).ok_or(Denial::ServerError)?;
            if ids.is_empty() {
                return Ok(Expr::val(false).into());
            }
            Ok(column.is_in(ids.iter().copied()))
        }
        FilterValue::Strings(items) => {
            let column = s_strings_in_column_i(name).ok_or(Denial::ServerError)?;
            if items.is_empty() {
                return Ok(Expr::val(false).into());
            }
            if name == "issue_intake__status__in" {
                let mut numbers = Vec::with_capacity(items.len());
                for item in items {
                    numbers.push(
                        item.parse::<i32>().map_err(|_| {
                            Denial::BadError("Please provide valid detail".to_owned())
                        })?,
                    );
                }
                return Ok(column.is_in(numbers));
            }
            if name == "estimate_point__in" {
                let mut ids = Vec::with_capacity(items.len());
                for item in items {
                    ids.push(
                        item.parse::<uuid::Uuid>().map_err(|_| {
                            Denial::BadError("Please provide valid detail".to_owned())
                        })?,
                    );
                }
                return Ok(column.is_in(ids));
            }
            Ok(column.is_in(items.iter().cloned()))
        }
        FilterValue::Text(raw) => {
            if name == "name__icontains" {
                return Ok(p_col("i", issues::Column::Name).ilike(s_like_pattern(raw)));
            }
            let Some((term, operator)) = name.rsplit_once("__") else {
                return Err(Denial::ServerError);
            };
            if !matches!(operator, "gte" | "lte" | "contains") {
                return Err(Denial::ServerError);
            }
            let left = s_date_term_i(term).ok_or(Denial::ServerError)?;
            if raw.parse::<chrono::NaiveDate>().is_err() && p_parse_datetime_param(raw).is_none() {
                return Err(Denial::BadError("Please provide valid detail".to_owned()));
            }
            if operator == "contains" {
                return Ok(left.cast_as(p_a("text")).like(s_like_pattern(raw)));
            }
            s_date_bound_i(left, operator, raw.to_owned()).ok_or(Denial::ServerError)
        }
        FilterValue::Flag(_) => Err(Denial::ServerError),
        FilterValue::Day(day) => {
            let (term, operator) = name.rsplit_once("__").ok_or(Denial::ServerError)?;
            let left = s_date_term_i(term).ok_or(Denial::ServerError)?;
            s_date_bound_i(left, operator, day.to_string()).ok_or(Denial::ServerError)
        }
        FilterValue::Null => {
            let column = s_isnull_column_i(name).ok_or(Denial::ServerError)?;
            Ok(column.is_null())
        }
    }
}

/// The priority `CASE` over the `i` alias (`urgent=0 .. none=4`, else 5).
fn s_priority_case_i() -> SimpleExpr {
    let mut case = CaseStatement::new();
    for (index, priority) in pidash_services::app_issues::ordering::PRIORITY_ORDER
        .iter()
        .enumerate()
    {
        case = case.case(
            p_col("i", issues::Column::Priority).eq(*priority),
            Expr::val(index as i32),
        );
    }
    case.finally(Expr::val(5)).into()
}

/// Relation legs for a stats legacy condition (INNER for value predicates,
/// LEFT for `__isnull`-only; `ia` reused when the preamble carries it).
fn s_stats_relation_joins(
    stmt: &mut SelectStatement,
    legacy: &Option<Condition>,
    has_assignee_join: bool,
) {
    let Some(cond) = legacy else {
        return;
    };
    let (fragment, _) = render_condition(cond);
    const LEGS: &[(&str, &str)] = &[
        ("label_issue", "issue_labels"),
        ("issue_cycle", "cycle_issues"),
        ("issue_module", "module_issues"),
        ("issue_mention", "issue_mentions"),
        ("issue_subscribers", "issue_subscribers"),
        ("issue_intake", "intake_issues"),
    ];
    let referenced = |alias: &str| {
        fragment.contains(&format!("\"{alias}\".")) || fragment.contains(&format!("{alias}."))
    };
    for (alias, table) in LEGS {
        if !referenced(alias) {
            continue;
        }
        let kind = if alias_nullable_only(&fragment, alias) {
            JoinType::LeftJoin
        } else {
            JoinType::InnerJoin
        };
        p_add_stats_leg(stmt, table, alias, kind);
    }
    if referenced("ia") && !has_assignee_join {
        let kind = if alias_nullable_only(&fragment, "ia") {
            JoinType::LeftJoin
        } else {
            JoinType::InnerJoin
        };
        stmt.join_as(
            kind,
            issue_assignees::Entity,
            p_a("ia"),
            p_col("ia", issue_assignees::Column::IssueId).equals((p_a("i"), issues::Column::Id)),
        );
    }
}

fn p_add_stats_leg(stmt: &mut SelectStatement, table: &str, alias: &'static str, kind: JoinType) {
    let on = |issue_id: Expr| issue_id.equals((p_a("i"), issues::Column::Id));
    match table {
        "issue_labels" => {
            stmt.join_as(
                kind,
                issue_labels::Entity,
                p_a(alias),
                on(p_col(alias, issue_labels::Column::IssueId)),
            );
        }
        "cycle_issues" => {
            stmt.join_as(
                kind,
                cycle_issues::Entity,
                p_a(alias),
                on(p_col(alias, cycle_issues::Column::IssueId)),
            );
        }
        "module_issues" => {
            stmt.join_as(
                kind,
                pidash_db::entity::module_issues::Entity,
                p_a(alias),
                on(p_col(
                    alias,
                    pidash_db::entity::module_issues::Column::IssueId,
                )),
            );
        }
        "issue_mentions" => {
            stmt.join_as(
                kind,
                pidash_db::entity::issue_mentions::Entity,
                p_a(alias),
                on(p_col(
                    alias,
                    pidash_db::entity::issue_mentions::Column::IssueId,
                )),
            );
        }
        "issue_subscribers" => {
            stmt.join_as(
                kind,
                pidash_db::entity::issue_subscribers::Entity,
                p_a(alias),
                on(p_col(
                    alias,
                    pidash_db::entity::issue_subscribers::Column::IssueId,
                )),
            );
        }
        "intake_issues" => {
            stmt.join_as(
                kind,
                intake_issues::Entity,
                p_a(alias),
                on(p_col(alias, intake_issues::Column::IssueId)),
            );
        }
        _ => {}
    }
}

/// `updated_at__gt`-style datetime param parsing (Django
/// `DateTimeField.get_prep_value` parity: RFC-3339 plus the naive
/// `DATETIME_INPUT_FORMATS` shapes as UTC).
fn p_parse_datetime_param(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(aware) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(aware.with_timezone(&chrono::Utc));
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%m/%d/%Y %H:%M:%S",
        "%m/%d/%Y %H:%M",
        "%m/%d/%y %H:%M:%S",
        "%m/%d/%y %H:%M",
        "%Y-%m-%d",
        "%m/%d/%Y",
        "%m/%d/%y",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return Some(naive.and_utc());
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(text, format) {
            return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
        }
    }
    None
}

/// Compile the legacy predicates for the Q7 subscriber scope
/// (`user.py:484-494`): the SAME `**filters` dict, resolved against
/// `IssueSubscriber`. Only `created_by`, `project` and `created_at`
/// lookups resolve there; every other name is a Django `FieldError` →
/// generic 500. Date validation mirrors the pilot compiler (garbage
/// bounds are `ValidationError` → 400 invalid detail).
fn subscriber_legacy_expr(name: &str, value: &FilterValue) -> Result<SimpleExpr, Denial> {
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
            p_col(
                "sub",
                pidash_db::entity::issue_subscribers::Column::CreatedById,
            )
            .is_null()
        } else {
            p_col(
                "sub",
                pidash_db::entity::issue_subscribers::Column::CreatedById,
            )
            .is_not_null()
        });
    }
    match value {
        FilterValue::Uuids(ids) => {
            let column = match name {
                "created_by__in" => p_col(
                    "sub",
                    pidash_db::entity::issue_subscribers::Column::CreatedById,
                ),
                "project__in" => p_col(
                    "sub",
                    pidash_db::entity::issue_subscribers::Column::ProjectId,
                ),
                _ => return Err(Denial::ServerError),
            };
            if ids.is_empty() {
                return Ok(Expr::val(false).into());
            }
            Ok(column.is_in(ids.iter().copied()))
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
                "contains" => Ok(p_col(
                    "sub",
                    pidash_db::entity::issue_subscribers::Column::CreatedAt,
                )
                .cast_as(p_a("text"))
                .like(s_like_pattern(text))),
                "gte" | "lte" => {
                    let right = Expr::val(text.clone()).cast_as(p_a("date"));
                    let left = p_col(
                        "sub",
                        pidash_db::entity::issue_subscribers::Column::CreatedAt,
                    )
                    .cast_as(p_a("date"));
                    Ok(if operator == "gte" {
                        left.gte(right)
                    } else {
                        left.lte(right)
                    })
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
            let left = p_col(
                "sub",
                pidash_db::entity::issue_subscribers::Column::CreatedAt,
            )
            .cast_as(p_a("date"));
            let right = Expr::val(day.to_string()).cast_as(p_a("date"));
            Ok(if operator == ">=" {
                left.gte(right)
            } else {
                left.lte(right)
            })
        }
        FilterValue::Null => {
            if name != "created_by" {
                return Err(Denial::ServerError);
            }
            Ok(p_col(
                "sub",
                pidash_db::entity::issue_subscribers::Column::CreatedById,
            )
            .is_null())
        }
    }
}

/// Compile the request's legacy `issue_filters(params, "GET")` once per
/// scope: [`s_legacy_expr_i`] for issue-rooted queries (Q1-Q6),
/// [`subscriber_legacy_expr`] for Q7. `None` means unfiltered.
fn compile_stats_legacy(
    query: &QueryMap,
) -> Result<(Option<Condition>, Option<Condition>), Denial> {
    let flat: HashMap<String, String> = query
        .keys()
        .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let legacy = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
        .map_err(|_| Denial::ServerError)?;
    let mut issue_cond = Condition::all();
    let mut sub_cond = Condition::all();
    let mut any = false;
    for (name, value) in legacy.predicates() {
        // UTC keeps this slice's current truncation; its own actor-zone
        // fix follows the app_issues pattern separately.
        issue_cond = issue_cond.add(s_legacy_expr_i(name, value)?);
        sub_cond = sub_cond.add(subscriber_legacy_expr(name, value)?);
        any = true;
    }
    if any {
        Ok((Some(issue_cond), Some(sub_cond)))
    } else {
        Ok((None, None))
    }
}

/// Shared FROM/WHERE preamble for the issue-rooted stats queries (Q1-Q6):
/// root `issue_objects` scope, slug, requester membership.
fn stats_issue_base(
    user_id: uuid::Uuid,
    slug: &str,
    viewer: uuid::Uuid,
    extra: Option<SimpleExpr>,
    legacy: &Option<Condition>,
    has_assignee_join: bool,
) -> SelectStatement {
    let mut stmt = SeaQuery::select();
    stmt.from_as(issues::Entity, p_a("i"));
    if has_assignee_join {
        stmt.join_as(
            JoinType::InnerJoin,
            issue_assignees::Entity,
            p_a("ia"),
            p_col("ia", issue_assignees::Column::IssueId).equals((p_a("i"), issues::Column::Id)),
        );
    }
    stmt.join(
        JoinType::InnerJoin,
        workspaces::Entity,
        Expr::col((workspaces::Entity, workspaces::Column::Id))
            .equals((p_a("i"), issues::Column::WorkspaceId)),
    )
    .join_as(
        JoinType::InnerJoin,
        projects::Entity,
        p_a("p"),
        p_col("p", projects::Column::Id).equals((p_a("i"), issues::Column::ProjectId)),
    )
    .join_as(
        JoinType::LeftJoin,
        states::Entity,
        p_a("s"),
        p_col("s", states::Column::Id).equals((p_a("i"), issues::Column::StateId)),
    )
    .join_as(
        JoinType::InnerJoin,
        project_members::Entity,
        p_a("rpm"),
        p_col("rpm", project_members::Column::ProjectId).equals((p_a("p"), projects::Column::Id)),
    );
    s_stats_relation_joins(&mut stmt, legacy, has_assignee_join);
    // `rpm` carries no deleted guard (filter joins never apply a related
    // manager — ported as-is).
    let mut cond = Condition::all()
        .add(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
        .add(p_col("rpm", project_members::Column::MemberId).eq(viewer))
        .add(p_col("rpm", project_members::Column::IsActive).eq(true))
        .add(p_col("i", issues::Column::DeletedAt).is_null())
        .add(p_not_triage("s"))
        .add(p_col("i", issues::Column::ArchivedAt).is_null())
        .add(p_col("p", projects::Column::ArchivedAt).is_null())
        .add(p_col("i", issues::Column::IsDraft).eq(false));
    if has_assignee_join {
        cond = cond
            .add(p_col("ia", issue_assignees::Column::AssigneeId).eq(user_id))
            .add(p_col("ia", issue_assignees::Column::DeletedAt).is_null());
    }
    if let Some(extra) = extra {
        cond = cond.add(extra);
    }
    if let Some(legacy_cond) = legacy.clone() {
        cond = cond.add(legacy_cond);
    }
    stmt.cond_where(cond);
    stmt
}

#[derive(Debug, FromQueryResult)]
struct SStateRow {
    state_group: Option<String>,
    state_count: i64,
}

#[derive(Debug, FromQueryResult)]
struct SPriorityRow {
    priority: Option<String>,
    priority_count: i64,
    priority_order: i64,
}

#[derive(Debug, FromQueryResult)]
struct SCycleRow {
    name: String,
    id: uuid::Uuid,
    project_id: uuid::Uuid,
}

/// Q1 state distribution (`user.py:401-413`).
fn stats_state_select(
    user_id: uuid::Uuid,
    slug: &str,
    viewer: uuid::Uuid,
    legacy: &Option<Condition>,
) -> SelectStatement {
    let mut stmt = stats_issue_base(user_id, slug, viewer, None, legacy, true);
    stmt.expr_as(p_col("s", states::Column::Group), p_a("state_group"))
        .expr_as(
            Func::count(p_col("s", states::Column::Group)),
            p_a("state_count"),
        )
        .group_by_col(p_a("state_group"))
        .order_by(p_a("state_group"), Order::Asc);
    stmt
}

/// Q2 priority distribution (`user.py:417-436`). Q2 `HAVING COUNT(*) >= 1`
/// is always true — omitted (semantics identical).
fn stats_priority_select(
    user_id: uuid::Uuid,
    slug: &str,
    viewer: uuid::Uuid,
    legacy: &Option<Condition>,
) -> SelectStatement {
    let mut stmt = stats_issue_base(user_id, slug, viewer, None, legacy, true);
    stmt.expr_as(p_col("i", issues::Column::Priority), p_a("priority"))
        .expr_as(
            Func::count(p_col("i", issues::Column::Priority)),
            p_a("priority_count"),
        )
        .expr_as(s_priority_case_i(), p_a("priority_order"))
        .group_by_col(p_a("priority"))
        .order_by(p_a("priority_order"), Order::Asc);
    stmt
}

/// Q3 created count (`created_by_id = uid`, no assignee joins).
fn stats_created_select(
    user_id: uuid::Uuid,
    slug: &str,
    viewer: uuid::Uuid,
    legacy: &Option<Condition>,
) -> SelectStatement {
    let mut stmt = stats_issue_base(
        user_id,
        slug,
        viewer,
        Some(p_col("i", issues::Column::CreatedById).eq(user_id)),
        legacy,
        false,
    );
    stmt.expr_as(Func::count(Expr::col(Asterisk)), p_a("n"));
    stmt
}

/// Q4 assigned count (single-join assignee scope; fanout overcounts).
fn stats_assigned_select(
    user_id: uuid::Uuid,
    slug: &str,
    viewer: uuid::Uuid,
    legacy: &Option<Condition>,
) -> SelectStatement {
    let mut stmt = stats_issue_base(user_id, slug, viewer, None, legacy, true);
    stmt.expr_as(Func::count(Expr::col(Asterisk)), p_a("n"));
    stmt
}

/// Q5 pending count (`~Q(state__group__in=CLOSED)` keeps NULL-state rows).
fn stats_pending_select(
    user_id: uuid::Uuid,
    slug: &str,
    viewer: uuid::Uuid,
    legacy: &Option<Condition>,
) -> SelectStatement {
    let pending = p_col("s", states::Column::Group)
        .is_null()
        .or(p_col("s", states::Column::Group)
            .is_in(["completed", "cancelled"])
            .not());
    let mut stmt = stats_issue_base(user_id, slug, viewer, Some(pending), legacy, true);
    stmt.expr_as(Func::count(Expr::col(Asterisk)), p_a("n"));
    stmt
}

/// Q6 completed count (literal `"completed"`, not the `CLOSED` pair).
fn stats_completed_select(
    user_id: uuid::Uuid,
    slug: &str,
    viewer: uuid::Uuid,
    legacy: &Option<Condition>,
) -> SelectStatement {
    let mut stmt = stats_issue_base(
        user_id,
        slug,
        viewer,
        Some(p_col("s", states::Column::Group).eq("completed")),
        legacy,
        true,
    );
    stmt.expr_as(Func::count(Expr::col(Asterisk)), p_a("n"));
    stmt
}

/// Q7 subscribed count (own `deleted_at` guard + project-archived only).
fn stats_subscribed_select(
    user_id: uuid::Uuid,
    slug: &str,
    viewer: uuid::Uuid,
    legacy: &Option<Condition>,
) -> SelectStatement {
    use pidash_db::entity::issue_subscribers;
    let mut stmt = SeaQuery::select();
    stmt.expr_as(Func::count(Expr::col(Asterisk)), p_a("n"))
        .from_as(issue_subscribers::Entity, p_a("sub"))
        .join(
            JoinType::InnerJoin,
            workspaces::Entity,
            Expr::col((workspaces::Entity, workspaces::Column::Id))
                .equals((p_a("sub"), issue_subscribers::Column::WorkspaceId)),
        )
        .join_as(
            JoinType::InnerJoin,
            projects::Entity,
            p_a("p"),
            p_col("p", projects::Column::Id)
                .equals((p_a("sub"), issue_subscribers::Column::ProjectId)),
        )
        .join_as(
            JoinType::InnerJoin,
            project_members::Entity,
            p_a("rpm"),
            p_col("rpm", project_members::Column::ProjectId)
                .equals((p_a("p"), projects::Column::Id)),
        );
    let mut cond = Condition::all()
        .add(p_col("sub", issue_subscribers::Column::SubscriberId).eq(user_id))
        .add(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
        .add(p_col("rpm", project_members::Column::MemberId).eq(viewer))
        .add(p_col("rpm", project_members::Column::IsActive).eq(true))
        .add(p_col("p", projects::Column::ArchivedAt).is_null())
        .add(p_col("sub", issue_subscribers::Column::DeletedAt).is_null());
    if let Some(legacy_cond) = legacy.clone() {
        cond = cond.add(legacy_cond);
    }
    stmt.cond_where(cond);
    stmt
}

/// Q8 upcoming cycles (`start_date > now`, no filters, no scope).
fn stats_upcoming_select(
    slug: &str,
    now: chrono::DateTime<chrono::Utc>,
    user_id: uuid::Uuid,
) -> SelectStatement {
    let mut stmt = SeaQuery::select();
    stmt.column((p_a("c"), cycles::Column::Name))
        .column((p_a("c"), cycles::Column::Id))
        .column((p_a("c"), cycles::Column::ProjectId))
        .from_as(cycle_issues::Entity, p_a("ci"))
        .join_as(
            JoinType::InnerJoin,
            cycles::Entity,
            p_a("c"),
            p_col("c", cycles::Column::Id).equals((p_a("ci"), cycle_issues::Column::CycleId)),
        )
        .join(
            JoinType::InnerJoin,
            workspaces::Entity,
            Expr::col((workspaces::Entity, workspaces::Column::Id))
                .equals((p_a("ci"), cycle_issues::Column::WorkspaceId)),
        )
        .join_as(
            JoinType::InnerJoin,
            issues::Entity,
            p_a("i"),
            p_col("i", issues::Column::Id).equals((p_a("ci"), cycle_issues::Column::IssueId)),
        )
        .join_as(
            JoinType::InnerJoin,
            issue_assignees::Entity,
            p_a("ia"),
            p_col("ia", issue_assignees::Column::IssueId).equals((p_a("i"), issues::Column::Id)),
        )
        .and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
        .and_where(p_col("c", cycles::Column::StartDate).gt(now))
        .and_where(p_col("ia", issue_assignees::Column::AssigneeId).eq(user_id))
        .and_where(p_col("ci", cycle_issues::Column::DeletedAt).is_null())
        .order_by((p_a("ci"), cycle_issues::Column::CreatedAt), Order::Desc);
    stmt
}

/// Q9 present cycle (`start < now < end`).
fn stats_present_select(
    slug: &str,
    now: chrono::DateTime<chrono::Utc>,
    user_id: uuid::Uuid,
) -> SelectStatement {
    let mut stmt = SeaQuery::select();
    stmt.column((p_a("c"), cycles::Column::Name))
        .column((p_a("c"), cycles::Column::Id))
        .column((p_a("c"), cycles::Column::ProjectId))
        .from_as(cycle_issues::Entity, p_a("ci"))
        .join_as(
            JoinType::InnerJoin,
            cycles::Entity,
            p_a("c"),
            p_col("c", cycles::Column::Id).equals((p_a("ci"), cycle_issues::Column::CycleId)),
        )
        .join(
            JoinType::InnerJoin,
            workspaces::Entity,
            Expr::col((workspaces::Entity, workspaces::Column::Id))
                .equals((p_a("ci"), cycle_issues::Column::WorkspaceId)),
        )
        .join_as(
            JoinType::InnerJoin,
            issues::Entity,
            p_a("i"),
            p_col("i", issues::Column::Id).equals((p_a("ci"), cycle_issues::Column::IssueId)),
        )
        .join_as(
            JoinType::InnerJoin,
            issue_assignees::Entity,
            p_a("ia"),
            p_col("ia", issue_assignees::Column::IssueId).equals((p_a("i"), issues::Column::Id)),
        )
        .and_where(Expr::col((workspaces::Entity, workspaces::Column::Slug)).eq(slug))
        .and_where(p_col("c", cycles::Column::StartDate).lt(now))
        .and_where(p_col("c", cycles::Column::EndDate).gt(now))
        .and_where(p_col("ia", issue_assignees::Column::AssigneeId).eq(user_id))
        .and_where(p_col("ci", cycle_issues::Column::DeletedAt).is_null())
        .order_by((p_a("ci"), cycle_issues::Column::CreatedAt), Order::Desc);
    stmt
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
    // The `<uuid:user_id>` converter runs at URL resolution, before auth.
    let user_id = parse_uuid_path(&user_raw)?;
    let pool = pool_of(&state)?;
    let viewer = actor_user_id(extension)?;
    // No permission_classes (`:397`): any authenticated caller runs the
    // bundle; the queries scope by requester.
    let (legacy, sub_legacy) = compile_stats_legacy(&query)?;
    let db = pidash_db::pool::sea_orm_connection(&pool);
    let state_stmt = stats_state_select(user_id, &slug, viewer, &legacy);
    let state_rows = p_query_built::<SStateRow>(&db, &state_stmt).await?;
    let mut state_distribution = String::from("[");
    for (index, row) in state_rows.iter().enumerate() {
        if index > 0 {
            state_distribution.push(',');
        }
        let group = match &row.state_group {
            Some(text) => json_string(text),
            None => "null".to_owned(),
        };
        state_distribution.push_str(&format!(
            "{{\"state_group\":{group},\"state_count\":{}}}",
            row.state_count
        ));
    }
    state_distribution.push(']');
    let priority_stmt = stats_priority_select(user_id, &slug, viewer, &legacy);
    let priority_rows = p_query_built::<SPriorityRow>(&db, &priority_stmt).await?;
    let mut priority_distribution = String::from("[");
    for (index, row) in priority_rows.iter().enumerate() {
        if index > 0 {
            priority_distribution.push(',');
        }
        let priority = match &row.priority {
            Some(text) => json_string(text),
            None => "null".to_owned(),
        };
        priority_distribution.push_str(&format!(
            "{{\"priority\":{priority},\"priority_count\":{},\"priority_order\":{}}}",
            row.priority_count, row.priority_order
        ));
    }
    priority_distribution.push(']');
    let created_stmt = stats_created_select(user_id, &slug, viewer, &legacy);
    let created_issues = p_fetch_n(&pool, &created_stmt).await?;
    let assigned_stmt = stats_assigned_select(user_id, &slug, viewer, &legacy);
    let assigned_issues = p_fetch_n(&pool, &assigned_stmt).await?;
    let pending_stmt = stats_pending_select(user_id, &slug, viewer, &legacy);
    let pending_issues = p_fetch_n(&pool, &pending_stmt).await?;
    let completed_stmt = stats_completed_select(user_id, &slug, viewer, &legacy);
    let completed_issues = p_fetch_n(&pool, &completed_stmt).await?;
    let subscribed_stmt = stats_subscribed_select(user_id, &slug, viewer, &sub_legacy);
    let subscribed_issues = p_fetch_n(&pool, &subscribed_stmt).await?;
    // Q8/Q9 take no legacy filters and no requester scope (`:496-507`).
    let now = chrono::Utc::now();
    let upcoming_stmt = stats_upcoming_select(&slug, now, user_id);
    let upcoming_rows = p_query_built::<SCycleRow>(&db, &upcoming_stmt).await?;
    let present_stmt = stats_present_select(&slug, now, user_id);
    let present_rows = p_query_built::<SCycleRow>(&db, &present_stmt).await?;
    let cycles = |rows: Vec<SCycleRow>| {
        rows.into_iter()
            .map(|row| StatsCycle {
                name: row.name,
                id: row.id.to_string(),
                project_id: row.project_id.to_string(),
            })
            .collect::<Vec<_>>()
    };
    let present = cycles(present_rows);
    let upcoming = cycles(upcoming_rows);
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
            present_cycles: present,
            upcoming_cycles: upcoming,
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
fn activity_order_expr(order_by: &str) -> Result<(SimpleExpr, bool), Denial> {
    let descending = order_by.starts_with('-');
    let key = order_by.trim_start_matches('-');
    let column: SimpleExpr = match key {
        "id" => p_col("a", issue_activities::Column::Id).into(),
        "created_at" => p_col("a", issue_activities::Column::CreatedAt).into(),
        "updated_at" => p_col("a", issue_activities::Column::UpdatedAt).into(),
        "deleted_at" => p_col("a", issue_activities::Column::DeletedAt).into(),
        "verb" => p_col("a", issue_activities::Column::Verb).into(),
        "field" => p_col("a", issue_activities::Column::Field).into(),
        "old_value" => p_col("a", issue_activities::Column::OldValue).into(),
        "new_value" => p_col("a", issue_activities::Column::NewValue).into(),
        "comment" => p_col("a", issue_activities::Column::Comment).into(),
        "attachments" => p_col("a", issue_activities::Column::Attachments).into(),
        "old_identifier" => p_col("a", issue_activities::Column::OldIdentifier).into(),
        "new_identifier" => p_col("a", issue_activities::Column::NewIdentifier).into(),
        "epoch" => p_col("a", issue_activities::Column::Epoch).into(),
        "issue" | "issue_id" => p_col("a", issue_activities::Column::IssueId).into(),
        "project" | "project_id" => p_col("a", issue_activities::Column::ProjectId).into(),
        "workspace" | "workspace_id" => p_col("a", issue_activities::Column::WorkspaceId).into(),
        "issue_comment" | "issue_comment_id" => {
            p_col("a", issue_activities::Column::IssueCommentId).into()
        }
        "actor" | "actor_id" => p_col("a", issue_activities::Column::ActorId).into(),
        "created_by" | "created_by_id" => p_col("a", issue_activities::Column::CreatedById).into(),
        "updated_by" | "updated_by_id" => p_col("a", issue_activities::Column::UpdatedById).into(),
        _ => return Err(Denial::ServerError),
    };
    Ok((column, descending))
}

/// Wide row selects for the activity lists over the `a` / `w` / `p` /
/// `actor_u` / `i` aliases: every `IssueActivitySerializer` input plus the
/// nested lite columns (actor, issue flat, project lite, workspace lite).
/// Asset entity types resolve in a follow-up batched query
/// ([`fetch_asset_entities`]).
fn activity_annotate(stmt: &mut SelectStatement) {
    stmt.expr_as(p_col("a", issue_activities::Column::Id), p_a("aid"))
        .expr_as(
            p_col("a", issue_activities::Column::CreatedAt),
            p_a("a_created_at"),
        )
        .expr_as(
            p_col("a", issue_activities::Column::UpdatedAt),
            p_a("a_updated_at"),
        )
        .expr_as(
            p_col("a", issue_activities::Column::DeletedAt),
            p_a("a_deleted_at"),
        )
        .column((p_a("a"), issue_activities::Column::Verb))
        .column((p_a("a"), issue_activities::Column::Field))
        .column((p_a("a"), issue_activities::Column::OldValue))
        .column((p_a("a"), issue_activities::Column::NewValue))
        .column((p_a("a"), issue_activities::Column::Comment))
        .column((p_a("a"), issue_activities::Column::Attachments))
        .column((p_a("a"), issue_activities::Column::OldIdentifier))
        .column((p_a("a"), issue_activities::Column::NewIdentifier))
        .column((p_a("a"), issue_activities::Column::Epoch))
        .expr_as(
            p_col("a", issue_activities::Column::CreatedById),
            p_a("created_by"),
        )
        .expr_as(
            p_col("a", issue_activities::Column::UpdatedById),
            p_a("updated_by"),
        )
        .expr_as(
            p_col("a", issue_activities::Column::ProjectId),
            p_a("project"),
        )
        .expr_as(
            p_col("a", issue_activities::Column::WorkspaceId),
            p_a("workspace"),
        )
        .expr_as(p_col("a", issue_activities::Column::IssueId), p_a("issue"))
        .expr_as(
            p_col("a", issue_activities::Column::IssueCommentId),
            p_a("issue_comment"),
        )
        .expr_as(p_col("a", issue_activities::Column::ActorId), p_a("actor"))
        .expr_as(p_col("actor_u", users::Column::Id), p_a("actor_id"))
        .expr_as(
            p_col("actor_u", users::Column::FirstName),
            p_a("actor_first_name"),
        )
        .expr_as(
            p_col("actor_u", users::Column::LastName),
            p_a("actor_last_name"),
        )
        .expr_as(p_col("actor_u", users::Column::Avatar), p_a("actor_avatar"))
        .expr_as(
            p_col("actor_u", users::Column::AvatarAssetId),
            p_a("actor_avatar_asset_id"),
        )
        .expr_as(p_col("actor_u", users::Column::IsBot), p_a("actor_is_bot"))
        .expr_as(
            p_col("actor_u", users::Column::DisplayName),
            p_a("actor_display_name"),
        )
        .expr_as(p_col("i", issues::Column::Id), p_a("issue_id"))
        .expr_as(p_col("i", issues::Column::Name), p_a("issue_name"))
        .expr_as(
            p_col("i", issues::Column::DescriptionJson),
            p_a("issue_description_json"),
        )
        .expr_as(
            p_col("i", issues::Column::DescriptionHtml),
            p_a("issue_description_html"),
        )
        .expr_as(p_col("i", issues::Column::Priority), p_a("issue_priority"))
        .expr_as(
            p_col("i", issues::Column::ComplexityScore),
            p_a("issue_complexity_score"),
        )
        .expr_as(
            p_col("i", issues::Column::StartDate),
            p_a("issue_start_date"),
        )
        .expr_as(
            p_col("i", issues::Column::TargetDate),
            p_a("issue_target_date"),
        )
        .expr_as(
            p_col("i", issues::Column::SequenceId),
            p_a("issue_sequence_id"),
        )
        .expr_as(
            p_col("i", issues::Column::SortOrder),
            p_a("issue_sort_order"),
        )
        .expr_as(p_col("i", issues::Column::IsDraft), p_a("issue_is_draft"))
        .expr_as(p_col("p", projects::Column::Id), p_a("project_id"))
        .expr_as(
            p_col("p", projects::Column::Identifier),
            p_a("project_identifier"),
        )
        .expr_as(p_col("p", projects::Column::Name), p_a("project_name"))
        .expr_as(
            p_col("p", projects::Column::CoverImage),
            p_a("project_cover_image"),
        )
        .expr_as(
            p_col("p", projects::Column::CoverImageAssetId),
            p_a("project_cover_image_asset_id"),
        )
        .expr_as(
            p_col("p", projects::Column::LogoProps),
            p_a("project_logo_props"),
        )
        .expr_as(
            p_col("p", projects::Column::Description),
            p_a("project_description"),
        )
        .expr_as(
            p_col("p", projects::Column::IsDefault),
            p_a("project_is_default"),
        )
        .expr_as(p_col("w", workspaces::Column::Name), p_a("workspace_name"))
        .expr_as(p_col("w", workspaces::Column::Slug), p_a("workspace_slug"))
        .expr_as(p_col("w", workspaces::Column::Id), p_a("workspace_id"))
        .expr_as(p_col("w", workspaces::Column::Logo), p_a("workspace_logo"))
        .expr_as(
            p_col("w", workspaces::Column::LogoAssetId),
            p_a("workspace_logo_asset_id"),
        );
}

/// Shared joins for the activity lists (`select_related("actor",
/// "workspace", "issue", "project")`): `actor_u` is INNER (promoted under
/// the `actor_id =` equality), `i` is LEFT (nullable FK).
fn activity_joins(stmt: &mut SelectStatement, with_slug: bool) {
    stmt.join_as(
        JoinType::InnerJoin,
        workspaces::Entity,
        p_a("w"),
        p_col("w", workspaces::Column::Id)
            .equals((p_a("a"), issue_activities::Column::WorkspaceId)),
    )
    .join_as(
        JoinType::InnerJoin,
        projects::Entity,
        p_a("p"),
        p_col("p", projects::Column::Id).equals((p_a("a"), issue_activities::Column::ProjectId)),
    );
    if with_slug {
        stmt.join_as(
            JoinType::InnerJoin,
            project_members::Entity,
            p_a("rpm"),
            p_col("rpm", project_members::Column::ProjectId)
                .equals((p_a("p"), projects::Column::Id)),
        );
    }
    stmt.join_as(
        JoinType::InnerJoin,
        users::Entity,
        p_a("actor_u"),
        p_col("actor_u", users::Column::Id).equals((p_a("a"), issue_activities::Column::ActorId)),
    )
    .join_as(
        JoinType::LeftJoin,
        issues::Entity,
        p_a("i"),
        p_col("i", issues::Column::Id).equals((p_a("a"), issue_activities::Column::IssueId)),
    );
}

/// Workspace user-activity base (`user.py:377-387`): field exclusion (NULL-
/// inclusive), slug, requester project scope, project-unarchived, `actor
/// = uid`. Root `IssueActivity.objects` guard applies.
fn user_activity_base(
    user_id: uuid::Uuid,
    slug: &str,
    viewer: uuid::Uuid,
    projects: Option<Vec<uuid::Uuid>>,
) -> SelectStatement {
    let mut stmt = SeaQuery::select();
    stmt.from_as(issue_activities::Entity, p_a("a"));
    activity_joins(&mut stmt, true);
    let mut cond = Condition::all()
        .add(
            p_col("a", issue_activities::Column::Field)
                .is_in(["comment", "vote", "reaction", "draft"])
                .and(p_col("a", issue_activities::Column::Field).is_not_null())
                .not(),
        )
        .add(p_col("w", workspaces::Column::Slug).eq(slug))
        .add(p_col("rpm", project_members::Column::MemberId).eq(viewer))
        .add(p_col("rpm", project_members::Column::IsActive).eq(true))
        .add(p_col("p", projects::Column::ArchivedAt).is_null())
        .add(p_col("a", issue_activities::Column::ActorId).eq(user_id))
        .add(p_col("a", issue_activities::Column::DeletedAt).is_null());
    if let Some(ids) = projects {
        cond = cond.add(p_col("p", projects::Column::Id).is_in(ids));
    }
    stmt.cond_where(cond);
    stmt
}

/// Me-activities base (`user/base.py:392-404`): scoped ONLY by
/// `actor = request.user` — no slug, no project scope, no field exclusion.
fn me_activity_base(viewer: uuid::Uuid) -> SelectStatement {
    let mut stmt = SeaQuery::select();
    stmt.from_as(issue_activities::Entity, p_a("a"));
    activity_joins(&mut stmt, false);
    stmt.and_where(p_col("a", issue_activities::Column::ActorId).eq(viewer))
        .and_where(p_col("a", issue_activities::Column::DeletedAt).is_null());
    stmt
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
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows: Vec<(uuid::Uuid, Option<String>)> = file_assets::Entity::find()
        .select_only()
        .columns([file_assets::Column::Id, file_assets::Column::EntityType])
        .filter(file_assets::Column::Id.is_in(asset_ids.iter().copied()))
        .into_tuple()
        .all(&db)
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
    base: SelectStatement,
    key_expr: SimpleExpr,
    descending: bool,
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
    let mut inner = base.clone();
    activity_annotate(&mut inner);
    inner
        .order_by_expr_with_nulls(
            key_expr.clone(),
            if descending { Order::Desc } else { Order::Asc },
            NullOrdering::Last,
        )
        .order_by((p_a("a"), issue_activities::Column::CreatedAt), Order::Desc)
        .limit(p_non_negative(window.stop - window.offset)?)
        .offset(p_non_negative(window.offset)?);
    let rows = p_fetch_rows(pool, &inner).await?;
    let has_more = rows.len() as i64 > limit;
    let page: Vec<Map<String, Value>> = apply_offset_window(&rows, limit)
        .map_err(page_denial)?
        .into_iter()
        .collect();
    let total_count = {
        let mut stmt = base.clone();
        stmt.expr_as(Func::count(Expr::col(Asterisk)), p_a("n"));
        p_fetch_n(pool, &stmt).await?
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
    // The `<uuid:user_id>` converter runs at URL resolution, before auth
    // and before the class gate (a non-member with a garbage id 404s,
    // never 403s).
    let user_id = parse_uuid_path(&user_raw)?;
    let pool = pool_of(&state)?;
    let viewer = actor_user_id(extension)?;
    let gate = gates::gate_for("GET", "workspaces/<slug>/user-activity/<user_id>/")
        .expect("user-activity gate")
        .gate;
    if let Some(denied) = check_class_gate(&pool, &gate, &slug, &viewer).await? {
        return Ok(denied);
    }
    let (params, order_by) = activity_params(&query)?;
    let cursor = crate::paginator::Cursor::from_string(&params.cursor_raw).map_err(page_denial)?;
    // `?project=` getlist (`:375,386-387`): raw strings, UUID-validated
    // at lookup time (non-UUIDs 400).
    let projects_raw: Vec<String> = query_values(&query, "project").unwrap_or_default();
    let projects = if projects_raw.is_empty() {
        None
    } else {
        let refs: Vec<&str> = projects_raw.iter().map(String::as_str).collect();
        Some(
            qp::validate_project_uuids(&refs)
                .map_err(|_| Denial::BadError(INVALID_DETAIL_MSG.to_owned()))?,
        )
    };
    let base = user_activity_base(user_id, &slug, viewer, projects);
    let (key_expr, descending) = activity_order_expr(&order_by)?;
    let timezone = actor_timezone(&pool, &viewer).await?;
    activity_flat_response(
        &pool,
        base,
        key_expr,
        descending,
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
    let timezone = actor_timezone(&pool, &viewer).await?;
    let base = me_activity_base(viewer);
    activity_flat_response(
        &pool,
        base,
        key_expr,
        descending,
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
    fn uuid_spelling_follows_the_converter() {
        // PIDASHCONV-1713: only the canonical lowercase hyphenated spelling
        // routes; uppercase/simple/braced/urn/garbage 404 at resolution.
        let canonical = "12345678-1234-1234-1234-1234567890ab";
        assert_eq!(
            parse_uuid_path(canonical).unwrap(),
            uuid::Uuid::parse_str(canonical).unwrap()
        );
        for raw in [
            "12345678-1234-1234-1234-1234567890AB",
            "123456781234123412341234567890ab",
            "{12345678-1234-1234-1234-1234567890ab}",
            "urn:uuid:12345678-1234-1234-1234-1234567890ab",
            "xyz",
            "",
        ] {
            assert!(
                matches!(parse_uuid_path(raw), Err(Denial::PageNotFound)),
                "{raw}"
            );
        }
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
        let mut stmt = SeaQuery::select();
        stmt.from_as(issues::Entity, p_a("issue"));
        profile_annotate(&mut stmt, &[]);
        let text = stmt.to_string(PostgresQueryBuilder);
        // Four scalar subqueries correlated to `issue.id` (cycle + 3
        // counts), plain `COUNT` (0 when empty), never NULLIF.
        assert!(text.matches(r#""issue"."id""#).count() >= 4, "{text}");
        assert!(!text.contains("NULLIF"), "{text}");
    }

    #[test]
    fn row_selects_cover_every_on_results_key() {
        let mut stmt = SeaQuery::select();
        stmt.from_as(issues::Entity, p_a("issue"));
        profile_annotate(&mut stmt, &[]);
        let selects = stmt.to_string(PostgresQueryBuilder);
        for key in on_results_fields(None, None) {
            let found = match key.as_str() {
                "estimate_point" => selects.contains("AS \"estimate_point\""),
                "created_by" => selects.contains("AS \"created_by\""),
                "updated_by" => selects.contains("AS \"updated_by\""),
                "state__group" => selects.contains("AS \"state__group\""),
                "cycle_id" | "link_count" | "attachment_count" | "sub_issues_count" => {
                    selects.contains(&format!("AS \"{key}\""))
                }
                "label_ids" | "assignee_ids" | "module_ids" => {
                    selects.contains(&format!("AS \"{key}\""))
                }
                _ => selects.contains(&format!("\"issue\".\"{key}\"")),
            };
            assert!(found, "missing select for {key}:\n{selects}");
        }
        // One array skipped per m2m group level; raw keys projected
        // for both (the grouper buckets each joined row by its raw key).
        let mut stmt = SeaQuery::select();
        stmt.from_as(issues::Entity, p_a("issue"));
        profile_annotate(
            &mut stmt,
            &skip_arrays_for(Some("assignees__id"), Some("labels__id")),
        );
        p_group_members(&mut stmt, Some("assignees__id"), Some("labels__id"));
        let selects = stmt.to_string(PostgresQueryBuilder);
        assert!(!selects.contains("AS \"assignee_ids\""));
        assert!(!selects.contains("AS \"label_ids\""));
        assert!(selects.contains("AS \"module_ids\""));
        assert!(selects.contains("AS \"assignees__id\""), "{selects}");
        assert!(selects.contains("AS \"labels__id\""), "{selects}");
    }

    #[test]
    fn module_array_has_no_deleted_guard() {
        // `grouper.py:63-72` filters only `module__archived_at` (the
        // pilot's extra `m.deleted_at` guard is its own divergence).
        let mut stmt = SeaQuery::select();
        stmt.from_as(issues::Entity, p_a("issue"));
        profile_annotate(&mut stmt, &[]);
        let selects = stmt.to_string(PostgresQueryBuilder);
        assert!(
            selects.contains(r#""m"."archived_at" IS NULL"#),
            "{selects}"
        );
        assert!(!selects.contains(r#""m"."deleted_at""#), "{selects}");
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
            ["issue_assignee"]
        );
        // Group == sub is rejected upstream, but dedupe anyway.
        assert_eq!(
            group_deleted_guards(Some("labels__id"), Some("labels__id")),
            ["label_issue"]
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
    fn legacy_expr_i_maps_issue_and_state_keep_relations() {
        let render = |expr: SimpleExpr| {
            let mut stmt = SeaQuery::select();
            stmt.expr(Expr::val(1)).and_where(expr);
            stmt.to_string(PostgresQueryBuilder)
        };
        // `issue.` → `i.`, `state.` → `s.`, `issue_assignee.` → `ia.`;
        // relation aliases survive verbatim.
        let label = uuid::Uuid::nil();
        let text =
            render(s_legacy_expr_i("labels__in", &FilterValue::Uuids(vec![label])).expect("c"));
        assert!(text.contains(r#""label_issue"."label_id""#), "{text}");
        let text =
            render(s_legacy_expr_i("assignees__in", &FilterValue::Uuids(vec![label])).expect("c"));
        assert!(text.contains(r#""ia"."assignee_id""#), "{text}");
        let text = render(
            s_legacy_expr_i(
                "priority__in",
                &FilterValue::Strings(vec!["high".to_owned()]),
            )
            .expect("c"),
        );
        assert!(text.contains(r#""i"."priority""#), "{text}");
        // Every other relation alias survives verbatim.
        let text = render(
            s_legacy_expr_i(
                "issue_cycle__cycle_id__in",
                &FilterValue::Uuids(vec![label]),
            )
            .expect("c"),
        );
        assert!(text.contains(r#""issue_cycle"."cycle_id""#), "{text}");
    }

    #[test]
    fn stats_joins_reuse_or_add_assignee_leg() {
        // Q1/Q2/Q4-Q6 reuse the preamble `ia` join; Q3 adds it.
        let uid = uuid::Uuid::nil();
        let viewer = uuid::Uuid::max();
        let label = uuid::Uuid::nil();
        let assignee_cond = Condition::all()
            .add(s_legacy_expr_i("assignees__in", &FilterValue::Uuids(vec![label])).expect("c"));
        let mut with_ia = stats_issue_base(
            uid,
            "acme",
            viewer,
            None,
            &Some(assignee_cond.clone()),
            true,
        );
        let text = with_ia.to_string(PostgresQueryBuilder);
        assert_eq!(
            text.matches(r#""issue_assignees" AS "ia""#).count(),
            1,
            "{text}"
        );
        // Q3 adds the `ia` leg for an assignee legacy filter.
        let mut q3 = stats_issue_base(
            uid,
            "acme",
            viewer,
            None,
            &Some(assignee_cond.clone()),
            false,
        );
        let text = q3.to_string(PostgresQueryBuilder);
        assert!(text.contains(r#""issue_assignees" AS "ia""#), "{text}");
        // Value predicates INNER, `__isnull`-only LEFT.
        let label_cond = Condition::all()
            .add(s_legacy_expr_i("labels__in", &FilterValue::Uuids(vec![label])).expect("c"));
        let mut stmt = SeaQuery::select();
        stmt.from_as(issues::Entity, p_a("i"));
        s_stats_relation_joins(&mut stmt, &Some(label_cond), true);
        let text = stmt.to_string(PostgresQueryBuilder);
        assert!(
            text.contains(r#"INNER JOIN "issue_labels" AS "label_issue""#),
            "{text}"
        );
        let null_cond = Condition::all()
            .add(s_legacy_expr_i("labels__isnull", &FilterValue::Flag(true)).expect("c"));
        let mut stmt = SeaQuery::select();
        stmt.from_as(issues::Entity, p_a("i"));
        s_stats_relation_joins(&mut stmt, &Some(null_cond), true);
        let text = stmt.to_string(PostgresQueryBuilder);
        assert!(
            text.contains(r#"LEFT JOIN "issue_labels" AS "label_issue""#),
            "{text}"
        );
        let mut stmt = SeaQuery::select();
        stmt.from_as(issues::Entity, p_a("i"));
        let prio = Condition::all().add(
            s_legacy_expr_i(
                "priority__in",
                &FilterValue::Strings(vec!["high".to_owned()]),
            )
            .expect("c"),
        );
        s_stats_relation_joins(&mut stmt, &Some(prio), true);
        let text = stmt.to_string(PostgresQueryBuilder);
        assert!(!text.contains("issue_labels"), "{text}");
        let _ = &mut with_ia;
        let _ = &mut q3;
    }

    #[test]
    fn subscriber_scope_resolves_three_lookups_only() {
        let id = uuid::Uuid::nil();
        let render = |expr: SimpleExpr| {
            let mut stmt = SeaQuery::select();
            stmt.expr(Expr::val(1)).and_where(expr);
            stmt.to_string(PostgresQueryBuilder)
        };
        let text = render(
            subscriber_legacy_expr("created_by__in", &FilterValue::Uuids(vec![id])).expect("c"),
        );
        assert!(text.contains(r#""sub"."created_by_id""#), "{text}");
        let text = render(
            subscriber_legacy_expr("project__in", &FilterValue::Uuids(vec![id])).expect("p"),
        );
        assert!(text.contains(r#""sub"."project_id""#), "{text}");
        let day = chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let text = render(
            subscriber_legacy_expr("created_at__date__gte", &FilterValue::Day(day)).expect("d"),
        );
        assert!(text.contains("CAST"), "{text}");
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
                    subscriber_legacy_expr(name, &value),
                    Err(Denial::ServerError)
                ),
                "name {name} should 500"
            );
        }
        // Garbage date bounds are `ValidationError` → 400.
        assert!(matches!(
            subscriber_legacy_expr(
                "created_at__date__gte",
                &FilterValue::Text("not-a-date".to_owned()),
            ),
            Err(Denial::BadError(_))
        ));
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
        let filtered = profile_filtered_set(
            &uid,
            "acme",
            &viewer,
            &query,
            Some("priority"),
            None,
            &chrono_tz::UTC,
        )
        .expect("filtered set");
        let text = filtered.select().to_string(PostgresQueryBuilder);
        assert_parens_balanced(&text);
        // The labels predicate forces its INNER leg over the outer alias.
        assert!(
            text.contains(r#"INNER JOIN "issue_labels" AS "label_issue""#),
            "{text}"
        );
        // The R2 scope rides as correlated EXISTS over the inner alias
        // (`i2.id = issue.id`), the builder form of the legacy
        // `id IN (SELECT ...)` id-set: same rows (PK equality, no NULL
        // divergence), one statement, no duplicate-row fanout.
        assert!(
            text.contains(r#"EXISTS(SELECT 1 FROM "issues" AS "i2""#),
            "{text}"
        );
        assert!(text.contains(r#""i2"."id" = "issue"."id""#), "{text}");
    }

    #[test]
    fn group_totals_statements_carry_group_by() {
        use crate::app_issues::OneOrMany;
        let uid = uuid::Uuid::nil();
        let viewer = uuid::Uuid::max();
        let mut query = QueryMap::new();
        query.insert("priority".to_owned(), OneOrMany::One("high".to_owned()));
        let filtered = profile_filtered_set(
            &uid,
            "acme",
            &viewer,
            &query,
            Some("priority"),
            Some("state__group"),
            &chrono_tz::UTC,
        )
        .expect("filtered set");
        let group_sql = profile_group_totals(&filtered, "priority")
            .expect("group totals")
            .to_string(PostgresQueryBuilder);
        let sub_sql = profile_sub_totals(&filtered, "priority", "state__group")
            .expect("sub totals")
            .to_string(PostgresQueryBuilder);
        // The bucket select(s) beside the COUNT aggregate require the
        // GROUP BY bucket alias, or Postgres rejects the statement (42803).
        assert!(group_sql.contains(r#"GROUP BY "bucket""#), "{group_sql}");
        assert!(sub_sql.contains(r#"GROUP BY "bucket", "sub""#), "{sub_sql}");
        for statement in [&group_sql, &sub_sql] {
            assert_eq!(
                statement.matches("COUNT(DISTINCT").count(),
                1,
                "{statement}"
            );
            assert_parens_balanced(statement);
        }
        // Exactly the bucket item(s) precede the aggregate.
        let head = group_sql.split("COUNT(DISTINCT").next().expect("head");
        assert_eq!(head.matches(" AS \"bucket\"").count(), 1, "{head}");
        let sub_head = sub_sql.split("COUNT(DISTINCT").next().expect("sub head");
        assert_eq!(sub_head.matches(" AS \"bucket\"").count(), 1, "{sub_head}");
        assert_eq!(sub_head.matches(" AS \"sub\"").count(), 1, "{sub_head}");
    }

    #[test]
    fn cycle_group_expression_matches_annotated_twin() {
        let expr = group_expression("cycle_id").expect("cycle expr");
        // Same pick as the QRY-C `cycle_id` annotation (`CycleIssue.Meta`
        // ordering): without the ORDER BY the LIMIT 1 is nondeterministic
        // over several live links.
        let mut stmt = SeaQuery::select();
        stmt.expr(Expr::val(1)).and_where(expr);
        let text = stmt.to_string(PostgresQueryBuilder);
        assert!(text.contains(r#""ci"."created_at" DESC"#), "{text}");
        assert!(text.contains("LIMIT 1"), "{text}");
    }

    #[test]
    fn stats_statements_assemble_valid_sql() {
        let uid = uuid::Uuid::nil();
        let viewer = uuid::Uuid::max();
        let label = uuid::Uuid::nil();
        let legacy = Condition::all()
            .add(s_legacy_expr_i("labels__in", &FilterValue::Uuids(vec![label])).expect("c"));
        for build in [
            stats_state_select(uid, "acme", viewer, &Some(legacy.clone())),
            stats_priority_select(uid, "acme", viewer, &Some(legacy.clone())),
            stats_created_select(uid, "acme", viewer, &Some(legacy.clone())),
            stats_assigned_select(uid, "acme", viewer, &Some(legacy.clone())),
            stats_pending_select(uid, "acme", viewer, &Some(legacy.clone())),
            stats_completed_select(uid, "acme", viewer, &Some(legacy.clone())),
        ] {
            let text = build.to_string(PostgresQueryBuilder);
            assert_parens_balanced(&text);
            assert!(text.contains(r#""label_issue"."label_id""#), "{text}");
            assert!(
                text.contains(r#"INNER JOIN "issue_labels" AS "label_issue""#),
                "{text}"
            );
        }
    }

    #[test]
    fn activity_statements_carry_matched_w_alias() {
        // `activity_annotate` selects the workspace lite columns as
        // `"w"."..."`; the joins must alias workspaces AS "w" (the old
        // `JOIN workspaces w`). An unaliased join leaves every select
        // dangling: Postgres rejects with `missing FROM-clause entry
        // for table "w"` on both activity endpoints.
        let uid = uuid::Uuid::nil();
        let viewer = uuid::Uuid::max();
        let mut stmt = user_activity_base(uid, "acme", viewer, Some(vec![uuid::Uuid::nil()]));
        activity_annotate(&mut stmt);
        let text = stmt.to_string(PostgresQueryBuilder);
        assert_parens_balanced(&text);
        assert!(text.contains(r#"INNER JOIN "workspaces" AS "w""#), "{text}");
        assert!(!text.contains(r#"INNER JOIN "workspaces" ON"#), "{text}");
        for column in ["name", "slug", "id", "logo", "logo_asset_id"] {
            assert!(
                text.contains(&format!(r#""w"."{column}" AS "workspace_"#)),
                "{text}"
            );
        }
        assert!(text.contains(r#""w"."slug""#), "{text}");
        // The me-activities base shares the join inventory minus the
        // slug predicate and the membership leg.
        let mut me = me_activity_base(viewer);
        activity_annotate(&mut me);
        let me_text = me.to_string(PostgresQueryBuilder);
        assert!(
            me_text.contains(r#"INNER JOIN "workspaces" AS "w""#),
            "{me_text}"
        );
        assert!(!me_text.contains(r#""rpm""#), "{me_text}");
    }

    #[test]
    fn activity_order_maps_single_level_fields() {
        let render = |expr: SimpleExpr| {
            let mut stmt = SeaQuery::select();
            stmt.expr(Expr::val(1)).and_where(expr);
            stmt.to_string(PostgresQueryBuilder)
        };
        let (expr, desc) = activity_order_expr("-created_at").unwrap();
        assert!(render(expr).contains(r#""a"."created_at""#));
        assert!(desc);
        let (expr, desc) = activity_order_expr("actor").unwrap();
        assert!(render(expr).contains(r#""a"."actor_id""#));
        assert!(!desc);
        let (expr, desc) = activity_order_expr("issue_comment_id").unwrap();
        assert!(render(expr).contains(r#""a"."issue_comment_id""#));
        assert!(!desc);
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

    /// The `project_data` aggregates against constraint-free temp tables:
    /// one created issue (no assignee) and one assigned issue in a
    /// `started` state (pending), so `created=1` / `assigned=1` /
    /// `pending=1` / `completed=0` — the `COUNT(CASE)` fanout decodes by
    /// name and the completed FILTER stays an exclusion, not a join.
    #[tokio::test]
    async fn live_profile_projects_aggregates() {
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
            "workspaces",
            "projects",
            "project_members",
            "issues",
            "issue_assignees",
            "states",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let (ws, project, user, viewer, started_state) = (
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        );
        for sql in [
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
            format!(
                "INSERT INTO projects (id, workspace_id, logo_props, archived_at, deleted_at) \
                 VALUES ('{project}', '{ws}', '{{}}'::jsonb, NULL, NULL)"
            ),
            format!(
                "INSERT INTO project_members (id, workspace_id, project_id, member_id, is_active, deleted_at) \
                 VALUES (gen_random_uuid(), '{ws}', '{project}', '{viewer}', true, NULL)"
            ),
            format!(
                "INSERT INTO states (id, workspace_id, project_id, \"group\", deleted_at) \
                 VALUES ('{started_state}', '{ws}', '{project}', 'started', NULL)"
            ),
            format!(
                "INSERT INTO issues (id, workspace_id, project_id, state_id, created_by_id, archived_at, is_draft, completed_at, deleted_at) \
                 VALUES (gen_random_uuid(), '{ws}', '{project}', '{started_state}', '{user}', NULL, false, NULL, NULL)"
            ),
            format!(
                "INSERT INTO issues (id, workspace_id, project_id, state_id, created_by_id, archived_at, is_draft, completed_at, deleted_at) \
                 VALUES (gen_random_uuid(), '{ws}', '{project}', '{started_state}', '{viewer}', NULL, false, NULL, NULL)"
            ),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }
        let assigned: uuid::Uuid =
            sqlx::query_scalar("SELECT id FROM issues WHERE created_by_id = $1")
                .bind(viewer)
                .fetch_one(&pool)
                .await
                .expect("assigned id");
        sqlx::query(
            "INSERT INTO issue_assignees (id, issue_id, assignee_id, deleted_at) \
             VALUES (gen_random_uuid(), $1, $2, NULL)",
        )
        .bind(assigned)
        .bind(user)
        .execute(&pool)
        .await
        .expect("assignee seed");
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let stmt = profile_projects_select(user, "acme", viewer);
        let rows = p_query_built::<ProfileProjectRow>(&db, &stmt)
            .await
            .expect("projects select");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.id, project);
        assert_eq!(row.created_issues, 1);
        assert_eq!(row.assigned_issues, 1);
        assert_eq!(row.completed_issues, 0);
        assert_eq!(row.pending_issues, 1);
    }
}
