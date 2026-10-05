#![forbid(unsafe_code)]

//! App search + views handlers (D-29, stage 5, PIDASHCONV-276 + PIDASHCONV-275).
//!
//! Ports `apps/api/pi_dash/app/views/search/` onto the merged D-29
//! foundation:
//! - `GET /api/workspaces/<slug>/search/` (`GlobalSearchEndpoint.get`,
//!   `base.py:255-286`)
//! - `GET /api/workspaces/<slug>/projects/<project_id>/search-issues/`
//!   (`IssueSearchEndpoint.get`, `issue.py:104-166`)
//! - `GET /api/workspaces/<slug>/entity-search/` (`SearchEndpoint.get`,
//!   `base.py:293-691`)
//!
//! Only these three GET paths are registered, so the edge serves exactly
//! this family from Rust while every sibling path keeps proxying to
//! Django — route registration is the cutover granularity, no flag
//! needed. Reads stay on the primary (`base.py:47-52`, `issue.py:18-22`:
//! read-your-writes for the pickers backed by these endpoints).
//!
//! Layering: SQL text plus param parsing live in
//! `pidash_services::app_views_search::{queries_search, fts}` (PIDASHCONV-272);
//! the guard semantics in `permissions` (PIDASHCONV-273). This module owns
//! the HTTP shell (routes, session auth, the project-kwarg rewrite), the
//! small lookups the issue pipeline needs (parent row, relations, guest
//! membership), row fetching and the `.values()` shaping.
//!
//! [`routes`] merges the search routes with the views routes below —
//! merges keep both sides.
//!
//! Views (PIDASHCONV-275): the 7 views routes from
//! `apps/api/pi_dash/app/views/view/base.py` — `WorkspaceViewViewSet`
//! (global-view routes, `:52-136`), `IssueViewViewSet` (project-view
//! routes, `:256-398`), `WorkspaceViewIssuesViewSet.list`
//! (global-view-issues route, `:217-253`), `IssueViewFavoriteViewSet`
//! (user-favorite-views routes, `:401-433`). Only the owned methods are
//! registered; every sibling path and non-owned method keeps proxying.
//! Views queryset SQL lives in
//! `pidash_services::app_views_search::queries_views`, row shapes in
//! `...::serializers`, gates in `...::permissions`, the retrieve
//! publishers in `...::tasks`.
//!
//! Ported views bugs (also listed in the PR):
//! - B1 (`base.py:102-112`): workspace retrieve serializes `.first()`
//!   unconditionally — unknown pk answers 200 with the serializer's
//!   `get_initial()` body (not 404, and not JSON `null`).
//! - B1b (`base.py:317-327`): project retrieve dereferences
//!   `issue_view.owned_by` after `.first()`; unknown pk plus a guest
//!   without `guest_view_all_features` raises (500), not 404 — other
//!   callers get the same `get_initial()` 200 as B1.
//! - B2 (`serializers/base.py:12-18`): `?fields=` is silently ignored on
//!   both list views — full objects always render.
//! - B4 (`base.py:404-411`): the favorite list queryset raises `FieldError`
//!   (`select_related("view")` names no FK) — every GET answers the
//!   generic 500.
//!
//! Sibling plumbing mirrors the D-26 `app_issues` and D-02 `space` shapes:
//!
//! - [`QueryMap`] / [`query_last`]: Django `QueryDict.get` (last wins).
//! - [`Denial`]: exact error bodies (`app/views/base.py:87-152`, DRF
//!   `NotAuthenticated` default).
//! - [`owned`]: cutover granularity — GET serves from Rust, the rest proxy
//!   so Django's 405-after-auth responses survive byte for byte.

pub mod handlers_search;
pub mod handlers_views;

use std::collections::HashMap;

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use sqlx::{Postgres, Row};

use crate::state::AppState;

/// Merge the app-search route groups with the views route groups
/// (PIDASHCONV-275); merges keep both sides.
pub fn routes() -> Router<AppState> {
    handlers_search::routes().merge(handlers_views::routes())
}

