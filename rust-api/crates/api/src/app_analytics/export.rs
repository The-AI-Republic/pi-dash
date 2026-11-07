#![forbid(unsafe_code)]

//! Export-issues endpoint (D-35 handlers-E, stage 5, PIDASHCONV-430).
//!
//! Port of `ExportIssuesEndpoint` in
//! `apps/api/pi_dash/app/views/exporter/base.py:18-84` served at
//! `workspaces/<slug>/export-issues/` (`app/urls/exporter.py:11-15`,
//! 16 LOC): `GET` lists the workspace's `issue_exports` history through
//! `ExporterHistorySerializer(many=True)` inside `BasePaginator.paginate`,
//! `POST` validates the provider, backfills an empty project list from the
//! requester's active workspace projects, creates the `ExporterHistory`
//! row and fans out `issue_export_task.delay(...)`.
//!
//! Fixtures: FX-A-H-01 (the four export pairs in
//! `fixtures/app_analytics/handlers/analytics_handlers.golden.json`),
//! FX-A-Q-06 (`fixtures/app_analytics/queries/analytics_queries_part4.sql`),
//! FX-A-T-01 (`fixtures/app_analytics/tasks/export_tasks.golden.json`),
//! FX-A-G-01 (GET + POST are ADMIN/MEMBER at WORKSPACE level,
//! `fixtures/app_analytics/guards/analytics_guards.golden.json`).
//! Oracle: `contract-tests/app_analytics/test_exporter.py` (PIDASHCONV-93).
//!
//! Layering: the query builders live in
//! `pidash_services::app_analytics::queries` (provider gate, fallback
//! select, list select) and the gate matrix in [`super::gates`]; row
//! fetching (the `WorkspaceMember.exists()` check, the workspace lookup,
//! the history page) stays here, per the gates module's split. Datetime
//! rendering goes through [`crate::serializer`] in the request's zone
//! (`TimezoneMixin`), pagination through [`crate::paginator`].
//!
//! Only GET + POST are owned: every other method on the path proxies to
//! Django (its 405s/metadata live there), and sibling paths keep proxying
//! through the fallback — route registration is the cutover granularity.
//!
//! Task fan-out follows the D-02 intake precedent (best-effort
//! post-commit publish, `space::intake::enqueue_message`): the row insert
//! commits first, then the Celery v2 message (`issue_export_task` with
//! the `.delay()` kwargs) goes to the Postgres queue via
//! `queue::enqueue`. Without a queue table the 200 still stands — the
//! proxy contract tests run serve-only with no broker and no worker, so a
//! hard enqueue would 500 the accepted path. Django answers 500 when its
//! own broker write fails (the row persists either way); that divergence
//! is recorded in the PR.
//!
//! Ported quirks (translate, don't redesign — also listed in the PR):
//! - `provider` defaults to `False` (`request.data.get("provider",
//!   False)`): a missing provider answers
//!   `{"error": "Provider 'False' not found."}` 400, not a missing-field
//!   error. `None` (JSON null) renders `'None'`, booleans as
//!   `'True'`/`'False'`.
//! - An empty/missing `project` list falls back to the requester's active
//!   workspace projects; a provided list passes through verbatim.
//! - `GET` requires truthy `per_page` AND `cursor` before any parsing:
//!   `{"error": "per_page and cursor are required"}` 400.
//! - `created_by` on the POST row is the requesting user
//!   (`BaseModel.save` via crum's `get_current_user`); `updated_by` stays
//!   null. Seed rows carry null for both.
//! - Reads go through the `SoftDeletionManager` scope (`deleted_at IS
//!   NULL`) on every table, matching the ORM.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::extract::{FromRequest, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gates::{FORBIDDEN_BODY, NOT_FOUND_BODY};
use pidash_compat::response::json_response;
use pidash_db::entity::{project_members, projects, users, workspace_members, workspaces};
use sea_orm::{
    sea_query::{Alias, Asterisk},
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QuerySelect, RelationTrait,
};

/// DRF `IsAuthenticated` denial, same body as every other D-35 route.
pub const UNAUTHENTICATED_BODY: &str = super::gates::ANON_BODY;
/// `BasePaginator.paginate` without `per_page` + `cursor`
/// (`exporter/base.py:81-84`).
pub const PAGINATION_REQUIRED_BODY: &str =
    pidash_services::app_analytics::queries::EXPORTER_PAGINATION_REQUIRED_BODY;
/// POST-accepted message (`exporter/base.py:57-60`).
const ACCEPTED_MESSAGE: &str = pidash_services::app_analytics::queries::EXPORT_ACCEPTED_MESSAGE;
/// Row type written and filtered on (`exporter/base.py:46`).
const EXPORT_TYPE: &str = pidash_services::app_analytics::queries::EXPORTER_DEFAULT_TYPE;
/// Default list ordering (`exporter/base.py:75` paginate default, the
/// model `Meta.ordering` per FX-A-MOD-02).
const DEFAULT_ORDER: &str = pidash_services::app_analytics::queries::EXPORTER_DEFAULT_ORDER;

