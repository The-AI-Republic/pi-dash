//! Views + favorites handlers (PIDASHCONV-275).
//!
//! Ports `WorkspaceViewViewSet`, `IssueViewViewSet`,
//! `WorkspaceViewIssuesViewSet.list` and `IssueViewFavoriteViewSet`
//! (`apps/api/pi_dash/app/views/view/base.py`) with identical URL paths,
//! status codes and JSON bytes. Routes live in
//! `apps/api/pi_dash/app/urls/views.py:16-65`.
//!
//! Handler notes (all verified against the Python source):
//! - `create` on both viewsets is the DRF default (no `allow_permission`
//!   decorator in Python): session auth only, then `perform_create`
//!   scoping. The response serializes the bare instance, so `is_favorite`
//!   is absent on every create response.
//! - `PUT` on both detail routes is the DRF default `update` (no
//!   locked/owner recheck in Python): full write through the serializer,
//!   response re-read through the *annotated* queryset on project views,
//!   so `is_favorite` is present. `PATCH` is the custom `partial_update`
//!   with the locked/owner rechecks and the un-annotated read, so
//!   `is_favorite` is absent.
//! - Destroy of a missing row calls `.get()` directly: `DoesNotExist`
//!   answers the `ObjectDoesNotExist` branch (`{"error": "The required
//!   object does not exist."}`, 404), not the DRF `{"detail": ...}` body.
//! - The creator bypass everywhere reads `created_by`, not `owned_by`
//!   (`app/permissions/base.py:34`).
//! - `recent_visited_task.delay` is a deferred publish: the retrieve
//!   handlers enqueue into `rust_job_queue` best-effort after the
//!   response is built (the space intake precedent); failures are traced
//!   and the response stands.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::Row;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use pidash_services::app_views_search::{permissions, queries_views};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the 7 views routes. Nothing else: sibling paths stay unmatched
/// and proxy to Django, and every non-owned method on an owned path falls
/// through to Django too (its 401-anon-before-405, DRF metadata and sibling
/// actions live there).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/views/",
            owned(
                axum::routing::get(project_list).post(project_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
            owned(
                axum::routing::get(project_retrieve)
                    .put(project_put)
                    .patch(project_patch)
                    .delete(project_destroy),
                &["GET", "PUT", "PATCH", "DELETE"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/views/",
            owned(
                axum::routing::get(workspace_list).post(workspace_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/views/{pk}/",
            owned(
                axum::routing::get(workspace_retrieve)
                    .put(workspace_put)
                    .patch(workspace_patch)
                    .delete(workspace_destroy),
                &["GET", "PUT", "PATCH", "DELETE"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/issues/",
            owned(axum::routing::get(view_issues_list), &["GET"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/",
            owned(
                axum::routing::get(favorite_list).post(favorite_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/{view_id}/",
            owned(axum::routing::delete(favorite_destroy), &["DELETE"]),
        )
}

/// Django's `<uuid:pk>` / `<uuid:view_id>` converters reject non-UUID
/// segments at routing time (HTML 404, never reaching the view). Axum
/// path captures match any segment, so every detail handler takes the
/// raw [`Request`] last and proxies non-UUID tails to Django,
/// reproducing its routing 404 byte-for-byte in every `DEBUG` setting
/// instead of a handler 400.
///
/// Bodies on the detail write paths parse through the same `Json`
/// extractor the collection routes declare, so rejection bytes are
/// identical on both.
// `Response` is axum's handle type, so boxing it buys no runtime win;
// the crate-wide `Result<_, Response>` helper shape stays as-is.
#[allow(clippy::result_large_err)]
async fn detail_body(state: &AppState, req: axum::extract::Request) -> Result<Value, Response> {
    use axum::extract::FromRequest;
    match axum::Json::<Value>::from_request(req, state).await {
        Ok(axum::Json(body)) => Ok(body),
        Err(rejection) => Err(rejection.into_response()),
    }
}

/// An owned path: listed methods serve from Rust, everything else proxies
/// to Django. OPTIONS proxies too: DRF answers metadata (401 anon / 200
/// authed) where axum would 405. HEAD rides axum's `get` handling like
/// Django's `GET`-backed `HEAD`.
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    methods: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        if methods.contains(&method) {
            continue;
        }
        router = match method {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Exact bytes of the DRF `IsAuthenticated` denial.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (`app/views/base.py`).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// DRF's default `Http404` body (a bare `Http404()` with no message).
pub const NOT_FOUND_DETAIL_BODY: &str = r#"{"detail":"Not found."}"#;
/// `Project.resolve` miss on a non-UUID identifier
/// (`db/models/project.py:213-217`): the `Http404("Project not found")`
/// message survives DRF's `Http404` → `NotFound` conversion verbatim, so
/// the body carries the message, not the default.
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `handle_exception`'s `IntegrityError` branch.
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// DRF `get_object()` miss through a viewset queryset: the `Http404`
/// message carries the model name (`rest_framework/generics.py`).
pub const VIEW_PUT_MISSING_BODY: &str = r#"{"detail":"No IssueView matches the given query."}"#;
/// Serializing a missing view (`IssueViewSerializer(None).data`): DRF
/// answers the serializer's `get_initial()` — every writable field with
/// its initial (strings `""`, everything else `null`), read-only fields
/// (`id`, `is_favorite`, timestamps, FKs, `query`, `access`, `is_locked`)
/// absent — with a 200. Ported verbatim (this is B1's actual body, not
/// JSON `null`).
pub const MISSING_VIEW_BODY: &str = r#"{"deleted_at":null,"name":"","description":"","filters":null,"display_filters":null,"display_properties":null,"rich_filters":null,"sort_order":null,"logo_props":null,"archived_at":null,"created_by":null,"updated_by":null}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` / view-inline body.
    ForbiddenBody(String),
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, DRF `Http404` default body.
    NotFoundDetail,
    /// 400, `{"detail": ...}` (`ParseError`, filter validation).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline, serializer field errors use
    /// their own pre-rendered body via [`Denial::Raw`]).
    BadError(String),
    /// 400, `{"message": ..., "code": ...}` (filter validation).
    BadFilter(String, String),
    /// 500, generic branch.
    ServerError,
    /// A pre-rendered exact body with its status (serializer `errors`
    /// dicts, whose key order DRF fixes).
    Raw(StatusCode, String),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::ForbiddenBody(body) => (StatusCode::FORBIDDEN, body.clone()),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::NotFoundDetail => (StatusCode::NOT_FOUND, NOT_FOUND_DETAIL_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadFilter(message, code) => (
                StatusCode::BAD_REQUEST,
                format!(
                    "{{\"message\":{},\"code\":{}}}",
                    json_string(message),
                    json_string(code)
                ),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
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
            .expect("static denial response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Map a services permission [`permissions::ErrorBody`] to its exact
/// status + body.
fn gate_denial(error: permissions::ErrorBody) -> Denial {
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    Denial::Raw(status, error.body.to_string())
}

/// Map a paginator-kernel error to its HTTP fate (the app_issues
/// precedent): `BadPaginationError` subclasses become `ParseError` 400s;
/// lazy-queryset `ValueError`s and arithmetic errors propagate to the
/// generic 500.
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

/// Map a kernel [`pidash_db::filter::FilterError`] to the exact
/// `{"message", "code"}` 400 the Python backend renders. Same contract as
/// the app_issues precedent; messages that embed runtime values the kernel
/// does not carry fall back to the kernel's own text with the Python code.
fn filter_denial(error: pidash_db::filter::FilterError) -> Denial {
    use pidash_db::filter::FilterError as E;
    let (message, code) = match &error {
        E::InvalidJson => (
            "Invalid JSON for 'filter'. Expected a valid JSON object.".to_owned(),
            "invalid_json",
        ),
        E::InvalidNode => (
            "Each filter node must be a JSON object".to_owned(),
            "invalid_filter_node",
        ),
        E::EmptyNode => (
            "Filter objects must not be empty".to_owned(),
            "empty_filter_object",
        ),
        E::MaxDepthExceeded(max) => (
            format!(
                "Filter nesting is too deep (max {max}); found depth {}",
                max + 1
            ),
            "max_depth_exceeded",
        ),
        E::MultipleOperators => (
            "A filter object cannot contain multiple logical operators at the same level"
                .to_owned(),
            "multiple_logical_operators",
        ),
        E::MixedOperatorAndFields => (error.to_string(), "mixed_operator_and_fields"),
        E::InvalidOperatorChildren => (error.to_string(), "invalid_operator_children"),
        E::InvalidNotChild => (
            "'not' must be a single JSON object".to_owned(),
            "invalid_not_child",
        ),
        E::OperatorInLeaf => (
            "Logical operators cannot appear in a leaf filter object".to_owned(),
            "operator_in_leaf",
        ),
        E::EmptyListValue => (error.to_string(), "empty_list_value"),
        E::InvalidValue => (error.to_string(), "invalid_value_type"),
        E::FilteringNotEnabled => (
            "Filtering is not enabled for this endpoint (missing filterset_class)".to_owned(),
            "filtering_not_enabled",
        ),
        E::InvalidField(field) => (
            format!("Filtering on field '{field}' is not allowed"),
            "invalid_filter_field",
        ),
        E::InvalidLookupValue(field) => (
            format!("Invalid value for lookup on field '{field}'"),
            "invalid_filterset",
        ),
        E::EmptyRangeBounds(_) => return Denial::ServerError,
    };
    Denial::BadFilter(message, code.to_owned())
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated` on
/// `BaseViewSet`): anonymous answers the DRF `NotAuthenticated` body
/// before anything else runs — including before the project-identifier
/// rewrite, so the slug-existence oracle stays closed.
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

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
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

/// `Project.resolve(workspace_slug, value)`: UUIDs pass through (the row
/// check happens in the view body); other identifiers match
/// `UPPER(identifier)` in the workspace; misses raise `Http404`.
async fn resolve_project_id(
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
    row.map(|row| row.0).ok_or(Denial::Raw(
        StatusCode::NOT_FOUND,
        PROJECT_NOT_FOUND_BODY.to_owned(),
    ))
}

/// Load the workspace row by slug. A miss raises `DoesNotExist`
/// (`perform_create`'s `Workspace.objects.get`).
async fn workspace_by_slug(pool: &sqlx::PgPool, slug: &str) -> Result<uuid::Uuid, Denial> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL"#)
            .bind(slug)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::NotFound)
}

/// Membership facts for the services gates, resolved with the same row
/// filters Python uses: active, non-deleted rows scoped to the workspace
/// slug (and project id for the project row).
async fn membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: Option<&uuid::Uuid>,
    user_id: &uuid::Uuid,
) -> Result<permissions::Membership, Denial> {
    let workspace_role: Option<(Option<i16>,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id AND w.deleted_at IS NULL
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `role` is non-nullable; the outer Option is row presence.
    let workspace_role = workspace_role.and_then(|row| row.0);
    let project_role = if let Some(project_id) = project_id {
        let row: Option<(Option<i16>,)> = sqlx::query_as(
            r#"SELECT pm.role FROM project_members pm
               JOIN workspaces w ON w.id = pm.workspace_id AND w.deleted_at IS NULL
               WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
                 AND pm.is_active AND pm.deleted_at IS NULL"#,
        )
        .bind(slug)
        .bind(project_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        row.and_then(|row| row.0)
    } else {
        None
    };
    Ok(permissions::Membership {
        project_role,
        workspace_role,
    })
}

/// The view's creator fact for the `creator=True` bypass:
/// `IssueView.objects.filter(id=pk, created_by=user).exists()`.
async fn is_view_creator(
    pool: &sqlx::PgPool,
    pk: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM issue_views WHERE id = $1 AND created_by_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(pk)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// Parse a UUID path segment for the locked-row reads. A malformed pk
/// never reaches these (the detail routes proxy non-UUID tails to Django
/// at routing time); the fallback 400 mirrors the ORM `ValidationError`
/// body for defense in depth.
fn parse_pk(raw: &str) -> Option<uuid::Uuid> {
    raw.parse::<uuid::Uuid>().ok()
}

// ---------------------------------------------------------------------------
// View row fetching + shaping
// ---------------------------------------------------------------------------

/// Fetch view rows as JSON maps each, via `row_to_json` (the app_issues
/// precedent): numbers, strings, bools, nulls, arrays and objects splice
/// verbatim downstream; only the datetime / float keys are re-rendered by
/// the caller.
async fn fetch_view_rows(
    pool: &sqlx::PgPool,
    inner_sql: &str,
    slug: &str,
    project_id: Option<&uuid::Uuid>,
    user_id: &uuid::Uuid,
) -> Result<Vec<Map<String, Value>>, Denial> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner_sql}) AS __r");
    let mut query = sqlx::query(&sql).bind(slug);
    if let Some(project_id) = project_id {
        query = query.bind(project_id);
    }
    query = query.bind(user_id);
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
        match value {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

/// Render one `row_to_json` datetime: `null` stays `null`, otherwise the
/// RFC 3339 text re-renders in the request zone (DRF renders `DateTimeField`
/// in the activated actor zone; `+00:00` becomes `Z`, microseconds print
/// only when nonzero — the serializer kernel).
fn shape_datetime(value: Option<&Value>, timezone: &Tz) -> String {
    let text = match value {
        Some(Value::String(text)) => text,
        _ => return "null".to_owned(),
    };
    match chrono::DateTime::parse_from_rfc3339(text) {
        Ok(aware) => {
            let rendered = crate::serializer::render_datetime_in(&aware, timezone);
            serde_json::to_string(&rendered).unwrap_or("null".to_owned())
        }
        Err(_) => serde_json::to_string(value.unwrap_or(&Value::Null)).unwrap_or("null".to_owned()),
    }
}

/// Render one `row_to_json` date: `null` stays `null`, otherwise the
/// ISO text passes through quoted (DRF `DateField`).
fn shape_date(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(_)) => {
            serde_json::to_string(value.expect("string")).unwrap_or("null".to_owned())
        }
        _ => "null".to_owned(),
    }
}

/// Fetch one view row by pk for the write paths, shaped without
/// `is_favorite` (the un-annotated read the custom `partial_update` and
/// the DRF default `update`/`create` responses serialize).
async fn fetch_view_by_id(
    pool: &sqlx::PgPool,
    id: &uuid::Uuid,
    timezone: &Tz,
) -> Result<Option<String>, Denial> {
    let cols = pidash_db::app_views_search::models::issue_view::COLUMNS
        .iter()
        .map(|col| format!("v.{col}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM (SELECT {cols} FROM issue_views AS v WHERE v.id = $1 AND v.deleted_at IS NULL) AS __r",
    );
    let row: Option<(String,)> = sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some((text,)) = row else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
    match value {
        Value::Object(map) => Ok(Some(shape_view(&map, None, timezone))),
        _ => Err(Denial::ServerError),
    }
}

/// Shape one view row in live DRF key order (`IssueViewSerializer`:
/// `id`, declared `is_favorite`, then the model fields in Django `_meta`
/// order). `is_favorite` renders only when `Some` (DRF `SkipField` drops
/// the key otherwise — workspace views and create responses).
#[allow(clippy::too_many_arguments)]
fn shape_view(row: &Map<String, Value>, is_favorite: Option<bool>, timezone: &Tz) -> String {
    use std::fmt::Write as _;
    let raw = |key: &str| row.get(key).unwrap_or(&Value::Null);
    let mut out = String::from("{");
    let _ = write!(
        out,
        "\"id\":{}",
        serde_json::to_string(raw("id")).unwrap_or("null".to_owned())
    );
    if let Some(favorite) = is_favorite {
        let _ = write!(out, ",\"is_favorite\":{favorite}");
    }
    let _ = write!(
        out,
        ",\"created_at\":{}",
        shape_datetime(row.get("created_at"), timezone)
    );
    let _ = write!(
        out,
        ",\"updated_at\":{}",
        shape_datetime(row.get("updated_at"), timezone)
    );
    let _ = write!(
        out,
        ",\"deleted_at\":{}",
        shape_datetime(row.get("deleted_at"), timezone)
    );
    let _ = write!(
        out,
        ",\"name\":{}",
        serde_json::to_string(raw("name")).unwrap_or("null".to_owned())
    );
    let _ = write!(
        out,
        ",\"description\":{}",
        serde_json::to_string(raw("description")).unwrap_or("null".to_owned())
    );
    for key in [
        "query",
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
    ] {
        let _ = write!(
            out,
            ",\"{key}\":{}",
            serde_json::to_string(raw(key)).unwrap_or("null".to_owned())
        );
    }
    let access = raw("access").as_i64().unwrap_or(0);
    let _ = write!(out, ",\"access\":{access}");
    let sort_order = raw("sort_order").as_f64().unwrap_or(0.0);
    let _ = write!(
        out,
        ",\"sort_order\":{}",
        crate::paginator::py_float_str(sort_order)
    );
    let _ = write!(
        out,
        ",\"logo_props\":{}",
        serde_json::to_string(raw("logo_props")).unwrap_or("null".to_owned())
    );
    let is_locked = raw("is_locked").as_bool().unwrap_or(false);
    let _ = write!(out, ",\"is_locked\":{is_locked}");
    let _ = write!(
        out,
        ",\"archived_at\":{}",
        shape_datetime(row.get("archived_at"), timezone)
    );
    for key in [
        "created_by",
        "updated_by",
        "workspace",
        "project",
        "owned_by",
    ] {
        let column = match key {
            "created_by" => "created_by_id",
            "updated_by" => "updated_by_id",
            "workspace" => "workspace_id",
            "project" => "project_id",
            _ => "owned_by_id",
        };
        let value = row
            .get(column)
            .or_else(|| row.get(key))
            .unwrap_or(&Value::Null);
        let _ = write!(
            out,
            ",\"{key}\":{}",
            serde_json::to_string(value).unwrap_or("null".to_owned())
        );
    }
    out.push('}');
    out
}

// ---------------------------------------------------------------------------
// DRF `DateTimeField` parse for `archived_at` (PIDASHCONV-783)
// ---------------------------------------------------------------------------

/// Truncate an instant to microsecond precision.
fn trunc_micros(dt: DateTime<Utc>) -> DateTime<Utc> {
    let nanos = dt.timestamp_subsec_nanos();
    dt - chrono::Duration::nanoseconds(i64::from(nanos % 1000))
}

/// Map an `archived_at` parse failure to the DRF field-error body
/// (`serializers/view.py` renders `{"archived_at": [...]}`).
fn archived_at_denial(error: &ParseDatetimeError, timezone: &Tz) -> Denial {
    let message = match error {
        ParseDatetimeError::Invalid => "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
            .to_owned(),
        ParseDatetimeError::Nonexistent => {
            format!("Invalid datetime for the timezone \"{timezone}\".")
        }
        ParseDatetimeError::Overflow => "Datetime value out of range.".to_owned(),
    };
    // The message renders through the JSON encoder like DRF's
    // `ErrorDetail` list: the `Nonexistent` arm quotes the zone name, and
    // a raw `format!` would emit invalid JSON.
    let body = format!(
        "{{\"archived_at\":[{}]}}",
        serde_json::to_string(&message).expect("error string")
    );
    Denial::Raw(StatusCode::BAD_REQUEST, body)
}

/// Django `parse_datetime` (`django/utils/dateparse.py`, Django 4.2.30) over
/// the default `DATETIME_INPUT_FORMATS` (`iso-8601`): CPython 3.12
/// `datetime.fromisoformat` first, the `datetime_re` fallback second.
/// Naive values attach the request time zone (`enforce_timezone` under
/// `USE_TZ`); aware values convert to UTC, and values the conversion pushes
/// outside years `0001..9999` take the `overflow` arm. Returns the instant.
#[derive(Debug, PartialEq, Eq)]
enum ParseDatetimeError {
    Invalid,
    Nonexistent,
    /// DRF `enforce_timezone`: `astimezone` overflow → `Datetime value out
    /// of range.` Serializer path only.
    Overflow,
}

/// A parsed UTC offset with the sign applied to both parts. CPython drops
/// a tz fraction when the whole part is zero, so `micros` is zero whenever
/// `seconds` is zero.
struct ParsedOffset {
    seconds: i64,
    micros: i64,
}

impl ParsedOffset {
    fn total_micros(&self) -> i64 {
        self.seconds * 1_000_000 + self.micros
    }
}

/// Python `re` `\s` (unicode): Rust's `White_Space` plus U+001C-U+001F.
fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Exactly two ASCII digits at `i`.
fn two_digits(b: &[u8], i: usize) -> Option<u32> {
    if i + 2 <= b.len() && b[i].is_ascii_digit() && b[i + 1].is_ascii_digit() {
        Some(u32::from(b[i] - b'0') * 10 + u32::from(b[i + 1] - b'0'))
    } else {
        None
    }
}

fn is_two_digits(b: &[u8], i: usize) -> bool {
    two_digits(b, i).is_some()
}

/// Fraction digits (any length) as microseconds: first six, right-padded.
fn frac_value(digits: &[u8]) -> u32 {
    let mut value: u32 = 0;
    for (n, d) in digits.iter().take(6).enumerate() {
        value += u32::from(d - b'0') * 10_u32.pow(5 - n as u32);
    }
    value
}

/// Starts of the Unicode `Decimal_Number` runs Python `\d` matches on
/// `str`: 68 runs of 10, values 0-9 in order (probed: `re` match over
/// U+0000-U+10FFFF, zero diff vs `unicodedata` Nd; Unicode 15.0.0,
/// CPython 3.12). Neither `char::is_numeric` (wider: Nl/No, e.g. `½`)
/// nor `char::to_digit(10)` (ASCII-only) matches.
const ND_RUN_STARTS: [u32; 68] = [
    0x00030, // U+0030 DIGIT 0-9
    0x00660, // U+0660 ARABIC-INDIC DIGIT 0-9
    0x006F0, // U+06F0 EXTENDED ARABIC-INDIC DIGIT 0-9
    0x007C0, // U+07C0 NKO DIGIT 0-9
    0x00966, // U+0966 DEVANAGARI DIGIT 0-9
    0x009E6, // U+09E6 BENGALI DIGIT 0-9
    0x00A66, // U+0A66 GURMUKHI DIGIT 0-9
    0x00AE6, // U+0AE6 GUJARATI DIGIT 0-9
    0x00B66, // U+0B66 ORIYA DIGIT 0-9
    0x00BE6, // U+0BE6 TAMIL DIGIT 0-9
    0x00C66, // U+0C66 TELUGU DIGIT 0-9
    0x00CE6, // U+0CE6 KANNADA DIGIT 0-9
    0x00D66, // U+0D66 MALAYALAM DIGIT 0-9
    0x00DE6, // U+0DE6 SINHALA LITH DIGIT 0-9
    0x00E50, // U+0E50 THAI DIGIT 0-9
    0x00ED0, // U+0ED0 LAO DIGIT 0-9
    0x00F20, // U+0F20 TIBETAN DIGIT 0-9
    0x01040, // U+1040 MYANMAR DIGIT 0-9
    0x01090, // U+1090 MYANMAR SHAN DIGIT 0-9
    0x017E0, // U+17E0 KHMER DIGIT 0-9
    0x01810, // U+1810 MONGOLIAN DIGIT 0-9
    0x01946, // U+1946 LIMBU DIGIT 0-9
    0x019D0, // U+19D0 NEW TAI LUE DIGIT 0-9
    0x01A80, // U+1A80 TAI THAM HORA DIGIT 0-9
    0x01A90, // U+1A90 TAI THAM THAM DIGIT 0-9
    0x01B50, // U+1B50 BALINESE DIGIT 0-9
    0x01BB0, // U+1BB0 SUNDANESE DIGIT 0-9
    0x01C40, // U+1C40 LEPCHA DIGIT 0-9
    0x01C50, // U+1C50 OL CHIKI DIGIT 0-9
    0x0A620, // U+A620 VAI DIGIT 0-9
    0x0A8D0, // U+A8D0 SAURASHTRA DIGIT 0-9
    0x0A900, // U+A900 KAYAH LI DIGIT 0-9
    0x0A9D0, // U+A9D0 JAVANESE DIGIT 0-9
    0x0A9F0, // U+A9F0 MYANMAR TAI LAING DIGIT 0-9
    0x0AA50, // U+AA50 CHAM DIGIT 0-9
    0x0ABF0, // U+ABF0 MEETEI MAYEK DIGIT 0-9
    0x0FF10, // U+FF10 FULLWIDTH DIGIT 0-9
    0x104A0, // U+104A0 OSMANYA DIGIT 0-9
    0x10D30, // U+10D30 HANIFI ROHINGYA DIGIT 0-9
    0x11066, // U+11066 BRAHMI DIGIT 0-9
    0x110F0, // U+110F0 SORA SOMPENG DIGIT 0-9
    0x11136, // U+11136 CHAKMA DIGIT 0-9
    0x111D0, // U+111D0 SHARADA DIGIT 0-9
    0x112F0, // U+112F0 KHUDAWADI DIGIT 0-9
    0x11450, // U+11450 NEWA DIGIT 0-9
    0x114D0, // U+114D0 TIRHUTA DIGIT 0-9
    0x11650, // U+11650 MODI DIGIT 0-9
    0x116C0, // U+116C0 TAKRI DIGIT 0-9
    0x11730, // U+11730 AHOM DIGIT 0-9
    0x118E0, // U+118E0 WARANG CITI DIGIT 0-9
    0x11950, // U+11950 DIVES AKURU DIGIT 0-9
    0x11C50, // U+11C50 BHAIKSUKI DIGIT 0-9
    0x11D50, // U+11D50 MASARAM GONDI DIGIT 0-9
    0x11DA0, // U+11DA0 GUNJALA GONDI DIGIT 0-9
    0x11F50, // U+11F50 KAWI DIGIT 0-9
    0x16A60, // U+16A60 MRO DIGIT 0-9
    0x16AC0, // U+16AC0 TANGSA DIGIT 0-9
    0x16B50, // U+16B50 PAHAWH HMONG DIGIT 0-9
    0x1D7CE, // U+1D7CE MATHEMATICAL BOLD DIGIT 0-9
    0x1D7D8, // U+1D7D8 MATHEMATICAL DOUBLE-STRUCK DIGIT 0-9
    0x1D7E2, // U+1D7E2 MATHEMATICAL SANS-SERIF DIGIT 0-9
    0x1D7EC, // U+1D7EC MATHEMATICAL SANS-SERIF BOLD DIGIT 0-9
    0x1D7F6, // U+1D7F6 MATHEMATICAL MONOSPACE DIGIT 0-9
    0x1E140, // U+1E140 NYIAKENG PUACHUE HMONG DIGIT 0-9
    0x1E2F0, // U+1E2F0 WANCHO DIGIT 0-9
    0x1E4F0, // U+1E4F0 NAG MUNDARI DIGIT 0-9
    0x1E950, // U+1E950 ADLAM DIGIT 0-9
    0x1FBF0, // U+1FBF0 SEGMENTED DIGIT 0-9
];

/// Python `\d` decimal value of `c` (`unicodedata.decimal`).
fn nd_value(c: char) -> Option<u32> {
    let n = c as u32;
    let i = ND_RUN_STARTS.partition_point(|&s| s <= n);
    if i == 0 {
        return None;
    }
    let v = n - ND_RUN_STARTS[i - 1];
    if v < 10 {
        Some(v)
    } else {
        None
    }
}

/// One `Nd` char at byte index `i`: its decimal value plus the next
/// index. `None` past the end, off a char boundary, or not a digit.
fn nd_char(text: &str, i: usize) -> Option<(u32, usize)> {
    let c = text.get(i..)?.chars().next()?;
    nd_value(c).map(|v| (v, i + c.len_utf8()))
}

/// Exactly two `Nd` chars at `i` (regex-arm `two_digits`).
fn nd_two(text: &str, i: usize) -> Option<(u32, usize)> {
    let (a, j) = nd_char(text, i)?;
    let (b, k) = nd_char(text, j)?;
    Some((a * 10 + b, k))
}

/// One or two `Nd` chars at `i`, plus the next index.
fn nd_one_two(text: &str, i: usize) -> Option<(u32, usize)> {
    let (a, j) = nd_char(text, i)?;
    match nd_char(text, j) {
        Some((b, k)) => Some((a * 10 + b, k)),
        None => Some((a, j)),
    }
}

/// Fraction `Nd` chars as microseconds: first six values, right-padded
/// (the `ljust(6, "0")` in `parse_datetime`). The slice holds only `Nd`
/// chars (scanned with `nd_char` by the caller).
fn nd_frac_value(digits: &str) -> u32 {
    let mut value: u32 = 0;
    for (n, c) in digits.chars().take(6).enumerate() {
        if let Some(v) = nd_value(c) {
            value += v * 10_u32.pow(5 - n as u32);
        }
    }
    value
}

/// Four-digit year `0001..9999` at the start.
fn parse_iso_year(b: &[u8]) -> Option<i32> {
    if b.len() < 4 || !b[0..4].iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year = i32::from(b[0] - b'0') * 1000
        + i32::from(b[1] - b'0') * 100
        + i32::from(b[2] - b'0') * 10
        + i32::from(b[3] - b'0');
    if year < 1 {
        None
    } else {
        Some(year)
    }
}

/// Week dates resolving outside years `0001..9999` are rejected; a missing
/// day is Monday.
fn resolve_iso_week(year: i32, week: u32, day: Option<u32>) -> Option<NaiveDate> {
    let day = day.unwrap_or(1);
    if !(1..=7).contains(&day) {
        return None;
    }
    let weekday = Weekday::try_from((day - 1) as u8).ok()?;
    let date = NaiveDate::from_isoywd_opt(year, week, weekday)?;
    if date.year() < 1 || date.year() > 9999 {
        return None;
    }
    Some(date)
}

/// After a date: end of input is midnight, else one separator char of any
/// kind, then the time and optional zone.
fn parse_rest_after_date(
    text: &str,
    b: &[u8],
    date: NaiveDate,
    mut i: usize,
) -> Option<(NaiveDateTime, Option<ParsedOffset>)> {
    if i == b.len() {
        return Some((date.and_hms_opt(0, 0, 0)?, None));
    }
    let sep = text[i..].chars().next()?;
    i += sep.len_utf8();
    let (time, j) = parse_iso_time(b, text, i)?;
    let naive = NaiveDateTime::new(date, time);
    if j == b.len() {
        return Some((naive, None));
    }
    let (offset, k) = parse_iso_tz(b, j)?;
    if k != b.len() {
        return None;
    }
    Some((naive, Some(offset)))
}

/// `HH[MM[SS]]` / `HH:MM[:SS]` (all two-digit, ranges checked) plus the
/// optional fraction: one ASCII separator char (never a tz starter), then
/// any number of ASCII digits. Digits need `[.,]` — or `:` after extended
/// seconds; any other separator must be empty with a tz after it
/// (`HH:`/`HH:MM:`/`HHMM:`/`HHMMSS:` all take `+tz`). A `:` not followed by
/// two digits is that separator, not a component colon. Basic seconds
/// instead take a 2+ digit run itself as the fraction, separator-less.
/// Returns the time and how far the time scan ran (end of input or a tz
/// starter).
fn parse_iso_time(b: &[u8], text: &str, i: usize) -> Option<(NaiveTime, usize)> {
    let hh = two_digits(b, i)?;
    if hh > 23 {
        return None;
    }
    let mut j = i + 2;
    let (mm, ss, basic, has_ss, allow_colon_frac) =
        if b.get(j) == Some(&b':') && is_two_digits(b, j + 1) {
            let mm = two_digits(b, j + 1)?;
            if mm > 59 {
                return None;
            }
            j += 3;
            if b.get(j) == Some(&b':') && is_two_digits(b, j + 1) {
                let ss = two_digits(b, j + 1)?;
                if ss > 59 {
                    return None;
                }
                j += 3;
                (mm, ss, false, true, true)
            } else {
                // Extended minutes with basic seconds is not a form; a lone
                // `:` is the fraction separator below.
                if is_two_digits(b, j) {
                    return None;
                }
                (mm, 0, false, false, false)
            }
        } else if b.get(j) == Some(&b':') {
            // Hour-only `HH:` — the colon is the fraction separator below.
            (0, 0, false, false, false)
        } else {
            let mut mm = 0;
            let mut ss = 0;
            let mut has_ss = false;
            if let Some(v) = two_digits(b, j) {
                if v > 59 {
                    return None;
                }
                mm = v;
                j += 2;
                if let Some(v) = two_digits(b, j) {
                    if v > 59 {
                        return None;
                    }
                    ss = v;
                    j += 2;
                    has_ss = true;
                }
            }
            (mm, ss, true, has_ss, false)
        };
    // A 2+ digit run is only a fraction after basic seconds (the run
    // itself, no separator); anywhere else digits cannot start one.
    let run = digit_run_len(b, j);
    if run >= 2 {
        if !(basic && has_ss) {
            return None;
        }
        let micros = frac_value(&b[j..j + run]);
        let k = end_or_tz(text, b, j + run)?;
        let time = NaiveTime::from_hms_micro_opt(hh, mm, ss, micros)?;
        return Some((time, k));
    }
    let (micros, k) = parse_time_frac(text, b, j, allow_colon_frac)?;
    let time = NaiveTime::from_hms_micro_opt(hh, mm, ss, micros)?;
    Some((time, k))
}

/// Length of the ASCII digit run at `j`.
fn digit_run_len(b: &[u8], j: usize) -> usize {
    let mut k = j;
    while k < b.len() && b[k].is_ascii_digit() {
        k += 1;
    }
    k - j
}

/// End of input or a tz starter at `k` (the fraction before it may be
/// empty); anything else fails the arm.
fn end_or_tz(text: &str, b: &[u8], k: usize) -> Option<usize> {
    if k == b.len() {
        return Some(k);
    }
    match text[k..].chars().next() {
        Some(c) if c == 'Z' || c == '+' || c == '-' => Some(k),
        _ => None,
    }
}

fn parse_time_frac(text: &str, b: &[u8], j: usize, allow_colon: bool) -> Option<(u32, usize)> {
    let Some(ch) = text[j..].chars().next() else {
        return Some((0, j));
    };
    if ch == 'Z' || ch == '+' || ch == '-' {
        return Some((0, j));
    }
    if !ch.is_ascii() {
        return None;
    }
    let mut k = j + ch.len_utf8();
    let start = k;
    while k < b.len() && b[k].is_ascii_digit() {
        k += 1;
    }
    if k != start && ch != '.' && ch != ',' && !(ch == ':' && allow_colon) {
        return None;
    }
    if k == b.len() {
        if k == start {
            return None;
        }
        return Some((frac_value(&b[start..k]), k));
    }
    match text[k..].chars().next() {
        Some(c) if c == 'Z' || c == '+' || c == '-' => Some((frac_value(&b[start..k]), k)),
        _ => None,
    }
}

/// `Z` or `±HH[MM[SS]]` / `±HH:MM[:SS]` (two-digit components, no range
/// check beyond the 24h total) plus an optional `[.,:]` fraction of any
/// length. Returns the offset and how far the tz scan ran.
fn parse_iso_tz(b: &[u8], j: usize) -> Option<(ParsedOffset, usize)> {
    if b.get(j) == Some(&b'Z') {
        return Some((
            ParsedOffset {
                seconds: 0,
                micros: 0,
            },
            j + 1,
        ));
    }
    let sign = match b.get(j) {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return None,
    };
    let hh = i64::from(two_digits(b, j + 1)?);
    let mut k = j + 3;
    let (mm, ss) = if b.get(k) == Some(&b':') {
        let mm = i64::from(two_digits(b, k + 1)?);
        k += 3;
        if b.get(k) == Some(&b':') {
            let ss = i64::from(two_digits(b, k + 1)?);
            k += 3;
            (mm, ss)
        } else {
            if is_two_digits(b, k) {
                return None;
            }
            (mm, 0)
        }
    } else {
        let mut mm = 0;
        let mut ss = 0;
        if let Some(v) = two_digits(b, k) {
            mm = i64::from(v);
            k += 2;
            if let Some(v) = two_digits(b, k) {
                ss = i64::from(v);
                k += 2;
            }
        }
        if b.get(k) == Some(&b':') {
            return None;
        }
        (mm, ss)
    };
    let mut frac_us: i64 = 0;
    if k < b.len() && matches!(b[k], b'.' | b',' | b':') {
        let start = k + 1;
        let mut e = start;
        while e < b.len() && b[e].is_ascii_digit() {
            e += 1;
        }
        if e == start {
            return None;
        }
        frac_us = i64::from(frac_value(&b[start..e]));
        k = e;
    }
    let whole = hh * 3600 + mm * 60 + ss;
    if whole >= 86400 {
        return None;
    }
    if whole == 0 {
        Some((
            ParsedOffset {
                seconds: 0,
                micros: 0,
            },
            k,
        ))
    } else {
        Some((
            ParsedOffset {
                seconds: sign * whole,
                micros: sign * frac_us,
            },
            k,
        ))
    }
}

/// CPython `datetime.fromisoformat` (3.12): ASCII dates (calendar or week,
/// extended or basic), one separator char of any kind, ASCII times
/// (extended or basic, fractions welcome), ASCII zones.
fn parse_fromisoformat_arm(text: &str) -> Option<(NaiveDateTime, Option<ParsedOffset>)> {
    let b = text.as_bytes();
    let year = parse_iso_year(b)?;
    match b.get(4).copied() {
        // Extended week `YYYY-Www`: `-D` counts when no digit follows it.
        Some(b'-') if b.get(5) == Some(&b'W') => {
            if b.len() < 8 || !b[6].is_ascii_digit() || !b[7].is_ascii_digit() {
                return None;
            }
            let week = u32::from(b[6] - b'0') * 10 + u32::from(b[7] - b'0');
            let (day, i) = if b.get(8) == Some(&b'-')
                && b.get(9).is_some_and(|c| c.is_ascii_digit())
                && !b.get(10).is_some_and(|c| c.is_ascii_digit())
            {
                (Some(u32::from(b[9] - b'0')), 10)
            } else {
                (None, 8)
            };
            let date = resolve_iso_week(year, week, day)?;
            parse_rest_after_date(text, b, date, i)
        }
        // Extended calendar `YYYY-MM-DD` (fixed length).
        Some(b'-') => {
            if b.len() < 10
                || !b[5].is_ascii_digit()
                || !b[6].is_ascii_digit()
                || b[7] != b'-'
                || !b[8].is_ascii_digit()
                || !b[9].is_ascii_digit()
            {
                return None;
            }
            let month = u32::from(b[5] - b'0') * 10 + u32::from(b[6] - b'0');
            let day = u32::from(b[8] - b'0') * 10 + u32::from(b[9] - b'0');
            let date = NaiveDate::from_ymd_opt(year, month, day)?;
            parse_rest_after_date(text, b, date, 10)
        }
        // Basic week `YYYYWww[D]`: the day split goes first, Monday on
        // any failure (a digit separator reads either way).
        Some(b'W') => {
            if b.len() < 7 || !b[5].is_ascii_digit() || !b[6].is_ascii_digit() {
                return None;
            }
            let week = u32::from(b[5] - b'0') * 10 + u32::from(b[6] - b'0');
            if b.get(7).is_some_and(|c| c.is_ascii_digit()) {
                let day = u32::from(b[7] - b'0');
                if let Some(date) = resolve_iso_week(year, week, Some(day)) {
                    if let Some(parsed) = parse_rest_after_date(text, b, date, 8) {
                        return Some(parsed);
                    }
                }
            }
            let date = resolve_iso_week(year, week, None)?;
            parse_rest_after_date(text, b, date, 7)
        }
        // Basic calendar `YYYYMMDD`, not followed by a digit.
        Some(c) if c.is_ascii_digit() => {
            if b.len() < 8 || !b[4..8].iter().all(|c| c.is_ascii_digit()) {
                return None;
            }
            if b.len() > 8 && b[8].is_ascii_digit() {
                return None;
            }
            let month = u32::from(b[4] - b'0') * 10 + u32::from(b[5] - b'0');
            let day = u32::from(b[6] - b'0') * 10 + u32::from(b[7] - b'0');
            let date = NaiveDate::from_ymd_opt(year, month, day)?;
            parse_rest_after_date(text, b, date, 8)
        }
        _ => None,
    }
}

/// `YYYY-M-D` (1-2 digit month/day, Unicode decimal digits like the
/// regex `\d`) plus the next index.
fn parse_dashed_date(text: &str, b: &[u8]) -> Option<(i32, u32, u32, usize)> {
    let (d0, i0) = nd_char(text, 0)?;
    let (d1, i1) = nd_char(text, i0)?;
    let (d2, i2) = nd_char(text, i1)?;
    let (d3, i3) = nd_char(text, i2)?;
    if b.get(i3) != Some(&b'-') {
        return None;
    }
    let year = d0 as i32 * 1000 + d1 as i32 * 100 + d2 as i32 * 10 + d3 as i32;
    if year < 1 {
        return None;
    }
    let (month, i) = nd_one_two(text, i3 + 1)?;
    if b.get(i) != Some(&b'-') {
        return None;
    }
    let (day, j) = nd_one_two(text, i + 1)?;
    Some((year, month, day, j))
}

/// The `datetime_re` fallback: non-padded `YYYY-M-D[T ]H:M[:S[.f]]`
/// (Unicode decimal digits like the regex `\d`), Python whitespace
/// before an optional `Z`/short offset, then end or one `\n`.
fn parse_regex_arm(text: &str) -> Option<(NaiveDateTime, Option<ParsedOffset>)> {
    let b = text.as_bytes();
    let (year, month, day, j) = parse_dashed_date(text, b)?;
    if b.get(j) != Some(&b'T') && b.get(j) != Some(&b' ') {
        return None;
    }
    let (hh, k) = nd_one_two(text, j + 1)?;
    if b.get(k) != Some(&b':') {
        return None;
    }
    let (mm, mut l) = nd_one_two(text, k + 1)?;
    let mut ss = 0;
    let mut micros = 0;
    if b.get(l) == Some(&b':') {
        let (s, n) = nd_one_two(text, l + 1)?;
        ss = s;
        l = n;
        if b.get(l) == Some(&b'.') || b.get(l) == Some(&b',') {
            let start = l + 1;
            let mut e = start;
            let mut count = 0;
            while let Some((_, next)) = nd_char(text, e) {
                e = next;
                count += 1;
            }
            if count == 0 || count > 12 {
                return None;
            }
            micros = nd_frac_value(&text[start..e]);
            l = e;
        }
    }
    // `\s*`: all Python whitespace, then an optional short zone.
    let mut p = l;
    while p < b.len() {
        let ch = text[p..].chars().next()?;
        if !is_py_space(ch) {
            break;
        }
        p += ch.len_utf8();
    }
    let mut offset: Option<ParsedOffset> = None;
    if p < b.len() {
        if b[p] == b'Z' {
            offset = Some(ParsedOffset {
                seconds: 0,
                micros: 0,
            });
            p += 1;
        } else if b[p] == b'+' || b[p] == b'-' {
            let sign: i64 = if b[p] == b'+' { 1 } else { -1 };
            let (oh, q) = nd_two(text, p + 1)?;
            let oh = i64::from(oh);
            p = q;
            let mut om = 0;
            if b.get(p) == Some(&b':') {
                let (v, q) = nd_two(text, p + 1)?;
                om = i64::from(v);
                p = q;
            } else if let Some((v, q)) = nd_two(text, p) {
                om = i64::from(v);
                p = q;
            }
            let total_min = sign * (oh * 60 + om);
            if total_min.abs() > 1439 {
                return None;
            }
            offset = Some(ParsedOffset {
                seconds: total_min * 60,
                micros: 0,
            });
        }
    }
    // `$`: end of input, or one trailing newline.
    if p != b.len() && !(p + 1 == b.len() && b[p] == b'\n') {
        return None;
    }
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let time = NaiveTime::from_hms_micro_opt(hh, mm, ss, micros)?;
    Some((NaiveDateTime::new(date, time), offset))
}

/// Both `parse_datetime` arms: `fromisoformat`, then `datetime_re`.
fn parse_iso8601_core(text: &str) -> Option<(NaiveDateTime, Option<ParsedOffset>)> {
    parse_fromisoformat_arm(text).or_else(|| parse_regex_arm(text))
}

/// DRF `DateTimeField.to_internal_value` over `DATETIME_INPUT_FORMATS =
/// ['iso-8601']`: `parse_datetime`, then the `strptime(value, 'iso-8601')`
/// fallback, then `enforce_timezone`.
fn parse_django_datetime(text: &str, tz: &Tz) -> Result<DateTime<Utc>, ParseDatetimeError> {
    let (naive, offset) = parse_iso8601_core(text)
        .or_else(|| {
            // `strptime` compiles the literal format with `re.IGNORECASE`,
            // so the input `iso-8601` in any letter case yields naive
            // 1900-01-01. Exact match: padding fails on both sides (probed).
            // Python's fold also accepts İ/ı/ſ letter variants; the port is
            // ASCII-only (same family as the 765 unicode gap).
            text.eq_ignore_ascii_case("iso-8601").then(|| {
                let naive = NaiveDate::from_ymd_opt(1900, 1, 1)
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
                    .expect("1900-01-01 valid");
                (naive, None)
            })
        })
        .ok_or(ParseDatetimeError::Invalid)?;
    match offset {
        Some(off) => {
            let utc = naive.and_utc() - chrono::Duration::microseconds(off.total_micros());
            // DRF `enforce_timezone` converts into the field zone, and
            // `astimezone` overflow there is the `overflow` 400.
            if !(1..=9999).contains(&utc.with_timezone(tz).date_naive().year()) {
                return Err(ParseDatetimeError::Overflow);
            }
            Ok(trunc_micros(utc))
        }
        None => match tz.from_local_datetime(&naive) {
            chrono::LocalResult::Single(aware) => Ok(trunc_micros(aware.with_timezone(&Utc))),
            chrono::LocalResult::Ambiguous(early, _) => Ok(trunc_micros(early.with_timezone(&Utc))),
            chrono::LocalResult::None => Err(ParseDatetimeError::Nonexistent),
        },
    }
}

// ---------------------------------------------------------------------------
// Write kernels (create / update shared by both scopes)
// ---------------------------------------------------------------------------

/// Validated write fields for a view create/update body.
struct ViewWrite {
    name: Option<String>,
    description: Option<Value>,
    filters: Option<Value>,
    display_filters: Option<Value>,
    display_properties: Option<Value>,
    rich_filters: Option<Value>,
    logo_props: Option<Value>,
    archived_at: Option<Value>,
}

/// DRF field errors for `name`: required, non-blank, max 255. DRF renders
/// `serializer.errors` as `{"name": [...]}` with `ErrorDetail` strings.
fn validate_name(body: &Map<String, Value>, partial: bool) -> Result<Option<String>, Denial> {
    match body.get("name") {
        None if partial => Ok(None),
        None => Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            r#"{"name":["This field is required."]}"#.to_owned(),
        )),
        Some(Value::Null) => Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            r#"{"name":["This field may not be null."]}"#.to_owned(),
        )),
        Some(Value::String(name)) if name.is_empty() => Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            r#"{"name":["This field may not be blank."]}"#.to_owned(),
        )),
        Some(Value::String(name))
            if name.chars().count()
                > pidash_db::app_views_search::models::issue_view::NAME_MAX_LENGTH =>
        {
            Err(Denial::Raw(
                StatusCode::BAD_REQUEST,
                r#"{"name":["Ensure this field has no more than 255 characters."]}"#.to_owned(),
            ))
        }
        Some(Value::String(name)) => Ok(Some(name.clone())),
        Some(_) => Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            r#"{"name":["Not a valid string."]}"#.to_owned(),
        )),
    }
}

/// Split a create/update body into validated fields. Read-only serializer
/// fields (`workspace`, `project`, `query`, `owned_by`, `access`,
/// `is_locked`, `is_favorite`) and unknown keys are accepted and ignored,
/// exactly like DRF's `to_internal_value`.
fn split_write(body: &Map<String, Value>, partial: bool) -> Result<ViewWrite, Denial> {
    Ok(ViewWrite {
        name: validate_name(body, partial)?,
        description: body.get("description").cloned(),
        filters: body.get("filters").cloned(),
        display_filters: body.get("display_filters").cloned(),
        display_properties: body.get("display_properties").cloned(),
        rich_filters: body.get("rich_filters").cloned(),
        logo_props: body.get("logo_props").cloned(),
        archived_at: body.get("archived_at").cloned(),
    })
}

/// Reject an explicit JSON `null` for a non-nullable JSON column: DRF's
/// `JSONField` (`allow_null=False`) answers `{"<field>": ["This field may
/// not be null."]}` at validation, before any query computation runs.
fn reject_null_json(body: &Map<String, Value>) -> Result<(), Denial> {
    for field in [
        "filters",
        "display_filters",
        "display_properties",
        "rich_filters",
        "logo_props",
    ] {
        if matches!(body.get(field), Some(Value::Null)) {
            return Err(Denial::Raw(
                StatusCode::BAD_REQUEST,
                format!("{{\"{field}\":[\"This field may not be null.\"]}}"),
            ));
        }
    }
    Ok(())
}

/// Compute the stored `query` for a create: `issue_filters(filters,
/// "POST")` when filters are truthy, else `{}` (`view.py:71-77` via
/// [`pidash_services::app_views_search::serializers::resolve_create_query`]).
/// The `IssueView.save()` recompute (`db/models/view.py:79-81`) agrees on
/// both branches, so one computation serves.
fn create_query(filters: Option<&Value>) -> Result<Value, Denial> {
    use pidash_services::app_views_search::serializers::is_truthy_json;
    match filters {
        // Missing or falsy: the `bool(query_params)` gate skips the
        // mapping (note falsy non-dicts never reach the key loop).
        None => Ok(empty_query()),
        Some(value) if !is_truthy_json(value) => Ok(empty_query()),
        Some(Value::Object(_)) => stored_lookup_query(filters),
        // Truthy non-dicts run the key loop over a non-mapping: `in`
        // either matches nothing (`{}`) or the lookup crashes (500).
        Some(_) => lookup_non_dict(filters),
    }
}

/// Compute the stored `query` for an update (`view.py:79-86` plus the
/// `IssueView.save()` recompute at `db/models/view.py:79-81`, which runs
/// on every save and wins when the body omits `filters`):
/// - `filters` provided: the unconditional PATCH line maps the new value
///   (dicts through the kernel, non-dicts through the key-loop rule —
///   a hit crashes before `save()` ever runs);
/// - `filters` absent: the PATCH line maps `{}` but `save()` then
///   recomputes from the row's own (old) filters, restoring the old
///   mapping — so the effective value maps the old filters.
///
/// A missing key is NOT the `{}` default here (that reading belongs to
/// the fixture prose, not the code): the row's filters fill in.
fn update_query(filters_new: Option<&Value>, filters_old: Option<&Value>) -> Result<Value, Denial> {
    let effective = filters_new.or(filters_old);
    match effective {
        None => Ok(empty_query()),
        Some(Value::Object(_)) => stored_lookup_query(effective),
        Some(_) => lookup_non_dict(effective),
    }
}

fn empty_query() -> Value {
    Value::Object(serde_json::Map::new())
}

/// The key loop `issue_filters(params, METHOD)` over a non-mapping
/// value: `key in params` is a substring test (strings) or a membership
/// test (lists) — a hit runs the filter func, which crashes on the
/// non-mapping (`AttributeError`, generic 500); a miss stores `{}`.
/// Anything else (`TypeError` on `in`) is the generic 500 too.
fn lookup_non_dict(params: Option<&Value>) -> Result<Value, Denial> {
    use pidash_db::issue_filters::ISSUE_FILTER_KEYS;
    let hit = match params {
        Some(Value::String(text)) => ISSUE_FILTER_KEYS.iter().any(|key| text.contains(*key)),
        Some(Value::Array(items)) => items.iter().any(|item| match item {
            Value::String(text) => ISSUE_FILTER_KEYS.iter().any(|key| key == text),
            _ => false,
        }),
        _ => return Err(Denial::ServerError),
    };
    if hit {
        return Err(Denial::ServerError);
    }
    Ok(empty_query())
}

/// Render one compiled `issue_filters` predicate set back to the Django
/// lookup-dict JSON the `query` column stores. Predicate names are already
/// Django lookup strings; values mirror the Python objects
/// (`DjangoJSONEncoder`: UUIDs and dates as strings).
fn predicates_to_query(predicates: &[(String, pidash_db::issue_filters::FilterValue)]) -> Value {
    use pidash_db::issue_filters::FilterValue as F;
    let mut map = serde_json::Map::new();
    for (name, value) in predicates {
        let json = match value {
            F::Uuids(ids) => {
                Value::Array(ids.iter().map(|id| Value::String(id.to_string())).collect())
            }
            F::Strings(items) => Value::Array(
                items
                    .iter()
                    .map(|item| Value::String(item.clone()))
                    .collect(),
            ),
            F::Text(text) => Value::String(text.clone()),
            F::Flag(flag) => Value::Bool(*flag),
            F::Day(day) => Value::String(day.format("%Y-%m-%d").to_string()),
            F::Null => Value::Null,
        };
        map.insert(name.clone(), json);
    }
    Value::Object(map)
}

/// `issue_filters(filters_dict, method)` over a JSON-object filters value:
/// JSON arrays of scalars become id-list params, strings become text
/// params; `null` values are dropped (Python's falsy check skips them).
/// Unrecognized keys are ignored by the kernel, like the Python
/// `ISSUE_FILTER` loop. Kernel failures (relative-date overflow) escape
/// as 500s in Python (`TypeError`/`OverflowError` uncaught), mapped here
/// to the generic 500.
/// POST and PATCH share the `else` branches in every filter func, so one
/// kernel serves both write paths. Takes the `Option` directly: `None`
/// is the validated `{}` default for a missing key.
fn stored_lookup_query(filters: Option<&Value>) -> Result<Value, Denial> {
    use pidash_db::issue_filters::{issue_filters_post, PostVal};
    let object = match filters {
        Some(Value::Object(object)) => object,
        // The validated `{}` default for a missing key maps to no
        // predicates; explicit `null` is rejected at validation above.
        _ => return Ok(empty_query()),
    };
    let mut params: HashMap<String, PostVal> = HashMap::new();
    for (key, value) in object {
        match value {
            Value::Null => {}
            Value::Array(items) => {
                let mut list = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        Value::String(text) => list.push(text.clone()),
                        Value::Null => {}
                        other => {
                            list.push(serde_json::to_string(other).unwrap_or("null".to_owned()))
                        }
                    }
                }
                params.insert(key.clone(), PostVal::List(list));
            }
            Value::String(text) => {
                params.insert(key.clone(), PostVal::Text(text.clone()));
            }
            // Scalars ride as their JSON text; Python stores exotic values
            // verbatim and no client sends them here. Dates resolve against
            // the UTC day, like `timezone.now().date()` (`now()` is UTC).
            _ => {}
        }
    }
    let today = chrono::Utc::now().date_naive();
    match issue_filters_post(&params, "", today) {
        Ok(compiled) => Ok(predicates_to_query(compiled.predicates())),
        // Relative-date overflow escapes as an uncaught 500 in Python
        // (`TypeError`/`OverflowError`); same here.
        Err(_) => Err(Denial::ServerError),
    }
}

/// Next `sort_order` for a new sibling (`db/models/view.py:83-95`):
/// `max + 10000` in scope, else the field default.
async fn next_sort_order(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    project_id: Option<&uuid::Uuid>,
) -> Result<f64, Denial> {
    use pidash_db::app_views_search::models::issue_view;
    let largest: Option<(Option<f64>,)> = if let Some(project_id) = project_id {
        sqlx::query_as(
            r#"SELECT MAX(sort_order) FROM issue_views WHERE project_id = $1 AND deleted_at IS NULL"#,
        )
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    } else {
        sqlx::query_as(
            r#"SELECT MAX(sort_order) FROM issue_views WHERE workspace_id = $1 AND project_id IS NULL AND deleted_at IS NULL"#,
        )
        .bind(workspace_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    };
    let largest = largest.and_then(|row| row.0);
    Ok(issue_view::next_sort_order(largest).unwrap_or(issue_view::DEFAULT_SORT_ORDER))
}

/// Insert a view row and return its id. Mirrors `IssueView.objects.create`
/// through `WorkspaceBaseModel.save` + `BaseModel.save` + `IssueView.save`:
/// workspace falls back from the project, `created_by` is the request
/// user, `updated_by` stays null, `query` recomputes from filters, and the
/// sibling-maximum `sort_order` applies on add. `archived_at` parses
/// through the DRF `DateTimeField` grammar (PIDASHCONV-783). Read-only
/// body keys never reach the row.
#[allow(clippy::too_many_arguments)]
async fn insert_view(
    pool: &sqlx::PgPool,
    workspace_id: &uuid::Uuid,
    project_id: Option<&uuid::Uuid>,
    user_id: &uuid::Uuid,
    timezone: &Tz,
    write: &ViewWrite,
    query: &Value,
    sort_order: f64,
) -> Result<uuid::Uuid, Denial> {
    use pidash_db::app_views_search::models::issue_view;
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let name = write.name.clone().ok_or(Denial::Raw(
        StatusCode::BAD_REQUEST,
        r#"{"name":["This field is required."]}"#.to_owned(),
    ))?;
    let description = match &write.description {
        None => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) => {
            return Err(Denial::Raw(
                StatusCode::BAD_REQUEST,
                r#"{"description":["This field may not be null."]}"#.to_owned(),
            ));
        }
        Some(_) => {
            return Err(Denial::Raw(
                StatusCode::BAD_REQUEST,
                r#"{"description":["Not a valid string."]}"#.to_owned(),
            ));
        }
    };
    // `archived_at`: writable DRF `DateTimeField` (`null=True`), so a
    // missing or explicit-null value stores NULL; strings parse through
    // the serializer grammar (after `description`, preserving the
    // field-error precedence), and any other JSON shape is the
    // wrong-format 400.
    let archived_at = match &write.archived_at {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(
            parse_django_datetime(text, timezone)
                .map_err(|error| archived_at_denial(&error, timezone))?,
        ),
        Some(_) => {
            return Err(archived_at_denial(&ParseDatetimeError::Invalid, timezone));
        }
    };
    let filters = write
        .filters
        .clone()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let display_filters = write
        .display_filters
        .clone()
        .unwrap_or_else(issue_view::default_display_filters);
    let display_properties = write
        .display_properties
        .clone()
        .unwrap_or_else(issue_view::default_display_properties);
    let rich_filters = write
        .rich_filters
        .clone()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let logo_props = write
        .logo_props
        .clone()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let query_text = serde_json::to_string(query).map_err(|_| Denial::ServerError)?;
    let filters_text = serde_json::to_string(&filters).map_err(|_| Denial::ServerError)?;
    let display_filters_text =
        serde_json::to_string(&display_filters).map_err(|_| Denial::ServerError)?;
    let display_properties_text =
        serde_json::to_string(&display_properties).map_err(|_| Denial::ServerError)?;
    let rich_filters_text =
        serde_json::to_string(&rich_filters).map_err(|_| Denial::ServerError)?;
    let logo_props_text = serde_json::to_string(&logo_props).map_err(|_| Denial::ServerError)?;
    let result = sqlx::query(
        r#"INSERT INTO issue_views
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            workspace_id, project_id, name, description, query, filters,
            display_filters, display_properties, rich_filters, access, sort_order,
            logo_props, owned_by_id, is_locked, archived_at)
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8,
                   $9::jsonb, $10::jsonb, $11::jsonb, $12::jsonb, $13::jsonb,
                   $14, $15, $16::jsonb, $17, FALSE, $18)"#,
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(&name)
    .bind(&description)
    .bind(&query_text)
    .bind(&filters_text)
    .bind(&display_filters_text)
    .bind(&display_properties_text)
    .bind(&rich_filters_text)
    .bind(issue_view::DEFAULT_ACCESS)
    .bind(sort_order)
    .bind(&logo_props_text)
    .bind(user_id)
    .bind(archived_at)
    .execute(pool)
    .await;
    match result {
        Ok(_) => Ok(id),
        Err(error) => {
            if let sqlx::Error::Database(db_error) = &error {
                // FK miss (unknown project) or unique violation: DRF's
                // `IntegrityError` branch.
                if db_error.code().is_some() {
                    return Err(Denial::Raw(
                        StatusCode::BAD_REQUEST,
                        INVALID_PAYLOAD_BODY.to_owned(),
                    ));
                }
            }
            Err(Denial::ServerError)
        }
    }
}

/// Apply a full (PUT) or partial (PATCH) update to a locked row, then
/// return the re-read body. `query` always recomputes through the PATCH
/// mapping (B3); `updated_by` stamps the request user (`BaseModel.save` on
/// an existing row). Read-only keys never reach the row.
async fn apply_view_update(
    pool: &sqlx::PgPool,
    id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    timezone: &Tz,
    write: &ViewWrite,
    query: &Value,
    partial: bool,
) -> Result<String, Denial> {
    if !partial && write.name.is_none() {
        return Err(Denial::Raw(
            StatusCode::BAD_REQUEST,
            r#"{"name":["This field is required."]}"#.to_owned(),
        ));
    }
    let query_text = serde_json::to_string(query).map_err(|_| Denial::ServerError)?;
    let now = chrono::Utc::now();
    let mut sets = vec![
        "query = $2::jsonb".to_owned(),
        "updated_by_id = $3".to_owned(),
        "updated_at = $4".to_owned(),
    ];
    let mut binds: Vec<String> = Vec::new();
    // The parsed `archived_at`, bound typed after the string binds: the
    // arm below runs last, so `$N` with `N = binds.len() + 5` still owns
    // the next placeholder when the typed bind is appended.
    let mut archived_at: Option<DateTime<Utc>> = None;
    // Placeholders `$1..$4` are taken by id/query/updated_by/updated_at;
    // each appended bind owns `$N` with `N = binds.len() + 5`.
    macro_rules! maybe_set {
        ($field:expr, $column:expr) => {
            if let Some(value) = &$field {
                let text = serde_json::to_string(value).map_err(|_| Denial::ServerError)?;
                binds.push(text);
                sets.push(format!("{} = ${}::jsonb", $column, binds.len() + 4));
            }
        };
    }
    if !partial || write.name.is_some() {
        if let Some(name) = &write.name {
            binds.push(name.clone());
            sets.push(format!("name = ${}", binds.len() + 4));
        }
    }
    if let Some(description) = &write.description {
        match description {
            Value::String(text) => {
                binds.push(text.clone());
                sets.push(format!("description = ${}", binds.len() + 4));
            }
            Value::Null => {
                return Err(Denial::Raw(
                    StatusCode::BAD_REQUEST,
                    r#"{"description":["This field may not be null."]}"#.to_owned(),
                ));
            }
            _ => {
                return Err(Denial::Raw(
                    StatusCode::BAD_REQUEST,
                    r#"{"description":["Not a valid string."]}"#.to_owned(),
                ));
            }
        }
    }
    maybe_set!(write.filters, "filters");
    maybe_set!(write.display_filters, "display_filters");
    maybe_set!(write.display_properties, "display_properties");
    maybe_set!(write.rich_filters, "rich_filters");
    maybe_set!(write.logo_props, "logo_props");
    if let Some(archived) = &write.archived_at {
        match archived {
            Value::Null => sets.push("archived_at = NULL".to_owned()),
            Value::String(text) => match parse_django_datetime(text, timezone) {
                Ok(parsed) => {
                    archived_at = Some(parsed);
                    sets.push(format!("archived_at = ${}", binds.len() + 5));
                }
                Err(error) => return Err(archived_at_denial(&error, timezone)),
            },
            _ => {
                return Err(archived_at_denial(&ParseDatetimeError::Invalid, timezone));
            }
        }
    }
    let sql = format!("UPDATE issue_views SET {} WHERE id = $1", sets.join(", "));
    let mut query_builder = sqlx::query(&sql)
        .bind(id)
        .bind(&query_text)
        .bind(user_id)
        .bind(now);
    for bind in &binds {
        query_builder = query_builder.bind(bind);
    }
    if let Some(parsed) = archived_at {
        query_builder = query_builder.bind(parsed);
    }
    // `select_for_update().get()` misses raise `DoesNotExist` before any
    // write; the row is locked by this statement's own write lock.
    let result = query_builder
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if result.rows_affected() == 0 {
        return Err(Denial::NotFound);
    }
    let body = fetch_view_by_id(pool, id, timezone)
        .await?
        .ok_or(Denial::ServerError)?;
    Ok(body)
}

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

/// Best-effort deferred publish of a retrieve visit (the space intake
/// precedent): without the queue the response still stands.
async fn enqueue_message(pool: &sqlx::PgPool, message: pidash_jobs::celery::CeleryTaskMessage) {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

// ---------------------------------------------------------------------------
// Workspace (global) views
// ---------------------------------------------------------------------------

/// `GET /api/workspaces/<slug>/views/` (`base.py:71-78`): WORKSPACE
/// ADMIN/MEMBER/GUEST; guests narrow to their own rows. `?fields=` is
/// accepted and ignored (B2).
async fn workspace_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<crate::app_issues::QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let member = membership(&pool, &slug, None, &user_id).await?;
    permissions::workspace_view_list_gate(&member).map_err(gate_denial)?;
    let guest_scoped = permissions::workspace_view_list_guest_scoped(&member);
    // `order_by` splices verbatim into the queryset ordering (`:68`):
    // Django field names resolve, anything else raises `FieldError`
    // (generic 500). The contract suite only sends the default.
    let order_by =
        crate::app_issues::query_last(&query, "order_by").unwrap_or("-created_at".to_owned());
    let order_sql = resolve_view_order(&order_by)?;
    let sql = queries_views::workspace_view_list_sql(&order_sql);
    let sql = if guest_scoped {
        push_and(&sql, "v.owned_by_id = ($2)")
    } else {
        sql
    };
    let rows = fetch_view_rows(&pool, &sql, &slug, None, &user_id).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&shape_view(row, None, &timezone));
    }
    out.push(']');
    Ok(json_response(StatusCode::OK, out))
}

/// Append an `AND` condition to a builder statement that already ends in
/// `ORDER BY ...`: the condition splices before the ordering, never after
/// it (a trailing `AND` past `ORDER BY` is a syntax error → 500).
fn push_and(sql: &str, condition: &str) -> String {
    match sql.rfind(" ORDER BY ") {
        Some(index) => format!("{} AND {}{}", &sql[..index], condition, &sql[index..]),
        None => format!("{sql} AND {condition}"),
    }
}

/// Resolve a workspace-list `order_by` param to an `ORDER BY` fragment.
/// Django splices the param verbatim as a field path: concrete columns
/// and FK names (ordered by their id column) resolve; relation
/// traversals and unknown names raise `FieldError`, which answers the
/// generic 500. Only identifier-safe keys interpolate, so non-matching
/// input can only fail closed — never inject.
fn resolve_view_order(raw: &str) -> Result<String, Denial> {
    let descending = raw.starts_with('-');
    let key = raw.trim_start_matches('-');
    if key.is_empty()
        || !key
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '_')
    {
        return Err(Denial::ServerError);
    }
    let column = match key {
        "owned_by" => "v.owned_by_id".to_owned(),
        "workspace" => "v.workspace_id".to_owned(),
        "project" => "v.project_id".to_owned(),
        "created_by" => "v.created_by_id".to_owned(),
        "updated_by" => "v.updated_by_id".to_owned(),
        known if pidash_db::app_views_search::models::issue_view::COLUMNS.contains(&known) => {
            format!("v.{known}")
        }
        _ => return Err(Denial::ServerError),
    };
    Ok(if descending {
        format!("{column} DESC")
    } else {
        format!("{column} ASC")
    })
}

