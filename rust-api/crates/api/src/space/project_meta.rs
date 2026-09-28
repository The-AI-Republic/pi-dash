//! Space project/meta/taxonomy handlers (PIDASHCONV-174, stage 4).
//!
//! Port of `apps/api/pi_dash/space/views/{project,meta,cycle,module,state,label}.py`
//! with route strings from `space/urls/project.py` plus the
//! `workspaces/<slug>/project-boards/` route served from `space/urls/intake.py`
//! (`WorkspaceProjectDeployBoardEndpoint`, also defined in `views/project.py`).
//! The `anchor/<anchor>/issues/` route in `urls/project.py` belongs to a
//! sibling issue and is NOT registered here.
//!
//! Nine `AllowAny` GETs (anonymous proceeds; an authenticated session only
//! selects the rendering timezone via `TimezoneMixin`, `views/base.py:31-42`):
//!
//! * `GET anchor/<anchor>/settings/` (`ProjectDeployBoardPublicSettingsEndpoint`,
//!   `views/project.py:19-25`)
//! * `GET workspaces/<slug>/project-boards/` (`WorkspaceProjectDeployBoardEndpoint`,
//!   `views/project.py:28-51`; dead path, always 500 — see BUG-boards below)
//! * `GET workspaces/<slug>/projects/<project_id>/anchor/`
//!   (`WorkspaceProjectAnchorEndpoint`, `views/project.py:54-62`)
//! * `GET anchor/<anchor>/members/` (`ProjectMembersEndpoint`, `:65-86`)
//! * `GET anchor/<anchor>/meta/` (`ProjectMetaDataEndpoint`, `views/meta.py:16-32`)
//! * `GET anchor/<anchor>/cycles/` (`views/cycle.py:15-28`)
//! * `GET anchor/<anchor>/modules/` (`views/module.py:15-28`)
//! * `GET anchor/<anchor>/states/` (`views/state.py:18-32`)
//! * `GET anchor/<anchor>/labels/` (`views/label.py:15-28`)
//!
//! Layering: SQL text comes from
//! `pidash_services::space::queries::project_meta`, anchor/error mapping from
//! `pidash_services::space::guards`, the meta leaf from
//! `pidash_services::space::serializers::lite`. This module owns the HTTP
//! shell (routes, session timezone, row fetching) plus the two reads no
//! query builder covers: the workspace-detail row and the logo/cover asset
//! row for the `DeployBoardSerializer` nests (see below).
//!
//! Reads go through `row_to_json` (the `app_issues` pattern): Postgres
//! renders every column and the handlers shape typed values from the JSON
//! object, so key order and scalar bytes stay under handler control.
//! Datetimes re-render through the serializer kernel in the request's zone
//! (anonymous → `deactivate()` → UTC, `views/base.py:42`).
//!
//! `DeployBoardSerializer` observed wire shape (record-only, verified against
//! live Django 2026-09-28): declared fields first (`id` from `BaseSerializer`,
//! then `project_details`, `workspace_detail`), then model definition order —
//! `created_at, updated_at, deleted_at, entity_identifier, entity_name,
//! anchor, is_comments_enabled, is_reactions_enabled, is_votes_enabled,
//! view_props, is_activity_enabled, is_disabled, created_by, updated_by,
//! workspace, project, intake`. FKs render as PK strings (`workspace`,
//! `project`, `intake`) or `null`; `project_details` is `null` when the
//! board's `project_id` is null (nested `None`); `project_details` uses the
//! app `ProjectLiteSerializer` keys
//! (`app/serializers/project.py:120-133`: `id, identifier, name,
//! cover_image, cover_image_url, logo_props, description, is_default`) and
//! `workspace_detail` the app `WorkspaceLiteSerializer` keys
//! (`app/serializers/workspace.py:79-83`: `name, slug, id, logo_url`).
//! `cover_image_url` / `logo_url` follow the model properties
//! (`db/models/project.py:176-185`, `db/models/workspace.py:146-153`):
//! the asset's `asset_url` when the FK is set (`db/models/asset.py:80-99`),
//! else the raw string column, else `null`.
//!
//! Forward-FK reads (`board.project`, `board.workspace`, the logo/cover
//! assets) use Django's `_base_manager`, which here IS the
//! `SoftDeletionManager` (no `base_manager_name` override in
//! `db/mixins.py:56-67`), so the nested fetches below keep the
//! `deleted_at IS NULL` guard — identical to the forward-FK fetches — and a
//! soft-deleted nested row surfaces the `ObjectDoesNotExist` envelope, the
//! same mapping DRF's dispatch produces for the Python `DoesNotExist`.
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * BUG-boards (`views/project.py:32`): `...values_list` without the call
//!   binds the method, so `.workspace` (`:34`) raises `AttributeError`
//!   before any SQL runs. Over HTTP this surfaces as the `handle_exception`
//!   fallback 500 (`{"error":"Something went wrong please try again later"}`):
//!   DRF's own dispatch catches the error and maps it; the outer
//!   `dispatch` `return exc` (`views/base.py:199-200`) never fires because
//!   nothing escapes DRF's dispatch (same reachability note as the license
//!   `handlers_base`). [`get_workspace_boards`] reproduces the wire effect
//!   by mapping the queries layer's `BoardsError::ValuesListNotCalled` onto
//!   the 500 envelope. (The view's `get(self, request, anchor)` signature
//!   could never receive the URL's `slug` kwarg either — doubly dead.)
//! * QUIRK-unscoped-board (`views/cycle.py:19`, `views/module.py:19`,
//!   `views/project.py:69`, `views/state.py:23`, `views/label.py:21`): the
//!   taxonomy/member board lookup has no `entity_name="project"` scoping —
//!   ported as-is via `board_by_anchor_first_sql`.
//! * BUG-triage-name (`views/state.py:27`): triage excluded by NAME
//!   (`~Q(name="Triage")`) on top of the manager's `group != 'triage'` half —
//!   ported as-is via `states_sql`.
//!
//! Non-obvious faithful corners:
//!
//! * `filter(project=None)` renders `project_id IS NULL`, never `= NULL`:
//!   when the resolved board carries no `project_id`, the scoped-list
//!   queries swap that one predicate ([`nullable_project_sql`]).
//! * `workspaces/<slug>/projects/<project_id>/anchor/` with a non-UUID
//!   `project_id` matches no Django route (the `<uuid:>` converter); the
//!   handler proxies to Django so Django's own 404 stays the contract.
//! * DRF always renders Python floats with a decimal point (`65535.0`):
//!   `sequence` is decoded as `f64` and re-rendered, never passed through
//!   as row text (Postgres `float8out` would print `65535`).

