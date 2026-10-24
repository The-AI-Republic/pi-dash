#![forbid(unsafe_code)]

//! Instance-admin runner list handler (PDASHOSS01-303).
//!
//! Ports `apps/api/pi_dash/runner/admin_views.py` (`InstanceRunnerListEndpoint`;
//! route `runner/admin_urls.py`, mounted under `/api/instances/runners/` by
//! `license/urls.py`). Read-only `GET`; every other method proxies so
//! Django's 405-after-auth and metadata responses stay byte-for-byte.
//!
//! Layering: params, SQL fragments and the row shape come from
//! `pidash_services::runner_admin`; this module owns the HTTP shell (the
//! shared license `InstanceAdminPermission` gate, row fetching, response
//! rendering). Order of operations follows the view: auth, filter
//! validation, the page slice, then `count()`.

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};
use sqlx::PgPool;

use pidash_services::runner_admin::{
    render_body, render_row, ListParams, RunnerRow, FROM_SQL, PAGE_SIZE, SELECT_SQL,
};

use crate::license::{owned, query_last, require_admin, Denial, QueryMap};
use crate::state::AppState;

/// Register the owned `GET`; the license router merges this in.
pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/instances/runners/",
        owned(get(list_runners), &["GET"]),
    )
}

fn json_body(status: StatusCode, body: &Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(render_body(body)))
        .expect("json response")
}

/// The page slice: `qs[start : start + per]` over `-created_at`.
async fn fetch_page(
    pool: &PgPool,
    params: &ListParams,
    offset: i64,
) -> Result<Vec<RunnerRow>, Denial> {
    let mut sql = format!(
        "{SELECT_SQL} {FROM_SQL}{} ORDER BY r.created_at DESC LIMIT {PAGE_SIZE}",
        params.where_sql()
    );
    if offset > 0 {
        sql.push_str(&format!(" OFFSET {offset}"));
    }
    let mut query = sqlx::query_as::<_, RunnerRow>(&sql);
    for bind in params.binds() {
        query = query.bind(bind);
    }
    query.fetch_all(pool).await.map_err(|_| Denial::ServerError)
}

/// `qs.count()` with the same filters.
async fn count(pool: &PgPool, params: &ListParams) -> Result<i64, Denial> {
    let sql = format!("SELECT COUNT(*) {FROM_SQL}{}", params.where_sql());
    let mut query = sqlx::query_scalar::<_, i64>(&sql);
    for bind in params.binds() {
        query = query.bind(bind);
    }
    query.fetch_one(pool).await.map_err(|_| Denial::ServerError)
}

/// `GET /api/instances/runners/`.
async fn list_runners(
    State(state): State<AppState>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    match list(&state, &query, extension).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn list(
    state: &AppState,
    query: &QueryMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Response, Denial> {
    require_admin(state, extension).await?;
    let pool = state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)?;

    let provisioning = query_last(query, "provisioning");
    let status = query_last(query, "status");
    let workspace = query_last(query, "workspace");
    let page = query_last(query, "page");
    let params = match ListParams::parse(
        provisioning.as_deref(),
        status.as_deref(),
        workspace.as_deref(),
        page.as_deref(),
    ) {
        Ok(params) => params,
        Err(error) => return Ok(json_body(StatusCode::BAD_REQUEST, &error.body())),
    };
    // An OFFSET past bigint is a Postgres error in Django: the generic 500.
    let offset = params.offset().ok_or(Denial::ServerError)?;

    let rows = fetch_page(pool, &params, offset).await?;
    let total = count(pool, &params).await?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(render_row(row).map_err(|_| Denial::ServerError)?);
    }
    let body = json!({
        "page": params.page,
        "total": total,
        "results": results,
    });
    Ok(json_body(StatusCode::OK, &body))
}