/// `POST /api/workspaces/<slug>/views/`: the DRF default create (no role
/// gate in Python) with `perform_create` scoping (`:56-58`) — 201 with
/// the bare instance (no `is_favorite`).
async fn workspace_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    // DRF parses the body after initial (auth): serializer validation
    // (400) precedes `perform_create`'s workspace lookup (404).
    let body = match detail_body(&state, req).await {
        Ok(body) => body,
        Err(into) => return Ok(into),
    };
    let object = body.as_object().cloned().unwrap_or_default();
    reject_null_json(&object)?;
    let write = split_write(&object, false)?;
    let workspace_id = workspace_by_slug(&pool, &slug).await?;
    let query = create_query(write.filters.as_ref())?;
    let sort_order = next_sort_order(&pool, &workspace_id, None).await?;
    let id = insert_view(
        &pool,
        &workspace_id,
        None,
        &user_id,
        &timezone,
        &write,
        &query,
        sort_order,
    )
    .await?;
    let rendered = fetch_view_by_id(&pool, &id, &timezone)
        .await?
        .ok_or(Denial::ServerError)?;
    Ok(json_response(StatusCode::CREATED, rendered))
}

/// Workspace queryset read plus a pk constraint: `get_queryset()` with
/// `.filter(pk=pk)` — the `.first()` retrieve read and the DRF default
/// `update`'s `get_object()` share it.
async fn fetch_workspace_view(
    pool: &sqlx::PgPool,
    slug: &str,
    user_id: uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<Map<String, Value>>, Denial> {
    // The queryset filters (`workspace__slug`, `project__isnull`,
    // owner-or-public) apply to the read, like `get_queryset`.
    let sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM ({} AND v.id = $3 AND v.deleted_at IS NULL) AS __r",
        queries_views::workspace_view_list_sql("v.created_at DESC")
            .trim_end_matches(" ORDER BY v.created_at DESC"),
    );
    let row: Option<(String,)> = sqlx::query_as(&sql)
        .bind(slug)
        .bind(user_id)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        None => Ok(None),
        Some((text,)) => {
            let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
            match value {
                Value::Object(map) => Ok(Some(map)),
                _ => Err(Denial::ServerError),
            }
        }
    }
}