use axum::extract::{Extension, Path, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;

use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_services::space::guards::{self, AnchorLookup, ErrorBody, ExceptionKind};
use pidash_services::space::queries::project_meta as queries;
use pidash_services::space::serializers::lite;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the nine project/meta/taxonomy GET routes under `api/public/`.
/// Nothing else: sibling paths (notably `anchor/<anchor>/issues/`) stay
/// unmatched and proxy to Django.
///
/// Non-GET methods on owned paths proxy too (same `owned` shape as the
/// `app_issues` list family): DRF authenticates before it checks the method
/// and sibling splits own the write endpoints — answering 405 in Rust would
/// break both. `HEAD` rides axum's `get` handling like Django's `GET`-backed
/// `HEAD`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/public/anchor/{anchor}/settings/",
            owned(get(get_settings)),
        )
        .route(
            "/api/public/workspaces/{slug}/project-boards/",
            owned(get(get_workspace_boards)),
        )
        .route(
            "/api/public/workspaces/{slug}/projects/{project_id}/anchor/",
            owned(get(get_workspace_anchor)),
        )
        .route(
            "/api/public/anchor/{anchor}/members/",
            owned(get(get_members)),
        )
        .route("/api/public/anchor/{anchor}/meta/", owned(get(get_meta)))
        .route(
            "/api/public/anchor/{anchor}/cycles/",
            owned(get(get_cycles)),
        )
        .route(
            "/api/public/anchor/{anchor}/modules/",
            owned(get(get_modules)),
        )
        .route(
            "/api/public/anchor/{anchor}/states/",
            owned(get(get_states)),
        )
        .route(
            "/api/public/anchor/{anchor}/labels/",
            owned(get(get_labels)),
        )
}

/// A project/meta path: the GET handler owns reads, everything else falls
/// through to Django. OPTIONS proxies too: DRF answers metadata where axum
/// would 405.
fn owned(
    get_handler: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    get_handler
        .post(crate::edge::proxy)
        .put(crate::edge::proxy)
        .patch(crate::edge::proxy)
        .delete(crate::edge::proxy)
        .options(crate::edge::proxy)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Handler failure: the exact status + body Python answers. Variants map
/// one-to-one onto the guards layer (`resolve_anchor` for board misses,
/// `handle_exception` for the envelope): the `handle_exception`
/// `ObjectDoesNotExist` / fallback branches (`views/base.py:170-186`) plus
/// the endpoint-inline 404s (`Invalid anchor`, `Project is not published`).
#[derive(Debug)]
enum HandlerError {
    /// 404 `{"error":"The required object does not exist."}` — bare
    /// `.get()` misses (settings, workspace-anchor) and soft-deleted nested
    /// rows (project/workspace details).
    ObjectNotFound,
    /// 404 `{"error":"Invalid anchor"}` — taxonomy/member board misses.
    InvalidAnchor,
    /// 404 `{"error":"Project is not published"}` — meta board/project misses.
    NotPublished,
    /// 500 `{"error":"Something went wrong please try again later"}` —
    /// the fallback envelope (boards bug, DB/row-shape failures).
    ServerError,
}

impl HandlerError {
    fn error_body(&self) -> ErrorBody {
        match self {
            HandlerError::ObjectNotFound => {
                guards::handle_exception(ExceptionKind::ObjectDoesNotExist)
            }
            HandlerError::InvalidAnchor => guards::invalid_anchor(),
            HandlerError::NotPublished => guards::project_not_published(),
            HandlerError::ServerError => guards::handle_exception(ExceptionKind::Other),
        }
    }
}

impl IntoResponse for HandlerError {
    fn into_response(self) -> Response {
        if matches!(self, HandlerError::ServerError) {
            // `log_exception(e)` (`views/base.py:182`).
            tracing::warn!("space project_meta handler: internal error");
        }
        let body = self.error_body();
        let status = StatusCode::from_u16(body.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        json_response(status, body.body.to_string())
    }
}

/// Render `body` (already exact JSON bytes) as a JSON response.
fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, HandlerError> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(HandlerError::ServerError)
}

// ---------------------------------------------------------------------------
// Session actor + request timezone (TimezoneMixin, views/base.py:31-42)
// ---------------------------------------------------------------------------

/// The request's time zone. Anonymous (no session, unknown user, inactive
/// user) → `deactivate()` → UTC (`views/base.py:42`); authenticated →
/// `activate(ZoneInfo(user_timezone))` (`:39-40`) with no `try/except`, so an
/// unknown zone (or a missing one) is a 500 through the fallback branch.
/// Mirrors the license `request_actor` shape (same `BaseSessionAuthentication`
/// without CSRF); these routes are `AllowAny`, so `None` proceeds.
async fn request_tz(
    pool: &sqlx::PgPool,
    extension: Option<Extension<SessionHandle>>,
) -> Result<chrono_tz::Tz, HandlerError> {
    let raw = extension
        .and_then(|Extension(handle)| {
            handle
                .snapshot()
                .get("_auth_user_id")
                .and_then(|v| v.as_str().map(str::to_owned))
        })
        .and_then(|raw| raw.parse::<uuid::Uuid>().ok());
    let id = match raw {
        Some(id) => id,
        None => return Ok(chrono_tz::UTC),
    };
    let row: Option<(bool, Option<String>)> =
        sqlx::query_as("SELECT is_active, user_timezone FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    match row {
        Some((true, timezone)) => timezone
            .as_deref()
            .unwrap_or("")
            .parse::<chrono_tz::Tz>()
            .map_err(|_| HandlerError::ServerError),
        _ => Ok(chrono_tz::UTC),
    }
}

/// Render a `row_to_json` timestamptz value exactly like DRF `DateTimeField`
/// with the default `iso-8601` format: ISO-8601 in the request's zone with
/// `+00:00` rewritten to `Z`, microseconds only when nonzero (kernel rule).
fn render_dt(raw: &str, tz: &chrono_tz::Tz) -> Result<String, HandlerError> {
    let dt = chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| HandlerError::ServerError)?;
    Ok(crate::serializer::render_datetime_in(&dt, tz))
}

// ---------------------------------------------------------------------------
// row_to_json fetching + field access
// ---------------------------------------------------------------------------