/// A search path: the GET handler owns reads, everything else falls
/// through to Django. OPTIONS proxies too: DRF answers metadata (401 anon
/// / 200 authed) where axum would 405; `HEAD` rides axum's `get` handling
/// like Django's `GET`-backed `HEAD`.
pub fn owned(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

/// Exact bytes of the DRF `IsAuthenticated` denial.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ValidationError` branch (`app/views/base.py`):
/// a badly-formed UUID in a filter raises Django `ValidationError`, not
/// DRF's parse error.
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s generic 500 branch. Covers `ValueError` from the
/// unguarded `int(count)` (B8), `AttributeError` from the unguarded
/// `issue.parent` read (B5), and anything unclassified.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// DRF `exception_handler` maps `Http404(*args)` to `NotFound(*args)`, so
/// the `_rewrite_project_kwarg` miss (`Project.resolve`, "Project not
/// found") renders with the resolve message — verified against DRF 3.15.2
/// (`rest_framework/views.py:81-82`), not the bare default.
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;

/// One query value, repeated or not. `serde_html_form` (axum's `Query`
/// backend) does not coerce a lone `?key=value` into a sequence, so the
/// extractor uses this untagged shape and callers read first/last —
/// mirroring Django's `QueryDict`, where repeats are legal and `.get`
/// returns the last value.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every search handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// All values for `key`, in order; `None` when absent.
pub fn query_values(query: &QueryMap, key: &str) -> Option<Vec<String>> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => vec![one.clone()],
        OneOrMany::Many(many) => many.clone(),
    })
}

/// Django `QueryDict.get`: the last value, or `None`.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query_values(query, key).and_then(|values| values.into_iter().last())
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 400, `{"error":"Please provide valid detail"}` (bad UUID in a filter).
    InvalidDetail,
    /// 404, `{"detail":"Project not found"}` (project-kwarg rewrite miss).
    ProjectNotFound,
    /// 500, generic branch (B5, B8, unclassified).
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, &'static str) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY),
            Denial::InvalidDetail => (StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY),
            Denial::ServerError => (StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY),
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
            .expect("static denial response")
    }
}

/// Render a pre-shaped compact-JSON body (`JSONRenderer`, `COMPACT_JSON`).
pub fn json_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("search response")
}