/// `GET /api/workspaces/<slug>/views/<pk>/` (`:102-112`): no decorator —
/// auth only. Unknown pk answers the `get_initial()` 200 (B1). Fires the
/// retrieve visit publish for found rows (the task runs unconditionally
/// in Python, but there is no row to name when missing).
async fn workspace_retrieve(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    // Routing first (Django's `<uuid:pk>` 404 precedes auth).
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let row = fetch_workspace_view(
        &pool,
        &slug,
        user_id,
        &parse_pk(&pk).unwrap_or(uuid::Uuid::nil()),
    )
    .await?;
    let Some(map) = row else {
        // B1: `.first()` serializes unconditionally — the missing row
        // answers the serializer's `get_initial()` body, not 404.
        return Ok(json_response(StatusCode::OK, MISSING_VIEW_BODY.to_owned()));
    };
    let rendered = shape_view(&map, None, &timezone);
    let emit = pidash_services::app_views_search::tasks::workspace_view_retrieve_emit(
        &pk,
        &user_id.to_string(),
        &slug,
    );
    enqueue_message(
        &pool,
        pidash_jobs::celery::CeleryTaskMessage::new(emit.task_name(), vec![], emit.kwargs()),
    )
    .await;
    Ok(json_response(StatusCode::OK, rendered))
}