/// One bound `$N` parameter. UUID columns must bind typed `Uuid`: binding
/// text fails at runtime (`operator does not exist: uuid = text`). Text
/// columns (`anchor`, `slug`) bind as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqlParam<'a> {
    Text(&'a str),
    Uuid(uuid::Uuid),
}

/// Parse a UUID-typed bind. Callers pass path segments Django's `<uuid:>`
/// converter already validated, or ids read back from the database —
/// anything else is a 500 like Python's `ValidationError` → fallback.
fn uuid_param(raw: &str) -> Result<SqlParam<'_>, HandlerError> {
    uuid::Uuid::parse_str(raw)
        .map(SqlParam::Uuid)
        .map_err(|_| HandlerError::ServerError)
}

/// Fetch zero or one row of `inner` as a JSON object. `inner` is a query
/// builder's SQL with `$N` placeholders bound positionally from `params`.
async fn fetch_optional_object(
    pool: &sqlx::PgPool,
    inner: &str,
    params: &[SqlParam<'_>],
) -> Result<Option<Value>, HandlerError> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let mut query = sqlx::query(&sql);
    for param in params {
        query = match param {
            SqlParam::Text(text) => query.bind(*text),
            SqlParam::Uuid(id) => query.bind(*id),
        };
    }
    let row: Option<sqlx::postgres::PgRow> = query
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    match row {
        None => Ok(None),
        Some(row) => {
            let text: String = row
                .try_get("__row")
                .map_err(|_| HandlerError::ServerError)?;
            let value: Value =
                serde_json::from_str(&text).map_err(|_| HandlerError::ServerError)?;
            match value {
                Value::Object(_) => Ok(Some(value)),
                _ => Err(HandlerError::ServerError),
            }
        }
    }
}

/// Fetch every row of `inner` as JSON objects.
async fn fetch_all_objects(
    pool: &sqlx::PgPool,
    inner: &str,
    params: &[SqlParam<'_>],
) -> Result<Vec<Value>, HandlerError> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let mut query = sqlx::query(&sql);
    for param in params {
        query = match param {
            SqlParam::Text(text) => query.bind(*text),
            SqlParam::Uuid(id) => query.bind(*id),
        };
    }
    let rows: Vec<sqlx::postgres::PgRow> = query
        .fetch_all(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row
            .try_get("__row")
            .map_err(|_| HandlerError::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| HandlerError::ServerError)?;
        match value {
            Value::Object(_) => out.push(value),
            _ => return Err(HandlerError::ServerError),
        }
    }
    Ok(out)
}

fn obj(value: &Value) -> Result<&serde_json::Map<String, Value>, HandlerError> {
    value.as_object().ok_or(HandlerError::ServerError)
}

fn req_str(obj: &serde_json::Map<String, Value>, key: &str) -> Result<String, HandlerError> {
    match obj.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(HandlerError::ServerError),
    }
}

fn opt_str(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, HandlerError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(HandlerError::ServerError),
    }
}

fn req_bool(obj: &serde_json::Map<String, Value>, key: &str) -> Result<bool, HandlerError> {
    match obj.get(key) {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(HandlerError::ServerError),
    }
}

