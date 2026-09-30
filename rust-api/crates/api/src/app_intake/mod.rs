//! App intake-issue handlers (D-32, stage 5).
//!
//! Ports the six handler units of `app/views/intake/base.py:176-637`
//! (`IntakeIssueViewSet.list` / `.create` / `.partial_update` /
//! `.retrieve` / `.destroy` and
//! `IntakeWorkItemDescriptionVersionEndpoint.get`) with routes from
//! `app/urls/intake.py`:
//!
//! * `GET` + `POST intake-issues/` + `inbox-issues/` (`issues`,
//!   PIDASHCONV-385)
//! * `PATCH intake-issues/<pk>/` + `inbox-issues/<pk>/` (`issues`)
//! * `GET intake-issues/<pk>/` + `inbox-issues/<pk>/` (`issues`)
//! * `DELETE intake-issues/<pk>/` + `inbox-issues/<pk>/` (`issues`)
//! * `GET intake-work-items/<id>/description-versions[/<pk>/]` (`versions`)
//!
//! Sibling handler issues own their files and share this module's
//! plumbing (mirrors the D-02 `space` layout): PIDASHCONV-360 (Intake
//! CRUD, [`intakes`]) and PIDASHCONV-385 (intake-issue list + create)
//! extend [`routes`] with their own routers; merges keep both sides.
//!
//! Only the methods above are owned: `PUT` on the detail paths proxies
//! to Django (BUG-intake-issue-put — `update` carries no decorator, so
//! any authenticated user passes; the proxy preserves that byte for
//! byte), as do `POST`/`OPTIONS` and every non-`GET` method on the
//! versions paths (Django's own 405s live there).
//!
//! The intake CRUD routes below own their five methods the same way:
//! `PUT` on the intake detail and every non-owned method on the intake
//! paths proxy to Django, where DRF's own 405-after-auth and metadata
//! responses live.
//!
//! Layering: permission gates in `pidash_services::app_intake::permissions`,
//! serializer shapes in `::shape`, task emits in `::tasks`, SQL fragments
//! in `pidash_db::app_intake::queries`. This module owns the HTTP shell
//! (routes, session auth, tenant + membership resolution), the SQL text
//! for the handler-owned lookups/writes, the DRF field-validation
//! mirrors, and the row rendering.

pub mod intakes;
pub mod issues;
pub mod versions;

use std::collections::HashMap;

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono_tz::Tz;
use sqlx::PgPool;
use uuid::Uuid;

use crate::state::AppState;

/// Exact bytes of the DRF `IsAuthenticated` denial.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`app/views/base.py:129-133`).
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// DRF's default `Http404` body (unresolvable project identifier).
pub const NOT_FOUND_DETAIL_BODY: &str = r#"{"detail":"Not found."}"#;
/// `handle_exception`'s generic 500 branch.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `ValidationError` branch
/// (`app/views/base.py:125-128`).
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;

/// One query value, repeated or not. Mirrors the D-26 `app_issues` shape:
/// callers read last-wins like Django's `QueryDict`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every list handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// Django `QueryDict.get`: the last value, or `None`.
/// All values for `key`, in order; `None` when absent.
pub fn query_values(query: &QueryMap, key: &str) -> Option<Vec<String>> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => vec![one.clone()],
        OneOrMany::Many(many) => many.clone(),
    })
}

pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => one.clone(),
        OneOrMany::Many(many) => many.last().cloned().unwrap_or_default(),
    })
}

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (anonymous on a guarded route).
    Unauthorized,
    /// 403, `allow_permission` fallthrough.
    Forbidden,
    /// 404, `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, DRF default `Http404` (unresolvable project identifier).
    NotFoundDetail,
    /// 404, view-inline `{"error": ...}` (missing intake row).
    NotFoundError(String),
    /// 404, DRF `get_object` miss on intakes: `Http404("No Intake
    /// matches the given query.")` (PIDASHCONV-360).
    IntakeNotFound,
    /// 404, `Project.resolve` miss: `Http404("Project not found")`
    /// (`db/models/project.py:214-218`, PIDASHCONV-360).
    ProjectNotFound,
    /// 404, Django `handler404` (`app/views/error_404.py`,
    /// `custom_404_view`, prod): unmatched paths, including non-UUID
    /// ids under `<uuid:>` converters (PIDASHCONV-360; DEBUG renders
    /// the HTML technical page instead, so no JSON can match there).
    PageNotFound,
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 400, DRF `ParseError` `{"detail": ...}` (per_page/cursor/JSON).
    BadDetail(String),
    /// 400, pre-rendered serializer-errors body (`{"field": [...]}`).
    BadJson(serde_json::Value),
    /// 400, Django `ValidationError` branch.
    InvalidDetail,
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                r#"{"error":"You don't have the required permissions."}"#.to_owned(),
            ),
            Denial::NotFound => (StatusCode::NOT_FOUND, NOT_FOUND_BODY.to_owned()),
            Denial::NotFoundDetail => (StatusCode::NOT_FOUND, NOT_FOUND_DETAIL_BODY.to_owned()),
            Denial::NotFoundError(message) => (
                StatusCode::NOT_FOUND,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::IntakeNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"detail":"No Intake matches the given query."}"#.to_owned(),
            ),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned(),
            ),
            Denial::PageNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"error":"Page not found."}"#.to_owned(),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).expect("serializable denial"),
            ),
            Denial::InvalidDetail => (StatusCode::BAD_REQUEST, INVALID_DETAIL_BODY.to_owned()),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned(),
            ),
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

pub fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render a guards-layer
/// [`pidash_services::app_intake::permissions::ErrorBody`] as the exact
/// wire response.
pub fn guards_error(error: pidash_services::app_intake::permissions::ErrorBody) -> Response {
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            serde_json::to_string(&error.body).expect("serializable guard body"),
        ))
        .expect("guard denial response")
}

/// Render pre-serialized JSON bytes as the response body.
pub fn raw_json_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("json response")
}

/// `json.dumps` with CPython defaults (`, ` / `: ` separators): the task
/// `requested_data` / `current_instance` / `updated_issue` payloads.
/// Same algorithm as the D-02 `space` twin; kept per module so sibling
/// handler issues never fork a shared helper.
pub fn python_dumps(value: &serde_json::Value) -> String {
    let mut out = String::new();
    python_dump_into(&mut out, value);
    out
}

fn python_dump_into(out: &mut String, value: &serde_json::Value) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(true) => out.push_str("true"),
        serde_json::Value::Bool(false) => out.push_str("false"),
        serde_json::Value::Number(number) => out.push_str(&number.to_string()),
        serde_json::Value::String(text) => python_dump_str(out, text),
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_into(out, item);
            }
            out.push(']');
        }
        serde_json::Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_str(out, key);
                out.push_str(": ");
                python_dump_into(out, item);
            }
            out.push('}');
        }
    }
}

fn python_dump_str(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 || (ch as u32) == 0x7F => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch if (ch as u32) > 0x7E => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let v = code - 0x10000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (v >> 10),
                        0xDC00 + (v & 0x3FF)
                    ));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// An intake path: the owned methods serve from Rust, everything else