/// Shared locked-row read for the workspace write paths:
/// `select_for_update().get(pk, workspace__slug)` — a miss is the
/// `DoesNotExist` 404.
async fn lock_workspace_view(
    pool: &sqlx::PgPool,
    slug: &str,
    pk: &uuid::Uuid,
) -> Result<(bool, uuid::Uuid, Value), Denial> {
    let row: Option<(bool, uuid::Uuid, Value)> = sqlx::query_as(
        r#"SELECT v.is_locked, v.owned_by_id, v.filters FROM issue_views v
           JOIN workspaces w ON w.id = v.workspace_id AND w.deleted_at IS NULL
           WHERE v.id = $1 AND w.slug = $2 AND v.deleted_at IS NULL
           FOR UPDATE OF v"#,
    )
    .bind(pk)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::NotFound)
}

/// `PATCH /api/workspaces/<slug>/views/<pk>/` (`:80-100`): WORKSPACE
/// creator-or-empty-roles gate, then the locked/owner rechecks.
async fn workspace_patch(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let pk = parse_pk(&pk).ok_or(Denial::Raw(
        StatusCode::BAD_REQUEST,
        r#"{"error":"Please provide valid detail"}"#.to_owned(),
    ))?;
    let member = membership(&pool, &slug, None, &user_id).await?;
    let creator = is_view_creator(&pool, &pk, &user_id).await?;
    permissions::workspace_view_partial_gate(&member, creator).map_err(gate_denial)?;
    let (is_locked, owned_by, filters_old) = lock_workspace_view(&pool, &slug, &pk).await?;
    permissions::view_partial_recheck(permissions::ViewRow::Found {
        is_locked,
        is_owner: owned_by == user_id,
    })
    .map_err(gate_denial)?;
    // Body parses after the gates (DRF parses on serializer access).
    let body = match detail_body(&state, req).await {
        Ok(body) => body,
        Err(into) => return Ok(into),
    };
    let object = body.as_object().cloned().unwrap_or_default();
    reject_null_json(&object)?;
    let write = split_write(&object, true)?;
    let query = update_query(write.filters.as_ref(), Some(&filters_old))?;
    let rendered = apply_view_update(&pool, &pk, &user_id, &timezone, &write, &query, true).await?;
    Ok(json_response(StatusCode::OK, rendered))
}