fn req_json(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Value, HandlerError> {
    obj.get(key).cloned().ok_or(HandlerError::ServerError)
}

fn req_f64(obj: &serde_json::Map<String, Value>, key: &str) -> Result<f64, HandlerError> {
    match obj.get(key) {
        Some(Value::Number(n)) => n.as_f64().ok_or(HandlerError::ServerError),
        _ => Err(HandlerError::ServerError),
    }
}

/// Django renders `filter(project=None)` as `project_id IS NULL`, never
/// `= NULL` (a `= $N` bind of null matches nothing). The builders record the
/// non-null `=` shape (the fixture's case); when the resolved board carries
/// no `project_id`, swap that one predicate. `param` is the builder's `$N`
/// token for the project id.
fn nullable_project_sql(sql: &str, param: &str) -> String {
    sql.replacen(
        &format!("\"project_id\" = {param}"),
        "\"project_id\" IS NULL",
        1,
    )
}

// ---------------------------------------------------------------------------
// Typed rows
// ---------------------------------------------------------------------------

/// One `deploy_boards` row (the builders' 18-column `BOARD_COLUMNS` shape).
/// Datetimes stay raw `row_to_json` text until render (request zone).
struct BoardRow {
    id: String,
    created_at: String,
    updated_at: String,
    created_by_id: Option<String>,
    updated_by_id: Option<String>,
    deleted_at: Option<String>,
    workspace_id: String,
    project_id: Option<String>,
    entity_identifier: Option<String>,
    entity_name: Option<String>,
    anchor: String,
    is_comments_enabled: bool,
    is_reactions_enabled: bool,
    intake_id: Option<String>,
    is_votes_enabled: bool,
    view_props: Value,
    is_activity_enabled: bool,
    is_disabled: bool,
}

impl BoardRow {
    fn from_value(value: &Value) -> Result<Self, HandlerError> {
        let o = obj(value)?;
        Ok(Self {
            id: req_str(o, "id")?,
            created_at: req_str(o, "created_at")?,
            updated_at: req_str(o, "updated_at")?,
            created_by_id: opt_str(o, "created_by_id")?,
            updated_by_id: opt_str(o, "updated_by_id")?,
            deleted_at: opt_str(o, "deleted_at")?,
            workspace_id: req_str(o, "workspace_id")?,
            project_id: opt_str(o, "project_id")?,
            entity_identifier: opt_str(o, "entity_identifier")?,
            entity_name: opt_str(o, "entity_name")?,
            anchor: req_str(o, "anchor")?,
            is_comments_enabled: req_bool(o, "is_comments_enabled")?,
            is_reactions_enabled: req_bool(o, "is_reactions_enabled")?,
            intake_id: opt_str(o, "intake_id")?,
            is_votes_enabled: req_bool(o, "is_votes_enabled")?,
            view_props: req_json(o, "view_props")?,
            is_activity_enabled: req_bool(o, "is_activity_enabled")?,
            is_disabled: req_bool(o, "is_disabled")?,
        })
    }
}

/// The project columns both the board serializer (`project_details`) and the
/// meta leaf need. Fetched with `project_get_sql` (filtered, same as
/// `Project.objects.get`); the board-serializer path reads the same row
/// Django's forward FK would (the managers coincide — see module docs).
struct ProjectRow {
    id: String,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_image_asset_id: Option<String>,
    icon_prop: Value,
    emoji: Option<String>,
    logo_props: Value,
    description: Option<String>,
    is_default: bool,
}

impl ProjectRow {
    fn from_value(value: &Value) -> Result<Self, HandlerError> {
        let o = obj(value)?;
        Ok(Self {
            id: req_str(o, "id")?,
            identifier: req_str(o, "identifier")?,
            name: req_str(o, "name")?,
            cover_image: opt_str(o, "cover_image")?,
            cover_image_asset_id: opt_str(o, "cover_image_asset_id")?,
            icon_prop: req_json(o, "icon_prop")?,
            emoji: opt_str(o, "emoji")?,
            logo_props: req_json(o, "logo_props")?,
            description: opt_str(o, "description")?,
            is_default: req_bool(o, "is_default")?,
        })
    }
}

/// The workspace columns `workspace_detail` needs.
struct WorkspaceRow {
    id: String,
    name: String,
    slug: String,
    logo: Option<String>,
    logo_asset_id: Option<String>,
}

/// `SELECT` for the workspace-detail row. No query builder covers it (the
/// Python reads it through the board's forward FK inside the serializer);
/// the `deleted_at IS NULL` guard matches that fetch (module docs).
fn workspace_detail_sql() -> String {
    "SELECT \"workspaces\".\"id\", \"workspaces\".\"name\", \"workspaces\".\"slug\", \"workspaces\".\"logo\", \"workspaces\".\"logo_asset_id\" FROM \"workspaces\" WHERE (\"workspaces\".\"deleted_at\" IS NULL AND \"workspaces\".\"id\" = $1)".to_string()
}

impl WorkspaceRow {
    fn from_value(value: &Value) -> Result<Self, HandlerError> {
        let o = obj(value)?;
        Ok(Self {
            id: req_str(o, "id")?,
            name: req_str(o, "name")?,
            slug: req_str(o, "slug")?,
            logo: opt_str(o, "logo")?,
            logo_asset_id: opt_str(o, "logo_asset_id")?,
        })
    }
}

// ---------------------------------------------------------------------------
// logo_url / cover_image_url (model properties)
// ---------------------------------------------------------------------------

/// `SELECT` for the logo/cover asset row. No query builder covers it (the
/// Python reads it through the FK inside the property); the
/// `deleted_at IS NULL` guard matches that fetch (module docs). The
/// workspace slug rides along for the attachment URL branches.
fn logo_asset_sql() -> String {
    "SELECT \"a\".\"id\", \"a\".\"entity_type\", \"a\".\"workspace_id\", \"a\".\"project_id\", \"a\".\"issue_id\", \"w\".\"slug\" AS \"workspace_slug\" FROM \"file_assets\" AS \"a\" LEFT OUTER JOIN \"workspaces\" AS \"w\" ON (\"a\".\"workspace_id\" = \"w\".\"id\") WHERE (\"a\".\"deleted_at\" IS NULL AND \"a\".\"id\" = $1)".to_string()
}

/// Port of `FileAsset.asset_url` (`db/models/asset.py:80-99`): static-asset
/// branches render `/api/assets/v2/static/<id>/`; attachment/description
/// branches interpolate the workspace slug and ids; anything else (including
/// a null `entity_type`) renders `None`.
fn asset_url(
    asset_id: &str,
    entity_type: Option<&str>,
    workspace_slug: Option<&str>,
    project_id: Option<&str>,
    issue_id: Option<&str>,
) -> Option<String> {
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Some(format!("/api/assets/v2/static/{asset_id}/"))
        }
        Some("ISSUE_ATTACHMENT") => {
            let (slug, project, issue) = match (workspace_slug, project_id, issue_id) {
                (Some(s), Some(p), Some(i)) => (s, p, i),
                _ => return None,
            };
            Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/issues/{issue}/attachments/{asset_id}/"
            ))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let (slug, project) = match (workspace_slug, project_id) {
                (Some(s), Some(p)) => (s, p),
                _ => return None,
            };
            Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/{asset_id}/"
            ))
        }
        _ => None,
    }
}

/// Resolve a logo/cover URL like the model properties do
/// (`db/models/workspace.py:146-153`, `db/models/project.py:176-185`): the
/// asset's `asset_url` when the FK is set (even when that computes to
/// `None`), else the raw string column, else `None`. A dangling FK is a 500
/// like Python's `DoesNotExist` → fallback envelope (unreachable under FK
/// integrity).
async fn logo_or_cover_url(
    pool: &sqlx::PgPool,
    raw: Option<String>,
    asset_id: Option<String>,
) -> Result<Option<String>, HandlerError> {
    let Some(asset_id) = asset_id else {
        return Ok(raw);
    };
    let row = fetch_optional_object(pool, &logo_asset_sql(), &[uuid_param(&asset_id)?])
        .await?
        .ok_or(HandlerError::ServerError)?;
    let o = obj(&row)?;
    let entity_type = opt_str(o, "entity_type")?;
    let workspace_slug = opt_str(o, "workspace_slug")?;
    let project_id = opt_str(o, "project_id")?;
    let issue_id = opt_str(o, "issue_id")?;
    let id = req_str(o, "id")?;
    Ok(asset_url(
        &id,
        entity_type.as_deref(),
        workspace_slug.as_deref(),
        project_id.as_deref(),
        issue_id.as_deref(),
    ))
}

// ---------------------------------------------------------------------------
// Wire shapes (DeployBoardSerializer observed field order)
// ---------------------------------------------------------------------------

/// `project_details`: app `ProjectLiteSerializer`
/// (`app/serializers/project.py:120-133`), in `Meta.fields` order.
#[derive(Debug, Serialize)]
struct ProjectDetailsView {
    id: String,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_image_url: Option<String>,
    logo_props: Value,
    description: Option<String>,
    is_default: bool,
}

/// `workspace_detail`: app `WorkspaceLiteSerializer`
/// (`app/serializers/workspace.py:79-83`), in `Meta.fields` order.
#[derive(Debug, Serialize)]
struct WorkspaceDetailView {
    name: String,
    slug: String,
    id: String,
    logo_url: Option<String>,
}

/// `DeployBoardSerializer` (`app/serializers/project.py:259-266`):
/// declared fields first (`id` from `BaseSerializer`, then the two nests),
/// then model definition order (verified against live Django).
#[derive(Debug, Serialize)]
struct BoardView {
    id: String,
    project_details: Option<ProjectDetailsView>,
    workspace_detail: WorkspaceDetailView,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    entity_identifier: Option<String>,
    entity_name: Option<String>,
    anchor: String,
    is_comments_enabled: bool,
    is_reactions_enabled: bool,
    is_votes_enabled: bool,
    view_props: Value,
    is_activity_enabled: bool,
    is_disabled: bool,
    created_by: Option<String>,
    updated_by: Option<String>,
    workspace: String,
    project: Option<String>,
    intake: Option<String>,
}

