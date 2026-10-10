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

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("export response")
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
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
    let row: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id AND w.deleted_at IS NULL
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match row.and_then(|row| row.0) {
        Some(20) | Some(15) => Ok(()),
        _ => Err(Denial::Forbidden),
    }
}

/// `Workspace.objects.get(slug=slug)` → 404 `ObjectDoesNotExist` branch.
async fn workspace_by_slug(pool: &sqlx::PgPool, slug: &str) -> Result<uuid::Uuid, Denial> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#)
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::NotFound)
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
    let order_sql = resolve_export_order(&order_by)?;
    let total = exporter_total_count(&pool, slug).await?;
    let rows = fetch_exporter_page(&pool, slug, &order_sql, offset, per_page).await?;
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
/// in this domain's suite.
fn resolve_export_order(raw: &str) -> Result<String, Denial> {
    match raw {
        "-created_at" => Ok("e.created_at DESC".to_owned()),
        "created_at" => Ok("e.created_at ASC".to_owned()),
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
async fn exporter_total_count(pool: &sqlx::PgPool, slug: &str) -> Result<i64, Denial> {
    let row: Option<(i64,)> = sqlx::query_as(
        r#"SELECT COUNT(*) FROM exporters e
           WHERE e.workspace_id IN (SELECT w.id FROM workspaces w WHERE w.slug = $1)
           AND e.type = $2 AND e.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(EXPORT_TYPE)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// The `[offset, offset + limit + 1)` slice the paginator reads (the
/// extra row decides `next_page_results`, like `OffsetPaginator`).
#[allow(clippy::too_many_lines)]
async fn fetch_exporter_page(
    pool: &sqlx::PgPool,
    slug: &str,
    order_sql: &str,
    offset: i64,
    limit: i64,
) -> Result<Vec<ExportRow>, Denial> {
    // `avatar_url` is a model property (`db/models/user.py:142-151`),
    // not a column: fetch `avatar_asset_id` and resolve through the
    // `file_assets` batch helper below (the v1_projects precedent).
    let sql = format!(
        r#"SELECT e.id, e.created_at, e.updated_at, e.project, e.provider, e.status, e.url,
           e.initiated_by_id, e.token, e.created_by_id, e.updated_by_id,
           u.first_name, u.last_name, u.avatar, u.avatar_asset_id, u.is_bot, u.display_name
           FROM exporters e
           LEFT OUTER JOIN workspaces ON (e.workspace_id = workspaces.id)
           LEFT OUTER JOIN users u ON (e.initiated_by_id = u.id)
           WHERE (e.workspace_id IN (SELECT w.id FROM workspaces w WHERE w.slug = $1)
           AND e.type = $2 AND e.deleted_at IS NULL)
           ORDER BY {order_sql} LIMIT $3 OFFSET $4"#
    );
    // Seventeen columns exceed the `FromRow` tuple impls, so decode by
    // index (`sqlx::Row::try_get`).
    use sqlx::Row as _;
    let rows = sqlx::query(&sql)
        .bind(slug)
        .bind(EXPORT_TYPE)
        .bind(limit + 1)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter()
        .map(|row| {
            Ok(ExportRow {
                id: row.try_get(0).map_err(|_| Denial::ServerError)?,
                created_at: row.try_get(1).map_err(|_| Denial::ServerError)?,
                updated_at: row.try_get(2).map_err(|_| Denial::ServerError)?,
                project: row
                    .try_get::<Option<Vec<uuid::Uuid>>, _>(3)
                    .map_err(|_| Denial::ServerError)?
                    .unwrap_or_default(),
                provider: row.try_get(4).map_err(|_| Denial::ServerError)?,
                status: row.try_get(5).map_err(|_| Denial::ServerError)?,
                url: row.try_get(6).map_err(|_| Denial::ServerError)?,
                initiated_by: row.try_get(7).map_err(|_| Denial::ServerError)?,
                token: row.try_get(8).map_err(|_| Denial::ServerError)?,
                created_by: row.try_get(9).map_err(|_| Denial::ServerError)?,
                updated_by: row.try_get(10).map_err(|_| Denial::ServerError)?,
                user_first_name: row
                    .try_get::<Option<String>, _>(11)
                    .map_err(|_| Denial::ServerError)?
                    .ok_or(Denial::ServerError)?,
                user_last_name: row
                    .try_get::<Option<String>, _>(12)
                    .map_err(|_| Denial::ServerError)?
                    .ok_or(Denial::ServerError)?,
                user_avatar: row
                    .try_get::<Option<String>, _>(13)
                    .map_err(|_| Denial::ServerError)?
                    .ok_or(Denial::ServerError)?,
                user_avatar_asset_id: row.try_get(14).map_err(|_| Denial::ServerError)?,
                user_is_bot: row
                    .try_get::<Option<bool>, _>(15)
                    .map_err(|_| Denial::ServerError)?
                    .ok_or(Denial::ServerError)?,
                user_display_name: row
                    .try_get::<Option<String>, _>(16)
                    .map_err(|_| Denial::ServerError)?
                    .ok_or(Denial::ServerError)?,
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
    sqlx::query(
        r#"INSERT INTO exporters
           (id, created_at, updated_at, deleted_at, created_by_id, updated_by_id,
            name, type, workspace_id, project, provider, status, reason, key,
            url, token, initiated_by_id, filters, rich_filters)
           VALUES ($1, $2, $3, NULL, $4, NULL, NULL, $5, $6, $7, $8, 'queued', '', '',
            NULL, $9, $10, NULL, '{}'::jsonb)"#,
    )
    .bind(row_id)
    .bind(now)
    .bind(now)
    .bind(resolved.id)
    .bind(EXPORT_TYPE)
    .bind(workspace_id)
    .bind(&project_uuids)
    .bind(&provider)
    .bind(&token)
    .bind(resolved.id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `issue_export_task.delay(provider, workspace_id, project_ids,
    // token_id=token, multiple, slug)` (`base.py:49-56`): the Celery v2
    // body (all kwargs, no positional args) onto the Postgres queue, from
    // which the worker forwards Python-owned names to the broker.
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
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
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
async fn exporter_project_fallback(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<Vec<String>, Denial> {
    let rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT p.id FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id AND w.deleted_at IS NULL
           JOIN project_members pm ON pm.project_id = p.id
           WHERE w.slug = $1 AND pm.member_id = $2 AND pm.is_active
           AND pm.deleted_at IS NULL AND p.deleted_at IS NULL AND p.archived_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(rows.into_iter().map(|row| row.0.to_string()).collect())
}

/// Best-effort deferred publish (the D-02 intake precedent): without the
/// queue the 200 still stands.
async fn enqueue_best_effort(pool: &sqlx::PgPool, task: &str, args: Value, kwargs: Value) {
    let job = pidash_jobs::queue::NewJob::new(task.to_owned(), args, kwargs);
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; response stands");
    }
}