/// `PUT /api/workspaces/<slug>/views/<pk>/`: the DRF default `update` —
/// no decorator and no locked/owner recheck in Python, so auth only;
/// full write through the queryset, response without `is_favorite` (the
/// un-annotated queryset serves workspace views). A miss is the
/// model-named `Http404`.
async fn workspace_put(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let pk = parse_pk(&pk).ok_or(Denial::Raw(
        StatusCode::BAD_REQUEST,
        r#"{"error":"Please provide valid detail"}"#.to_owned(),
    ))?;
    // `get_object()` through the queryset precedes validation: a miss is
    // the model-named 404.
    let existing = fetch_workspace_view(&pool, &slug, user_id, &pk).await?;
    let Some(existing) = existing else {
        return Err(Denial::Raw(
            StatusCode::NOT_FOUND,
            VIEW_PUT_MISSING_BODY.to_owned(),
        ));
    };
    let body = match detail_body(&state, req).await {
        Ok(body) => body,
        Err(into) => return Ok(into),
    };
    let object = body.as_object().cloned().unwrap_or_default();
    reject_null_json(&object)?;
    let write = split_write(&object, false)?;
    let query = update_query(write.filters.as_ref(), existing.get("filters"))?;
    let rendered =
        apply_view_update(&pool, &pk, &user_id, &timezone, &write, &query, false).await?;
    Ok(json_response(StatusCode::OK, rendered))
}