/// Render one board row with its nests. `Board` fetch misses for the
/// nested project/workspace surface the `ObjectDoesNotExist` envelope, the
/// same mapping DRF's dispatch produces for the Python `DoesNotExist`.
async fn render_board(
    pool: &sqlx::PgPool,
    board: &BoardRow,
    tz: &chrono_tz::Tz,
) -> Result<String, HandlerError> {
    let project_details = match &board.project_id {
        None => None,
        Some(project_id) => {
            let row = fetch_optional_object(
                pool,
                &queries::project_get_sql(),
                &[uuid_param(project_id)?],
            )
            .await?
            .ok_or(HandlerError::ObjectNotFound)?;
            let project = ProjectRow::from_value(&row)?;
            let cover_image_url = logo_or_cover_url(
                pool,
                project.cover_image.clone(),
                project.cover_image_asset_id.clone(),
            )
            .await?;
            Some(ProjectDetailsView {
                id: project.id,
                identifier: project.identifier,
                name: project.name,
                cover_image: project.cover_image,
                cover_image_url,
                logo_props: project.logo_props,
                description: project.description,
                is_default: project.is_default,
            })
        }
    };
    let workspace_row = fetch_optional_object(
        pool,
        &workspace_detail_sql(),
        &[uuid_param(&board.workspace_id)?],
    )
    .await?
    .ok_or(HandlerError::ObjectNotFound)?;
    let workspace = WorkspaceRow::from_value(&workspace_row)?;
    let logo_url = logo_or_cover_url(
        pool,
        workspace.logo.clone(),
        workspace.logo_asset_id.clone(),
    )
    .await?;
    let deleted_at = match &board.deleted_at {
        None => None,
        Some(raw) => Some(render_dt(raw, tz)?),
    };
    let view = BoardView {
        id: board.id.clone(),
        project_details,
        workspace_detail: WorkspaceDetailView {
            name: workspace.name,
            slug: workspace.slug,
            id: workspace.id,
            logo_url,
        },
        created_at: render_dt(&board.created_at, tz)?,
        updated_at: render_dt(&board.updated_at, tz)?,
        deleted_at,
        entity_identifier: board.entity_identifier.clone(),
        entity_name: board.entity_name.clone(),
        anchor: board.anchor.clone(),
        is_comments_enabled: board.is_comments_enabled,
        is_reactions_enabled: board.is_reactions_enabled,
        is_votes_enabled: board.is_votes_enabled,
        view_props: board.view_props.clone(),
        is_activity_enabled: board.is_activity_enabled,
        is_disabled: board.is_disabled,
        created_by: board.created_by_id.clone(),
        updated_by: board.updated_by_id.clone(),
        workspace: board.workspace_id.clone(),
        project: board.project_id.clone(),
        intake: board.intake_id.clone(),
    };
    serde_json::to_string(&view).map_err(|_| HandlerError::ServerError)
}

/// Render the meta leaf through the reviewed lite serializer
/// (`space/serializer/project.py:10-22`): `id, identifier, name,
/// cover_image, icon_prop, emoji, description`.
fn render_meta(project: &ProjectRow) -> Result<String, HandlerError> {
    let description = project
        .description
        .as_deref()
        .ok_or(HandlerError::ServerError)?;
    let row = lite::ProjectLiteRow {
        id: &project.id,
        identifier: &project.identifier,
        name: &project.name,
        cover_image: project.cover_image.as_deref(),
        icon_prop: &project.icon_prop,
        emoji: project.emoji.as_deref(),
        description,
    };
    serde_json::to_string(&lite::project_lite_to_representation(&row))
        .map_err(|_| HandlerError::ServerError)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `ProjectDeployBoardPublicSettingsEndpoint.get` (`views/project.py:22-25`):
/// `DeployBoard.objects.get(anchor, entity_name="project")` with no
/// `try/except` — a miss raises through to the `ObjectDoesNotExist` envelope
/// (`AnchorLookup::GetRaises`, guards).
async fn get_settings(
    State(state): State<AppState>,
    Path(anchor): Path<String>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    match settings_inner(&state, &anchor, extension).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(e) => e.into_response(),
    }
}

async fn settings_inner(
    state: &AppState,
    anchor: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let tz = request_tz(pool, extension).await?;
    let row = fetch_optional_object(
        pool,
        &queries::settings_get_sql(),
        &[SqlParam::Text(anchor)],
    )
    .await?;
    if guards::resolve_anchor(AnchorLookup::GetRaises, row.is_some()).is_err() {
        return Err(HandlerError::ObjectNotFound);
    }
    render_board(
        pool,
        &BoardRow::from_value(&row.unwrap_or(Value::Null))?,
        &tz,
    )
    .await
}

/// `WorkspaceProjectDeployBoardEndpoint.get` (`views/project.py:28-51`):
/// BUG-boards — `...values_list` without the call raises before any SQL
/// runs. The queries layer models this as `Err(BoardsError)`, mapped here
/// onto the 500 fallback envelope Django answers over HTTP (module docs).
async fn get_workspace_boards(State(state): State<AppState>) -> Response {
    match workspace_boards_inner(&state) {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(e) => e.into_response(),
    }
}

fn workspace_boards_inner(_state: &AppState) -> Result<String, HandlerError> {
    match queries::workspace_boards_sql("unreached") {
        Ok(_) => Err(HandlerError::ServerError),
        Err(_) => Err(HandlerError::ServerError),
    }
}

/// `WorkspaceProjectAnchorEndpoint.get` (`views/project.py:57-62`):
/// `DeployBoard.objects.get(workspace__slug, project_id,
/// entity_name="project")` with no `try/except` — a miss raises through to
/// the `ObjectDoesNotExist` envelope. A non-UUID `project_id` matches no
/// Django route (the `<uuid:>` converter), so it proxies to Django and
/// Django's own 404 stays the contract.
async fn get_workspace_anchor(
    State(state): State<AppState>,
    Path((slug, project_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&project_id).is_err() {
        return crate::edge::proxy(State(state), req).await;
    }
    match workspace_anchor_inner(&state, &slug, &project_id, extension).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(e) => e.into_response(),
    }
}