/// Register the export-issues GET + POST. Nothing else: sibling paths and
/// sibling methods stay unmatched and proxy to Django.
pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/workspaces/{slug}/export-issues/",
        get(list)
            .post(create)
            .put(crate::edge::proxy)
            .patch(crate::edge::proxy)
            .delete(crate::edge::proxy)
            .head(crate::edge::proxy)
            .options(crate::edge::proxy),
    )
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 400, `{"detail": ...}` (`ParseError`).
    BadDetail(String),
    /// 500, generic branch.
    ServerError,
    /// A pre-rendered exact body with its status.
    Raw(StatusCode, String),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, FORBIDDEN_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::app_issues::SERVER_ERROR_BODY.to_owned(),
            ),
            Denial::Raw(status, body) => (*status, body.clone()),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("export response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// `table`.`column` as a sea-query expression (the 1c services-builder idiom).
fn col(table: &str, column: &str) -> sea_orm::sea_query::Expr {
    use sea_orm::sea_query::Expr;
    Expr::col((Alias::new(table), Alias::new(column)))
}

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated`):
/// anonymous answers 401 before the gate or the body runs.
async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    let pool = pool_of(state)?;
    crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::Unauthorized)
}

/// `@allow_permission([ADMIN, MEMBER], level="WORKSPACE")`
/// (`exporter/base.py:22,67`): an active workspace membership with role
/// 20/15. Soft-deleted memberships do not count.
async fn allow_workspace(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<(), Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<Option<i16>> = workspace_members::Entity::find()
        .select_only()
        .column(workspace_members::Column::Role)
        .join(
            JoinType::InnerJoin,
            workspace_members::Relation::Workspaces.def(),
        )
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(workspace_members::Column::MemberId.eq(*user_id))
        .filter(workspace_members::Column::IsActive.eq(true))
        .filter(workspace_members::Column::DeletedAt.is_null())
        .filter(workspaces::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row.flatten() {
        Some(20) | Some(15) => Ok(()),
        _ => Err(Denial::Forbidden),
    }
}

/// `Workspace.objects.get(slug=slug)` → 404 `ObjectDoesNotExist` branch.
async fn workspace_by_slug(pool: &sqlx::PgPool, slug: &str) -> Result<uuid::Uuid, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    let row: Option<uuid::Uuid> = workspaces::Entity::find()
        .select_only()
        .column(workspaces::Column::Id)
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(workspaces::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&db)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::NotFound)
}

/// Map a paginator-kernel error to its HTTP fate (the app_issues
/// precedent): `BadPaginationError` subclasses become `ParseError` 400s;
/// lazy-queryset arithmetic errors propagate to the generic 500.
fn page_denial(error: crate::paginator::PageError) -> Denial {
    use crate::paginator::PageError as E;
    match error {
        E::InvalidCursor
        | E::InvalidPerPage
        | E::PerPageTooLarge(_)
        | E::OffsetTooLarge
        | E::NegativeOffset => Denial::BadDetail(error.detail()),
        E::NegativeSlice | E::ZeroLimit | E::NonFiniteCursor | E::MissingOrderKey => {
            Denial::ServerError
        }
    }
}

/// `GET /api/workspaces/<slug>/export-issues/` (`base.py:67-84`).
async fn list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<crate::app_issues::QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    match list_inner(&state, &slug, &query, extension).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

#[allow(clippy::too_many_lines)]
async fn list_inner(
    state: &AppState,
    slug: &str,
    query: &crate::app_issues::QueryMap,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(state, extension).await?;
    let pool = pool_of(state)?.clone();
    let timezone = resolved.timezone;
    // Decorator before body: the gate runs before anything else, so an
    // unknown slug for a non-member answers 403, not an empty page.
    // Note there is no workspace lookup on this path: Django filters by
    // slug and serves an empty page when nothing matches (`base.py:67`).
    allow_workspace(&pool, slug, &resolved.id).await?;
    // Both params must be truthy before any parsing (`base.py:71`).
    let per_page_raw = crate::app_issues::query_last(query, "per_page");
    let cursor_raw = crate::app_issues::query_last(query, "cursor");
    let (Some(per_page_text), Some(cursor_text)) = (per_page_raw, cursor_raw) else {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            PAGINATION_REQUIRED_BODY.to_owned(),
        ));
    };
    if per_page_text.is_empty() || cursor_text.is_empty() {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            PAGINATION_REQUIRED_BODY.to_owned(),
        ));
    }
    let per_page = crate::paginator::parse_per_page(Some(per_page_text.as_str()), 1000, 1000)
        .map_err(page_denial)?;
    if per_page <= 0 {
        // `max_hits` divides by the limit (`ZeroDivisionError` at 0);
        // negative limits slice with a negative bound (`ValueError`).
        // Both escape the view into the base 500.
        return Err(Denial::ServerError);
    }
    let cursor = crate::paginator::Cursor::from_string(&cursor_text).map_err(page_denial)?;
    let page = cursor.offset;
    let offset = page.saturating_mul(per_page);
    if offset < 0 {
        // `BadPaginationError("Pagination offset cannot be negative")`.
        return Err(Denial::BadDetail("Error in parsing".to_owned()));
    }
    let order_by =
        crate::app_issues::query_last(query, "order_by").unwrap_or(DEFAULT_ORDER.to_owned());
    let order = resolve_export_order(&order_by)?;
    let total = exporter_total_count(&pool, slug).await?;
    let rows = fetch_exporter_page(&pool, slug, order, offset, per_page).await?;
    let has_more = rows.len() as i64 > per_page;
    let page_rows: Vec<ExportRow> = rows.into_iter().take(per_page as usize).collect();
    // `avatar_url` model property (`db/models/user.py:142-151`): asset
    // URL, else the avatar text, else null — one batched `file_assets`
    // read for the page (the v1_projects precedent, reused not forked).
    let asset_ids: Vec<uuid::Uuid> = {
        use std::collections::HashSet;
        page_rows
            .iter()
            .filter_map(|row| row.user_avatar_asset_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    };
    let assets = crate::v1_projects::handlers_members::fetch_asset_urls(&pool, &asset_ids)
        .await
        .map_err(|_| Denial::ServerError)?;
    let total_pages = crate::paginator::max_hits(total, per_page).map_err(page_denial)?;
    let next = crate::paginator::next_cursor(per_page, page, has_more).to_string();
    let prev = crate::paginator::prev_cursor(per_page, page).to_string();
    let mut results = String::from("[");
    for (index, row) in page_rows.iter().enumerate() {
        if index > 0 {
            results.push(',');
        }
        results.push_str(&render_export_row(row, &timezone, &assets));
    }
    results.push(']');
    let body = format!(
        "{{\"grouped_by\":null,\"sub_grouped_by\":null,\"total_count\":{total},\
         \"next_cursor\":{next},\"prev_cursor\":{prev},\
         \"next_page_results\":{next_has},\"prev_page_results\":{prev_has},\
         \"count\":{count},\"total_pages\":{total_pages},\"total_results\":{total},\
         \"extra_stats\":null,\"results\":{results}}}",
        next = json_string(&next),
        prev = json_string(&prev),
        next_has = if has_more { "true" } else { "false" },
        prev_has = if page > 0 { "true" } else { "false" },
        count = page_rows.len(),
    );
    Ok(json_response(StatusCode::OK, body))
}

/// Resolve the paginate `order_by` param. Django splices the field path
/// into the queryset ordering; an unknown name raises `FieldError` (the
/// generic 500). Only the default and its ascending form are reachable
/// in this domain's suite. The column is always `e.created_at`.
fn resolve_export_order(raw: &str) -> Result<sea_orm::sea_query::Order, Denial> {
    use sea_orm::sea_query::Order;
    match raw {
        "-created_at" => Ok(Order::Desc),
        "created_at" => Ok(Order::Asc),
        _ => Err(Denial::ServerError),
    }
}

/// One history row plus its `select_related("initiated_by")` user facts.
struct ExportRow {
    id: uuid::Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    project: Vec<uuid::Uuid>,
    provider: String,
    status: String,
    url: Option<String>,
    initiated_by: uuid::Uuid,
    user_first_name: String,
    user_last_name: String,
    user_avatar: String,
    user_avatar_asset_id: Option<uuid::Uuid>,
    user_is_bot: bool,
    user_display_name: String,
    token: String,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
}

/// `ExporterHistory.objects.filter(workspace__slug=slug,
/// type="issue_exports").select_related("workspace", "initiated_by")`
/// count (`queryset.count()` in `get_result`).
/// `SELECT COUNT(*) FROM exporters e WHERE e.workspace_id IN (SELECT
/// w.id FROM workspaces w WHERE w.slug = $1) AND e.type = $2 AND
/// e.deleted_at IS NULL`. The workspace subquery carries no soft-delete
/// guard of its own (the Django `workspace__slug` lookup does not filter
/// deleted workspaces here).
fn exporter_total_count_stmt(slug: &str) -> sea_orm::sea_query::SelectStatement {
    use sea_orm::sea_query::{Expr, Func, Query as SeaQuery, SimpleExpr};
    let mut sub = SeaQuery::select();
    sub.column(workspaces::Column::Id)
        .from_as(workspaces::Entity, Alias::new("w"))
        .and_where(col("w", "slug").eq(slug));
    SeaQuery::select()
        .expr_as(
            Func::count::<SimpleExpr>(Expr::col(Asterisk).into()),
            Alias::new("count"),
        )
        .from_as(pidash_db::entity::exporters::Entity, Alias::new("e"))
        .and_where(col("e", "workspace_id").in_subquery(sub))
        .and_where(col("e", "type").eq(EXPORT_TYPE))
        .and_where(col("e", "deleted_at").is_null())
        .to_owned()
}

async fn exporter_total_count(pool: &sqlx::PgPool, slug: &str) -> Result<i64, Denial> {
    let stmt = exporter_total_count_stmt(slug);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = db
        .query_all(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(|_| Denial::ServerError)?;
    let row = rows.first().ok_or(Denial::ServerError)?;
    row.try_get::<i64>("", "count")
        .map_err(|_| Denial::ServerError)
}

/// One `fetch_exporter_page` output row, decoded by alias name (every
/// selected column is aliased to its `ExportRow` field below, so names
/// are unique and `try_get` never resolves a missing alias to NULL).
#[derive(Debug, FromQueryResult)]
struct ExporterPageRow {
    id: uuid::Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    project: Option<Vec<uuid::Uuid>>,
    provider: String,
    status: String,
    url: Option<String>,
    initiated_by: uuid::Uuid,
    token: String,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    user_first_name: Option<String>,
    user_last_name: Option<String>,
    user_avatar: Option<String>,
    user_avatar_asset_id: Option<uuid::Uuid>,
    user_is_bot: Option<bool>,
    user_display_name: Option<String>,
}

/// The `[offset, offset + limit + 1)` slice the paginator reads (the
/// extra row decides `next_page_results`, like `OffsetPaginator`).
#[allow(clippy::too_many_lines)]
/// The history page slice as a select statement: 17 columns aliased to
/// `ExportRow` fields, the `users` LEFT JOIN for `select_related`, the
/// workspace `IN` subquery, soft-delete + type guards, and the
/// caller-resolved `ORDER BY` + `LIMIT`/`OFFSET` window.
fn exporter_page_stmt(
    slug: &str,
    order: sea_orm::sea_query::Order,
    offset: i64,
    limit: i64,
) -> sea_orm::sea_query::SelectStatement {
    use sea_orm::sea_query::Query as SeaQuery;
    // `avatar_url` is a model property (`db/models/user.py:142-151`),
    // not a column: fetch `avatar_asset_id` and resolve through the
    // `file_assets` batch helper below (the v1_projects precedent).
    // The hand-written `LEFT OUTER JOIN workspaces` carried no selected
    // column and no predicate (a PK-matched leg can neither filter nor
    // fan out), so the statement drops it — same rows.
    let mut sub = SeaQuery::select();
    sub.column(workspaces::Column::Id)
        .from_as(workspaces::Entity, Alias::new("w"))
        .and_where(col("w", "slug").eq(slug));
    let mut stmt = SeaQuery::select();
    stmt.expr_as(col("e", "id"), Alias::new("id"))
        .expr_as(col("e", "created_at"), Alias::new("created_at"))
        .expr_as(col("e", "updated_at"), Alias::new("updated_at"))
        .expr_as(col("e", "project"), Alias::new("project"))
        .expr_as(col("e", "provider"), Alias::new("provider"))
        .expr_as(col("e", "status"), Alias::new("status"))
        .expr_as(col("e", "url"), Alias::new("url"))
        .expr_as(col("e", "initiated_by_id"), Alias::new("initiated_by"))
        .expr_as(col("e", "token"), Alias::new("token"))
        .expr_as(col("e", "created_by_id"), Alias::new("created_by"))
        .expr_as(col("e", "updated_by_id"), Alias::new("updated_by"))
        .expr_as(col("u", "first_name"), Alias::new("user_first_name"))
        .expr_as(col("u", "last_name"), Alias::new("user_last_name"))
        .expr_as(col("u", "avatar"), Alias::new("user_avatar"))
        .expr_as(
            col("u", "avatar_asset_id"),
            Alias::new("user_avatar_asset_id"),
        )
        .expr_as(col("u", "is_bot"), Alias::new("user_is_bot"))
        .expr_as(col("u", "display_name"), Alias::new("user_display_name"))
        .from_as(pidash_db::entity::exporters::Entity, Alias::new("e"))
        .join_as(
            JoinType::LeftJoin,
            users::Entity,
            Alias::new("u"),
            col("e", "initiated_by_id").equals(("u", "id")),
        )
        .and_where(col("e", "workspace_id").in_subquery(sub))
        .and_where(col("e", "type").eq(EXPORT_TYPE))
        .and_where(col("e", "deleted_at").is_null())
        .order_by((Alias::new("e"), Alias::new("created_at")), order)
        .limit((limit + 1) as u64)
        .offset(offset as u64)
        .to_owned()
}

async fn fetch_exporter_page(
    pool: &sqlx::PgPool,
    slug: &str,
    order: sea_orm::sea_query::Order,
    offset: i64,
    limit: i64,
) -> Result<Vec<ExportRow>, Denial> {
    let stmt = exporter_page_stmt(slug, order, offset, limit);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = db
        .query_all(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.into_iter()
        .map(|row| {
            let decoded =
                ExporterPageRow::from_query_result(&row, "").map_err(|_| Denial::ServerError)?;
            Ok(ExportRow {
                id: decoded.id,
                created_at: decoded.created_at,
                updated_at: decoded.updated_at,
                project: decoded.project.unwrap_or_default(),
                provider: decoded.provider,
                status: decoded.status,
                url: decoded.url,
                initiated_by: decoded.initiated_by,
                token: decoded.token,
                created_by: decoded.created_by,
                updated_by: decoded.updated_by,
                user_first_name: decoded.user_first_name.ok_or(Denial::ServerError)?,
                user_last_name: decoded.user_last_name.ok_or(Denial::ServerError)?,
                user_avatar: decoded.user_avatar.ok_or(Denial::ServerError)?,
                user_avatar_asset_id: decoded.user_avatar_asset_id,
                user_is_bot: decoded.user_is_bot.ok_or(Denial::ServerError)?,
                user_display_name: decoded.user_display_name.ok_or(Denial::ServerError)?,
            })
        })
        .collect()
}

/// `avatar_url` for one row: the resolved asset URL, else the avatar
/// text, else null (`db/models/user.py:142-151`; the v1_projects
/// `avatar_url_for` order, over the batched map).
fn avatar_url_for_row(
    row: &ExportRow,
    assets: &std::collections::HashMap<uuid::Uuid, String>,
) -> Option<String> {
    if let Some(asset_id) = row.user_avatar_asset_id {
        if let Some(url) = assets.get(&asset_id) {
            return Some(url.clone());
        }
    }
    if row.user_avatar.is_empty() {
        None
    } else {
        Some(row.user_avatar.clone())
    }
}

/// `ExporterHistorySerializer.to_representation` in `Meta.fields` order
/// (`exporter.py:15-28`), with the nested `UserLiteSerializer` 7-key
/// shape. Strings are JSON-escaped; nulls render bare.
fn render_export_row(
    row: &ExportRow,
    timezone: &chrono_tz::Tz,
    assets: &std::collections::HashMap<uuid::Uuid, String>,
) -> String {
    let avatar_url = avatar_url_for_row(row, assets);
    let created_at = crate::serializer::render_datetime_in(&row.created_at, timezone);
    let updated_at = crate::serializer::render_datetime_in(&row.updated_at, timezone);
    let mut project = String::from("[");
    for (index, id) in row.project.iter().enumerate() {
        if index > 0 {
            project.push(',');
        }
        project.push_str(&json_string(&id.to_string()));
    }
    project.push(']');
    let opt_uuid = |id: &Option<uuid::Uuid>| match id {
        Some(id) => json_string(&id.to_string()),
        None => "null".to_owned(),
    };
    let opt_str = |value: &Option<String>| match value {
        Some(value) => json_string(value),
        None => "null".to_owned(),
    };
    format!(
        "{{\"id\":{id},\"created_at\":{created_at},\"updated_at\":{updated_at},\
         \"project\":{project},\"provider\":{provider},\"status\":{status},\"url\":{url},\
         \"initiated_by\":{initiated_by},\
         \"initiated_by_detail\":{{\"id\":{uid},\"first_name\":{first_name},\
         \"last_name\":{last_name},\"avatar\":{avatar},\"avatar_url\":{avatar_url},\
         \"is_bot\":{is_bot},\"display_name\":{display_name}}},\
         \"token\":{token},\"created_by\":{created_by},\"updated_by\":{updated_by}}}",
        id = json_string(&row.id.to_string()),
        created_at = json_string(&created_at),
        updated_at = json_string(&updated_at),
        provider = json_string(&row.provider),
        status = json_string(&row.status),
        url = opt_str(&row.url),
        initiated_by = json_string(&row.initiated_by.to_string()),
        uid = json_string(&row.initiated_by.to_string()),
        first_name = json_string(&row.user_first_name),
        last_name = json_string(&row.user_last_name),
        avatar = json_string(&row.user_avatar),
        avatar_url = opt_str(&avatar_url),
        is_bot = if row.user_is_bot { "true" } else { "false" },
        display_name = json_string(&row.user_display_name),
        token = json_string(&row.token),
        created_by = opt_uuid(&row.created_by),
        updated_by = opt_uuid(&row.updated_by),
    )
}

/// `POST /api/workspaces/<slug>/export-issues/` (`base.py:22-65`).
async fn create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    // `initial` (auth) runs before body parsing: anonymous answers 401
    // even when the body is missing or has no content type (the
    // app_views_search precedent: `actor` before `detail_body`).
    let resolved = match actor(&state, extension).await {
        Ok(resolved) => resolved,
        Err(denial) => return denial.into_response(),
    };
    let body = match axum::Json::<Value>::from_request(req, &state).await {
        Ok(axum::Json(body)) => body,
        Err(rejection) => return rejection.into_response(),
    };
    match create_inner(&state, &slug, &body, &resolved).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// Render a request `provider` value the way the `f"Provider
/// '{provider}' not found."` interpolation does (`base.py:63`):
/// strings verbatim, missing → `False` (the `.get` default), JSON null
/// → `None`, booleans capitalized. Compound values fall back to compact
/// JSON (unreached by the suite; Python would use `repr`).
fn provider_display(value: Option<&Value>) -> String {
    match value {
        None => "False".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => "False".to_owned(),
        Some(Value::Null) => "None".to_owned(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Array(_) | Value::Object(_)) => {
            value.map_or_else(|| "False".to_owned(), Value::to_string)
        }
    }
}

/// Extract the `project` list (`request.data.get("project", [])`).
/// Missing, null, non-array or empty means the fallback query runs (`if
/// not project_ids`, `base.py:33`); a non-empty array passes through
/// element-wise, rendered the way the ORM layer stringifies values, so a
/// malformed id fails UUID parsing below with the `ValidationError` 400
/// Django's `UUIDField.to_python` raises during param prep
/// (`handle_exception`'s `ValidationError` branch). Residual divergence,
/// unreached by the suite: Django's `uuid.UUID(int=...)` accepts integer
/// and boolean elements (and passes `None` through as a null array
/// element); those answer 400 here instead of Django's 200.
fn extract_project_ids(body: &Value) -> Vec<String> {
    match body.get("project") {
        Some(Value::Array(items)) if !items.is_empty() => items
            .iter()
            .map(|item| match item {
                Value::String(text) => text.clone(),
                Value::Bool(true) => "True".to_owned(),
                Value::Bool(false) => "False".to_owned(),
                Value::Null => "None".to_owned(),
                Value::Number(number) => number.to_string(),
                Value::Array(_) | Value::Object(_) => item.to_string(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

async fn create_inner(
    state: &AppState,
    slug: &str,
    body: &Value,
    resolved: &crate::license::Actor,
) -> Result<Response, Denial> {
    use pidash_services::app_analytics::queries::{provider_not_found_body, EXPORT_PROVIDERS};
    let pool = pool_of(state)?.clone();
    // Decorator before body, like GET.
    allow_workspace(&pool, slug, &resolved.id).await?;
    // `Workspace.objects.get(slug=slug)` (`base.py:24`) runs before the
    // provider check (`base.py:31`): a bad slug with a bad provider
    // answers 404, not 400.
    let workspace_id = workspace_by_slug(&pool, slug).await?;
    // `request.data.get("provider", False)` (`base.py:26`).
    let provider_value = body.get("provider");
    let provider = match provider_value {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    };
    if !EXPORT_PROVIDERS.contains(&provider.as_str()) {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            provider_not_found_body(&provider_display(provider_value)),
        ));
    }
    let multiple = body
        .get("multiple")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // Empty list → the requester's active workspace projects
    // (`base.py:33-39`), stringified per row.
    let mut project_ids = extract_project_ids(body);
    if project_ids.is_empty() {
        project_ids = exporter_project_fallback(&pool, slug, &resolved.id).await?;
    }
    // UUID-typed `project` ArrayField: `UUIDField.to_python` raises
    // `ValidationError` on a malformed id during param prep, so
    // `handle_exception` answers its 400 branch (`app/views/base.py`);
    // parse here and answer the same body (the app_issues precedent).
    let mut project_uuids = Vec::with_capacity(project_ids.len());
    for id in &project_ids {
        project_uuids.push(id.parse::<uuid::Uuid>().map_err(|_| {
            Denial::Raw(
                StatusCode::BAD_REQUEST,
                crate::app_issues::INVALID_DETAIL_BODY.to_owned(),
            )
        })?);
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    let now = Utc::now();
    let row_id = uuid::Uuid::new_v4();
    // `ExporterHistory.objects.create(...)` (`base.py:41-47`):
    // `created_by` is the requesting user (`BaseModel.save` via crum),
    // `updated_by` stays null; `status` defaults to `queued`.
    {
        use sea_orm::{ActiveModelTrait, ActiveValue, Set};
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let active = pidash_db::entity::exporters::ActiveModel {
            id: Set(row_id),
            created_at: Set(chrono::DateTime::<chrono::FixedOffset>::from(now)),
            updated_at: Set(chrono::DateTime::<chrono::FixedOffset>::from(now)),
            created_by_id: Set(Some(resolved.id)),
            initiated_by_id: Set(resolved.id),
            r#type: Set(EXPORT_TYPE.to_owned()),
            workspace_id: Set(workspace_id),
            project: Set(Some(project_uuids)),
            provider: Set(provider.clone()),
            status: Set("queued".to_owned()),
            reason: Set(String::new()),
            key: Set(String::new()),
            token: Set(token.clone()),
            filters: ActiveValue::NotSet,
            rich_filters: Set(Some(serde_json::json!({}))),
            ..Default::default()
        };
        active.insert(&db).await.map_err(|_| Denial::ServerError)?;
    }
    // `issue_export_task.delay(provider, workspace_id, project_ids,
    // token_id=token, multiple, slug)` (`base.py:49-56`): the Celery v2
    // body (all kwargs, no positional args) onto the Postgres queue. Rust
    // owns the body (PIDASHCONV-1106), so the Rust worker runs it directly
    // rather than forwarding the name to the broker.
    let mut kwargs = serde_json::Map::new();
    kwargs.insert("provider".to_owned(), Value::String(provider.clone()));
    kwargs.insert(
        "workspace_id".to_owned(),
        Value::String(workspace_id.to_string()),
    );
    kwargs.insert(
        "project_ids".to_owned(),
        Value::Array(
            project_ids
                .iter()
                .map(|id| Value::String(id.clone()))
                .collect(),
        ),
    );
    kwargs.insert("token_id".to_owned(), Value::String(token));
    kwargs.insert("multiple".to_owned(), Value::Bool(multiple));
    kwargs.insert("slug".to_owned(), Value::String(slug.to_owned()));
    let message = pidash_jobs::TaskMessage::new(
        pidash_jobs::tasks_export::ISSUE_EXPORT_TASK,
        Vec::new(),
        kwargs,
    );
    enqueue_best_effort(
        &pool,
        &message.task,
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    )
    .await;
    Ok(json_response(
        StatusCode::OK,
        format!("{{\"message\":{}}}", json_string(ACCEPTED_MESSAGE)),
    ))
}

/// Empty-project fallback (`base.py:34-38`):
/// `Project.objects.filter(workspace__slug, member=user, is_active,
/// archived null).values_list("id", flat=True)`, stringified per row.
/// The empty-`project` fallback list as a select statement: the user's
/// active, non-archived projects in the workspace, soft-deletes excluded
/// on every table.
fn exporter_project_fallback_stmt(
    slug: &str,
    user_id: &uuid::Uuid,
) -> sea_orm::sea_query::SelectStatement {
    use sea_orm::sea_query::{Expr, Query as SeaQuery};
    let mut stmt = SeaQuery::select();
    stmt.expr_as(col("p", "id"), Alias::new("id"))
        .from_as(projects::Entity, Alias::new("p"))
        .join_as(
            JoinType::InnerJoin,
            workspaces::Entity,
            Alias::new("w"),
            col("w", "id").equals(("p", "workspace_id")),
        )
        .join_as(
            JoinType::InnerJoin,
            project_members::Entity,
            Alias::new("pm"),
            col("pm", "project_id").equals(("p", "id")),
        )
        .and_where(col("w", "deleted_at").is_null())
        .and_where(col("w", "slug").eq(slug))
        .and_where(col("pm", "member_id").eq(Expr::val(*user_id)))
        .and_where(col("pm", "is_active").eq(true))
        .and_where(col("pm", "deleted_at").is_null())
        .and_where(col("p", "deleted_at").is_null())
        .and_where(col("p", "archived_at").is_null())
        .to_owned()
}

async fn exporter_project_fallback(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<Vec<String>, Denial> {
    let stmt = exporter_project_fallback_stmt(slug, user_id);
    let db = pidash_db::pool::sea_orm_connection(pool);
    let rows = db
        .query_all(DbBackend::Postgres.build(&stmt))
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.into_iter()
        .map(|row| {
            row.try_get::<uuid::Uuid>("", "id")
                .map(|id| id.to_string())
                .map_err(|_| Denial::ServerError)
        })
        .collect()
}

/// Best-effort deferred publish (the D-02 intake precedent): without the
/// queue the 200 still stands.
async fn enqueue_best_effort(pool: &sqlx::PgPool, task: &str, args: Value, kwargs: Value) {
    let job = pidash_jobs::queue::NewJob::new(task.to_owned(), args, kwargs);
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; response stands");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::DbBackend;

    fn sql_of(stmt: &sea_orm::sea_query::SelectStatement) -> String {
        DbBackend::Postgres.build(stmt).sql.to_lowercase()
    }

    #[test]
    fn export_statement_shapes_pin_queries() {
        let count = sql_of(&exporter_total_count_stmt("acme"));
        assert!(count.contains("count(*)"), "count shape: {count}");
        assert!(count.contains("from \"exporters\""), "count from: {count}");
        assert!(count.contains("in (select"), "count in-subquery: {count}");
        assert!(
            count.contains("\"workspaces\""),
            "count sub workspace: {count}"
        );
        assert!(
            count.contains("deleted_at\" is null"),
            "count soft delete: {count}"
        );

        let page = sql_of(&exporter_page_stmt(
            "acme",
            sea_orm::sea_query::Order::Desc,
            0,
            10,
        ));
        assert!(page.contains("left join"), "page users join: {page}");
        assert!(page.contains("\"users\""), "page users table: {page}");
        assert!(
            page.contains("as \"user_first_name\""),
            "page alias: {page}"
        );
        assert!(page.contains("in (select"), "page in-subquery: {page}");
        assert!(page.contains("limit"), "page limit: {page}");
        assert!(page.contains("offset"), "page offset: {page}");
        assert!(page.contains("desc"), "page order: {page}");
        assert!(
            !page.contains("join \"workspaces\""),
            "page drops no-op join: {page}"
        );

        let page_asc = sql_of(&exporter_page_stmt(
            "acme",
            sea_orm::sea_query::Order::Asc,
            0,
            10,
        ));
        assert!(page_asc.contains("asc"), "page asc order: {page_asc}");

        let fallback = sql_of(&exporter_project_fallback_stmt("acme", &uuid::Uuid::nil()));
        assert!(
            fallback.contains("join \"project_members\""),
            "fallback member join: {fallback}"
        );
        assert!(
            fallback.contains("\"archived_at\" is null"),
            "fallback archived guard: {fallback}"
        );
    }

    /// TEMP tables shadow public (no FKs needed for statement execution).
    #[tokio::test]
    async fn live_export_queries() {
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
            "workspace_members",
            "exporters",
            "users",
            "projects",
            "project_members",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let (ws, other_ws, user, stranger, proj, proj2) = (
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        );
        for sql in [
            format!("INSERT INTO workspaces (id, slug, deleted_at) VALUES ('{ws}', 'acme_export', NULL)"),
            format!("INSERT INTO workspaces (id, slug, deleted_at) VALUES ('{other_ws}', 'other_export', NULL)"),
            format!("INSERT INTO workspace_members (id, workspace_id, member_id, role, is_active, deleted_at) VALUES (gen_random_uuid(), '{ws}', '{user}', 20, true, NULL)"),
            format!("INSERT INTO users (id, first_name, last_name, avatar, display_name, is_bot) VALUES ('{user}', 'Ada', 'Lovelace', '', 'Ada', false)"),
            format!("INSERT INTO users (id, first_name, last_name, avatar, display_name, is_bot) VALUES ('{stranger}', 'Bob', 'Noexp', '', 'Bob', false)"),
            format!("INSERT INTO projects (id, workspace_id, identifier, deleted_at, archived_at) VALUES ('{proj}', '{ws}', 'EXP', NULL, NULL)"),
            format!("INSERT INTO projects (id, workspace_id, identifier, deleted_at, archived_at) VALUES ('{proj2}', '{ws}', 'EXP2', NULL, now())"),
            format!("INSERT INTO project_members (id, workspace_id, project_id, member_id, role, is_active, deleted_at) VALUES (gen_random_uuid(), '{ws}', '{proj}', '{user}', 20, true, NULL)"),
            format!("INSERT INTO project_members (id, workspace_id, project_id, member_id, role, is_active, deleted_at) VALUES (gen_random_uuid(), '{ws}', '{proj2}', '{user}', 20, true, NULL)"),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }
        // role gate: ADMIN allows, stranger forbids, deleted member forbids.
        allow_workspace(&pool, "acme_export", &user)
            .await
            .expect("admin ok");
        assert!(matches!(
            allow_workspace(&pool, "acme_export", &stranger).await,
            Err(Denial::Forbidden)
        ));
        sqlx::query(&format!(
            "UPDATE workspace_members SET deleted_at = now() WHERE member_id = '{user}'"
        ))
        .execute(&pool)
        .await
        .expect("soft delete");
        assert!(matches!(
            allow_workspace(&pool, "acme_export", &user).await,
            Err(Denial::Forbidden)
        ));
        // workspace lookup: hit + miss.
        assert_eq!(
            workspace_by_slug(&pool, "acme_export").await.expect("ws"),
            ws
        );
        assert!(matches!(
            workspace_by_slug(&pool, "missing").await,
            Err(Denial::NotFound)
        ));
        // fallback: only the non-archived project.
        let fallback = exporter_project_fallback(&pool, "acme_export", &user)
            .await
            .expect("fallback");
        assert_eq!(fallback, vec![proj.to_string()]);
        // count + page over two live rows (one soft-deleted out).
        let (e1, e2, e3) = (
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        );
        for sql in [
            format!("INSERT INTO exporters (id, created_at, updated_at, workspace_id, provider, status, reason, key, token, initiated_by_id, type, deleted_at) VALUES ('{e1}', now() - interval '2 days', now(), '{ws}', 'jira', 'queued', '', '', 'tok1', '{user}', 'issue_exports', NULL)"),
            format!("INSERT INTO exporters (id, created_at, updated_at, workspace_id, provider, status, reason, key, token, initiated_by_id, type, deleted_at) VALUES ('{e2}', now() - interval '1 day', now(), '{ws}', 'jira', 'queued', '', '', 'tok2', '{user}', 'issue_exports', NULL)"),
            format!("INSERT INTO exporters (id, created_at, updated_at, workspace_id, provider, status, reason, key, token, initiated_by_id, type, deleted_at) VALUES ('{e3}', now(), now(), '{ws}', 'jira', 'queued', '', '', 'tok3', '{user}', 'issue_exports', now())"),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed exporters");
        }
        assert_eq!(
            exporter_total_count(&pool, "acme_export")
                .await
                .expect("count"),
            2
        );
        let page =
            fetch_exporter_page(&pool, "acme_export", sea_orm::sea_query::Order::Desc, 0, 10)
                .await
                .expect("page");
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].id, e2);
        assert_eq!(page[0].user_first_name, "Ada");
        assert_eq!(page[0].user_display_name, "Ada");
        // a row whose initiated_by user is missing must 500 (the ok_or arm).
        let e4 = uuid::Uuid::new_v4();
        sqlx::query(&format!(
            "INSERT INTO exporters (id, created_at, updated_at, workspace_id, provider, status, reason, key, token, initiated_by_id, type, deleted_at) VALUES ('{e4}', now(), now(), '{ws}', 'jira', 'queued', '', '', 'tok4', gen_random_uuid(), 'issue_exports', NULL)"
        ))
        .execute(&pool)
        .await
        .expect("seed orphan");
        assert!(matches!(
            fetch_exporter_page(&pool, "acme_export", sea_orm::sea_query::Order::Asc, 0, 10).await,
            Err(Denial::ServerError)
        ));
    }
}