/// `DELETE /api/workspaces/<slug>/views/<pk>/` (`:114-135`): WORKSPACE
/// ADMIN-or-creator gate, then the admin-or-owner recheck; soft-deletes
/// the view and its global favorites.
async fn workspace_destroy(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let pk = parse_pk(&pk).ok_or(Denial::Raw(
        StatusCode::BAD_REQUEST,
        r#"{"error":"Please provide valid detail"}"#.to_owned(),
    ))?;
    let member = membership(&pool, &slug, None, &user_id).await?;
    let creator = is_view_creator(&pool, &pk, &user_id).await?;
    permissions::workspace_view_destroy_gate(&member, creator).map_err(gate_denial)?;
    let (_, owned_by, _) = lock_workspace_view(&pool, &slug, &pk).await?;
    permissions::workspace_view_destroy_recheck(&member, owned_by == user_id)
        .map_err(gate_denial)?;
    let now = chrono::Utc::now();
    sqlx::query(r#"UPDATE issue_views SET deleted_at = $2 WHERE id = $1"#)
        .bind(pk)
        .bind(now)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    sqlx::query(
        r#"UPDATE user_favorites SET deleted_at = $3 FROM workspaces w
           WHERE user_favorites.workspace_id = w.id AND w.slug = $1 AND w.deleted_at IS NULL
             AND user_favorites.entity_identifier = $2 AND user_favorites.project_id IS NULL
             AND user_favorites.entity_type = 'view' AND user_favorites.deleted_at IS NULL"#,
    )
    .bind(&slug)
    .bind(pk)
    .bind(now)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::NO_CONTENT, String::new()))
}

// ---------------------------------------------------------------------------
// Project views
// ---------------------------------------------------------------------------

/// The project row the project-view bodies need: workspace scope plus the
/// guest flag. A miss raises `DoesNotExist` (the `Project.objects.get`
/// calls); on the create path the row check happens implicitly through
/// the FK insert instead.
struct ProjectRow {
    workspace_id: uuid::Uuid,
    guest_view_all_features: bool,
}

async fn project_row(pool: &sqlx::PgPool, project_id: &uuid::Uuid) -> Result<ProjectRow, Denial> {
    let row: Option<(uuid::Uuid, uuid::Uuid, Option<bool>)> = sqlx::query_as(
        r#"SELECT id, workspace_id, guest_view_all_features FROM projects
           WHERE id = $1 AND deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((_, workspace_id, guest_view_all_features)) = row else {
        return Err(Denial::NotFound);
    };
    Ok(ProjectRow {
        workspace_id,
        guest_view_all_features: guest_view_all_features.unwrap_or(false),
    })
}

/// `GET /api/workspaces/<slug>/projects/<project_id>/views/` (`:289-306`):
/// PROJECT ADMIN/MEMBER/GUEST; guests without view-all narrow to their
/// own rows. Rows carry the `is_favorite` annotation; `?fields=` is
/// ignored (B2).
async fn project_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<crate::app_issues::QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, Some(&project_id), &user_id).await?;
    permissions::project_view_list_gate(&member).map_err(gate_denial)?;
    // `Project.objects.get` raises before the guest narrowing when the
    // project row is gone.
    let project = project_row(&pool, &project_id).await?;
    let sql = queries_views::project_view_list_sql();
    let sql =
        if permissions::project_view_list_guest_scoped(&member, project.guest_view_all_features) {
            push_and(&sql, "v.owned_by_id = ($3)")
        } else {
            sql
        };
    let _ = crate::app_issues::query_last(&query, "fields");
    let rows = fetch_view_rows(&pool, &sql, &slug, Some(&project_id), &user_id).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let is_favorite = row.get("is_favorite").and_then(Value::as_bool);
        out.push_str(&shape_view(row, is_favorite, &timezone));
    }
    out.push(']');
    Ok(json_response(StatusCode::OK, out))
}

/// `POST /api/workspaces/<slug>/projects/<project_id>/views/`: the DRF
/// default create (no role gate in Python) with `perform_create` scoping
/// (`:260-262`) — 201 with the bare instance (no `is_favorite`).
async fn project_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    // The identifier rewrite runs in `initial`, before the view: it stays
    // ahead of validation. Serializer validation (400) then precedes the
    // save-time workspace fallback.
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let body = match detail_body(&state, req).await {
        Ok(body) => body,
        Err(into) => return Ok(into),
    };
    let object = body.as_object().cloned().unwrap_or_default();
    reject_null_json(&object)?;
    let write = split_write(&object, false)?;
    // The workspace falls back from the project
    // (`WorkspaceBaseModel.save`); a missing project row surfaces as the
    // FK `IntegrityError`, not a 404.
    let workspace_id = project_row(&pool, &project_id)
        .await
        .map(|row| row.workspace_id)
        .map_err(|error| match error {
            Denial::NotFound => {
                Denial::Raw(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned())
            }
            other => other,
        })?;
    let query = create_query(write.filters.as_ref())?;
    let sort_order = next_sort_order(&pool, &workspace_id, Some(&project_id)).await?;
    let id = insert_view(
        &pool,
        &workspace_id,
        Some(&project_id),
        &user_id,
        &timezone,
        &write,
        &query,
        sort_order,
    )
    .await?;
    let rendered = fetch_view_by_id(&pool, &id, &timezone)
        .await?
        .ok_or(Denial::ServerError)?;
    Ok(json_response(StatusCode::CREATED, rendered))
}

/// Annotated project-view read for retrieve/PUT: the queryset SQL plus a
/// pk constraint, `is_favorite` included.
async fn fetch_project_view(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<Map<String, Value>>, Denial> {
    let sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM ({} AND v.id = $4 AND v.deleted_at IS NULL) AS __r",
        queries_views::project_view_list_sql()
            .trim_end_matches(" ORDER BY is_favorite DESC, v.name ASC"),
    );
    let row: Option<(String,)> = sqlx::query_as(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(user_id)
        .bind(pk)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match row {
        None => Ok(None),
        Some((text,)) => {
            let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
            match value {
                Value::Object(map) => Ok(Some(map)),
                _ => Err(Denial::ServerError),
            }
        }
    }
}

/// `GET .../views/<pk>/` (`:308-341`): PROJECT ADMIN/MEMBER/GUEST plus
/// the in-body guest recheck (`:317-331`, error string verbatim). Unknown
/// pk answers the `get_initial()` 200 for members; a guest without
/// view-all crashes on the `owned_by` deref first (B1b → generic 500).
async fn project_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, pk)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, Some(&project_id), &user_id).await?;
    permissions::project_view_retrieve_gate(&member).map_err(gate_denial)?;
    let row = fetch_project_view(
        &pool,
        &slug,
        &project_id,
        &user_id,
        &parse_pk(&pk).unwrap_or(uuid::Uuid::nil()),
    )
    .await?;
    let project = project_row(&pool, &project_id).await?;
    // The guest recheck order is load-bearing (B1b): the `owned_by`
    // deref runs before any missing-row branch — a missing row under a
    // guest without view-all answers the `AttributeError` 500, which the
    // gate already encodes as its `Missing` arm.
    let lookup = match &row {
        Some(map) => {
            // `owned_by` is non-nullable; a parse miss can only be a
            // corrupt row, which reads as not-owned (the 403 arm), never
            // as the B1b missing-row crash.
            let owned_by = map
                .get("owned_by_id")
                .and_then(|value| value.as_str())
                .and_then(|raw| raw.parse::<uuid::Uuid>().ok())
                .unwrap_or(uuid::Uuid::nil());
            permissions::ProjectViewLookup::Found {
                is_owner: owned_by == user_id,
            }
        }
        None => permissions::ProjectViewLookup::Missing,
    };
    permissions::project_view_retrieve_guest_gate(&member, project.guest_view_all_features, lookup)
        .map_err(gate_denial)?;
    let Some(map) = row else {
        // Same `get_initial()` 200 as the workspace retrieve (B1's body);
        // only the guest-without-view-all branch crashes first (B1b).
        return Ok(json_response(StatusCode::OK, MISSING_VIEW_BODY.to_owned()));
    };
    let is_favorite = map.get("is_favorite").and_then(Value::as_bool);
    let rendered = shape_view(&map, is_favorite, &timezone);
    let emit = pidash_services::app_views_search::tasks::issue_view_retrieve_emit(
        &pk,
        &user_id.to_string(),
        &project_id.to_string(),
        &slug,
    );
    enqueue_message(
        &pool,
        pidash_jobs::celery::CeleryTaskMessage::new(emit.task_name(), vec![], emit.kwargs()),
    )
    .await;
    Ok(json_response(StatusCode::OK, rendered))
}