async fn workspace_anchor_inner(
    state: &AppState,
    slug: &str,
    project_id: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let tz = request_tz(pool, extension).await?;
    // The combo is not unique, so `.get()` semantics need the count:
    // 0 → DoesNotExist envelope, 1 → the row, >1 →
    // MultipleObjectsReturned → unmapped → fallback 500.
    let rows = fetch_all_objects(
        pool,
        &queries::anchor_get_sql(),
        &[SqlParam::Text(slug), uuid_param(project_id)?],
    )
    .await?;
    let row = match rows.len() {
        0 => return Err(HandlerError::ObjectNotFound),
        1 => &rows[0],
        _ => return Err(HandlerError::ServerError),
    };
    render_board(pool, &BoardRow::from_value(row)?, &tz).await
}

/// Resolve the shared taxonomy/member board closure
/// (`DeployBoard.objects.filter(anchor=...).first()`, unscoped —
/// QUIRK-unscoped-board): a miss answers `{"error":"Invalid anchor"}` 404
/// inline (`AnchorLookup::FilterFirstInvalidAnchor`, guards).
async fn resolve_board_first(pool: &sqlx::PgPool, anchor: &str) -> Result<BoardRow, HandlerError> {
    let row = fetch_optional_object(
        pool,
        &queries::board_by_anchor_first_sql(),
        &[SqlParam::Text(anchor)],
    )
    .await?;
    if guards::resolve_anchor(AnchorLookup::FilterFirstInvalidAnchor, row.is_some()).is_err() {
        return Err(HandlerError::InvalidAnchor);
    }
    BoardRow::from_value(&row.unwrap_or(Value::Null))
}

/// Bind params for a workspace/project-scoped list: the board's ids, with
/// the `IS NULL` predicate when the board carries no project
/// ([`nullable_project_sql`]). Returns the (possibly rewritten) SQL plus the
/// typed bind values.
fn scoped_list_query<'a>(
    builder_sql: &str,
    project_param: &str,
    workspace_id: &'a str,
    project_id: Option<&'a str>,
) -> Result<(String, Vec<SqlParam<'a>>), HandlerError> {
    match project_id {
        Some(pid) => {
            // Param order differs per builder; the two shapes are:
            // members ($1=project, $2=workspace) and the taxonomy reads
            // ($1=workspace, $2=project). Rebuild positionally from the
            // token the caller names. UUID columns bind typed (see
            // [`SqlParam`]).
            let project = uuid_param(pid)?;
            let workspace = uuid_param(workspace_id)?;
            let params = if project_param == "$1" {
                vec![project, workspace]
            } else {
                vec![workspace, project]
            };
            Ok((builder_sql.to_owned(), params))
        }
        None => Ok((
            nullable_project_sql(builder_sql, project_param),
            vec![uuid_param(workspace_id)?],
        )),
    }
}

async fn scoped_list(
    pool: &sqlx::PgPool,
    builder_sql: &str,
    project_param: &str,
    board: &BoardRow,
) -> Result<Vec<Value>, HandlerError> {
    let (sql, params) = scoped_list_query(
        builder_sql,
        project_param,
        &board.workspace_id,
        board.project_id.as_deref(),
    )?;
    fetch_all_objects(pool, &sql, &params).await
}

/// `ProjectMembersEndpoint.get` (`views/project.py:68-86`): invalid anchor →
/// 404 inline; otherwise the verbatim 4-key `.values(...)` list.
async fn get_members(State(state): State<AppState>, Path(anchor): Path<String>) -> Response {
    match members_inner(&state, &anchor).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(e) => e.into_response(),
    }
}

async fn members_inner(state: &AppState, anchor: &str) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let board = resolve_board_first(pool, anchor).await?;
    let rows = scoped_list(pool, &queries::members_sql(), "$1", &board).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let o = obj(row)?;
        out.push(queries::MemberRow {
            id: req_str(o, "id")?,
            member: opt_str(o, "member")?,
            member_display_name: opt_str(o, "member__display_name")?,
            member_avatar: opt_str(o, "member__avatar")?,
        });
    }
    serde_json::to_string(&out).map_err(|_| HandlerError::ServerError)
}

/// `ProjectMetaDataEndpoint.get` (`views/meta.py:19-32`): board miss OR
/// project miss → `{"error":"Project is not published"}` 404
/// (`AnchorLookup::GetMeta`, guards); otherwise the lite leaf.
async fn get_meta(State(state): State<AppState>, Path(anchor): Path<String>) -> Response {
    match meta_inner(&state, &anchor).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(e) => e.into_response(),
    }
}

async fn meta_inner(state: &AppState, anchor: &str) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let board_row = fetch_optional_object(
        pool,
        &queries::settings_get_sql(),
        &[SqlParam::Text(anchor)],
    )
    .await?;
    if guards::resolve_anchor(AnchorLookup::GetMeta, board_row.is_some()).is_err() {
        return Err(HandlerError::NotPublished);
    }
    let board = BoardRow::from_value(&board_row.unwrap_or(Value::Null))?;
    // `project_id = deploy_board.entity_identifier` (`views/meta.py:26-27`).
    let entity_identifier = board.entity_identifier.ok_or(HandlerError::NotPublished)?;
    let project_row = fetch_optional_object(
        pool,
        &queries::project_get_sql(),
        &[uuid_param(&entity_identifier)?],
    )
    .await?;
    if guards::resolve_anchor(AnchorLookup::GetMeta, project_row.is_some()).is_err() {
        return Err(HandlerError::NotPublished);
    }
    render_meta(&ProjectRow::from_value(
        &project_row.unwrap_or(Value::Null),
    )?)
}

/// `ProjectCyclesEndpoint.get` (`views/cycle.py:18-28`).
async fn get_cycles(State(state): State<AppState>, Path(anchor): Path<String>) -> Response {
    match taxonomy_inner(&state, &anchor, &queries::cycles_sql(), "$2").await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(e) => e.into_response(),
    }
}

/// `ProjectModulesEndpoint.get` (`views/module.py:18-28`).
async fn get_modules(State(state): State<AppState>, Path(anchor): Path<String>) -> Response {
    match taxonomy_inner(&state, &anchor, &queries::modules_sql(), "$2").await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(e) => e.into_response(),
    }
}