/// falls through to Django (its 405-after-auth and metadata responses
/// live there). `HEAD` rides axum's `get` handling like Django's
/// `GET`-backed `HEAD`.
pub fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the owned intake-issue + versions routes (both aliases share
/// the viewsets, exactly like the Django URL conf). Sibling handler
/// issues merge their own routers here; merges keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/intake-issues/",
            owned(
                axum::routing::get(issues::collection_list).post(issues::collection_create),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/intakes/",
            owned(
                axum::routing::get(intakes::list).post(intakes::create),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/inbox-issues/",
            owned(
                axum::routing::get(issues::collection_list).post(issues::collection_create),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/",
            owned(
                axum::routing::get(intakes::retrieve)
                    .patch(intakes::partial_update)
                    .delete(intakes::destroy),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/inboxes/",
            owned(
                axum::routing::get(intakes::list).post(intakes::create),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/",
            owned(
                axum::routing::get(intakes::retrieve)
                    .patch(intakes::partial_update)
                    .delete(intakes::destroy),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
            owned(
                axum::routing::get(issues::retrieve)
                    .patch(issues::partial_update)
                    .delete(issues::destroy),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/inbox-issues/{pk}/",
            owned(
                axum::routing::get(issues::retrieve)
                    .patch(issues::partial_update)
                    .delete(issues::destroy),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/",
            owned(axum::routing::get(versions::list), &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"]),
        )
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/{pk}/",
            owned(axum::routing::get(versions::detail), &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"]),
        )
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

/// Authenticated actor plus time zone (`TimezoneMixin.initial` activates
/// the user's zone; datetimes render in it).
pub struct Actor {
    pub id: Uuid,
    pub timezone: Tz,
}

/// `request.user` through Django-session auth.
///
/// `BaseSessionAuthentication` + `IsAuthenticated`
/// (`views/base.py:48,52`): anonymous answers the DRF `NotAuthenticated`
/// body before anything else runs.
pub async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Actor, Denial> {
    let pool = pool_of(state)?;
    let resolved =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| Denial::ServerError)?
            .ok_or(Denial::Unauthorized)?;
    Ok(Actor {
        id: resolved.id,
        timezone: resolved.timezone,
    })
}

pub fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// The resolved tenant scope: workspace + project.
pub struct Tenant {
    pub workspace_id: Uuid,
    pub project_id: Uuid,
}

/// Resolve `slug` + `project_id` the way the view kwargs do:
/// `_rewrite_project_kwarg` (`app/views/base.py:49-79`) accepts a UUID
/// or a workspace-scoped project identifier (upper-cased, like
/// `Project.save` normalizes it). An unresolvable identifier answers
/// DRF's default `Http404` body; a missing project row answers the
/// `ObjectDoesNotExist` branch.
pub async fn resolve_tenant(
    pool: &PgPool,
    slug: &str,
    project_raw: &str,
) -> Result<Tenant, Denial> {
    if let Ok(id) = project_raw.parse::<Uuid>() {
        let row: Option<(Uuid,)> = sqlx::query_as(
            r#"SELECT p.id FROM projects p WHERE p.id = $1 AND p.deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        let Some((project_id,)) = row else {
            return Err(Denial::NotFound);
        };
        let workspace_id = workspace_of_project(pool, &project_id).await?;
        return Ok(Tenant {
            workspace_id,
            project_id,
        });
    }
    let upper = project_raw.trim().to_uppercase();
    let row: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT p.id, p.workspace_id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(upper)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match row {
        Some((project_id, workspace_id)) => Ok(Tenant {
            workspace_id,
            project_id,
        }),
        None => Err(Denial::NotFoundDetail),
    }
}

async fn workspace_of_project(pool: &PgPool, project_id: &Uuid) -> Result<Uuid, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT p.workspace_id FROM projects p WHERE p.id = $1 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::NotFound)
}

/// Membership facts for the guards layer, resolved with the same row
/// filters Python uses (`app/permissions/base.py:26-30,45-59`).
pub struct Membership {
    pub project_role: Option<i16>,
    pub workspace_member: bool,
    pub workspace_admin: bool,
}

impl Membership {
    pub fn as_guard(&self) -> pidash_services::app_intake::permissions::Membership {
        pidash_services::app_intake::permissions::Membership {
            project_role: self.project_role,
            workspace_member: self.workspace_member,
            workspace_admin: self.workspace_admin,
        }
    }
}

pub async fn load_membership(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<Membership, Denial> {
    let project_role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
             AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `EXISTS` (never aggregates): an empty membership set reads as
    // `False`, exactly like the Django `.exists()` probes.
    let workspace_member: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT wm.id FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2
             AND wm.is_active AND wm.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let workspace_admin: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT wm.id FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.role = 20
             AND wm.is_active AND wm.deleted_at IS NULL
           LIMIT 1"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (workspace_member, workspace_admin) =
        (workspace_member.is_some(), workspace_admin.is_some());
    Ok(Membership {
        project_role: project_role.map(|row| row.0),
        workspace_member,
        workspace_admin,
    })
}

/// The decorator's creator branch (`app/permissions/base.py:26-38`):
/// `Issue.objects.filter(id=pk, created_by=user).exists()` — the
/// soft-delete-scoped default manager, where `pk` is the issue id.
pub async fn is_issue_creator(
    pool: &PgPool,
    issue_id: &Uuid,
    user_id: &Uuid,
) -> Result<bool, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM issues WHERE id = $1 AND created_by_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(issue_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

/// Parse a UUID path segment: Django's `<uuid:>` converter 404s on
/// garbage, so unparseable ids behave as missing rows
/// (`ObjectDoesNotExist` branch).
#[allow(clippy::result_large_err)]
pub fn parse_id(raw: &str) -> Result<Uuid, Response> {
    raw.parse::<Uuid>()
        .map_err(|_| Denial::NotFound.into_response())
}

/// Convert a guards-layer denial into its wire response. Call sites only
/// invoke this on the denying branch, so `Ok` degrades to the 500
/// envelope instead of panicking.
pub fn guard_denial(
    result: Result<(), pidash_services::app_intake::permissions::ErrorBody>,
) -> Response {
    match result {
        Ok(()) => Denial::ServerError.into_response(),
        Err(error) => guards_error(error),
    }
}

/// Parse the request body the way DRF does for JSON posts: empty → `{}`;
/// malformed → `ParseError` 400; non-object JSON → the attribute errors
/// the view code hits (500 envelope).
#[allow(clippy::result_large_err)]
pub fn parse_body(raw: &[u8]) -> Result<serde_json::Value, Response> {
    if raw.is_empty() {
        return Ok(serde_json::Value::Object(Default::default()));
    }
    match serde_json::from_slice::<serde_json::Value>(raw) {
        Ok(value) if value.is_object() => Ok(value),
        Ok(_) => Err(Denial::ServerError.into_response()),
        Err(error) => Err(Denial::BadJson(serde_json::json!({
            "detail": format!("JSON parse error - {error}"),
        }))
        .into_response()),
    }
}

/// Python truthiness for JSON values (`bool(issue_data)`,
/// `skip_activity and ...`): `""`/`null`/`false`/`0`/`[]`/`{}` are all
/// falsy.
pub fn json_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::Number(number) => {
            number.as_i64().is_some_and(|n| n != 0)
                || number.as_u64().is_some_and(|n| n != 0)
                || number.as_f64().is_some_and(|n| n != 0.0)
        }
        serde_json::Value::String(text) => !text.is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(map) => !map.is_empty(),
    }
}

/// Look up an intake row id by tenant (`Intake.objects.filter(...).first()`,
/// soft-delete-scoped). A miss reads as `None`, exactly like `.first()`.
pub async fn intake_id_for(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT i.id FROM intakes i
           JOIN workspaces w ON w.id = i.workspace_id
           WHERE w.slug = $1 AND i.project_id = $2 AND i.deleted_at IS NULL
           ORDER BY i.name LIMIT 1"#,
    )
    .bind(slug)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// `project.guest_view_all_features` for the guest creator checks.
pub async fn guest_view_all(pool: &PgPool, project_id: &Uuid) -> Result<bool, Denial> {
    let row: Option<(bool,)> = sqlx::query_as(
        r#"SELECT p.guest_view_all_features FROM projects p
           WHERE p.id = $1 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::NotFound)
}

/// Best-effort post-commit task enqueue (the D-02 intake pattern):
/// without a queue table the response still stands — Django would have
/// answered 500 only when its own broker write failed, and the proxy
/// contract tests never run a worker.
pub async fn enqueue_message(pool: &PgPool, message: pidash_jobs::celery::CeleryTaskMessage) {
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        serde_json::Value::Array(message.args.clone()),
        serde_json::Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// `soft_delete_related_objects.delay(app_label, model_name, pk, using)`
/// (`db/mixins.py:77`): positional args, no kwargs. The app views call
/// `.delete()` with no `using`, so the fourth arg is null.
pub async fn enqueue_soft_delete(pool: &PgPool, app_label: &str, model_name: &str, pk: &Uuid) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            serde_json::Value::String(app_label.to_string()),
            serde_json::Value::String(model_name.to_string()),
            serde_json::Value::String(pk.to_string()),
            serde_json::Value::Null,
        ],
        Default::default(),
    );
    enqueue_message(pool, message).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Project.resolve` raises `Http404("Project not found")`
    /// (`db/models/project.py:214-218`, no trailing period); DRF's
    /// `exception_handler` maps the args verbatim to `{"detail": ...}`
    /// (`rest_framework/views.py:81-82`), so the port must not add one.
    /// Pinned by review (PIDASHCONV-360): the intake tenant lookup
    /// ([`crate::app_intake::intakes::intake_tenant`]) serves this body
    /// for identifier-form project ids that resolve to no row.
    #[test]
    fn project_resolve_miss_matches_django_byte_for_byte() {
        let (status, body) = Denial::ProjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, r#"{"detail":"Project not found"}"#);
    }
}