/// Shared locked-row read for the project write paths:
/// `select_for_update().get(pk, workspace__slug, project_id)`.
async fn lock_project_view(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<(bool, uuid::Uuid, Value), Denial> {
    let row: Option<(bool, uuid::Uuid, Value)> = sqlx::query_as(
        r#"SELECT v.is_locked, v.owned_by_id, v.filters FROM issue_views v
           JOIN workspaces w ON w.id = v.workspace_id AND w.deleted_at IS NULL
           WHERE v.id = $1 AND w.slug = $2 AND v.project_id = $3 AND v.deleted_at IS NULL
           FOR UPDATE OF v"#,
    )
    .bind(pk)
    .bind(slug)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.ok_or(Denial::NotFound)
}

/// `PATCH .../views/<pk>/` (`:343-363`): PROJECT creator-or-empty-roles
/// gate, then the locked/owner rechecks; response without `is_favorite`.
async fn project_patch(
    State(state): State<AppState>,
    Path((slug, project_raw, pk)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let pk = parse_pk(&pk).ok_or(Denial::Raw(
        StatusCode::BAD_REQUEST,
        r#"{"error":"Please provide valid detail"}"#.to_owned(),
    ))?;
    let member = membership(&pool, &slug, Some(&project_id), &user_id).await?;
    let creator = is_view_creator(&pool, &pk, &user_id).await?;
    permissions::project_view_partial_gate(&member, creator).map_err(gate_denial)?;
    let (is_locked, owned_by, filters_old) =
        lock_project_view(&pool, &slug, &project_id, &pk).await?;
    permissions::view_partial_recheck(permissions::ViewRow::Found {
        is_locked,
        is_owner: owned_by == user_id,
    })
    .map_err(gate_denial)?;
    let body = match detail_body(&state, req).await {
        Ok(body) => body,
        Err(into) => return Ok(into),
    };
    let object = body.as_object().cloned().unwrap_or_default();
    reject_null_json(&object)?;
    let write = split_write(&object, true)?;
    let query = update_query(write.filters.as_ref(), Some(&filters_old))?;
    let rendered = apply_view_update(&pool, &pk, &user_id, &timezone, &write, &query, true).await?;
    Ok(json_response(StatusCode::OK, rendered))
}

/// `PUT .../views/<pk>/`: the DRF default `update` (no decorator and no
/// locked/owner recheck in Python) — auth only, full write through the
/// annotated queryset, so `is_favorite` is present.
async fn project_put(
    State(state): State<AppState>,
    Path((slug, project_raw, pk)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let pk = parse_pk(&pk).ok_or(Denial::Raw(
        StatusCode::BAD_REQUEST,
        r#"{"error":"Please provide valid detail"}"#.to_owned(),
    ))?;
    // `get_object()` through the annotated queryset precedes validation:
    // a miss is the model-named `Http404`.
    let existing = fetch_project_view(&pool, &slug, &project_id, &user_id, &pk).await?;
    let Some(existing) = existing else {
        return Err(Denial::Raw(
            StatusCode::NOT_FOUND,
            VIEW_PUT_MISSING_BODY.to_owned(),
        ));
    };
    let body = match detail_body(&state, req).await {
        Ok(body) => body,
        Err(into) => return Ok(into),
    };
    let object = body.as_object().cloned().unwrap_or_default();
    reject_null_json(&object)?;
    let write = split_write(&object, false)?;
    let query = update_query(write.filters.as_ref(), existing.get("filters"))?;
    apply_view_update(&pool, &pk, &user_id, &timezone, &write, &query, false).await?;
    let reread = fetch_project_view(&pool, &slug, &project_id, &user_id, &pk)
        .await?
        .ok_or(Denial::ServerError)?;
    let is_favorite = reread.get("is_favorite").and_then(Value::as_bool);
    Ok(json_response(
        StatusCode::OK,
        shape_view(&reread, is_favorite, &timezone),
    ))
}

/// `DELETE .../views/<pk>/` (`:365-398`): PROJECT ADMIN-or-creator gate,
/// then the admin-or-owner recheck; soft-deletes the view and its
/// favorites, hard-deletes its recent visits.
async fn project_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, pk)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if pk.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let pk = parse_pk(&pk).ok_or(Denial::Raw(
        StatusCode::BAD_REQUEST,
        r#"{"error":"Please provide valid detail"}"#.to_owned(),
    ))?;
    let member = membership(&pool, &slug, Some(&project_id), &user_id).await?;
    let creator = is_view_creator(&pool, &pk, &user_id).await?;
    permissions::project_view_destroy_gate(&member, creator).map_err(gate_denial)?;
    let (_, owned_by, _) = lock_project_view(&pool, &slug, &project_id, &pk).await?;
    permissions::project_view_destroy_recheck(&member, owned_by == user_id).map_err(gate_denial)?;
    let now = chrono::Utc::now();
    sqlx::query(r#"UPDATE issue_views SET deleted_at = $2 WHERE id = $1"#)
        .bind(pk)
        .bind(now)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    sqlx::query(
        r#"UPDATE user_favorites SET deleted_at = $4 FROM workspaces w
           WHERE user_favorites.workspace_id = w.id AND w.slug = $1 AND w.deleted_at IS NULL
             AND user_favorites.project_id = $2 AND user_favorites.entity_identifier = $3
             AND user_favorites.entity_type = 'view' AND user_favorites.deleted_at IS NULL"#,
    )
    .bind(&slug)
    .bind(project_id)
    .bind(pk)
    .bind(now)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    sqlx::query(
        r#"DELETE FROM user_recent_visits USING workspaces w
           WHERE user_recent_visits.workspace_id = w.id AND w.slug = $1 AND w.deleted_at IS NULL
             AND user_recent_visits.project_id = $2 AND user_recent_visits.entity_identifier = $3
             AND user_recent_visits.entity_name = 'view'"#,
    )
    .bind(&slug)
    .bind(project_id)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::NO_CONTENT, String::new()))
}

// ---------------------------------------------------------------------------
// Favorites
// ---------------------------------------------------------------------------

/// `GET .../user-favorite-views/`: the ported B4 — `get_queryset` raises
/// `FieldError` (`select_related("view")` names no FK) before any row is
/// read, so every authenticated GET answers the generic 500. Auth still
/// runs first (anonymous → 401).
async fn favorite_list(
    State(state): State<AppState>,
    Path((_slug, _project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    let _ = actor(&state, extension).await?;
    // `favorite_list_sql` always errs with the `InvalidSelectRelated`
    // `FieldError`; the handler maps it to Django's generic 500.
    let _ = queries_views::favorite_list_sql();
    Err(Denial::ServerError)
}

/// `POST .../user-favorite-views/` (`:413-421`): PROJECT ADMIN/MEMBER.
/// Creates the favorite with the workspace falling back from the project
/// and the crum audit stamp; a missing `view` key stores a NULL
/// identifier (no validation in Python). Duplicates hit the partial
/// unique constraint → the `IntegrityError` 400.
async fn favorite_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, Some(&project_id), &user_id).await?;
    permissions::favorite_create_gate(&member).map_err(gate_denial)?;
    let body = match detail_body(&state, req).await {
        Ok(body) => body,
        Err(into) => return Ok(into),
    };
    // `request.data.get("view")`: missing → NULL identifier; an invalid
    // UUID string fails the column cast (`ValidationError` → the
    // "valid detail" 400); anything else binds as-is.
    let entity_identifier: Option<uuid::Uuid> = match body.get("view") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) => match raw.parse::<uuid::Uuid>() {
            Ok(id) => Some(id),
            Err(_) => {
                return Err(Denial::Raw(
                    StatusCode::BAD_REQUEST,
                    r#"{"error":"Please provide valid detail"}"#.to_owned(),
                ));
            }
        },
        Some(_) => {
            return Err(Denial::Raw(
                StatusCode::BAD_REQUEST,
                r#"{"error":"Please provide valid detail"}"#.to_owned(),
            ));
        }
    };
    let project = project_row(&pool, &project_id)
        .await
        .map_err(|error| match error {
            Denial::NotFound => {
                Denial::Raw(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned())
            }
            other => other,
        })?;
    let now = chrono::Utc::now();
    let id = uuid::Uuid::new_v4();
    // `sequence` bumps over the workspace maximum on add
    // (`db/models/favorite.py:52-64`).
    let largest: Option<(Option<f64>,)> = sqlx::query_as(
        r#"SELECT MAX(sequence) FROM user_favorites WHERE workspace_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(project.workspace_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sequence = largest
        .and_then(|row| row.0)
        .map(|max| max + 10000.0)
        .unwrap_or(65535.0);
    let result = sqlx::query(
        r#"INSERT INTO user_favorites
           (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
            workspace_id, project_id, user_id, entity_type, entity_identifier,
            name, is_folder, sequence, parent_id)
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, 'view', $8,
                   NULL, FALSE, $9, NULL)"#,
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(user_id)
    .bind(project.workspace_id)
    .bind(project_id)
    .bind(user_id)
    .bind(entity_identifier)
    .bind(sequence)
    .execute(&pool)
    .await;
    match result {
        Ok(_) => Ok(json_response(StatusCode::NO_CONTENT, String::new())),
        Err(error) => {
            if let sqlx::Error::Database(db_error) = &error {
                if db_error.code().is_some() {
                    return Err(Denial::Raw(
                        StatusCode::BAD_REQUEST,
                        INVALID_PAYLOAD_BODY.to_owned(),
                    ));
                }
            }
            Err(Denial::ServerError)
        }
    }
}

/// `DELETE .../user-favorite-views/<view_id>/` (`:423-433`): PROJECT
/// ADMIN/MEMBER. The lookup uses the `project` relation name with the raw
/// URL value; a miss is the `DoesNotExist` 404. Deletes hard
/// (`soft=False`).
async fn favorite_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, view_id)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    req: axum::extract::Request,
) -> Result<Response, Denial> {
    if view_id.parse::<uuid::Uuid>().is_err() {
        return Ok(crate::edge::proxy(State(state), req).await);
    }
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, Some(&project_id), &user_id).await?;
    permissions::favorite_destroy_gate(&member).map_err(gate_denial)?;
    // The raw URL value casts through the UUID column: an unparseable
    // `view_id` is Django's `ValidationError` 400, a well-formed miss is
    // the `DoesNotExist` 404 (B9).
    let view_id = view_id.parse::<uuid::Uuid>().map_err(|_| {
        Denial::Raw(
            StatusCode::BAD_REQUEST,
            r#"{"error":"Please provide valid detail"}"#.to_owned(),
        )
    })?;
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT uf.id FROM user_favorites uf
           JOIN workspaces w ON w.id = uf.workspace_id AND w.deleted_at IS NULL
           WHERE uf.project_id = $1 AND uf.user_id = $2 AND w.slug = $3
             AND uf.entity_type = 'view' AND uf.entity_identifier = $4
             AND uf.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&slug)
    .bind(view_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id,)) = row else {
        return Err(Denial::NotFound);
    };
    sqlx::query(r#"DELETE FROM user_favorites WHERE id = $1"#)
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::NO_CONTENT, String::new()))
}

// ---------------------------------------------------------------------------
// Workspace view-issues list
// ---------------------------------------------------------------------------

/// Relation joins the rich/legacy filter fragments may reference (the
/// app_issues table verbatim: Django's `filter()` join aliases over the
/// `issue` base alias, with no soft-delete guard on the joined tables).
const FILTER_JOINS: &[(&str, &str)] = &[
    ("issue_assignees", "issue_assignee"),
    ("cycle_issues", "issue_cycle"),
    ("module_issues", "issue_module"),
    ("issue_mentions", "issue_mention"),
    ("issue_labels", "label_issue"),
    ("issue_subscribers", "issue_subscribers"),
];

/// True when every `"alias".` reference in the filter SQL is an
/// `IS NULL` / `IS NOT NULL` test (the `__isnull` predicates): Django
/// LEFT-joins those, INNER-joins everything else.
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

/// Shape one view-issues row in `ViewIssueListSerializer` source order
/// (`app/serializers/view.py:28-55`): datetimes re-render in the request
/// zone, `sort_order` through Python-float formatting, `estimate_point`
/// / `created_by` / `updated_by` from their `*_id` columns, `state__group`
/// None-guarded, and the three id arrays from the prefetch maps.
#[allow(clippy::too_many_arguments)]
fn shape_view_issue(
    row: &Map<String, Value>,
    timezone: &Tz,
    assignees: &[String],
    labels: &[String],
    modules: &[String],
) -> String {
    use std::fmt::Write as _;
    let raw = |key: &str| row.get(key).unwrap_or(&Value::Null);
    let str_of =
        |key: &str| -> String { serde_json::to_string(raw(key)).unwrap_or("null".to_owned()) };
    let mut out = String::from("{");
    let _ = write!(out, "\"id\":{}", str_of("id"));
    let _ = write!(out, ",\"name\":{}", str_of("name"));
    let _ = write!(out, ",\"state_id\":{}", str_of("state_id"));
    let sort_order = raw("sort_order").as_f64().unwrap_or(0.0);
    let _ = write!(
        out,
        ",\"sort_order\":{}",
        crate::paginator::py_float_str(sort_order)
    );
    let _ = write!(
        out,
        ",\"completed_at\":{}",
        shape_datetime(row.get("completed_at"), timezone)
    );
    let _ = write!(out, ",\"estimate_point\":{}", str_of("estimate_point"));
    let _ = write!(out, ",\"priority\":{}", str_of("priority"));
    let _ = write!(out, ",\"start_date\":{}", shape_date(row.get("start_date")));
    let _ = write!(
        out,
        ",\"target_date\":{}",
        shape_date(row.get("target_date"))
    );
    let sequence_id = raw("sequence_id").as_i64().unwrap_or(0);
    let _ = write!(out, ",\"sequence_id\":{sequence_id}");
    let _ = write!(out, ",\"project_id\":{}", str_of("project_id"));
    let _ = write!(out, ",\"parent_id\":{}", str_of("parent_id"));
    let _ = write!(out, ",\"cycle_id\":{}", str_of("cycle_id"));
    let sub_count = raw("sub_issues_count").as_i64().unwrap_or(0);
    let _ = write!(out, ",\"sub_issues_count\":{sub_count}");
    let _ = write!(
        out,
        ",\"created_at\":{}",
        shape_datetime(row.get("created_at"), timezone)
    );
    let _ = write!(
        out,
        ",\"updated_at\":{}",
        shape_datetime(row.get("updated_at"), timezone)
    );
    let _ = write!(out, ",\"created_by\":{}", str_of("created_by"));
    let _ = write!(out, ",\"updated_by\":{}", str_of("updated_by"));
    let attachment_count = raw("attachment_count").as_i64().unwrap_or(0);
    let _ = write!(out, ",\"attachment_count\":{attachment_count}");
    let link_count = raw("link_count").as_i64().unwrap_or(0);
    let _ = write!(out, ",\"link_count\":{link_count}");
    let is_draft = raw("is_draft").as_bool().unwrap_or(false);
    let _ = write!(out, ",\"is_draft\":{is_draft}");
    let _ = write!(
        out,
        ",\"archived_at\":{}",
        shape_datetime(row.get("archived_at"), timezone)
    );
    let _ = write!(out, ",\"state__group\":{}", str_of("state__group"));
    let _ = write!(
        out,
        ",\"assignee_ids\":{}",
        serde_json::to_string(assignees).unwrap_or("[]".to_owned())
    );
    let _ = write!(
        out,
        ",\"label_ids\":{}",
        serde_json::to_string(labels).unwrap_or("[]".to_owned())
    );
    let _ = write!(
        out,
        ",\"module_ids\":{}",
        serde_json::to_string(modules).unwrap_or("[]".to_owned())
    );
    out.push('}');
    out
}