/// Shared cycles/modules body: the verbatim 2-key `.values("id", "name")` list.
async fn taxonomy_inner(
    state: &AppState,
    anchor: &str,
    builder_sql: &str,
    project_param: &str,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let board = resolve_board_first(pool, anchor).await?;
    let rows = scoped_list(pool, builder_sql, project_param, &board).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let o = obj(row)?;
        out.push(queries::TaxonomyRow {
            id: req_str(o, "id")?,
            name: req_str(o, "name")?,
        });
    }
    serde_json::to_string(&out).map_err(|_| HandlerError::ServerError)
}

/// `ProjectStatesEndpoint.get` (`views/state.py:21-32`): triage excluded by
/// NAME plus the manager half (both in `states_sql`); the verbatim 5-key
/// `.values(...)` list in view key order.
async fn get_states(State(state): State<AppState>, Path(anchor): Path<String>) -> Response {
    match states_inner(&state, &anchor).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(e) => e.into_response(),
    }
}

async fn states_inner(state: &AppState, anchor: &str) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let board = resolve_board_first(pool, anchor).await?;
    let rows = scoped_list(pool, &queries::states_sql(), "$2", &board).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let o = obj(row)?;
        out.push(queries::StateRow {
            name: req_str(o, "name")?,
            group: req_str(o, "group")?,
            color: req_str(o, "color")?,
            id: req_str(o, "id")?,
            sequence: req_f64(o, "sequence")?,
        });
    }
    serde_json::to_string(&out).map_err(|_| HandlerError::ServerError)
}

/// `ProjectLabelsEndpoint.get` (`views/label.py:18-28`): the verbatim 4-key
/// `.values(...)` list.
async fn get_labels(State(state): State<AppState>, Path(anchor): Path<String>) -> Response {
    match labels_inner(&state, &anchor).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(e) => e.into_response(),
    }
}