/// `request.user` from the Django session. No session, no key, or a
/// non-UUID id means anonymous → 401. (Django PKs are UUIDs; a session id
/// that is not a UUID cannot be a user.)
pub fn actor_user_id(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Option<uuid::Uuid> {
    let handle = extension?.0;
    let mut session = handle.snapshot();
    let raw = session.get("_auth_user_id")?.as_str()?.to_owned();
    raw.parse::<uuid::Uuid>().ok()
}

/// The request pool: reads stay on the primary (`search/base.py:47-52`,
/// `search/issue.py:18-22` — the pickers backed by these endpoints are hit
/// right after writes, where replica lag would hide the new row).
pub fn primary(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
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

/// `_rewrite_project_kwarg` for the issue-search `project_id` URL kwarg
/// (`app/views/base.py:39-72`): UUIDs pass through (no row check — the
/// view body filters on it); anything else resolves as a workspace-scoped
/// identifier (`UPPER(identifier)`, the `save()`-normalized form); a miss
/// raises the resolve `Http404`, i.e. [`Denial::ProjectNotFound`].
/// Skipped for anonymous callers (the slug-existence oracle stays closed) —
/// so callers must check auth first, in Django's order.
pub async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = normalize_resolve_identifier(raw);
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

/// A badly-formed UUID where Django would raise `ValidationError` in the
/// filter (`handle_exception` → 400 `INVALID_DETAIL_BODY`).
pub fn parse_uuid_or_invalid(raw: &str) -> Result<uuid::Uuid, Denial> {
    raw.parse::<uuid::Uuid>().map_err(|_| Denial::InvalidDetail)
}

// ---------------------------------------------------------------------------
// Row fetching + shaping
// ---------------------------------------------------------------------------

/// Column decoder for one position of a section query.
///
/// A spec entry is `(output key, SELECT position, decoder)`. Positions
/// follow the builder's `SELECT` order exactly; the output order follows
/// Django's compiler instead — concrete columns first (in `.values()`
/// order), annotations last — which differs from `SELECT` order whenever a
/// builder inlines an annotation mid-list (pages `project_ids`,
/// cycle `status`). The extra `created_at` riding along for
/// `DISTINCT`+`ORDER BY` compliance has no entry at all: never decoded,
/// never emitted (the services contract: shaping drops it).
#[derive(Debug, Clone, Copy)]
pub enum Col {
    Text,
    Int,
    Uuid,
    Date,
    Json,
    UuidArray,
    StrArray,
}

/// One shaped column: the response key, its `SELECT` position, its decoder.
pub type Spec<'a> = &'a [(/* key */ &'a str, /* position */ usize, Col)];

/// One decoded cell.
#[derive(Debug, Clone)]
enum Cell {
    Null,
    Text(String),
    Int(i32),
    Uid(uuid::Uuid),
    Date(chrono::NaiveDate),
    Json(serde_json::Value),
    UidArray(Vec<uuid::Uuid>),
    StrArray(Vec<String>),
}

impl Cell {
    fn render(&self) -> String {
        match self {
            Cell::Null => "null".to_owned(),
            Cell::Text(text) => json_quote(text),
            Cell::Int(n) => n.to_string(),
            Cell::Uid(id) => json_quote(&id.to_string()),
            // DRF `DateField`: `isoformat()` — `YYYY-MM-DD`, same bytes the
            // Postgres JSON cast renders.
            Cell::Date(date) => json_quote(&date.format("%Y-%m-%d").to_string()),
            // `logo_props` JSONB: Django reads the stored document and
            // re-renders it; the stored bytes are the source of truth on
            // both sides, so key order and spacing match.
            Cell::Json(value) => serde_json::to_string(value).unwrap_or("null".to_owned()),
            Cell::UidArray(ids) => {
                let parts: Vec<String> = ids.iter().map(|id| json_quote(&id.to_string())).collect();
                format!("[{}]", parts.join(","))
            }
            Cell::StrArray(items) => {
                let parts: Vec<String> = items.iter().map(|item| json_quote(item)).collect();
                format!("[{}]", parts.join(","))
            }
        }
    }
}

fn json_quote(text: &str) -> String {
    serde_json::to_string(text).expect("json string")
}

/// Bind a services [`BuiltQuery`](pidash_services::app_views_search::queries_search::BuiltQuery)
/// to sqlx in `$N` order. `IntegerArray` binds `Vec<i32>`: the SQL casts
/// the bind to `int[]`, and an `int8[]` would not cast.
fn bind_built<'q>(
    query: sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>,
    params: &[pidash_services::app_views_search::queries_search::SqlParam],
) -> Result<sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>, Denial> {
    use pidash_services::app_views_search::queries_search::SqlParam;
    let mut query = query;
    for param in params {
        query = match param {
            SqlParam::Text(text) => query.bind(text.clone()),
            SqlParam::Integer(n) => query.bind(*n),
            SqlParam::IntegerArray(values) => {
                query.bind(values.iter().map(|n| *n as i32).collect::<Vec<i32>>())
            }
            // Builders only emit UUIDs the handler constructed from
            // validated input or straight from the database, so a parse
            // failure here is unreachable — 500 either way.
            SqlParam::Uuid(raw) => {
                query.bind(raw.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError)?)
            }
        };
    }
    Ok(query)
}

/// Run one section query and shape every row: keys in Django's compiler
/// order, compact JSON, `null` for missing/NULL — the exact bytes of DRF
/// rendering the `values()` dicts.
pub async fn fetch_shaped(
    pool: &sqlx::PgPool,
    built: &pidash_services::app_views_search::queries_search::BuiltQuery,
    spec: Spec<'_>,
) -> Result<Vec<String>, Denial> {
    let query = bind_built(sqlx::query(&built.sql), &built.params)?;
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(shape_row(row, spec)?);
    }
    Ok(out)
}

fn decode_cell(row: &sqlx::postgres::PgRow, index: usize, col: Col) -> Result<Cell, Denial> {
    let cell = match col {
        Col::Text => match row
            .try_get::<Option<String>, _>(index)
            .map_err(|_| Denial::ServerError)?
        {
            Some(text) => Cell::Text(text),
            None => Cell::Null,
        },
        Col::Int => match row
            .try_get::<Option<i32>, _>(index)
            .map_err(|_| Denial::ServerError)?
        {
            Some(n) => Cell::Int(n),
            None => Cell::Null,
        },
        Col::Uuid => match row
            .try_get::<Option<uuid::Uuid>, _>(index)
            .map_err(|_| Denial::ServerError)?
        {
            Some(id) => Cell::Uid(id),
            None => Cell::Null,
        },
        Col::Date => match row
            .try_get::<Option<chrono::NaiveDate>, _>(index)
            .map_err(|_| Denial::ServerError)?
        {
            Some(date) => Cell::Date(date),
            None => Cell::Null,
        },
        Col::Json => match row
            .try_get::<Option<serde_json::Value>, _>(index)
            .map_err(|_| Denial::ServerError)?
        {
            Some(value) => Cell::Json(value),
            None => Cell::Null,
        },
        Col::UuidArray => match row
            .try_get::<Option<Vec<uuid::Uuid>>, _>(index)
            .map_err(|_| Denial::ServerError)?
        {
            Some(ids) => Cell::UidArray(ids),
            None => Cell::Null,
        },
        Col::StrArray => match row
            .try_get::<Option<Vec<String>>, _>(index)
            .map_err(|_| Denial::ServerError)?
        {
            Some(items) => Cell::StrArray(items),
            None => Cell::Null,
        },
    };
    Ok(cell)
}

fn shape_row(row: &sqlx::postgres::PgRow, spec: Spec<'_>) -> Result<String, Denial> {
    let mut out = String::from("{");
    for (entry, (key, position, col)) in spec.iter().enumerate() {
        if entry > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(key);
        out.push_str("\":");
        out.push_str(&decode_cell(row, *position, *col)?.render());
    }
    out.push('}');
    Ok(out)
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