/// `GET /api/workspaces/<slug>/issues/` (`:217-253`): WORKSPACE
/// ADMIN/MEMBER/GUEST; `filter_queryset` (ComplexFilterBackend +
/// `IssueFilterSet`) plus the legacy `issue_filters(params, "GET")`
/// predicates, the project permission Q, the four annotations, the
/// `order_issue_queryset` ordering and the `paginate()` envelope with the
/// `ViewIssueListSerializer` rows.
async fn view_issues_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<crate::app_issues::QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, Denial> {
    use crate::paginator::{
        apply_offset_window, max_hits, next_cursor, offset_window, parse_per_page, prev_cursor,
        Cursor,
    };
    let resolved = actor(&state, extension).await?;
    let pool = pool_of(&state)?.clone();
    let user_id = resolved.id;
    let timezone = resolved.timezone;
    let member = membership(&pool, &slug, None, &user_id).await?;
    permissions::view_issues_list_gate(&member).map_err(gate_denial)?;

    // Filter stack (`:221-227`): the rich `filter=` tree plus the legacy
    // GET predicates, ANDed. Binds run through one renumbering binder
    // seeded with the base statement's own `$1`/`$2`.
    let mut binder = crate::app_issues::Binder::new();
    binder.bind(sea_query::Value::String(Some(Box::new(slug.clone()))));
    binder.bind(sea_query::Value::Uuid(Some(Box::new(user_id))));
    let filters_raw = crate::app_issues::query_last(&query, "filters").unwrap_or_default();
    let complex_where = if filters_raw.is_empty() {
        None
    } else {
        let tree =
            pidash_db::filter::FilterTree::parse_param(&filters_raw).map_err(filter_denial)?;
        let condition = crate::app_issues::compile_filter_tree(&tree).map_err(filter_denial)?;
        let (fragment, values) = crate::app_issues::render_condition(&condition);
        Some(binder.splice(&fragment, values))
    };
    let flat: HashMap<String, String> = query
        .keys()
        .filter_map(|key| {
            crate::app_issues::query_last(&query, key).map(|last| (key.clone(), last))
        })
        .collect();
    // `order_by`, `per_page`, `cursor`, `fields`, `expand` are paginator /
    // serializer params, not issue filters — keep them out of the legacy
    // compiler's input like Django's `issue_filters` key loop does (it
    // only reads its known keys anyway; the kernel skips the rest).
    let today = chrono::Utc::now().date_naive();
    let legacy = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
        .map_err(|_| Denial::ServerError)?;
    let mut legacy_parts: Vec<String> = Vec::new();
    for (name, value) in legacy.predicates() {
        let fragment = crate::app_issues::legacy_sql(&mut binder, name, value)
            .map_err(|_| Denial::ServerError)?;
        legacy_parts.push(fragment);
    }
    let legacy_where = if legacy_parts.is_empty() {
        None
    } else {
        Some(legacy_parts.join(" AND "))
    };

    // Ordering (`:242-244`): the rewritten paginator key over the
    // annotation expressions, `NULLS LAST`, `issue.created_at` tiebreak —
    // exactly `OffsetPaginator.get_result`.
    let order_param =
        crate::app_issues::query_last(&query, "order_by").unwrap_or("-created_at".to_owned());
    let spec = queries_views::view_issues_order_sql(&order_param);
    let (key_expr, descending) = crate::app_issues::order_key(&spec.out_param, &order_param)
        .map_err(|_| Denial::ServerError)?;
    let direction = if descending { "DESC" } else { "ASC" };
    let order_by_sql = format!("({key_expr}) {direction} NULLS LAST, issue.created_at DESC");

    let mut list_sql = queries_views::view_issues_list_sql(
        complex_where.as_deref(),
        legacy_where.as_deref(),
        &order_by_sql,
    );
    let mut count_sql =
        queries_views::view_issues_count_sql(complex_where.as_deref(), legacy_where.as_deref());
    // Relation joins for filter references (Django's `filter()` joins):
    // INNER when referenced positively, LEFT when only `__isnull`-tested.
    let where_probe = format!("{list_sql} {count_sql}");
    let mut joins = String::new();
    for (table, alias) in FILTER_JOINS {
        let marker = format!("\"{alias}\".");
        if !where_probe.contains(&marker) {
            continue;
        }
        let kind = if alias_nullable_only(&where_probe, alias) {
            "LEFT JOIN"
        } else {
            "INNER JOIN"
        };
        joins.push_str(&format!(
            " {kind} {table} AS {alias} ON {alias}.issue_id = issue.id"
        ));
    }
    if where_probe.contains("\"issue_intake\".") {
        joins.push_str(
            " INNER JOIN intake_issues AS issue_intake ON issue_intake.issue_id = issue.id",
        );
    }
    if !joins.is_empty() {
        list_sql = list_sql.replacen(" ORDER BY ", &format!("{joins} ORDER BY "), 1);
        count_sql = format!("{count_sql}{joins}");
    }
    // The queries layer's `project_permission_filter` names the issue
    // alias `i`, but every statement it composes into aliases the table
    // `issue` — the fragment as shipped references a missing alias and no
    // view-issues query can execute with it. Normalize the alias at the
    // composition boundary (filed as a foundation issue for the real
    // fix); the replace turns into a no-op once the helper is corrected,
    // and no other fragment spells the bare `i.` alias.
    let list_sql = list_sql.replace("i.created_by_id", "issue.created_by_id");
    let count_sql = count_sql.replace("i.created_by_id", "issue.created_by_id");

    // Pagination (`:247-253`): `per_page` (default/max 1000), the
    // `cursor` protocol, total over the pre-annotation deepcopy.
    let per_page_raw = crate::app_issues::query_last(&query, "per_page");
    let per_page = parse_per_page(per_page_raw.as_deref(), 1000, 1000).map_err(page_denial)?;
    let limit = per_page.min(1000);
    let cursor_raw = crate::app_issues::query_last(&query, "cursor");
    let cursor = match cursor_raw {
        None => Cursor::default_for(per_page),
        Some(raw) => Cursor::from_string(&raw).map_err(page_denial)?,
    };
    let window = offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;
    let values = binder.values();
    let page_sql = format!(
        "{} LIMIT {} OFFSET {}",
        list_sql,
        window.stop - window.offset,
        window.offset
    );
    let rows = crate::app_issues::fetch_json_rows(&pool, &page_sql, values.clone())
        .await
        .map_err(|_| Denial::ServerError)?;
    let has_more = rows.len() as i64 > limit;
    let page: Vec<Map<String, Value>> = apply_offset_window(&rows, limit).map_err(page_denial)?;
    let total_count = crate::app_issues::fetch_count(&pool, &count_sql, values)
        .await
        .map_err(|_| Denial::ServerError)?;

    // Prefetches (`:192-209`): three separate `ANY($1)` queries over the
    // page's issue ids.
    let page_ids: Vec<uuid::Uuid> = page
        .iter()
        .filter_map(|row| {
            row.get("id")
                .and_then(|value| value.as_str())
                .and_then(|raw| raw.parse::<uuid::Uuid>().ok())
        })
        .collect();
    let mut assignee_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut label_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut module_map: HashMap<String, Vec<String>> = HashMap::new();
    if !page_ids.is_empty() {
        let assignee_rows: Vec<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
            "SELECT issue_id, assignee_id FROM issue_assignees WHERE issue_id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(&page_ids)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        for (issue_id, assignee_id) in assignee_rows {
            assignee_map
                .entry(issue_id.to_string())
                .or_default()
                .push(assignee_id.to_string());
        }
        let label_rows: Vec<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
            "SELECT issue_id, label_id FROM issue_labels WHERE issue_id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(&page_ids)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        for (issue_id, label_id) in label_rows {
            label_map
                .entry(issue_id.to_string())
                .or_default()
                .push(label_id.to_string());
        }
        let module_rows: Vec<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
            "SELECT issue_id, module_id FROM module_issues WHERE issue_id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(&page_ids)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        for (issue_id, module_id) in module_rows {
            module_map
                .entry(issue_id.to_string())
                .or_default()
                .push(module_id.to_string());
        }
    }
    let empty: Vec<String> = Vec::new();
    let mut shaped: Vec<String> = Vec::with_capacity(page.len());
    for row in &page {
        let id = row
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        shaped.push(shape_view_issue(
            row,
            &timezone,
            assignee_map.get(id).unwrap_or(&empty),
            label_map.get(id).unwrap_or(&empty),
            module_map.get(id).unwrap_or(&empty),
        ));
    }
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    Ok(json_response(
        StatusCode::OK,
        pidash_services::app_issues::envelope(
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

#[cfg(test)]
mod pidashconv_783_tests {
    use axum::http::StatusCode;
    use chrono_tz::Tz;

    use super::{archived_at_denial, parse_django_datetime, Denial, ParseDatetimeError};

    fn utc_tz() -> Tz {
        "UTC".parse().expect("utc")
    }

    #[test]
    fn archived_at_grammar() {
        // PIDASHCONV-783: mirror of the 772 `django_datetime_grammar` core —
        // normal datetimes pin the instant, unparseables the format arm.
        let utc = utc_tz();
        let instant = |text: &str| {
            parse_django_datetime(text, &utc)
                .expect("parses")
                .to_rfc3339()
        };
        assert_eq!(instant("2026-10-01T10:00:00Z"), "2026-10-01T10:00:00+00:00");
        assert_eq!(
            instant("2026-10-01T12:00:00+02:00"),
            "2026-10-01T10:00:00+00:00"
        );
        assert_eq!(instant("2026-10-01 10:00"), "2026-10-01T10:00:00+00:00");
        assert_eq!(instant("2026-10-01"), "2026-10-01T00:00:00+00:00");
        assert_eq!(
            instant("2026-10-01T10:00:00.123456Z"),
            "2026-10-01T10:00:00.123456+00:00"
        );
        assert_eq!(
            parse_django_datetime("garbage", &utc),
            Err(ParseDatetimeError::Invalid)
        );
        assert_eq!(
            parse_django_datetime("", &utc),
            Err(ParseDatetimeError::Invalid)
        );
        assert_eq!(
            parse_django_datetime("2026-13-01", &utc),
            Err(ParseDatetimeError::Invalid)
        );
    }

    #[test]
    fn archived_at_iso8601_literal_variants() {
        // PIDASHCONV-783: the `strptime(value, 'iso-8601')` fallthrough —
        // the literal in any ASCII letter case yields naive 1900-01-01.
        let utc = utc_tz();
        for text in [
            "iso-8601", "ISO-8601", "Iso-8601", "iSo-8601", "isO-8601", "ISo-8601", "IsO-8601",
            "iSO-8601",
        ] {
            assert_eq!(
                parse_django_datetime(text, &utc)
                    .expect("parses")
                    .to_rfc3339(),
                "1900-01-01T00:00:00+00:00",
                "{text:?}"
            );
        }
    }

    #[test]
    fn archived_at_zones_and_near_misses() {
        // PIDASHCONV-783: request-zone attach (gap/fold/offset) plus the
        // near-misses, which stay on the format arm.
        let utc = utc_tz();
        let eastern: Tz = "America/New_York".parse().expect("tz");
        assert_eq!(
            parse_django_datetime("2024-03-10T02:30:00", &eastern),
            Err(ParseDatetimeError::Nonexistent)
        );
        assert_eq!(
            parse_django_datetime("2024-11-03T01:30:00", &eastern)
                .expect("parses")
                .to_rfc3339(),
            "2024-11-03T05:30:00+00:00"
        );
        assert_eq!(
            parse_django_datetime("2026-10-01T10:00:00", &eastern)
                .expect("parses")
                .to_rfc3339(),
            "2026-10-01T14:00:00+00:00"
        );
        for text in [
            "iso8601",
            "xiso-8601",
            "iso-8601x",
            " iso-8601",
            "iso-8601 ",
            "iso-8601\n",
            "\tiso-8601",
        ] {
            assert_eq!(
                parse_django_datetime(text, &utc),
                Err(ParseDatetimeError::Invalid),
                "{text:?}"
            );
        }
    }

    #[test]
    fn archived_at_denial_bodies() {
        // PIDASHCONV-783: the DRF field-error bytes per arm.
        let eastern: Tz = "America/New_York".parse().expect("tz");
        let body = |error: &ParseDatetimeError, tz: &Tz| match archived_at_denial(error, tz) {
            Denial::Raw(status, text) => {
                assert_eq!(status, StatusCode::BAD_REQUEST);
                text
            }
            other => panic!("expected Raw denial, got {other:?}"),
        };
        assert_eq!(
            body(&ParseDatetimeError::Invalid, &eastern),
            r#"{"archived_at":["Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."]}"#
        );
        assert_eq!(
            body(&ParseDatetimeError::Nonexistent, &eastern),
            r#"{"archived_at":["Invalid datetime for the timezone \"America/New_York\"."]}"#
        );
        assert_eq!(
            body(&ParseDatetimeError::Overflow, &eastern),
            r#"{"archived_at":["Datetime value out of range."]}"#
        );
    }
}