async fn labels_inner(state: &AppState, anchor: &str) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let board = resolve_board_first(pool, anchor).await?;
    let rows = scoped_list(pool, &queries::labels_sql(), "$2", &board).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let o = obj(row)?;
        out.push(queries::LabelRow {
            id: req_str(o, "id")?,
            name: req_str(o, "name")?,
            color: req_str(o, "color")?,
            parent: opt_str(o, "parent")?,
        });
    }
    serde_json::to_string(&out).map_err(|_| HandlerError::ServerError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    /// The live-Django settings body shape recorded 2026-09-28 (probe
    /// against the oracle): declared fields first, then model definition
    /// order. The render below must reproduce the key order exactly.
    const DJANGO_SETTINGS_KEYS: [&str; 20] = [
        "id",
        "project_details",
        "workspace_detail",
        "created_at",
        "updated_at",
        "deleted_at",
        "entity_identifier",
        "entity_name",
        "anchor",
        "is_comments_enabled",
        "is_reactions_enabled",
        "is_votes_enabled",
        "view_props",
        "is_activity_enabled",
        "is_disabled",
        "created_by",
        "updated_by",
        "workspace",
        "project",
        "intake",
    ];

    fn board_fixture() -> BoardRow {
        BoardRow {
            id: "1682a552-fc7f-4b30-b8a4-4f41bcc674ec".to_owned(),
            created_at: "2026-09-28T09:09:58.403293+00:00".to_owned(),
            updated_at: "2026-09-28T09:09:58.403293+00:00".to_owned(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            workspace_id: "3d914201-e7b6-449b-b92f-3ab0bd4f330d".to_owned(),
            project_id: Some("62d604a8-db82-44de-9e8c-a3e4a0f28371".to_owned()),
            entity_identifier: Some("62d604a8-db82-44de-9e8c-a3e4a0f28371".to_owned()),
            entity_name: Some("project".to_owned()),
            anchor: "65c976ab1fe196369d8d171294afde39".to_owned(),
            is_comments_enabled: true,
            is_reactions_enabled: true,
            intake_id: Some("0fb49a3e-91db-46d9-89eb-b985d1af3b26".to_owned()),
            is_votes_enabled: true,
            view_props: serde_json::json!({}),
            is_activity_enabled: true,
            is_disabled: false,
        }
    }

    #[test]
    fn board_view_key_order_matches_django() {
        let view = BoardView {
            id: board_fixture().id.clone(),
            project_details: Some(ProjectDetailsView {
                id: "p".to_owned(),
                identifier: "CT00003".to_owned(),
                name: "n".to_owned(),
                cover_image: None,
                cover_image_url: None,
                logo_props: serde_json::json!({}),
                description: Some(String::new()),
                is_default: false,
            }),
            workspace_detail: WorkspaceDetailView {
                name: "w".to_owned(),
                slug: "s".to_owned(),
                id: "wid".to_owned(),
                logo_url: None,
            },
            created_at: "2026-09-28T09:09:58.403293Z".to_owned(),
            updated_at: "2026-09-28T09:09:58.403293Z".to_owned(),
            deleted_at: None,
            entity_identifier: board_fixture().entity_identifier.clone(),
            entity_name: Some("project".to_owned()),
            anchor: board_fixture().anchor.clone(),
            is_comments_enabled: true,
            is_reactions_enabled: true,
            is_votes_enabled: true,
            view_props: serde_json::json!({}),
            is_activity_enabled: true,
            is_disabled: false,
            created_by: None,
            updated_by: None,
            workspace: board_fixture().workspace_id.clone(),
            project: board_fixture().project_id.clone(),
            intake: board_fixture().intake_id.clone(),
        };
        let body = serde_json::to_string(&view).expect("serializes");
        let parsed: Value = serde_json::from_str(&body).expect("parses");
        let keys: Vec<&str> = parsed
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, DJANGO_SETTINGS_KEYS);
        // Nested key orders match the app serializers' Meta.fields order.
        let details = parsed.get("project_details").expect("details");
        let detail_keys: Vec<&str> = details
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            detail_keys,
            [
                "id",
                "identifier",
                "name",
                "cover_image",
                "cover_image_url",
                "logo_props",
                "description",
                "is_default"
            ]
        );
        let workspace = parsed.get("workspace_detail").expect("workspace");
        let workspace_keys: Vec<&str> = workspace
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(workspace_keys, ["name", "slug", "id", "logo_url"]);
    }

    #[test]
    fn error_bodies_match_guards_bytes() {
        assert_eq!(
            HandlerError::ObjectNotFound.error_body().body,
            serde_json::json!({"error": "The required object does not exist."})
        );
        assert_eq!(HandlerError::ObjectNotFound.error_body().status, 404);
        assert_eq!(
            HandlerError::InvalidAnchor.error_body().body,
            serde_json::json!({"error": "Invalid anchor"})
        );
        assert_eq!(HandlerError::InvalidAnchor.error_body().status, 404);
        assert_eq!(
            HandlerError::NotPublished.error_body().body,
            serde_json::json!({"error": "Project is not published"})
        );
        assert_eq!(HandlerError::NotPublished.error_body().status, 404);
        assert_eq!(
            HandlerError::ServerError.error_body().body,
            serde_json::json!({"error": "Something went wrong please try again later"})
        );
        assert_eq!(HandlerError::ServerError.error_body().status, 500);
    }

    #[test]
    fn asset_url_branches_match_model_property() {
        assert_eq!(
            asset_url("a1", Some("PROJECT_COVER"), None, None, None),
            Some("/api/assets/v2/static/a1/".to_owned())
        );
        assert_eq!(
            asset_url("a1", Some("WORKSPACE_LOGO"), None, None, None),
            Some("/api/assets/v2/static/a1/".to_owned())
        );
        assert_eq!(
            asset_url(
                "a1",
                Some("ISSUE_ATTACHMENT"),
                Some("ws"),
                Some("p"),
                Some("i")
            ),
            Some("/api/assets/v2/workspaces/ws/projects/p/issues/i/attachments/a1/".to_owned())
        );
        // Missing components (dangling FK parts) behave like the Python
        // AttributeError path: no URL.
        assert_eq!(
            asset_url("a1", Some("ISSUE_ATTACHMENT"), None, Some("p"), Some("i")),
            None
        );
        assert_eq!(
            asset_url(
                "a1",
                Some("COMMENT_DESCRIPTION"),
                Some("ws"),
                Some("p"),
                None
            ),
            Some("/api/assets/v2/workspaces/ws/projects/p/a1/".to_owned())
        );
        assert_eq!(
            asset_url(
                "a1",
                Some("DRAFT_ISSUE_ATTACHMENT"),
                Some("ws"),
                Some("p"),
                Some("i")
            ),
            None
        );
        assert_eq!(asset_url("a1", None, None, None, None), None);
        assert_eq!(asset_url("a1", Some("UNKNOWN"), None, None, None), None);
    }

    #[test]
    fn nullable_project_predicate_matches_django_is_null() {
        let members = queries::members_sql();
        assert!(members.contains("\"project_members\".\"project_id\" = $1"));
        let rewritten = nullable_project_sql(&members, "$1");
        assert!(rewritten.contains("\"project_members\".\"project_id\" IS NULL"));
        assert!(!rewritten.contains("$1"));
        // Other predicates (workspace bind) survive the rewrite.
        assert!(rewritten.contains("$2"));

        let states = queries::states_sql();
        let rewritten = nullable_project_sql(&states, "$2");
        assert!(rewritten.contains("\"states\".\"project_id\" IS NULL"));
        assert!(rewritten.contains("$1"));

        // scoped_list_query keeps builder order for the non-null case and
        // drops the project bind for the null case.
        let ws = "3d914201-e7b6-449b-b92f-3ab0bd4f330d";
        let p = "62d604a8-db82-44de-9e8c-a3e4a0f28371";
        let ws_id = uuid::Uuid::parse_str(ws).expect("fixture uuid");
        let p_id = uuid::Uuid::parse_str(p).expect("fixture uuid");
        let (sql, params) = scoped_list_query(&members, "$1", ws, Some(p)).expect("params");
        assert_eq!(sql, members);
        assert_eq!(params, vec![SqlParam::Uuid(p_id), SqlParam::Uuid(ws_id)]);
        let (cycles_sql, params) =
            scoped_list_query(&queries::cycles_sql(), "$2", ws, Some(p)).expect("params");
        assert!(cycles_sql.contains("\"cycles\".\"project_id\" = $2"));
        assert_eq!(params, vec![SqlParam::Uuid(ws_id), SqlParam::Uuid(p_id)]);
        let (sql, params) = scoped_list_query(&members, "$1", ws, None).expect("params");
        assert!(sql.contains("\"project_id\" IS NULL"));
        assert_eq!(params, vec![SqlParam::Uuid(ws_id)]);
        // Non-UUID binds are a 500 like Python's ValidationError fallback.
        assert!(scoped_list_query(&members, "$1", "not-a-uuid", Some(p)).is_err());
    }

    #[test]
    fn workspace_boards_is_always_500() {
        // BUG-boards (views/project.py:32): the builder never returns SQL.
        assert!(queries::workspace_boards_sql("anchor").is_err());
        assert!(matches!(
            workspace_boards_inner(&AppState::new("0.1.0")),
            Err(HandlerError::ServerError)
        ));
    }

    fn test_router() -> Router {
        crate::routes::with_routes(AppState::new("0.1.0"), routes())
    }

    async fn get_status(app: Router, path: &str) -> (StatusCode, Value) {
        let response = app
            .oneshot(
                axum::http::Request::get(path)
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, json)
    }

    #[tokio::test]
    async fn owned_gets_reach_handlers_without_pools() {
        // No pools → every owned GET runs its handler and answers the 500
        // fallback (proving Rust owns the path); an unowned sibling path
        // proxies and fails closed with 502 (proving the cutover boundary).
        let app = test_router();
        for path in [
            "/api/public/anchor/abc/settings/",
            "/api/public/anchor/abc/meta/",
            "/api/public/anchor/abc/members/",
            "/api/public/anchor/abc/cycles/",
            "/api/public/anchor/abc/modules/",
            "/api/public/anchor/abc/states/",
            "/api/public/anchor/abc/labels/",
            "/api/public/workspaces/slug/project-boards/",
        ] {
            let (status, body) = get_status(app.clone(), path).await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
            assert_eq!(
                body,
                serde_json::json!({"error": "Something went wrong please try again later"}),
                "{path}"
            );
        }
        let (status, _) = get_status(
            app.clone(),
            "/api/public/workspaces/slug/projects/62d604a8-db82-44de-9e8c-a3e4a0f28371/anchor/",
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        // Sibling path stays on Django: no Rust route → proxy → 502 closed.
        let (status, _) = get_status(app, "/api/public/anchor/abc/issues/").await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn non_get_methods_proxy() {
        // POST on an owned path is Django's (create/405/401 live there);
        // without an upstream the proxy fails closed, never a Rust 405.
        let app = test_router();
        let response = app
            .oneshot(
                axum::http::Request::post("/api/public/anchor/abc/meta/")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}
