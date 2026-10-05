//! Issue relations + links + PR + code-review handlers (D-26 handlers C).
//!
//! Ports four viewsets in `app/views/issue/` onto the foundation crates:
//!
//! * `relation.py:37-284` (`IssueRelationViewSet`: `GET`/`POST`
//!   `issue-relation/`, `POST remove-relation/`)
//! * `link.py:26-113` (`IssueLinkViewSet`: link CRUD incl. the DRF-default
//!   `retrieve` and `PUT update`)
//! * `github_pr.py:28-73` (`GithubPullRequestLinkViewSet`: list / create /
//!   destroy through the queries-C `pr_links` port)
//! * `git_code_review.py:24-74` (`GitCodeReviewLinkViewSet`: list / create /
//!   destroy through the D-05 `code_reviews` port)
//!
//! Fixtures: `FX-ISS-16.relations.json` (family behavior) +
//! `FX-ISS-21.signals_tasks.json` (enqueue goldens); contract suite
//! `test_relations_links.py`. No orchestration receiver fires on these
//! paths (no `Issue.save` anywhere here), so the port makes no
//! orchestration calls; it does publish `issue_activity`,
//! `crawl_work_item_link_title` and `soft_delete_related_objects` exactly
//! where the views `.delay()` them.
//!
//! Owned paths (registration lives in [`routes`], merged by
//! [`super::routes`]; every other method on these paths proxies to
//! Django, which also owns the DRF 405s and `OPTIONS` metadata):
//!
//! * `GET`/`POST .../issues/{issue_id}/issue-relation/`
//! * `POST .../issues/{issue_id}/remove-relation/`
//! * `GET`/`POST .../issues/{issue_id}/issue-links/` and
//!   `GET`/`PUT`/`PATCH`/`DELETE .../issue-links/{pk}/`
//! * `GET`/`POST .../issues/{issue_id}/github-pull-requests/` and
//!   `DELETE .../github-pull-requests/{pk}/`
//! * `GET`/`POST .../issues/{issue_id}/code-reviews/` and
//!   `DELETE .../code-reviews/{pk}/`
//!
//! Layering: relation buckets + link-list SQL come from
//! [`super::queries_core`]; PR/review read shapes + the app `UserLite`
//! nest from `pidash_services::app_issues::serializers_links`; the stored
//! type mapping from `relation_mapper`; attach/detach lifecycles from
//! `pr_links` (queries-C) and `integrations::code_reviews` (D-05) with the
//! store seams implemented here; gates through the F-06
//! `decide_project_entity` kernel; body negotiation through the shared
//! `v1_cycles_modules::body`. The app `IssueRelationSerializer` /
//! `RelatedIssueSerializer` read shapes (`:590-667`) are inlined: the
//! merged v1 `shape_relations` port covers the *api* classes, whose
//! `RelatedIssueSerializer` carries extra `type_id`/`is_epic` keys.
//!
//! Store mechanics (audit + delete) live in the store impls, mirroring
//! Python's `save()`/`delete()` exactly where the ports leave them to the
//! executing layer: CRUM stamps `created_by` on every create and
//! `updated_by` on every update (`db/models/base.py:26-49`,
//! `ProjectBaseModel.save` stamps `workspace_id` from the project row);
//! instance `delete()` soft-deletes (`db/mixins.py:71-78`) and each delete
//! fans out `soft_delete_related_objects`.
//!
//! DRF bytes below were pinned on Django 4.2.30 / DRF 3.15.2 probes (see
//! the issue workpad): relation row orders (both Shows), link/PR/review
//! wire orders, `{"detail": "No IssueLink matches the given query."}`,
//! `{"detail": ...}` vs `{"error": ...}` 404s, `204` with no content type,
//! `HEAD` served via `GET`, `CharField` int/float coercion with bool
//! rejection, 255-char (not byte) title cap, `int`/`bool` relation items
//! via `UUID(int=...)`, prep-before-execute fault ordering.
//!
//! Ported bugs (also listed in the PR):
//!
//! * `remove_relation` with a missing/unknown `related_issue` 500s:
//!   `.first()` yields `None`, whose serialization raises `AttributeError`
//!   before `.delete()` is even reached (`relation.py:270-272`).
//! * Non-object JSON bodies 500 on the `.get` paths: `request.data.get`
//!   on a list/`None` (`relation.py:210,263`, `github_pr.py:54`,
//!   `git_code_review.py:52`) and `IssueLinkSerializer.to_internal_value`'s
//!   `data.get("url")` ahead of DRF's dict check (`issue.py:817-823`).
//! * A non-string link `url` (int/bool/dict/list) 500s on
//!   `url.startswith` (`issue.py:819`).
//! * Link create/partial_update on an archived project persists the row
//!   and fires both tasks, then 404s on the scoped re-fetch
//!   (`link.py:65,92`).
//! * `ftp://`/`ftps://` URLs can never validate: the `http://` prepend
//!   turns them into `http://ftp://...` before `URLValidator` runs.
//! * Non-string relation `issues` items take `UUID(int=...)` for ints and
//!   bools (FK miss → 400) while floats/dicts/lists raise `ValidationError`
//!   → 400 with the other message; `None` becomes a NULL insert → 400.
//! * Uppercase/simple/braced `urn:` UUID spellings in `<uuid:>` path
//!   segments never route in Django (resolver 404, HTML); they proxy here
//!   instead of serving.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono_tz::Tz;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;

use pidash_services::app_issues::relation_mapper::get_actual_relation;

// ---- owned paths ------------------------------------------------------------

/// Relation collection (`app/urls/issue.py:258-262`).
pub const RELATION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-relation/";
/// Relation removal (`app/urls/issue.py:263-267`).
pub const REMOVE_RELATION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/remove-relation/";
/// Link collection (`app/urls/issue.py:113-117`).
pub const LINKS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/";
/// Link detail (`app/urls/issue.py:118-130`).
pub const LINK_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/{pk}/";
/// PR-link collection (`app/urls/issue.py:131-135`).
pub const PR_LINKS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/github-pull-requests/";
/// PR-link detail, DELETE-only (`app/urls/issue.py:136-140`).
pub const PR_LINK_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/github-pull-requests/{pk}/";
/// Code-review collection (`app/urls/issue.py:139-143`).
pub const REVIEWS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/code-reviews/";
/// Code-review detail, DELETE-only (`app/urls/issue.py:144-148`).
pub const REVIEW_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/code-reviews/{pk}/";

/// Register the owned routes. Unowned methods (incl. `HEAD`/`TRACE`,
/// whose axum defaults diverge from Django) proxy to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            RELATION_PATH,
            owned(
                axum::routing::get(relation_list).post(relation_create),
                &["GET", "POST"],
            ),
        )
        .route(
            REMOVE_RELATION_PATH,
            owned(axum::routing::post(remove_relation), &["POST"]),
        )
        .route(
            LINKS_PATH,
            owned(
                axum::routing::get(link_list).post(link_create),
                &["GET", "POST"],
            ),
        )
        .route(
            LINK_PATH,
            owned(
                axum::routing::get(link_retrieve)
                    .put(link_update)
                    .patch(link_partial_update)
                    .delete(link_destroy),
                &["GET", "PUT", "PATCH", "DELETE"],
            ),
        )
        .route(
            PR_LINKS_PATH,
            owned(
                axum::routing::get(pr_list).post(pr_create),
                &["GET", "POST"],
            ),
        )
        .route(
            PR_LINK_PATH,
            owned(axum::routing::delete(pr_destroy), &["DELETE"]),
        )
        .route(
            REVIEWS_PATH,
            owned(
                axum::routing::get(review_list).post(review_create),
                &["GET", "POST"],
            ),
        )
        .route(
            REVIEW_PATH,
            owned(axum::routing::delete(review_destroy), &["DELETE"]),
        )
}

/// A relations path: the owned methods serve from Rust, everything else
/// falls through to Django (the engage-handler cutover shape).
fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in [
        "GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD", "TRACE",
    ] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
            "GET" => router.get(crate::edge::proxy),
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            "TRACE" => router.trace(crate::edge::proxy),
            _ => router.options(crate::edge::proxy),
        };
    }
    router
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, DRF `PermissionDenied` (class-permission denials).
    ForbiddenDetail,
    /// 404, `ObjectDoesNotExist` branch (bare `.get()` misses).
    NotFound,
    /// 404, `{"error": ...}` (view-inline misses: provider 404s).
    NotFoundError(String),
    /// 404, `get_object_or_404` over links (retrieve / PUT misses).
    LinkNotFound,
    /// 404, `{"detail": "Project not found"}` (project-kwarg rewrite miss).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (JSON `ParseError`).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline + `IntegrityError` mapping).
    BadError(String),
    /// 400, `{"message": ...}` (relation-type required).
    BadMessage(String),
    /// 400, serializer-errors dict.
    BadJson(Value),
    /// 404, `{"error": "Work item not found."}` (attach issue probe).
    WorkItemNotFound,
    /// 409, `{"error": ...}` (already-linked conflicts).
    Conflict(String),
    /// 415, `UnsupportedMediaType` (content negotiation).
    UnsupportedMediaType(String),
    /// 413, `RequestBodySizeLimitMiddleware` past 5 MiB.
    RequestTooLarge,
    /// 500, generic branch.
    ServerError,
}

/// DRF's default permission-denied body (lowercase `detail`).
const VIEWSET_FORBIDDEN_BODY: &str =
    r#"{"detail":"You do not have permission to perform this action."}"#;
/// DRF `get_object_or_404` over links: the `Http404` message names the
/// model (`django/shortcuts.py`), rendered under lowercase `detail` —
/// verified against live Django.
const LINK_NOT_FOUND_BODY: &str = r#"{"detail":"No IssueLink matches the given query."}"#;
/// `{"error": "Work item not found."}` (`github_pr.py:61`,
/// `git_code_review.py:60`).
const WORK_ITEM_NOT_FOUND_BODY: &str = r#"{"error":"Work item not found."}"#;

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                super::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::ForbiddenDetail => (StatusCode::FORBIDDEN, VIEWSET_FORBIDDEN_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, super::NOT_FOUND_BODY.to_owned()),
            Denial::NotFoundError(message) => (
                StatusCode::NOT_FOUND,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::LinkNotFound => (StatusCode::NOT_FOUND, LINK_NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                super::PROJECT_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadMessage(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"message\":{}}}", json_string(message)),
            ),
            Denial::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).unwrap_or_else(|_| super::SERVER_ERROR_BODY.to_owned()),
            ),
            Denial::WorkItemNotFound => (
                StatusCode::NOT_FOUND,
                WORK_ITEM_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::Conflict(message) => (
                StatusCode::CONFLICT,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::RequestTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                r#"{"error":"REQUEST_BODY_TOO_LARGE","Detail":"The size of the request body exceeds the maximum allowed size."}"#.to_owned(),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                super::SERVER_ERROR_BODY.to_owned(),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        if matches!(self, Denial::ServerError) {
            tracing::warn!("app_issues relations handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("relations error response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Render `body` (already exact JSON bytes) with an explicit status.
fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("relations json response")
}

/// DRF's 204: empty body with NO content type (verified live: Django
/// only sets `Content-Type` when it renders a body).
fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .body(axum::body::Body::empty())
        .expect("relations empty response")
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Map a write failure the way `handle_exception` does: integrity
/// violations (unique / FK / not-null / check) are the `IntegrityError`
/// 400, anything else the fallback 500.
fn integrity_denial(error: sqlx::Error) -> Denial {
    if let sqlx::Error::Database(db) = &error {
        if db.code().is_some_and(|code| code.starts_with("23")) {
            return Denial::BadError("The payload is not valid".to_owned());
        }
    }
    let _ = error;
    Denial::ServerError
}

/// True when the failure is an integrity violation (SQLSTATE class 23).
fn is_integrity_error(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code.starts_with("23")))
}

/// True when the failure is a unique violation (SQLSTATE 23505): the
/// `ignore_conflicts` / create-race arm.
fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.code().is_some_and(|code| code == "23505"))
}

// ---------------------------------------------------------------------------
// Request plumbing: auth + project rewrite + permission gates
// ---------------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, a non-UUID id, or a session pointing at no user row means
/// anonymous → 401.
async fn actor_user_id(
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<uuid::Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    let raw = session
        .get("_auth_user_id")
        .and_then(|value| value.as_str().to_owned())
        .ok_or(Denial::Unauthorized)?;
    let id = raw
        .parse::<uuid::Uuid>()
        .map_err(|_| Denial::Unauthorized)?;
    let exists: Option<(i32,)> = sqlx::query_as("SELECT 1 FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if exists.is_none() {
        return Err(Denial::Unauthorized);
    }
    Ok(id)
}

/// `_rewrite_project_kwarg` (`app/views/base.py:49-80`): UUIDs pass
/// through unchecked; other identifiers resolve `UPPER(identifier)` in
/// the workspace, else `Http404("Project not found")`.
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return Ok(id);
    }
    let upper = raw.trim().to_uppercase();
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

/// Parse a `<uuid:>` path segment the way Django's `UUIDConverter` does:
/// lowercase hex, hyphenated, 36 chars (`django/urls/converters.py:25`).
/// Anything else never routes in Django (resolver 404, HTML), so it
/// proxies here instead of serving.
fn path_uuid(raw: &str) -> Result<uuid::Uuid, ()> {
    if raw.len() != 36 {
        return Err(());
    }
    let strict = raw.bytes().enumerate().all(|(index, byte)| {
        if index == 8 || index == 13 || index == 18 || index == 23 {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        }
    });
    if !strict {
        return Err(());
    }
    raw.parse::<uuid::Uuid>().map_err(|_| ())
}

/// `ProjectEntityPermission` (`app/permissions/project.py:85-116`) through
/// the F-06 kernel: safe methods need any active project membership;
/// writes need project Admin/Member. Denials render DRF's default
/// `PermissionDenied` body.
async fn check_entity_gate(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    method: &str,
) -> Result<(), Denial> {
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
    let role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let scope = pidash_auth::scope::TenantScope::new(pidash_types::WorkspaceId::from(slug));
    let facts = pidash_auth::permissions::project::ProjectFacts {
        workspace: pidash_types::WorkspaceId::from(slug),
        project_id: pidash_types::ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: false,
        has_workspace_admin_or_member: false,
        is_workspace_admin: false,
        is_project_member: role.is_some(),
        is_project_admin: role
            .map(|(role,)| i32::from(role) == ROLE_ADMIN)
            .unwrap_or(false),
        has_project_admin_or_member: role
            .map(|(role,)| [ROLE_ADMIN, ROLE_MEMBER].contains(&i32::from(role)))
            .unwrap_or(false),
        has_identifier_membership: false,
        has_project_identifier: false,
    };
    if pidash_auth::permissions::project::decide_project_entity(method, &scope, &facts) {
        Ok(())
    } else {
        Err(Denial::ForbiddenDetail)
    }
}

/// View-body tenant facts: the actor's timezone. (The project row itself
/// is resolved per-path below — `Project.objects.get` → 404 — because
/// each view reads it at a different point.)
async fn actor_timezone(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// `Project.objects.get(pk)` over the default manager: soft-deleted rows
/// miss → 404 `{"error": ...}`. Returns the project's workspace id (what
/// `ProjectBaseModel.save` stamps).
async fn project_workspace(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<uuid::Uuid, Denial> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL")
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|(id,)| id).ok_or(Denial::NotFound)
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

/// `RequestBodySizeLimitMiddleware` answers 413 before the view runs.
const MAX_BODY: usize = 5_242_880;

/// Relations body semantics for the shared negotiator: no list fields
/// (repeated form keys arrive as the last value, like `QueryDict.get`),
/// no blank-skipping (every blank runs field validation).
const RELATIONS_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// A negotiated request body: the validator map. Uploads live in
/// `request.FILES`, which no relations path reads, so files are dropped.
struct RequestBody {
    map: Map<String, Value>,
}

async fn read_body(req: axum::extract::Request) -> Result<RequestBody, Denial> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| Denial::RequestTooLarge)?;
    match shared_body::negotiate_body(&parts.headers, &bytes, &RELATIONS_BODY_SPEC) {
        Ok(shared_body::NegotiatedBody::Empty) => Ok(RequestBody { map: Map::new() }),
        Ok(shared_body::NegotiatedBody::JsonText { text, .. }) => {
            parse_json_body(&text).map(|map| RequestBody { map })
        }
        Ok(shared_body::NegotiatedBody::Form { map, .. }) => Ok(RequestBody { map }),
        Err(shared_body::BodyError::UnsupportedMediaType(message)) => {
            Err(Denial::UnsupportedMediaType(message))
        }
        Err(shared_body::BodyError::ParseDetail(message)) => Err(Denial::BadDetail(message)),
        Err(shared_body::BodyError::ServerError) => Err(Denial::ServerError),
    }
}

/// Parse negotiated JSON source text for the `.get` paths: objects → the
/// map (floats rewritten to their CPython literal); anything else → the
/// `AttributeError` 500 Python raises calling `.get` on it; malformed →
/// the engine-native `ParseError` text (the accepted cross-engine gap —
/// see the D-20 `body.rs` precedent).
fn parse_json_body(text: &str) -> Result<Map<String, Value>, Denial> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => {
            let mut normalized: Value = Value::Object(map);
            normalize_parse_floats(&mut normalized);
            match normalized {
                Value::Object(map) => Ok(map),
                _ => unreachable!("object stays an object"),
            }
        }
        Ok(_) => Err(Denial::ServerError),
        Err(error) => Err(Denial::BadDetail(format!("JSON parse error - {error}"))),
    }
}

// ---------------------------------------------------------------------------
// CPython value semantics (`dumps` / float rendering)
// ---------------------------------------------------------------------------

/// `json.dumps(value, cls=DjangoJSONEncoder)` for task kwargs: CPython
/// separators plus `ensure_ascii` string escaping. Inputs here are plain
/// JSON (no datetimes/UUIDs/Decimals reach these dumps unrendered), so
/// only separators and escaping are ported.
fn python_dumps(value: &serde_json::Value) -> String {
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

/// Rewrite every parsed f64 to its CPython `repr` literal, so `str()` of
/// a request float (`title: 1e-3` → `"0.001"`) matches Python.
fn normalize_parse_floats(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            if let Some(float) = number.as_f64() {
                let literal = py_float_repr(float);
                if let Ok(Value::Number(replacement)) = serde_json::from_str::<Value>(&literal) {
                    *number = replacement;
                }
            }
        }
        Value::Number(_) => {}
        Value::Array(items) => {
            for item in items {
                normalize_parse_floats(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                normalize_parse_floats(item);
            }
        }
        _ => {}
    }
}

/// CPython `repr` of an f64 (shortest round-trip, magnitude layout).
fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    for precision in 1..=19usize {
        let rendered = format!("{value:.dec$e}", dec = precision - 1);
        if rendered.parse::<f64>().ok() == Some(value) {
            return layout_py_float(&rendered, value);
        }
    }
    // Unreachable in practice (17 significant digits always identify
    // an f64); fall back to the longest rendering laid out.
    layout_py_float(&format!("{value:.18e}"), value)
}

/// Lay out a `d.dddde±xx` rendering per the CPython magnitude rule
/// (plain inside `[1e-4, 1e16)`, exponent outside). Same algorithm as the
/// engage handler's copy.
fn layout_py_float(rendered: &str, value: f64) -> String {
    let (mantissa, exp) = match rendered.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse::<i32>().unwrap_or(0)),
        None => (rendered, 0),
    };
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(|ch| ch.is_ascii_digit()).collect();
    let digits = digits.trim_start_matches('0');
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // Decimal exponent of `0.digits × 10^E`.
    let point_at = mantissa
        .find('.')
        .map(|pos| pos - usize::from(negative))
        .unwrap_or_else(|| mantissa.len() - usize::from(negative));
    let exp10 = exp + point_at as i32;
    let abs = value.abs();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (1e-4..1e16).contains(&abs) {
        // Plain layout.
        if exp10 <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-exp10) as usize));
            out.push_str(digits);
        } else if exp10 as usize >= digits.len() {
            out.push_str(digits);
            out.push_str(&"0".repeat(exp10 as usize - digits.len()));
            out.push_str(".0");
        } else {
            let at = exp10 as usize;
            out.push_str(&digits[..at]);
            out.push('.');
            out.push_str(&digits[at..]);
        }
        return out;
    }
    // Exponent layout: `d[.ddd]e±XX`.
    let head: String = digits.chars().take(1).collect();
    let tail: String = digits.chars().skip(1).collect();
    out.push_str(&head);
    if !tail.is_empty() {
        out.push('.');
        out.push_str(&tail);
    }
    let exp = exp10 - 1;
    out.push('e');
    out.push(if exp < 0 { '-' } else { '+' });
    let mag = exp.abs().to_string();
    if mag.len() < 2 {
        out.push('0');
    }
    out.push_str(&mag);
    out
}

// ---------------------------------------------------------------------------
// Tasks: best-effort publishing through the queue
// ---------------------------------------------------------------------------

/// `issue_activity` Celery wire name (bare `@shared_task` default:
/// `bgtasks/issue_activities_task.py:1503-1504`).
const ISSUE_ACTIVITY_TASK: &str = "pi_dash.bgtasks.issue_activities_task.issue_activity";
/// `crawl_work_item_link_title` wire name (`bgtasks/work_item_link_task.py:262`).
const CRAWL_LINK_TASK: &str = "pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title";

/// Enqueue a worker message for the worker to forward to the broker
/// (Python-owned task). Best-effort: without a queue the response still
/// stands (the intake-handler precedent).
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

/// Best-effort kwargs-only publish (every `.delay(...)` call below).
async fn enqueue_kwargs(pool: &sqlx::PgPool, task: &str, kwargs: Map<String, Value>) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, vec![], kwargs);
    enqueue_message(pool, message).await;
}

/// Best-effort positional-args publish (`crawl_work_item_link_title`
/// takes `(id, url)` positionally, `link.py:52,79`).
async fn enqueue_args(pool: &sqlx::PgPool, task: &str, args: Vec<Value>) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, args, Map::new());
    enqueue_message(pool, message).await;
}

/// `soft_delete_related_objects.delay("db", <model>, pk, using=None)`
/// (`db/mixins.py:77`): three positional args, `using` stays a null
/// kwarg. Fires on every instance `.delete()` in these views.
async fn enqueue_soft_delete(pool: &sqlx::PgPool, model: &str, pk: &uuid::Uuid) {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(pk.to_string()),
        ],
        kwargs,
    );
    enqueue_message(pool, message).await;
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-67`): the app
/// origin for task kwargs, never the inbound host.
fn app_origin(state: &AppState) -> String {
    if let Some(url) = state.settings().urls.app_base_url.clone() {
        return url;
    }
    if let Some(url) = state.settings().urls.web_url.clone() {
        return url;
    }
    "http://localhost".to_owned()
}

// ---------------------------------------------------------------------------
// Datetime rendering
// ---------------------------------------------------------------------------

/// DRF `iso-8601` in the actor zone (serializer paths, via the activated
/// timezone).
fn render_dt_str(raw: &str, tz: &Tz) -> String {
    match chrono::DateTime::parse_from_rfc3339(raw) {
        Ok(aware) => crate::serializer::render_datetime_in(&aware, tz),
        Err(_) => raw.to_owned(),
    }
}

/// DRF `iso-8601` UTC (`Z` suffix) for `.values()` dicts, which render
/// through the plain JSON encoder as stored.
fn render_dt_utc(raw: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(raw) {
        Ok(aware) => crate::serializer::render_datetime(&aware.with_timezone(&chrono::Utc)),
        Err(_) => raw.to_owned(),
    }
}

/// `logo_asset_sql` row: asset id, entity type, workspace/project/issue ids, slug.
type LogoAssetRow = (
    Uuid,
    Option<String>,
    Option<Uuid>,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
);
/// `users` lite row: id, first/last name, avatar, avatar asset, bot flag, display name.
type UserLiteQueryRow = (Uuid, String, String, String, Option<Uuid>, bool, String);
/// Relation-edge row: id, related issue, type, created/updated by, stamps.
type RelationEdgeQueryRow = (
    Uuid,
    Uuid,
    String,
    Option<Uuid>,
    Option<Uuid>,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::Utc>,
);

// ---------------------------------------------------------------------------
// User-lite nests (`created_by_detail`)
// ---------------------------------------------------------------------------

/// `SELECT` for the logo/cover/avatar asset row behind `*_url`
/// properties. The asset FK traversal is `_base_manager` (a
/// soft-deleted asset still renders its URL), so the read is unguarded.
fn logo_asset_sql() -> String {
    "SELECT \"a\".\"id\", \"a\".\"entity_type\", \"a\".\"workspace_id\", \"a\".\"project_id\", \"a\".\"issue_id\", \"w\".\"slug\" AS \"workspace_slug\" FROM \"file_assets\" AS \"a\" LEFT OUTER JOIN \"workspaces\" AS \"w\" ON (\"a\".\"workspace_id\" = \"w\".\"id\") WHERE (\"a\".\"id\" = $1)".to_owned()
}

/// Port of `FileAsset.asset_url` (`db/models/asset.py:80-99`):
/// static-asset branches render `/api/assets/v2/static/<id>/`;
/// attachment/description branches interpolate ids; anything else
/// renders `None`.
fn asset_url(
    asset_id: &str,
    entity_type: Option<&str>,
    workspace_slug: Option<&str>,
    project_id: Option<&str>,
    issue_id: Option<&str>,
) -> Result<Option<String>, Denial> {
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Ok(Some(format!("/api/assets/v2/static/{asset_id}/")))
        }
        Some("ISSUE_ATTACHMENT") => {
            // A null workspace/project/issue is Django's `None.slug`
            // `AttributeError` → the generic 500, not a quiet null.
            let (slug, project, issue) = match (workspace_slug, project_id, issue_id) {
                (Some(s), Some(p), Some(i)) => (s, p, i),
                _ => return Err(Denial::ServerError),
            };
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/issues/{issue}/attachments/{asset_id}/"
            )))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let (slug, project) = match (workspace_slug, project_id) {
                (Some(s), Some(p)) => (s, p),
                _ => return Err(Denial::ServerError),
            };
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/{asset_id}/"
            )))
        }
        _ => Ok(None),
    }
}

/// Resolve `User.avatar_url` (`db/models/user.py:142-151`): the asset's
/// `asset_url` when the FK is set (even when that computes to `None` —
/// no fall-through), else the raw `avatar` string when non-empty, else
/// `None`.
async fn avatar_url_of(
    pool: &sqlx::PgPool,
    raw: Option<String>,
    asset_id: Option<uuid::Uuid>,
) -> Result<Option<String>, Denial> {
    let Some(asset_id) = asset_id else {
        return Ok(raw.filter(|text| !text.is_empty()));
    };
    let row: Option<LogoAssetRow> = sqlx::query_as(&logo_asset_sql())
        .bind(asset_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let (id, entity_type, _, project_id, issue_id, workspace_slug) =
        row.ok_or(Denial::ServerError)?;
    asset_url(
        &id.to_string(),
        entity_type.as_deref(),
        workspace_slug.as_deref(),
        project_id.map(|id| id.to_string()).as_deref(),
        issue_id.map(|id| id.to_string()).as_deref(),
    )
}

/// Owned app `UserLiteSerializer` row (`user.py:141-153`).
struct UserLiteOwned {
    id: String,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
}

/// Fetch `created_by_detail` nests for the given user ids in one query.
async fn fetch_user_lites(
    pool: &sqlx::PgPool,
    user_ids: &[uuid::Uuid],
) -> Result<HashMap<uuid::Uuid, UserLiteOwned>, Denial> {
    let mut out = HashMap::new();
    if user_ids.is_empty() {
        return Ok(out);
    }
    let rows: Vec<UserLiteQueryRow> = sqlx::query_as(
        "SELECT id, first_name, last_name, avatar, avatar_asset_id, is_bot, display_name
         FROM users WHERE id = ANY($1)",
    )
    .bind(user_ids)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    for (id, first_name, last_name, avatar, avatar_asset_id, is_bot, display_name) in rows {
        let avatar_url = avatar_url_of(pool, Some(avatar.clone()), avatar_asset_id).await?;
        out.insert(
            id,
            UserLiteOwned {
                id: id.to_string(),
                first_name,
                last_name,
                avatar,
                avatar_url,
                is_bot,
                display_name,
            },
        );
    }
    Ok(out)
}

/// Render one `created_by_detail` nest through this domain's `UserLite`
/// port (`serializers_links.rs`, FX-ISS-04).
fn user_lite_value(row: &UserLiteOwned) -> Value {
    use pidash_services::app_issues::serializers_links::{
        user_lite_to_representation, UserLiteRow,
    };
    let input = UserLiteRow {
        id: &row.id,
        first_name: &row.first_name,
        last_name: &row.last_name,
        avatar: &row.avatar,
        avatar_url: row.avatar_url.as_deref(),
        is_bot: row.is_bot,
        display_name: &row.display_name,
    };
    let view = user_lite_to_representation(&input);
    serde_json::to_value(&view).unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------
// Relations: list
// ---------------------------------------------------------------------------

/// `GET issue-relation/` (`relation.py:37-51`): the eight relation
/// buckets, each a `.values()` list rendered through the plain JSON
/// encoder (UTC datetimes, `py_float` sort order), in call order.
async fn relation_list(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_entity_gate(&pool, &slug, &project_id, &user_id, "GET").await {
        return denial.into_response();
    }
    let mut body = Map::with_capacity(8);
    for bucket in super::queries_core::RELATION_BUCKETS {
        let mut binder = super::Binder::new();
        // The bucket builder scopes slug + issue sides only
        // (`relation.py:42-50`): the path `project_id` never filters
        // the list (it only gates + stamps), so related rows from any
        // project in the workspace show — there is no wrapper.
        let sql =
            super::queries_core::relation_bucket_union_sql(&mut binder, &slug, issue_id, *bucket);
        let rows = match super::fetch_json_rows(&pool, &sql, binder.values()).await {
            Ok(rows) => rows,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let shaped: Vec<Value> = rows.iter().map(shape_relation_row).collect();
        body.insert(bucket.label.to_owned(), Value::Array(shaped));
    }
    json_response(StatusCode::OK, Value::Object(body).to_string())
}

/// Shape one relation `.values()` dict in `RELATION_ROW_FIELDS` order:
/// plain-encoder UTC datetimes, `py_float` sort order, UUIDs as strings.
fn shape_relation_row(row: &Map<String, Value>) -> Value {
    let source = row;
    let mut out = Map::with_capacity(super::queries_core::RELATION_ROW_FIELDS.len());
    for field in super::queries_core::RELATION_ROW_FIELDS {
        let value = match *field {
            "created_at" | "updated_at" => source
                .get(*field)
                .and_then(Value::as_str)
                .map(render_dt_utc)
                .map(Value::String)
                .unwrap_or(Value::Null),
            "sort_order" => source
                .get("sort_order")
                .and_then(Value::as_f64)
                .map(|float| {
                    serde_json::from_str::<Value>(&crate::paginator::py_float_str(float))
                        .unwrap_or(Value::Null)
                })
                .unwrap_or(Value::Null),
            // The union emits raw column names; the `.values()` wire keys
            // drop the `_id` suffix on the two actor FKs.
            "created_by" => source.get("created_by_id").cloned().unwrap_or(Value::Null),
            "updated_by" => source.get("updated_by_id").cloned().unwrap_or(Value::Null),
            _ => source.get(*field).cloned().unwrap_or(Value::Null),
        };
        out.insert((*field).to_owned(), value);
    }
    Value::Object(out)
}

// ---------------------------------------------------------------------------
// Relations: create
// ---------------------------------------------------------------------------

/// `POST issue-relation/` (`relation.py:190-260`): bulk-creates one
/// relation row per `issues` item (duplicates skipped, still rendered),
/// fires `issue_activity`, answers 201 with the in-memory rows.
async fn relation_create(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_entity_gate(&pool, &slug, &project_id, &user_id, "POST").await {
        return denial.into_response();
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let requested_data = python_dumps(&Value::Object(body.map.clone()));
    // `relation_type` (`relation.py:204-208`): missing/`None` → 400;
    // anything else passes through, stored via `str()` + the mapper.
    let relation_raw = body.map.get("relation_type").unwrap_or(&Value::Null);
    if relation_raw.is_null() {
        return Denial::BadMessage("Issue relation type is required".to_owned()).into_response();
    }
    let relation_text = py_str(relation_raw);
    let stored_type = get_actual_relation(&relation_text);
    let workspace_id = match project_workspace(&pool, &project_id).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    // `issues` (`relation.py:210-233`): missing → `[]`; dicts iterate
    // keys, strings iterate chars, anything else is not iterable → 500.
    let items = match body.map.get("issues").unwrap_or(&Value::Null) {
        Value::Null => Vec::new(),
        Value::Array(items) => items.clone(),
        Value::Object(map) => map.keys().map(|key| Value::String(key.clone())).collect(),
        Value::String(text) => text
            .chars()
            .map(|ch| Value::String(ch.to_string()))
            .collect(),
        _ => return Denial::ServerError.into_response(),
    };
    // Item prep (`UUIDField`): bad strings / floats / dicts / lists →
    // `ValidationError`; ints/bools ride `UUID(int=)`; `None` becomes a
    // NULL insert. Prep runs for the whole batch before the first
    // `INSERT`, so prep failures beat DB failures whatever the order.
    let mut prepared: Vec<Option<uuid::Uuid>> = Vec::with_capacity(items.len());
    for item in &items {
        match uuid_prep(item) {
            Ok(id) => prepared.push(id),
            Err(denial) => return denial.into_response(),
        }
    }
    if prepared.iter().any(Option::is_none) {
        // A NULL `issue_id`/`related_issue_id` trips `NOT NULL` →
        // `IntegrityError` → the payload 400.
        return Denial::BadError("The payload is not valid".to_owned()).into_response();
    }
    let flipped = matches!(relation_raw, Value::String(text) if ["blocking", "start_after", "finish_after"].contains(&text.as_str()));
    // `bulk_create(..., batch_size=10, ignore_conflicts=True)`
    // (`relation.py:235`): one `INSERT ... ON CONFLICT DO NOTHING` per
    // batch; the first DB error aborts, earlier batches persist.
    let mut stamps: Vec<(
        uuid::Uuid,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
    )> = Vec::with_capacity(prepared.len());
    for chunk in prepared.chunks(10) {
        let mut ids: Vec<uuid::Uuid> = Vec::with_capacity(chunk.len());
        let mut issue_ids: Vec<uuid::Uuid> = Vec::with_capacity(chunk.len());
        let mut related_ids: Vec<uuid::Uuid> = Vec::with_capacity(chunk.len());
        let mut created: Vec<chrono::DateTime<chrono::Utc>> = Vec::with_capacity(chunk.len());
        let mut updated: Vec<chrono::DateTime<chrono::Utc>> = Vec::with_capacity(chunk.len());
        for slot in chunk {
            // `auto_now_add`/`auto_now` stamp each object in Python
            // before the `INSERT`.
            let stamp = chrono::Utc::now();
            let id = uuid::Uuid::new_v4();
            let item = slot.expect("null items rejected above");
            let (issue_side, related_side) = if flipped {
                (item, issue_id)
            } else {
                (issue_id, item)
            };
            stamps.push((item, stamp, stamp));
            ids.push(id);
            issue_ids.push(issue_side);
            related_ids.push(related_side);
            created.push(stamp);
            updated.push(stamp);
        }
        // Bare `ON CONFLICT DO NOTHING`: exactly what
        // `bulk_create(ignore_conflicts=True)` emits. No arbiter is
        // possible — the dedup index
        // (`issue_relation_unique_issue_related_issue_when_deleted_at_null`)
        // is a partial unique *index*, which `ON CONFLICT ON CONSTRAINT`
        // rejects (SQLSTATE 42P10).
        let outcome = sqlx::query(
            r#"INSERT INTO issue_relations
               (id, created_at, updated_at, relation_type, issue_id, related_issue_id,
                created_by_id, updated_by_id, project_id, workspace_id)
               SELECT u.id, u.created_at, u.updated_at, $6, u.issue_id, u.related_id,
                      $7, $8, $9, $10
               FROM UNNEST($1::uuid[], $2::timestamptz[], $3::timestamptz[],
                           $4::uuid[], $5::uuid[])
                    AS u(id, created_at, updated_at, issue_id, related_id)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(&ids)
        .bind(&created)
        .bind(&updated)
        .bind(&issue_ids)
        .bind(&related_ids)
        .bind(stored_type)
        .bind(user_id)
        .bind(user_id)
        .bind(project_id)
        .bind(workspace_id)
        .execute(&pool)
        .await;
        if let Err(error) = outcome {
            return integrity_denial(error).into_response();
        }
    }
    // The response renders the in-memory rows — skipped duplicates
    // included — reading the *item* issue's columns either way
    // (`related_issue.*` for the normal serializer, `issue.*` for the
    // flipped one; `:627,:667`). Only the key order differs.
    let tz = match actor_timezone(&pool, &user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let item_ids: Vec<uuid::Uuid> = prepared.iter().map(|slot| slot.expect("checked")).collect();
    let issues = match fetch_issues_by_id(&pool, &item_ids).await {
        Ok(map) => map,
        Err(denial) => return denial.into_response(),
    };
    let mut rendered: Vec<Value> = Vec::with_capacity(stamps.len());
    for (item, created_at, updated_at) in &stamps {
        let Some(info) = issues.get(item) else {
            return Denial::ServerError.into_response();
        };
        rendered.push(render_relation_create_row(
            info,
            stored_type,
            &user_id,
            created_at,
            updated_at,
            &tz,
            flipped,
        ));
    }
    enqueue_kwargs(
        &pool,
        ISSUE_ACTIVITY_TASK,
        issue_activity_kwargs(
            "issue_relation.activity.created",
            &requested_data,
            None,
            &user_id,
            &issue_id,
            &project_id,
            &app_origin(&state),
        ),
    )
    .await;
    json_response(StatusCode::CREATED, Value::Array(rendered).to_string())
}

/// Python `str()` of a request scalar: JSON strings pass through, numbers
/// render via the CPython literal, bools capitalize, `None` is `None`
/// (only reached for stored values — missing values 400 first).
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(_) => "Array".to_owned(),
        Value::Object(_) => "Object".to_owned(),
    }
}

/// `UUIDField` prep for one relation item: strings must parse (else the
/// `ValidationError` 400), floats/dicts/lists likewise; ints/bools ride
/// `UUID(int=)` (negative or >128-bit → 400); `None` passes through to
/// the NULL insert (`Ok(None)` → the payload 400).
fn uuid_prep(item: &Value) -> Result<Option<uuid::Uuid>, Denial> {
    match item {
        Value::Null => Ok(None),
        Value::String(text) => text
            .parse::<uuid::Uuid>()
            .map(Some)
            .map_err(|_| Denial::BadError("Please provide valid detail".to_owned())),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(Denial::BadError("Please provide valid detail".to_owned()));
                }
                return Ok(Some(uuid::Uuid::from_u128(int as u128)));
            }
            if let Some(int) = number.as_u64() {
                return Ok(Some(uuid::Uuid::from_u128(u128::from(int))));
            }
            // `arbitrary_precision` literals beyond u64: in-range rides,
            // out-of-range 400s.
            match number.to_string().parse::<u128>() {
                Ok(int) => Ok(Some(uuid::Uuid::from_u128(int))),
                Err(_) => Err(Denial::BadError("Please provide valid detail".to_owned())),
            }
        }
        Value::Bool(flag) => Ok(Some(uuid::Uuid::from_u128(u128::from(*flag as u8)))),
        Value::Array(_) | Value::Object(_) => {
            Err(Denial::BadError("Please provide valid detail".to_owned()))
        }
    }
}

/// Issue columns behind the relation-create shapes.
struct RelatedIssueInfo {
    id: uuid::Uuid,
    project_id: uuid::Uuid,
    sequence_id: i32,
    name: String,
    state_id: Option<uuid::Uuid>,
    priority: String,
}

async fn fetch_issues_by_id(
    pool: &sqlx::PgPool,
    ids: &[uuid::Uuid],
) -> Result<HashMap<uuid::Uuid, RelatedIssueInfo>, Denial> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let rows: Vec<(Uuid, Uuid, i32, String, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT id, project_id, sequence_id, name, state_id, priority
         FROM issues WHERE id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    for (id, project_id, sequence_id, name, state_id, priority) in rows {
        out.insert(
            id,
            RelatedIssueInfo {
                id,
                project_id,
                sequence_id,
                name,
                state_id,
                priority,
            },
        );
    }
    Ok(out)
}

/// `IssueRelationSerializer` / `RelatedIssueSerializer` read shape
/// (`issue.py:590-667`): 11 keys (`assignee_ids` is write-only), the
/// *item* issue's columns, identical except the flipped serializer puts
/// `updated_by` before `updated_at`. A null state drops `state_id`
/// (DRF `SkipField` on the `AttributeError`).
fn render_relation_create_row(
    info: &RelatedIssueInfo,
    stored_type: &str,
    user_id: &uuid::Uuid,
    created_at: &chrono::DateTime<chrono::Utc>,
    updated_at: &chrono::DateTime<chrono::Utc>,
    tz: &Tz,
    flipped: bool,
) -> Value {
    let mut out = Map::with_capacity(11);
    out.insert("id".to_owned(), Value::String(info.id.to_string()));
    out.insert(
        "project_id".to_owned(),
        Value::String(info.project_id.to_string()),
    );
    out.insert(
        "sequence_id".to_owned(),
        Value::Number(info.sequence_id.into()),
    );
    out.insert(
        "relation_type".to_owned(),
        Value::String(stored_type.to_owned()),
    );
    out.insert("name".to_owned(), Value::String(info.name.clone()));
    if let Some(state_id) = info.state_id {
        out.insert("state_id".to_owned(), Value::String(state_id.to_string()));
    }
    out.insert("priority".to_owned(), Value::String(info.priority.clone()));
    out.insert("created_by".to_owned(), Value::String(user_id.to_string()));
    out.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &created_at.fixed_offset(),
            tz,
        )),
    );
    if flipped {
        out.insert("updated_by".to_owned(), Value::String(user_id.to_string()));
        out.insert(
            "updated_at".to_owned(),
            Value::String(crate::serializer::render_datetime_in(
                &updated_at.fixed_offset(),
                tz,
            )),
        );
    } else {
        out.insert(
            "updated_at".to_owned(),
            Value::String(crate::serializer::render_datetime_in(
                &updated_at.fixed_offset(),
                tz,
            )),
        );
        out.insert("updated_by".to_owned(), Value::String(user_id.to_string()));
    }
    Value::Object(out)
}

/// `issue_activity.delay(...)` kwargs shared by every activity call
/// below, in the `.delay` call order (`relation.py:238-249`,
/// `link.py:52-62`): `type`, `requested_data`, `actor_id`, `issue_id`,
/// `project_id`, `current_instance`, `epoch`, `notification`, `origin`.
/// `subscriber` keeps its task default (`True`) — the views never pass
/// it. `requested_data`/`current_instance` are pre-dumped JSON strings
/// (absent → `None`).
fn issue_activity_kwargs(
    activity: &str,
    requested_data: &str,
    current_instance: Option<String>,
    actor_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    origin: &str,
) -> Map<String, Value> {
    let mut kwargs = Map::new();
    kwargs.insert("type".to_owned(), Value::String(activity.to_owned()));
    kwargs.insert(
        "requested_data".to_owned(),
        Value::String(requested_data.to_owned()),
    );
    kwargs.insert("actor_id".to_owned(), Value::String(actor_id.to_string()));
    kwargs.insert("issue_id".to_owned(), Value::String(issue_id.to_string()));
    kwargs.insert(
        "project_id".to_owned(),
        Value::String(project_id.to_string()),
    );
    kwargs.insert(
        "current_instance".to_owned(),
        current_instance.map(Value::String).unwrap_or(Value::Null),
    );
    kwargs.insert(
        "epoch".to_owned(),
        Value::Number(chrono::Utc::now().timestamp().into()),
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs.insert("origin".to_owned(), Value::String(origin.to_owned()));
    kwargs
}

// ---------------------------------------------------------------------------
// Relations: remove
// ---------------------------------------------------------------------------

/// `POST remove-relation/` (`relation.py:262-284`): finds the newest live
/// edge in either direction, serializes it for the activity row,
/// soft-deletes it, fires `issue_activity`, answers 204. A miss 500s.
async fn remove_relation(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_entity_gate(&pool, &slug, &project_id, &user_id, "POST").await {
        return denial.into_response();
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let requested_data = python_dumps(&Value::Object(body.map.clone()));
    // `related_issue` (`relation.py:263-270`): the filter preps UUIDs —
    // bad strings / floats / dicts / lists → the `ValidationError` 400;
    // ints/bools ride `UUID(int=)`; `None` filters `IS NULL` (then the
    // miss 500s below).
    let related_raw = body.map.get("related_issue").unwrap_or(&Value::Null);
    let related_id = match uuid_prep(related_raw) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let row: Option<RelationEdgeRow> =
        match fetch_relation_edge(&pool, &slug, &issue_id, related_id).await {
            Ok(row) => row,
            Err(denial) => return denial.into_response(),
        };
    let Some(edge) = row else {
        return Denial::ServerError.into_response();
    };
    let tz = match actor_timezone(&pool, &user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let issues = match fetch_issues_by_id(&pool, &[edge.related_issue_id]).await {
        Ok(map) => map,
        Err(denial) => return denial.into_response(),
    };
    let Some(info) = issues.get(&edge.related_issue_id) else {
        return Denial::ServerError.into_response();
    };
    // `IssueRelationSerializer(...).data`: the stored row's own stamps
    // and audit, in the normal key order.
    let mut shape = Map::with_capacity(11);
    shape.insert("id".to_owned(), Value::String(info.id.to_string()));
    // `project_id` reads `related_issue.project` (`issue.py:592`), not
    // the edge's own stamp (they differ on cross-project edges).
    shape.insert(
        "project_id".to_owned(),
        Value::String(info.project_id.to_string()),
    );
    shape.insert(
        "sequence_id".to_owned(),
        Value::Number(info.sequence_id.into()),
    );
    shape.insert(
        "relation_type".to_owned(),
        Value::String(edge.relation_type.clone()),
    );
    shape.insert("name".to_owned(), Value::String(info.name.clone()));
    if let Some(state_id) = info.state_id {
        shape.insert("state_id".to_owned(), Value::String(state_id.to_string()));
    }
    shape.insert("priority".to_owned(), Value::String(info.priority.clone()));
    shape.insert(
        "created_by".to_owned(),
        edge.created_by_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    shape.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &edge.created_at.fixed_offset(),
            &tz,
        )),
    );
    shape.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &edge.updated_at.fixed_offset(),
            &tz,
        )),
    );
    shape.insert(
        "updated_by".to_owned(),
        edge.updated_by_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    let current_instance = python_dumps(&Value::Object(shape));
    // `.delete()` first (soft row + fan-out), then the activity row.
    let now = chrono::Utc::now();
    if let Err(error) = sqlx::query(
        "UPDATE issue_relations SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3",
    )
    .bind(now)
    .bind(user_id)
    .bind(edge.id)
    .execute(&pool)
    .await
    {
        let _ = error;
        return Denial::ServerError.into_response();
    }
    enqueue_soft_delete(&pool, "issuerelation", &edge.id).await;
    enqueue_kwargs(
        &pool,
        ISSUE_ACTIVITY_TASK,
        issue_activity_kwargs(
            "issue_relation.activity.deleted",
            &requested_data,
            Some(current_instance),
            &user_id,
            &issue_id,
            &project_id,
            &app_origin(&state),
        ),
    )
    .await;
    empty_response(StatusCode::NO_CONTENT)
}

/// One live relation edge for the removal path.
struct RelationEdgeRow {
    id: uuid::Uuid,
    related_issue_id: uuid::Uuid,
    relation_type: String,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// Newest live edge in either direction (`Meta.ordering = -created_at`,
/// `.first()`).
async fn fetch_relation_edge(
    pool: &sqlx::PgPool,
    slug: &str,
    issue_id: &uuid::Uuid,
    related_id: Option<uuid::Uuid>,
) -> Result<Option<RelationEdgeRow>, Denial> {
    let row: Option<RelationEdgeQueryRow> = sqlx::query_as(
        r#"SELECT r.id, r.related_issue_id, r.relation_type,
                      r.created_by_id, r.updated_by_id, r.created_at, r.updated_at
               FROM issue_relations r JOIN workspaces w ON w.id = r.workspace_id
               WHERE w.slug = $1 AND r.deleted_at IS NULL
                 AND ((r.issue_id = $2 AND r.related_issue_id IS NOT DISTINCT FROM $3)
                      OR (r.issue_id IS NOT DISTINCT FROM $3 AND r.related_issue_id = $2))
               ORDER BY r.created_at DESC LIMIT 1"#,
    )
    .bind(slug)
    .bind(issue_id)
    .bind(related_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(
        |(
            id,
            related_issue_id,
            relation_type,
            created_by_id,
            updated_by_id,
            created_at,
            updated_at,
        )| {
            RelationEdgeRow {
                id,
                related_issue_id,
                relation_type,
                created_by_id,
                updated_by_id,
                created_at,
                updated_at,
            }
        },
    ))
}

// ---------------------------------------------------------------------------
// Links: validation (`IssueLinkSerializer`, `issue.py:801-877`)
// ---------------------------------------------------------------------------

/// Django `URLValidator` verdict (Django 4.2 `validators.py` on Python
/// 3.12): scheme allow-list, `urlsplit` (which strict-validates
/// bracketed hosts itself, with or without userinfo), the host regex,
/// the IDN retry, then the 253-char hostname cap. Same algorithm as the
/// module-links copy (per-module copies are the codebase precedent).
fn django_url_valid(url: &str) -> bool {
    let scheme = url.split("://").next().unwrap_or("").to_lowercase();
    if !["http", "https", "ftp", "ftps"].contains(&scheme.as_str()) {
        return false;
    }
    // `unsafe_chars` (`\t\r\n`) are rejected before the regex runs.
    if url.chars().any(|c| matches!(c, '\t' | '\r' | '\n')) {
        return false;
    }
    if url_host_pattern_valid(url) {
        // First match: `urlsplit`'s bracket check (strict IP parse) plus
        // the trailing hostname cap.
        return url_brackets_strict_valid(url) && url_hostname_within_cap(url);
    }
    // IDN retry: `netloc.encode("idna")` over the whole netloc, then the
    // bare regex re-match (no strict bracket re-check — Django's
    // `else` is skipped on this path) plus the cap on the ORIGINAL host.
    idna_ace_url(url).is_some_and(|ace| url_host_pattern_valid(&ace))
        && url_hostname_within_cap(url)
}

/// The `URLValidator.regex` match over one URL string: optional
/// `user:pass@`, then IPv4 | IPv6 | hostname, optional `:port`, optional
/// path/query/fragment without whitespace.
fn url_host_pattern_valid(url: &str) -> bool {
    let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    // Optional `user:pass@` (`[^\s:@/]+(?::[^\s:@/]*)?@`): the pattern
    // cannot cross `/`, so `@` past the first `/` starts the resource.
    let host_part = match after_scheme.find('@') {
        Some(at) if after_scheme.find('/').is_none_or(|slash| at < slash) => {
            let (userinfo, rest) = after_scheme.split_at(at);
            let rest = &rest[1..];
            // `user` / `pass` carry no whitespace, `:` or `/`.
            let mut pieces = userinfo.splitn(2, ':');
            let user_ok = pieces
                .next()
                .is_some_and(|user| !user.is_empty() && user.chars().all(valid_userinfo_char));
            let pass_ok = pieces
                .next()
                .is_none_or(|pass| pass.chars().all(valid_userinfo_char));
            if !(user_ok && pass_ok) || rest.is_empty() {
                return false;
            }
            rest
        }
        _ => after_scheme,
    };
    // Split host from `:port` / path: brackets (IPv6) protect colons.
    let tail = if let Some(rest) = host_part.strip_prefix('[') {
        let Some((inside, tail)) = rest.split_once(']') else {
            return false;
        };
        if !valid_ipv6_loose(inside) {
            return false;
        }
        tail
    } else {
        let end = host_part
            .find([':', '/', '?', '#'])
            .unwrap_or(host_part.len());
        let (host, tail) = host_part.split_at(end);
        if !valid_ipv4(host) && !valid_host_name(host) {
            return false;
        }
        tail
    };
    // Optional `:port` (1-5 digits), then optional resource without whitespace.
    let mut tail = tail;
    if let Some(port) = tail.strip_prefix(':') {
        let digits = port
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if digits.is_empty() || digits.len() > 5 {
            return false;
        }
        tail = &port[digits.len()..];
    }
    if tail.is_empty() {
        return true;
    }
    let mut chars = tail.chars();
    match chars.next() {
        Some('/' | '?' | '#') => {}
        _ => return false,
    }
    !tail.chars().any(is_python_space)
}

fn valid_userinfo_char(c: char) -> bool {
    !is_python_space(c) && c != ':' && c != '@' && c != '/'
}

fn is_python_space(char: char) -> bool {
    char.is_whitespace() || matches!(char, '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{1f}')
}

/// Django's `ipv4_re`: four dot-separated groups, each `0` (bare) or
/// 1-3 digits without a leading zero, 0-255.
fn valid_ipv4(host: &str) -> bool {
    let pieces: Vec<&str> = host.split('.').collect();
    if pieces.len() != 4 {
        return false;
    }
    pieces.iter().all(|piece| {
        !piece.is_empty()
            && piece.len() <= 3
            && piece.chars().all(|c| c.is_ascii_digit())
            && (piece.len() == 1 || !piece.starts_with('0'))
            && piece.parse::<u32>().is_ok_and(|n| n <= 255)
    })
}

/// Django's `ipv6_re` (`\[[0-9a-f:.]+\]`, matched case-insensitively):
/// the loose regex half — the brackets are stripped by the caller and
/// `url_brackets_strict_valid` applies the strict parse after the match.
fn valid_ipv6_loose(inside: &str) -> bool {
    !inside.is_empty()
        && inside
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
}

/// `urlsplit`'s bracket check (Python 3.12 `_check_bracketed_netloc`):
/// any `[` / `]` in the netloc must wrap exactly one strict IP literal
/// (optional userinfo before it is fine — it is stripped first).
fn url_brackets_strict_valid(url: &str) -> bool {
    let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    let netloc_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let netloc = &after_scheme[..netloc_end];
    if !netloc.contains(['[', ']']) {
        return true;
    }
    let host = match netloc.rfind('@') {
        Some(at) => &netloc[at + 1..],
        None => netloc,
    };
    let Some(bracketed) = host.strip_prefix('[') else {
        return false;
    };
    let Some((inside, tail)) = bracketed.split_once(']') else {
        return false;
    };
    if inside.parse::<std::net::Ipv6Addr>().is_err() {
        return false;
    }
    if tail.is_empty() {
        return true;
    }
    let Some(port) = tail.strip_prefix(':') else {
        return false;
    };
    !port.is_empty() && port.len() <= 5 && port.chars().all(|c| c.is_ascii_digit())
}

/// Django's trailing hostname cap: `urlsplit().hostname` (lowercased
/// host without userinfo, port or brackets) is present and at most 253
/// characters (code points, not bytes).
fn url_hostname_within_cap(url: &str) -> bool {
    let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    let netloc_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let mut host = &after_scheme[..netloc_end];
    if let Some(at) = host.rfind('@') {
        host = &host[at + 1..];
    }
    if let Some(bracketed) = host.strip_prefix('[') {
        host = bracketed.split(']').next().unwrap_or("");
    } else if let Some(at) = host.find(':') {
        host = &host[..at];
    }
    !host.is_empty() && host.chars().count() <= 253
}

/// Django's `host_re` (`hostname + domain + tld | localhost`): labels of
/// letters/digits/hyphens (the full Unicode `ul` range allowed — no
/// exclusions), dashes never leading/trailing a label, TLD of 2+
/// letters/hyphens (digits excluded) or a `xn--` punycode label,
/// optional trailing dot.
fn valid_host_name(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for (index, label) in labels.iter().enumerate() {
        if label.is_empty() || label.chars().count() > 63 {
            return false;
        }
        if !label.chars().all(valid_host_char) {
            return false;
        }
        let first = label.chars().next().expect("label char");
        let last = label.chars().last().expect("label char");
        if first == '-' || last == '-' {
            return false;
        }
        if index == labels.len() - 1 && !valid_top_label(label) {
            return false;
        }
    }
    true
}

/// Host label chars: ASCII letters/digits/hyphen plus Django's `ul`
/// (`\u00a1-\uffff`, the whole range).
fn valid_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || ('\u{00a1}'..='\u{ffff}').contains(&c)
}

/// TLD (`[a-z\ul-]{2,63}` — no digits — or `xn--` punycode): leading and
/// trailing dashes are already excluded by the caller. The punycode branch
/// is `xn--[a-z0-9]{1,59}` — ASCII alphanumerics only, no hyphens — so a
/// hyphenated `xn--` label with digits is invalid even though the general
/// branch allows hyphens.
fn valid_top_label(label: &str) -> bool {
    if label.chars().count() < 2 {
        return false;
    }
    if label.len() >= 4 && label.as_bytes()[..4].eq_ignore_ascii_case(b"xn--") {
        let rest = &label[4..];
        if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return true;
        }
        // Else fall through to the general branch: a hyphenated `xn--`
        // label without digits still matches `[a-z\ul-]{2,63}`.
    }
    label
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c == '-' || ('\u{00a1}'..='\u{ffff}').contains(&c))
}

/// IDN retry input: `netloc.encode("idna")` over the whole netloc, like
/// Django's `punycode(netloc)` — per dot-label, ASCII labels passing
/// through untouched (even with `:`/`@`), non-ASCII labels gaining the
/// `xn--` punycode form — with the URL rebuilt around it. `None` when
/// the netloc is all ASCII (the retry would re-match the same string).
fn idna_ace_url(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let netloc_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (netloc, tail) = rest.split_at(netloc_end);
    if netloc.is_ascii() {
        return None;
    }
    let mut ace = String::new();
    for (index, label) in netloc.split('.').enumerate() {
        if index > 0 {
            ace.push('.');
        }
        if label.is_ascii() {
            ace.push_str(label);
        } else {
            // `idna` C-implements the codec's per-label punycode step;
            // a label it refuses is a `UnicodeError` there too.
            ace.push_str(&idna::domain_to_ascii(label).ok()?);
        }
    }
    Some(format!("{scheme}://{ace}{tail}"))
}

/// One field's message list, in writable-field order (`title`, `url`,
/// `metadata` — model declaration order).
type FieldErrors = Vec<(String, Value)>;

fn push_error(errors: &mut FieldErrors, field: &str, message: Value) {
    match errors.iter_mut().find(|(name, _)| name == field) {
        Some((_, Value::Array(messages))) => messages.push(message),
        Some((_, slot)) => {
            let prior = std::mem::replace(slot, Value::Null);
            *slot = Value::Array(vec![prior, message]);
        }
        None => errors.push((field.to_owned(), Value::Array(vec![message]))),
    }
}

/// Direct (unlisted) field detail: `ValidationError({"url": {...}})`.
fn set_error(errors: &mut FieldErrors, field: &str, value: Value) {
    match errors.iter_mut().find(|(name, _)| name == field) {
        Some((_, slot)) => *slot = value,
        None => errors.push((field.to_owned(), value)),
    }
}

fn errors_value(errors: &FieldErrors) -> Value {
    let mut out = Map::with_capacity(errors.len());
    for (field, messages) in errors {
        out.insert(field.clone(), messages.clone());
    }
    Value::Object(out)
}

/// Python truthiness of a request value (`if url` in `to_internal_value`).
fn is_python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Validated link fields (`None` = key absent from the input).
#[derive(Debug)]
struct LinkFields {
    title: Option<Option<String>>,
    url: Option<String>,
    metadata: Option<Value>,
}

/// `IssueLinkSerializer` validation (`issue.py:804-877`): the
/// `to_internal_value` scheme step (with its truthy-non-string 500),
/// `CharField` rules for `title` (nullable + blank, 255 chars,
/// int/float coerced, bools rejected), `url` (required unless partial,
/// non-nullable, `URLValidator` → the nested `{"error": ...}` shape),
/// and `metadata` (any JSON but null). Unknown and read-only keys are
/// ignored. Errors collect in writable-field order.
fn validate_link_body(body: &Map<String, Value>, partial: bool) -> Result<LinkFields, Denial> {
    let mut errors: FieldErrors = Vec::new();
    // `to_internal_value` (`issue.py:817-823`) runs before field
    // validation: truthy non-string `url` 500s on `.startswith`; other
    // values pass through (the prepend below).
    let mut url_raw: Option<Value> = body.get("url").cloned();
    if let Some(raw) = url_raw.clone() {
        if !matches!(raw, Value::String(_)) && is_python_truthy(&raw) {
            return Err(Denial::ServerError);
        }
        if let Value::String(text) = &raw {
            if !text.is_empty() && !text.starts_with("http://") && !text.starts_with("https://") {
                url_raw = Some(Value::String(format!("http://{text}")));
            }
        }
    }
    // `title`: `CharField(max_length=255, null=True, blank=True)` →
    // `required=False`. Absent on full writes too (model null).
    let mut title: Option<Option<String>> = None;
    if let Some(raw) = body.get("title") {
        match raw {
            Value::Null => title = Some(None),
            Value::String(text) => {
                if text.chars().count() > 255 {
                    push_error(
                        &mut errors,
                        "title",
                        Value::String(
                            "Ensure this field has no more than 255 characters.".to_owned(),
                        ),
                    );
                } else {
                    title = Some(Some(text.clone()));
                }
            }
            Value::Bool(_) => push_error(
                &mut errors,
                "title",
                Value::String("Not a valid string.".to_owned()),
            ),
            Value::Number(number) => title = Some(Some(number.to_string())),
            Value::Array(_) | Value::Object(_) => push_error(
                &mut errors,
                "title",
                Value::String("Not a valid string.".to_owned()),
            ),
        }
    }
    // `url`: required (unless partial), non-nullable, non-blank, then
    // `URLValidator` (`issue.py:826-830`) whose nested dict error keeps
    // its `{"error": ...}` shape under the `url` key.
    let mut url: Option<String> = None;
    match url_raw {
        None if partial => {}
        None => push_error(
            &mut errors,
            "url",
            Value::String("This field is required.".to_owned()),
        ),
        Some(Value::Null) => push_error(
            &mut errors,
            "url",
            Value::String("This field may not be null.".to_owned()),
        ),
        Some(Value::String(text)) => {
            if text.is_empty() {
                push_error(
                    &mut errors,
                    "url",
                    Value::String("This field may not be blank.".to_owned()),
                );
            } else if !django_url_valid(&text) {
                // Bare dict, not a one-list: DRF renders a dict
                // detail as-is (`{"url": {"error": ...}}`).
                let mut nested = Map::new();
                nested.insert(
                    "error".to_owned(),
                    Value::String("Invalid URL format.".to_owned()),
                );
                set_error(&mut errors, "url", Value::Object(nested));
            } else {
                url = Some(text);
            }
        }
        // Falsy non-strings (`false`, `0`, `""`-handled-above): no
        // `.startswith` call, so field validation rejects them as
        // non-strings.
        Some(Value::Bool(_)) | Some(Value::Number(_)) => push_error(
            &mut errors,
            "url",
            Value::String("Not a valid string.".to_owned()),
        ),
        Some(Value::Array(_) | Value::Object(_)) => push_error(
            &mut errors,
            "url",
            Value::String("Not a valid string.".to_owned()),
        ),
    }
    // `metadata`: `JSONField(default=dict)` → `required=False`,
    // non-nullable, any other JSON verbatim.
    let mut metadata: Option<Value> = None;
    if let Some(raw) = body.get("metadata") {
        if raw.is_null() {
            push_error(
                &mut errors,
                "metadata",
                Value::String("This field may not be null.".to_owned()),
            );
        } else {
            metadata = Some(raw.clone());
        }
    }
    if !errors.is_empty() {
        return Err(Denial::BadJson(errors_value(&errors)));
    }
    Ok(LinkFields {
        title,
        url,
        metadata,
    })
}

// ---------------------------------------------------------------------------
// Links: shapes + reads
// ---------------------------------------------------------------------------

/// Owned link row behind the full `IssueLinkSerializer` read shape.
struct IssueLinkOwned {
    id: uuid::Uuid,
    created_by_id: Option<uuid::Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    title: Option<String>,
    url: String,
    metadata: Value,
    updated_by_id: Option<uuid::Uuid>,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    issue_id: uuid::Uuid,
}

/// Full `IssueLinkSerializer` read shape (`issue.py:801-815`,
/// `fields="__all__"`): 13 keys in model-declaration order with the
/// declared `created_by_detail` nest spliced after `id`. Inlined (rather
/// than reusing the space port) because the app `created_by` is nullable
/// and renders a null nest.
fn render_link_row(
    row: &IssueLinkOwned,
    lites: &HashMap<uuid::Uuid, UserLiteOwned>,
    tz: &Tz,
) -> Value {
    let mut out = Map::with_capacity(13);
    out.insert("id".to_owned(), Value::String(row.id.to_string()));
    out.insert(
        "created_by_detail".to_owned(),
        row.created_by_id
            .and_then(|id| lites.get(&id))
            .map(user_lite_value)
            .unwrap_or(Value::Null),
    );
    out.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &row.created_at.fixed_offset(),
            tz,
        )),
    );
    out.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &row.updated_at.fixed_offset(),
            tz,
        )),
    );
    out.insert(
        "deleted_at".to_owned(),
        row.deleted_at
            .map(|stamp| {
                Value::String(crate::serializer::render_datetime_in(
                    &stamp.fixed_offset(),
                    tz,
                ))
            })
            .unwrap_or(Value::Null),
    );
    out.insert(
        "title".to_owned(),
        row.title.clone().map(Value::String).unwrap_or(Value::Null),
    );
    out.insert("url".to_owned(), Value::String(row.url.clone()));
    out.insert("metadata".to_owned(), row.metadata.clone());
    out.insert(
        "created_by".to_owned(),
        row.created_by_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    out.insert(
        "updated_by".to_owned(),
        row.updated_by_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    out.insert(
        "project".to_owned(),
        Value::String(row.project_id.to_string()),
    );
    out.insert(
        "workspace".to_owned(),
        Value::String(row.workspace_id.to_string()),
    );
    out.insert("issue".to_owned(), Value::String(row.issue_id.to_string()));
    Value::Object(out)
}

/// Scoped link list (`link.py:26-35` + `queries_core::link_list_sql`).
async fn fetch_scoped_links(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Vec<IssueLinkOwned>, Denial> {
    let mut binder = super::Binder::new();
    let sql =
        super::queries_core::link_list_sql(&mut binder, slug, *project_id, *issue_id, *user_id);
    let rows = super::fetch_json_rows(pool, &sql, binder.values())
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.iter().map(link_owned_from_json).collect()
}

/// Scoped link detail: the same scope plus the pk (`get_object` over
/// the filtered queryset).
async fn fetch_scoped_link(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    pk: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Option<IssueLinkOwned>, Denial> {
    let mut binder = super::Binder::new();
    let list =
        super::queries_core::link_list_sql(&mut binder, slug, *project_id, *issue_id, *user_id);
    // The list SQL ends in `ORDER BY`: the pk predicate wraps it, it is
    // not appended.
    let marker = binder.bind_uuid(*pk);
    let sql = format!("SELECT * FROM ({list}) AS scoped WHERE scoped.id = {marker} LIMIT 1");
    let rows = super::fetch_json_rows(pool, &sql, binder.values())
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.first().map(link_owned_from_json).transpose()
}

/// Unscoped link read (`IssueLink.objects.get(workspace__slug,
/// project_id=, issue_id=, id=)` on the default manager): what the
/// custom `partial_update`/`destroy` use (member/archived-blind).
async fn fetch_plain_link(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<IssueLinkOwned>, Denial> {
    let mut binder = super::Binder::new();
    let slug_m = binder.bind_string(slug.to_owned());
    let project_m = binder.bind_uuid(*project_id);
    let issue_m = binder.bind_uuid(*issue_id);
    let pk_m = binder.bind_uuid(*pk);
    // Plain columns: `fetch_json_rows` applies its own `row_to_json`.
    let sql = format!("SELECT l.id, l.created_at, l.created_by_id, l.deleted_at, l.issue_id, l.metadata, l.project_id, l.title, l.updated_at, l.updated_by_id, l.url, l.workspace_id FROM issue_links l JOIN workspaces w ON w.id = l.workspace_id WHERE w.slug = {slug_m} AND l.project_id = {project_m} AND l.issue_id = {issue_m} AND l.id = {pk_m} AND l.deleted_at IS NULL LIMIT 1");
    let rows = super::fetch_json_rows(pool, &sql, binder.values())
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.first().map(link_owned_from_json).transpose()
}

fn link_owned_from_json(row: &Map<String, Value>) -> Result<IssueLinkOwned, Denial> {
    let o = row;
    let req_uuid = |key: &str| -> Result<uuid::Uuid, Denial> {
        o.get(key)
            .and_then(Value::as_str)
            .and_then(|text| text.parse::<uuid::Uuid>().ok())
            .ok_or(Denial::ServerError)
    };
    let opt_uuid = |key: &str| -> Result<Option<uuid::Uuid>, Denial> {
        match o.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => text
                .parse::<uuid::Uuid>()
                .map(Some)
                .map_err(|_| Denial::ServerError),
            Some(_) => Err(Denial::ServerError),
        }
    };
    let req_dt = |key: &str| -> Result<chrono::DateTime<chrono::Utc>, Denial> {
        o.get(key)
            .and_then(Value::as_str)
            .and_then(|text| {
                chrono::DateTime::parse_from_rfc3339(text)
                    .ok()
                    .map(|aware| aware.with_timezone(&chrono::Utc))
            })
            .ok_or(Denial::ServerError)
    };
    let opt_dt = |key: &str| -> Result<Option<chrono::DateTime<chrono::Utc>>, Denial> {
        match o.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => chrono::DateTime::parse_from_rfc3339(text)
                .map(|aware| Some(aware.with_timezone(&chrono::Utc)))
                .map_err(|_| Denial::ServerError),
            Some(_) => Err(Denial::ServerError),
        }
    };
    Ok(IssueLinkOwned {
        id: req_uuid("id")?,
        created_by_id: opt_uuid("created_by_id")?,
        created_at: req_dt("created_at")?,
        updated_at: req_dt("updated_at")?,
        deleted_at: opt_dt("deleted_at")?,
        title: o.get("title").and_then(Value::as_str).map(str::to_owned),
        url: o
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(Denial::ServerError)?,
        metadata: o.get("metadata").cloned().unwrap_or(Value::Null),
        updated_by_id: opt_uuid("updated_by_id")?,
        project_id: req_uuid("project_id")?,
        workspace_id: req_uuid("workspace_id")?,
        issue_id: req_uuid("issue_id")?,
    })
}

// ---------------------------------------------------------------------------
// Links: handlers
// ---------------------------------------------------------------------------

/// `GET issue-links/` (`link.py:26-35`): the DRF default list over the
/// scoped queryset — a bare array, `-created_at`, deduped.
async fn link_list(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_entity_gate(&pool, &slug, &project_id, &user_id, "GET").await {
        return denial.into_response();
    }
    let tz = match actor_timezone(&pool, &user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let rows = match fetch_scoped_links(&pool, &slug, &project_id, &issue_id, &user_id).await {
        Ok(rows) => rows,
        Err(denial) => return denial.into_response(),
    };
    let creator_ids: Vec<uuid::Uuid> = rows.iter().filter_map(|row| row.created_by_id).collect();
    let lites = match fetch_user_lites(&pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    let rendered: Vec<Value> = rows
        .iter()
        .map(|row| render_link_row(row, &lites, &tz))
        .collect();
    json_response(StatusCode::OK, Value::Array(rendered).to_string())
}

/// `POST issue-links/` (`link.py:37-67`): validates, dup-checks,
/// creates (CRUM stamps `created_by`), fires `crawl` + `issue_activity`,
/// then re-fetches through the *scoped* queryset (archived projects
/// 404 here, after the row and the tasks) and answers 201.
async fn link_create(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_entity_gate(&pool, &slug, &project_id, &user_id, "POST").await {
        return denial.into_response();
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let fields = match validate_link_body(&body.map, false) {
        Ok(fields) => fields,
        Err(denial) => return denial.into_response(),
    };
    let LinkFields {
        title,
        url,
        metadata,
    } = fields;
    let url = url.expect("url required on create");
    let workspace_id = match project_workspace(&pool, &project_id).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    // `create()` dup probe (`issue.py:836-847`): same URL + issue, live
    // rows only.
    let dupe: Option<(i32,)> = match sqlx::query_as(
        "SELECT 1 FROM issue_links
         WHERE url = $1 AND issue_id = $2 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(&url)
    .bind(issue_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if dupe.is_some() {
        return Denial::BadError("URL already exists for this Issue".to_owned()).into_response();
    }
    let now = chrono::Utc::now();
    let id = uuid::Uuid::new_v4();
    let metadata = metadata.unwrap_or(Value::Object(Map::new()));
    if let Err(error) = sqlx::query(
        r#"INSERT INTO issue_links
           (id, created_at, updated_at, title, url, metadata, created_by_id,
            updated_by_id, issue_id, project_id, workspace_id)
           VALUES ($1, $2, $2, $3, $4, $5, $6, NULL, $7, $8, $9)"#,
    )
    .bind(id)
    .bind(now)
    .bind(title.clone().unwrap_or(None))
    .bind(&url)
    .bind(&metadata)
    .bind(user_id)
    .bind(issue_id)
    .bind(project_id)
    .bind(workspace_id)
    .execute(&pool)
    .await
    {
        return integrity_denial(error).into_response();
    }
    enqueue_args(
        &pool,
        CRAWL_LINK_TASK,
        vec![Value::String(id.to_string()), Value::String(url.clone())],
    )
    .await;
    let tz = match actor_timezone(&pool, &user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let lites = match fetch_user_lites(&pool, &[user_id]).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    let shape = render_link_row(
        &IssueLinkOwned {
            id,
            created_by_id: Some(user_id),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            title: title.unwrap_or(None),
            url: url.clone(),
            metadata: metadata.clone(),
            updated_by_id: None,
            project_id,
            workspace_id,
            issue_id,
        },
        &lites,
        &tz,
    );
    // `requested_data` here is the saved serializer data, not the raw
    // input (`link.py:43`).
    let requested_data = python_dumps(&shape);
    enqueue_kwargs(
        &pool,
        ISSUE_ACTIVITY_TASK,
        issue_activity_kwargs(
            "link.activity.created",
            &requested_data,
            None,
            &user_id,
            &issue_id,
            &project_id,
            &app_origin(&state),
        ),
    )
    .await;
    // Scoped re-fetch: archived projects 404 *after* the row + tasks.
    let reread = match fetch_scoped_link(&pool, &slug, &project_id, &issue_id, &id, &user_id).await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(row) = reread else {
        return Denial::NotFound.into_response();
    };
    let shape = render_link_row(&row, &lites, &tz);
    json_response(StatusCode::CREATED, shape.to_string())
}

/// Shared detail preamble: strict pk (else proxy), pool, actor, project
/// rewrite, entity gate. Returns the resolved context for the pk paths.
struct DetailContext {
    pool: sqlx::PgPool,
    slug: String,
    project_id: uuid::Uuid,
    issue_id: uuid::Uuid,
    pk: uuid::Uuid,
    user_id: uuid::Uuid,
    tz: Tz,
}

async fn detail_context(
    state: &AppState,
    params: &HashMap<String, String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: &str,
) -> Result<DetailContext, Denial> {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let pk_raw = params.get("pk").cloned().unwrap_or_default();
    // Strictness is tri-state here: invalid segments proxy (handled by
    // the caller, which owns the request), so surface them distinctly.
    let issue_id = path_uuid(&issue_raw).map_err(|()| Denial::ServerError)?;
    let pk = path_uuid(&pk_raw).map_err(|()| Denial::ServerError)?;
    let pool = pool_of(state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    check_entity_gate(&pool, &slug, &project_id, &user_id, method).await?;
    let tz = actor_timezone(&pool, &user_id).await?;
    Ok(DetailContext {
        pool,
        slug,
        project_id,
        issue_id,
        pk,
        user_id,
        tz,
    })
}

/// `GET issue-links/{pk}/` (DRF default `retrieve`): scoped
/// `get_object` (404 `{"detail": "No IssueLink matches..."}`), 200.
async fn link_retrieve(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    if path_uuid(
        params
            .get("issue_id")
            .map(String::as_str)
            .unwrap_or_default(),
    )
    .is_err()
        || path_uuid(params.get("pk").map(String::as_str).unwrap_or_default()).is_err()
    {
        return crate::edge::proxy(State(state), req).await;
    }
    let ctx = match detail_context(&state, &params, extension, "GET").await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_scoped_link(
        &ctx.pool,
        &ctx.slug,
        &ctx.project_id,
        &ctx.issue_id,
        &ctx.pk,
        &ctx.user_id,
    )
    .await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(row) = row else {
        return Denial::LinkNotFound.into_response();
    };
    let creator_ids: Vec<uuid::Uuid> = row.created_by_id.into_iter().collect();
    let lites = match fetch_user_lites(&ctx.pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    json_response(
        StatusCode::OK,
        render_link_row(&row, &lites, &ctx.tz).to_string(),
    )
}

/// `PUT issue-links/{pk}/` (DRF default `update`, `link.py` defines no
/// `update`): scoped `get_object`, full validation, dup probe excluding
/// self, save — and no tasks — 200 with the saved row.
async fn link_update(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    if path_uuid(
        params
            .get("issue_id")
            .map(String::as_str)
            .unwrap_or_default(),
    )
    .is_err()
        || path_uuid(params.get("pk").map(String::as_str).unwrap_or_default()).is_err()
    {
        return crate::edge::proxy(State(state), req).await;
    }
    let ctx = match detail_context(&state, &params, extension, "PUT").await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_scoped_link(
        &ctx.pool,
        &ctx.slug,
        &ctx.project_id,
        &ctx.issue_id,
        &ctx.pk,
        &ctx.user_id,
    )
    .await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(mut row) = row else {
        return Denial::LinkNotFound.into_response();
    };
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let fields = match validate_link_body(&body.map, false) {
        Ok(fields) => fields,
        Err(denial) => return denial.into_response(),
    };
    let url = fields.url.expect("url required on full update");
    // `update()` dup probe (`issue.py:849-860`): same URL + issue,
    // excluding self.
    match link_dupe_exists(&ctx.pool, &url, &ctx.issue_id, Some(&ctx.pk)).await {
        Ok(true) => {
            return Denial::BadError("URL already exists for this Issue".to_owned()).into_response()
        }
        Ok(false) => {}
        Err(denial) => return denial.into_response(),
    }
    let now = chrono::Utc::now();
    // Absent keys stay out of `validated_data`: unchanged on full writes
    // too (only `url`, always present, is guaranteed to apply).
    if let Some(title) = fields.title {
        row.title = title;
    }
    row.url = url;
    if let Some(metadata) = fields.metadata {
        row.metadata = metadata;
    }
    row.updated_at = now;
    row.updated_by_id = Some(ctx.user_id);
    if let Err(error) = sqlx::query(
        "UPDATE issue_links SET title = $1, url = $2, metadata = $3,
                updated_at = $4, updated_by_id = $5 WHERE id = $6",
    )
    .bind(row.title.clone())
    .bind(&row.url)
    .bind(&row.metadata)
    .bind(now)
    .bind(ctx.user_id)
    .bind(ctx.pk)
    .execute(&ctx.pool)
    .await
    {
        return integrity_denial(error).into_response();
    }
    let creator_ids: Vec<uuid::Uuid> = row.created_by_id.into_iter().collect();
    let lites = match fetch_user_lites(&ctx.pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    json_response(
        StatusCode::OK,
        render_link_row(&row, &lites, &ctx.tz).to_string(),
    )
}

/// `PATCH issue-links/{pk}/` (`link.py:69-94`): unscoped `.get`
/// (member/archived-blind, 404 `{"error": ...}`), raw `requested_data`,
/// serialized `current_instance`, partial validation, dup probe
/// excluding self, save, `crawl` + `issue_activity`, scoped re-fetch
/// (archived 404s after persisting), 200.
async fn link_partial_update(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    if path_uuid(
        params
            .get("issue_id")
            .map(String::as_str)
            .unwrap_or_default(),
    )
    .is_err()
        || path_uuid(params.get("pk").map(String::as_str).unwrap_or_default()).is_err()
    {
        return crate::edge::proxy(State(state), req).await;
    }
    let ctx = match detail_context(&state, &params, extension, "PATCH").await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_plain_link(
        &ctx.pool,
        &ctx.slug,
        &ctx.project_id,
        &ctx.issue_id,
        &ctx.pk,
    )
    .await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(mut row) = row else {
        return Denial::NotFound.into_response();
    };
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let requested_data = python_dumps(&Value::Object(body.map.clone()));
    let creator_ids: Vec<uuid::Uuid> = row.created_by_id.into_iter().collect();
    let lites = match fetch_user_lites(&ctx.pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    let current_instance = python_dumps(&render_link_row(&row, &lites, &ctx.tz));
    let fields = match validate_link_body(&body.map, true) {
        Ok(fields) => fields,
        Err(denial) => return denial.into_response(),
    };
    if let Some(url) = fields.url {
        match link_dupe_exists(&ctx.pool, &url, &ctx.issue_id, Some(&ctx.pk)).await {
            Ok(true) => {
                return Denial::BadError("URL already exists for this Issue".to_owned())
                    .into_response()
            }
            Ok(false) => {}
            Err(denial) => return denial.into_response(),
        }
        row.url = url;
    }
    if let Some(title) = fields.title {
        row.title = title;
    }
    if let Some(metadata) = fields.metadata {
        row.metadata = metadata;
    }
    let now = chrono::Utc::now();
    row.updated_at = now;
    row.updated_by_id = Some(ctx.user_id);
    if let Err(error) = sqlx::query(
        "UPDATE issue_links SET title = $1, url = $2, metadata = $3,
                updated_at = $4, updated_by_id = $5 WHERE id = $6",
    )
    .bind(row.title.clone())
    .bind(&row.url)
    .bind(&row.metadata)
    .bind(now)
    .bind(ctx.user_id)
    .bind(ctx.pk)
    .execute(&ctx.pool)
    .await
    {
        return integrity_denial(error).into_response();
    }
    enqueue_args(
        &ctx.pool,
        CRAWL_LINK_TASK,
        vec![
            Value::String(row.id.to_string()),
            Value::String(row.url.clone()),
        ],
    )
    .await;
    enqueue_kwargs(
        &ctx.pool,
        ISSUE_ACTIVITY_TASK,
        issue_activity_kwargs(
            "link.activity.updated",
            &requested_data,
            Some(current_instance),
            &ctx.user_id,
            &ctx.issue_id,
            &ctx.project_id,
            &app_origin(&state),
        ),
    )
    .await;
    // Scoped re-fetch: archived projects 404 after persisting + tasks.
    let reread = match fetch_scoped_link(
        &ctx.pool,
        &ctx.slug,
        &ctx.project_id,
        &ctx.issue_id,
        &ctx.pk,
        &ctx.user_id,
    )
    .await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(row) = reread else {
        return Denial::NotFound.into_response();
    };
    let creator_ids: Vec<uuid::Uuid> = row.created_by_id.into_iter().collect();
    let lites = match fetch_user_lites(&ctx.pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    json_response(
        StatusCode::OK,
        render_link_row(&row, &lites, &ctx.tz).to_string(),
    )
}

/// `DELETE issue-links/{pk}/` (`link.py:96-113`): unscoped `.get`,
/// serialized `current_instance`, `issue_activity` *before* the delete,
/// soft delete + fan-out, 204.
async fn link_destroy(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    if path_uuid(
        params
            .get("issue_id")
            .map(String::as_str)
            .unwrap_or_default(),
    )
    .is_err()
        || path_uuid(params.get("pk").map(String::as_str).unwrap_or_default()).is_err()
    {
        return crate::edge::proxy(State(state), req).await;
    }
    let ctx = match detail_context(&state, &params, extension, "DELETE").await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_plain_link(
        &ctx.pool,
        &ctx.slug,
        &ctx.project_id,
        &ctx.issue_id,
        &ctx.pk,
    )
    .await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(row) = row else {
        return Denial::NotFound.into_response();
    };
    let creator_ids: Vec<uuid::Uuid> = row.created_by_id.into_iter().collect();
    let lites = match fetch_user_lites(&ctx.pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    let current_instance = python_dumps(&render_link_row(&row, &lites, &ctx.tz));
    let mut requested = Map::new();
    requested.insert("link_id".to_owned(), Value::String(row.id.to_string()));
    enqueue_kwargs(
        &ctx.pool,
        ISSUE_ACTIVITY_TASK,
        issue_activity_kwargs(
            "link.activity.deleted",
            &python_dumps(&Value::Object(requested)),
            Some(current_instance),
            &ctx.user_id,
            &ctx.issue_id,
            &ctx.project_id,
            &app_origin(&state),
        ),
    )
    .await;
    let now = chrono::Utc::now();
    if let Err(error) = sqlx::query(
        "UPDATE issue_links SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3",
    )
    .bind(now)
    .bind(ctx.user_id)
    .bind(ctx.pk)
    .execute(&ctx.pool)
    .await
    {
        let _ = error;
        return Denial::ServerError.into_response();
    }
    enqueue_soft_delete(&ctx.pool, "issuelink", &ctx.pk).await;
    empty_response(StatusCode::NO_CONTENT)
}

/// The `IssueLinkSerializer` dup probe: same URL + live issue row,
/// optionally excluding one pk.
async fn link_dupe_exists(
    pool: &sqlx::PgPool,
    url: &str,
    issue_id: &uuid::Uuid,
    exclude: Option<&uuid::Uuid>,
) -> Result<bool, Denial> {
    let row: Option<(i32,)> = match exclude {
        Some(pk) => {
            sqlx::query_as(
                "SELECT 1 FROM issue_links
                 WHERE url = $1 AND issue_id = $2 AND id != $3 AND deleted_at IS NULL LIMIT 1",
            )
            .bind(url)
            .bind(issue_id)
            .bind(pk)
            .fetch_optional(pool)
            .await
        }
        None => {
            sqlx::query_as(
                "SELECT 1 FROM issue_links
                 WHERE url = $1 AND issue_id = $2 AND deleted_at IS NULL LIMIT 1",
            )
            .bind(url)
            .bind(issue_id)
            .fetch_optional(pool)
            .await
        }
    }
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// PR links + code reviews: list SQL + shapes
// ---------------------------------------------------------------------------

/// `GithubPullRequestLinkViewSet.get_queryset` (`github_pr.py:37-48`) as
/// `row_to_json`: member join (no `deleted_at` guard — bug 7), archived
/// guard, `-created_at`, `.distinct()`. Same scope shape as
/// `queries_core::link_list_sql`.
fn pr_link_list_sql(
    binder: &mut super::Binder,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
) -> String {
    let slug_m = binder.bind_string(slug.to_owned());
    let project_m = binder.bind_uuid(*project_id);
    let issue_m = binder.bind_uuid(*issue_id);
    // Plain columns: `fetch_json_rows` applies its own `row_to_json`
    // wrapper. (A `row_to_json` here double-encodes and the parser sees
    // `{"row_to_json": {...}}` instead of the row.)
    format!(
        "SELECT DISTINCT link.id, link.created_at, link.updated_at, link.deleted_at, link.url, link.title, link.state, link.merged, link.draft, link.repo_owner, link.repo_name, link.pr_number, link.pr_updated_at, link.created_by_id, link.updated_by_id, link.project_id, link.workspace_id, link.issue_id FROM github_pull_request_links AS link INNER JOIN project_members AS member ON (link.project_id = member.project_id) INNER JOIN projects AS project ON (link.project_id = project.id) INNER JOIN workspaces AS ws ON (link.workspace_id = ws.id) WHERE (link.deleted_at IS NULL AND link.issue_id = {issue_m} AND link.project_id = {project_m} AND project.archived_at IS NULL AND ws.slug = {slug_m}) ORDER BY link.created_at DESC",
    )
}

/// `GitCodeReviewLinkViewSet.get_queryset` (`git_code_review.py:33-44`),
/// same scope shape.
fn review_link_list_sql(
    binder: &mut super::Binder,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
) -> String {
    let slug_m = binder.bind_string(slug.to_owned());
    let project_m = binder.bind_uuid(*project_id);
    let issue_m = binder.bind_uuid(*issue_id);
    // Plain columns (see `pr_link_list_sql`): the fetch helper wraps.
    format!(
        "SELECT DISTINCT link.id, link.created_at, link.updated_at, link.deleted_at, link.provider, link.host_url, link.namespace, link.repo_name, link.repo_external_id, link.external_id, link.external_iid, link.url, link.title, link.state, link.merged, link.draft, link.metadata, link.remote_updated_at, link.created_by_id, link.updated_by_id, link.project_id, link.workspace_id, link.issue_id FROM git_code_review_links AS link INNER JOIN project_members AS member ON (link.project_id = member.project_id) INNER JOIN projects AS project ON (link.project_id = project.id) INNER JOIN workspaces AS ws ON (link.workspace_id = ws.id) WHERE (link.deleted_at IS NULL AND link.issue_id = {issue_m} AND link.project_id = {project_m} AND project.archived_at IS NULL AND ws.slug = {slug_m}) ORDER BY link.created_at DESC",
    )
}

/// Owned PR-link row for the read shape.
struct PrLinkOwned {
    id: String,
    issue_id: String,
    repo_owner: String,
    repo_name: String,
    pr_number: i32,
    url: String,
    title: String,
    state: String,
    merged: bool,
    draft: bool,
    pr_updated_at: Option<String>,
    created_at: String,
    updated_at: String,
    created_by_id: Option<uuid::Uuid>,
}

/// Owned review-link row for the read shape.
struct ReviewLinkOwned {
    id: String,
    issue_id: String,
    provider: String,
    host_url: String,
    namespace: String,
    repo_name: String,
    repo_external_id: String,
    external_id: String,
    external_iid: String,
    url: String,
    title: String,
    state: String,
    merged: bool,
    draft: bool,
    remote_updated_at: Option<String>,
    metadata: Value,
    created_at: String,
    updated_at: String,
    created_by_id: Option<uuid::Uuid>,
}

fn json_str(row: &Map<String, Value>, key: &str) -> Result<String, Denial> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(Denial::ServerError)
}

fn json_opt_str(row: &Map<String, Value>, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn json_opt_uuid(row: &Map<String, Value>, key: &str) -> Result<Option<uuid::Uuid>, Denial> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => text
            .parse::<uuid::Uuid>()
            .map(Some)
            .map_err(|_| Denial::ServerError),
        Some(_) => Err(Denial::ServerError),
    }
}

fn pr_owned_from_json(row: &Map<String, Value>) -> Result<PrLinkOwned, Denial> {
    let o = row;
    Ok(PrLinkOwned {
        id: json_str(o, "id")?,
        issue_id: json_str(o, "issue_id")?,
        repo_owner: json_str(o, "repo_owner")?,
        repo_name: json_str(o, "repo_name")?,
        pr_number: o
            .get("pr_number")
            .and_then(Value::as_i64)
            .and_then(|number| i32::try_from(number).ok())
            .ok_or(Denial::ServerError)?,
        url: json_str(o, "url")?,
        title: json_str(o, "title")?,
        state: json_str(o, "state")?,
        merged: o
            .get("merged")
            .and_then(Value::as_bool)
            .ok_or(Denial::ServerError)?,
        draft: o
            .get("draft")
            .and_then(Value::as_bool)
            .ok_or(Denial::ServerError)?,
        pr_updated_at: json_opt_str(o, "pr_updated_at"),
        created_at: json_str(o, "created_at")?,
        updated_at: json_str(o, "updated_at")?,
        created_by_id: json_opt_uuid(o, "created_by_id")?,
    })
}

fn review_owned_from_json(row: &Map<String, Value>) -> Result<ReviewLinkOwned, Denial> {
    let o = row;
    Ok(ReviewLinkOwned {
        id: json_str(o, "id")?,
        issue_id: json_str(o, "issue_id")?,
        provider: json_str(o, "provider")?,
        host_url: json_str(o, "host_url")?,
        namespace: json_str(o, "namespace")?,
        repo_name: json_str(o, "repo_name")?,
        repo_external_id: json_str(o, "repo_external_id")?,
        external_id: json_str(o, "external_id")?,
        external_iid: json_str(o, "external_iid")?,
        url: json_str(o, "url")?,
        title: json_str(o, "title")?,
        state: json_str(o, "state")?,
        merged: o
            .get("merged")
            .and_then(Value::as_bool)
            .ok_or(Denial::ServerError)?,
        draft: o
            .get("draft")
            .and_then(Value::as_bool)
            .ok_or(Denial::ServerError)?,
        remote_updated_at: json_opt_str(o, "remote_updated_at"),
        metadata: o.get("metadata").cloned().unwrap_or(Value::Null),
        created_at: json_str(o, "created_at")?,
        updated_at: json_str(o, "updated_at")?,
        created_by_id: json_opt_uuid(o, "created_by_id")?,
    })
}

/// `GithubPullRequestLinkSerializer` read shape through the
/// serializers-D port (FX-ISS-04), datetimes in the actor zone.
fn render_pr_row(row: &PrLinkOwned, lites: &HashMap<uuid::Uuid, UserLiteOwned>, tz: &Tz) -> Value {
    use pidash_services::app_issues::serializers_links::{
        github_pull_request_link_to_representation, GithubPullRequestLinkRow, UserLiteRow,
    };
    let creator_lite = row.created_by_id.and_then(|id| lites.get(&id));
    let creator = creator_lite.map(|lite| UserLiteRow {
        id: &lite.id,
        first_name: &lite.first_name,
        last_name: &lite.last_name,
        avatar: &lite.avatar,
        avatar_url: lite.avatar_url.as_deref(),
        is_bot: lite.is_bot,
        display_name: &lite.display_name,
    });
    let created_by = row.created_by_id.map(|id| id.to_string());
    let pr_updated_at = row
        .pr_updated_at
        .as_deref()
        .map(|raw| render_dt_str(raw, tz));
    let created_at = render_dt_str(&row.created_at, tz);
    let updated_at = render_dt_str(&row.updated_at, tz);
    let input = GithubPullRequestLinkRow {
        id: &row.id,
        issue: &row.issue_id,
        repo_owner: &row.repo_owner,
        repo_name: &row.repo_name,
        pr_number: row.pr_number,
        url: &row.url,
        title: &row.title,
        state: &row.state,
        merged: row.merged,
        draft: row.draft,
        pr_updated_at: pr_updated_at.as_deref(),
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
        created_by_detail: creator,
    };
    let view = github_pull_request_link_to_representation(&input);
    serde_json::to_value(&view).unwrap_or(Value::Null)
}

/// `GitCodeReviewLinkSerializer` read shape through the serializers-D
/// port (FX-ISS-04), datetimes in the actor zone.
fn render_review_row(
    row: &ReviewLinkOwned,
    lites: &HashMap<uuid::Uuid, UserLiteOwned>,
    tz: &Tz,
) -> Value {
    use pidash_services::app_issues::serializers_links::{
        git_code_review_link_to_representation, GitCodeReviewLinkRow, UserLiteRow,
    };
    let creator_lite = row.created_by_id.and_then(|id| lites.get(&id));
    let creator = creator_lite.map(|lite| UserLiteRow {
        id: &lite.id,
        first_name: &lite.first_name,
        last_name: &lite.last_name,
        avatar: &lite.avatar,
        avatar_url: lite.avatar_url.as_deref(),
        is_bot: lite.is_bot,
        display_name: &lite.display_name,
    });
    let created_by = row.created_by_id.map(|id| id.to_string());
    let created_at = render_dt_str(&row.created_at, tz);
    let updated_at = render_dt_str(&row.updated_at, tz);
    let remote_updated_at = row
        .remote_updated_at
        .as_deref()
        .map(|raw| render_dt_str(raw, tz));
    let input = GitCodeReviewLinkRow {
        id: &row.id,
        issue: &row.issue_id,
        provider: &row.provider,
        host_url: &row.host_url,
        namespace: &row.namespace,
        repo_name: &row.repo_name,
        repo_external_id: &row.repo_external_id,
        external_id: &row.external_id,
        external_iid: &row.external_iid,
        url: &row.url,
        title: &row.title,
        state: &row.state,
        merged: row.merged,
        draft: row.draft,
        remote_updated_at: remote_updated_at.as_deref(),
        metadata: &row.metadata,
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
        created_by_detail: creator,
    };
    let view = git_code_review_link_to_representation(&input);
    serde_json::to_value(&view).unwrap_or(Value::Null)
}

/// `GET github-pull-requests/` (`github_pr.py:37-48`): the DRF default
/// list — a bare array, `-created_at`, deduped.
async fn pr_list(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_entity_gate(&pool, &slug, &project_id, &user_id, "GET").await {
        return denial.into_response();
    }
    let tz = match actor_timezone(&pool, &user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let mut binder = super::Binder::new();
    let sql = pr_link_list_sql(&mut binder, &slug, &project_id, &issue_id);
    let rows = match super::fetch_json_rows(&pool, &sql, binder.values()).await {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut owned: Vec<PrLinkOwned> = Vec::with_capacity(rows.len());
    for row in &rows {
        match pr_owned_from_json(row) {
            Ok(row) => owned.push(row),
            Err(denial) => return denial.into_response(),
        }
    }
    let creator_ids: Vec<uuid::Uuid> = owned.iter().filter_map(|row| row.created_by_id).collect();
    let lites = match fetch_user_lites(&pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    let rendered: Vec<Value> = owned
        .iter()
        .map(|row| render_pr_row(row, &lites, &tz))
        .collect();
    json_response(StatusCode::OK, Value::Array(rendered).to_string())
}

/// `GET code-reviews/` (`git_code_review.py:33-44`): the DRF default
/// list — a bare array, `-created_at`, deduped.
async fn review_list(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_entity_gate(&pool, &slug, &project_id, &user_id, "GET").await {
        return denial.into_response();
    }
    let tz = match actor_timezone(&pool, &user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let mut binder = super::Binder::new();
    let sql = review_link_list_sql(&mut binder, &slug, &project_id, &issue_id);
    let rows = match super::fetch_json_rows(&pool, &sql, binder.values()).await {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let mut owned: Vec<ReviewLinkOwned> = Vec::with_capacity(rows.len());
    for row in &rows {
        match review_owned_from_json(row) {
            Ok(row) => owned.push(row),
            Err(denial) => return denial.into_response(),
        }
    }
    let creator_ids: Vec<uuid::Uuid> = owned.iter().filter_map(|row| row.created_by_id).collect();
    let lites = match fetch_user_lites(&pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    let rendered: Vec<Value> = owned
        .iter()
        .map(|row| render_review_row(row, &lites, &tz))
        .collect();
    json_response(StatusCode::OK, Value::Array(rendered).to_string())
}

// ---------------------------------------------------------------------------
// Stores: the `pr_links` + `code_reviews` seams over sqlx
// ---------------------------------------------------------------------------

use pidash_services::app_issues::pr_links::{
    AttachError as PrAttachError, CreatePrLinkOutcome, InstallationRow, NewPrLink, NewReviewLink,
    PrLinkRow, PrLinksStore, ReviewLinkRow, ReviewMirrorFields, StoreError as PrStoreError,
};
use pidash_services::integrations::accounts::{
    GitStore, NewProviderAccount, StoreError as GitStoreError,
};
use pidash_services::integrations::code_reviews::{
    AdapterSource, AttachRequest, CodeReviewError, CodeReviewStore, NewCodeReviewLink,
    NewLegacyLink, ReviewAdapter, ReviewParser,
};
use pidash_services::integrations::repositories::{NewRepository, RepositoryDefaults};

/// Map a sqlx failure into the `pr_links` store error.
fn pr_store_error(context: &str, error: sqlx::Error) -> PrStoreError {
    let _ = context;
    PrStoreError(format!("{context}: {error}"))
}

/// Sentinel for the deleted-project lookup miss inside PR attach: the
/// handler maps it to the 404 `DoesNotExist` body (Python's
/// `self.project` fetch on `save()`).
const PROJECT_WORKSPACE_MISS: &str = "relations: project workspace not found";

/// Sentinel for the lost create race when the re-lookup also misses:
/// Python re-raises the `IntegrityError` → the payload 400.
const PR_RACE_MISS: &str = "pr link lost the create race";

/// Marker prefix for mid-attach integrity failures (FK races on the
/// mirror writes): Python's uncaught `IntegrityError` → the payload
/// 400, so the handler maps these to 400, not 500.
const PR_PAYLOAD_INVALID: &str = "relations: payload not valid";

/// Map a mid-attach write failure: integrity violations carry the
/// payload marker (Python's `IntegrityError` 400), anything else the
/// generic store error (500).
fn pr_write_error(context: &str, error: sqlx::Error) -> PrStoreError {
    if is_integrity_error(&error) {
        PrStoreError(format!("{PR_PAYLOAD_INVALID}: {context}: {error}"))
    } else {
        pr_store_error(context, error)
    }
}

/// The live `PrLinksStore`: pool + request actor. Audit columns follow
/// CRUM (`BaseModel.save`): creates stamp `created_by`, updates stamp
/// `updated_by`, deletes stamp `updated_by`. The project workspace
/// resolves at write time (mirroring `save()`'s lazy project fetch),
/// deleted projects miss → the 404 sentinel.
struct PrStore {
    pool: sqlx::PgPool,
}

impl PrStore {
    fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    async fn workspace_of(&self, project_id: &uuid::Uuid) -> Result<uuid::Uuid, PrStoreError> {
        // [`PROJECT_WORKSPACE_LOOKUP_SQL`](pidash_services::app_issues::pr_links::PROJECT_WORKSPACE_LOOKUP_SQL),
        // plus the default-manager `deleted_at` scope: Python's
        // `self.project` traversal misses soft-deleted projects
        // (`DoesNotExist` → 404), so the lookup must too.
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| pr_store_error("project workspace", error))?;
        row.map(|row| row.0)
            .ok_or_else(|| PrStoreError(PROJECT_WORKSPACE_MISS.to_owned()))
    }
}

fn pr_link_row_from(row: &sqlx::postgres::PgRow) -> Result<PrLinkRow, PrStoreError> {
    use sqlx::Row as _;
    Ok(PrLinkRow {
        id: row
            .try_get("id")
            .map_err(|error| pr_store_error("pr link map", error))?,
        project_id: row
            .try_get("project_id")
            .map_err(|error| pr_store_error("pr link map", error))?,
        issue_id: row
            .try_get("issue_id")
            .map_err(|error| pr_store_error("pr link map", error))?,
        repo_owner: row
            .try_get("repo_owner")
            .map_err(|error| pr_store_error("pr link map", error))?,
        repo_name: row
            .try_get("repo_name")
            .map_err(|error| pr_store_error("pr link map", error))?,
        pr_number: row
            .try_get::<i32, _>("pr_number")
            .map(i64::from)
            .map_err(|error| pr_store_error("pr link map", error))?,
        url: row
            .try_get("url")
            .map_err(|error| pr_store_error("pr link map", error))?,
        title: row
            .try_get("title")
            .map_err(|error| pr_store_error("pr link map", error))?,
        state: row
            .try_get("state")
            .map_err(|error| pr_store_error("pr link map", error))?,
        merged: row
            .try_get("merged")
            .map_err(|error| pr_store_error("pr link map", error))?,
        draft: row
            .try_get("draft")
            .map_err(|error| pr_store_error("pr link map", error))?,
        pr_updated_at: row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("pr_updated_at")
            .map(|stamp| {
                stamp.map(|stamp| stamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
            })
            .map_err(|error| pr_store_error("pr link map", error))?,
    })
}

fn review_link_row_from(row: &sqlx::postgres::PgRow) -> Result<ReviewLinkRow, PrStoreError> {
    use sqlx::Row as _;
    Ok(ReviewLinkRow {
        id: row
            .try_get("id")
            .map_err(|error| pr_store_error("review link map", error))?,
        issue_id: row
            .try_get("issue_id")
            .map_err(|error| pr_store_error("review link map", error))?,
        metadata: row
            .try_get("metadata")
            .map_err(|error| pr_store_error("review link map", error))?,
    })
}

impl PrLinksStore for PrStore {
    async fn installation_for_account(
        &self,
        workspace_slug: &str,
        owner: &str,
    ) -> Result<Option<InstallationRow>, PrStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::app_issues::pr_links::INSTALLATION_LOOKUP_SQL)
                .bind(workspace_slug)
                .bind(owner)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("installation lookup", error))?;
        row.map(|row| {
            use sqlx::Row as _;
            let id: Uuid = row
                .try_get("id")
                .map_err(|error| pr_store_error("installation map", error))?;
            let installation_id: i64 = row
                .try_get("installation_id")
                .map_err(|error| pr_store_error("installation map", error))?;
            Ok(InstallationRow {
                id,
                installation_id,
            })
        })
        .transpose()
    }

    async fn issue_exists(
        &self,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<bool, PrStoreError> {
        let row: Option<(i32,)> =
            sqlx::query_as(pidash_services::app_issues::pr_links::ISSUE_EXISTS_SQL)
                .bind(issue_id)
                .bind(project_id)
                .bind(workspace_slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("issue exists", error))?;
        Ok(row.is_some())
    }

    async fn pr_link_by_pr(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Option<PrLinkRow>, PrStoreError> {
        let number_i32 = i32::try_from(number).map_err(|_| {
            PrStoreError(format!(
                "pr_number {number} out of range for integer column"
            ))
        })?;
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::app_issues::pr_links::PR_LINK_LOOKUP_SQL)
                .bind(owner)
                .bind(name)
                .bind(number_i32)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("pr link lookup", error))?;
        row.map(|row| pr_link_row_from(&row)).transpose()
    }

    async fn review_link_by_pr(
        &self,
        owner: &str,
        name: &str,
        number: &str,
    ) -> Result<Option<ReviewLinkRow>, PrStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::app_issues::pr_links::REVIEW_LINK_LOOKUP_SQL)
                .bind(owner)
                .bind(name)
                .bind(number)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| pr_store_error("review link lookup", error))?;
        row.map(|row| review_link_row_from(&row)).transpose()
    }

    async fn create_pr_link(
        &self,
        new: &NewPrLink,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<CreatePrLinkOutcome, PrStoreError> {
        let workspace_id = self.workspace_of(&new.project_id).await?;
        let pr_number = i32::try_from(new.pr_number).map_err(|_| {
            PrStoreError(format!(
                "pr_number {} out of range for integer column",
                new.pr_number
            ))
        })?;
        let outcome = sqlx::query(pidash_services::app_issues::pr_links::PR_LINK_INSERT_SQL)
            .bind(new.id)
            .bind(now)
            .bind(now)
            .bind(actor_id)
            .bind(new.project_id)
            .bind(workspace_id)
            .bind(new.issue_id)
            .bind(&new.repo_owner)
            .bind(&new.repo_name)
            .bind(pr_number)
            .bind(&new.url)
            .bind(&new.title)
            .bind(&new.state)
            .bind(new.merged)
            .bind(new.draft)
            .bind(new.pr_updated_at.as_deref().and_then(parse_snapshot_dt))
            .execute(&self.pool)
            .await;
        match outcome {
            Ok(_) => Ok(CreatePrLinkOutcome::Created(Box::new(PrLinkRow {
                id: new.id,
                project_id: new.project_id,
                issue_id: new.issue_id,
                repo_owner: new.repo_owner.clone(),
                repo_name: new.repo_name.clone(),
                pr_number: new.pr_number,
                url: new.url.clone(),
                title: new.title.clone(),
                state: new.state.clone(),
                merged: new.merged,
                draft: new.draft,
                pr_updated_at: new.pr_updated_at.clone(),
            }))),
            Err(error) if is_unique_violation(&error) => Ok(CreatePrLinkOutcome::Conflict),
            Err(error) if is_integrity_error(&error) => Err(PrStoreError(format!(
                "{PR_PAYLOAD_INVALID}: pr link insert: {error}"
            ))),
            Err(error) => Err(pr_store_error("pr link insert", error)),
        }
    }

    async fn create_review_link(
        &self,
        new: &NewReviewLink,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<ReviewLinkRow, PrStoreError> {
        let workspace_id = self.workspace_of(&new.project_id).await?;
        sqlx::query(pidash_services::app_issues::pr_links::REVIEW_LINK_INSERT_SQL)
            .bind(new.id)
            .bind(now)
            .bind(now)
            .bind(actor_id)
            .bind(new.project_id)
            .bind(workspace_id)
            .bind(new.issue_id)
            .bind(&new.url)
            .bind(&new.title)
            .bind(&new.state)
            .bind(new.merged)
            .bind(new.draft)
            .bind(new.remote_updated_at.as_deref().and_then(parse_snapshot_dt))
            .bind(&new.metadata)
            .bind(&new.namespace)
            .bind(&new.repo_name)
            .bind(&new.external_iid)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_write_error("review link insert", error))?;
        Ok(ReviewLinkRow {
            id: new.id,
            issue_id: new.issue_id,
            metadata: new.metadata.clone(),
        })
    }

    async fn update_review_link(
        &self,
        id: Uuid,
        fields: &ReviewMirrorFields,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), PrStoreError> {
        let workspace_id = self.workspace_of(&fields.project_id).await?;
        sqlx::query(pidash_services::app_issues::pr_links::REVIEW_LINK_UPDATE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(fields.project_id)
            .bind(workspace_id)
            .bind(fields.issue_id)
            .bind(&fields.url)
            .bind(&fields.title)
            .bind(&fields.state)
            .bind(fields.merged)
            .bind(fields.draft)
            .bind(
                fields
                    .remote_updated_at
                    .as_deref()
                    .and_then(parse_snapshot_dt),
            )
            .bind(&fields.metadata)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_write_error("review link update", error))?;
        Ok(())
    }

    async fn delete_pr_link(
        &self,
        id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), PrStoreError> {
        sqlx::query(pidash_services::app_issues::pr_links::PR_LINK_SOFT_DELETE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_store_error("pr link delete", error))?;
        Ok(())
    }

    async fn delete_review_link(
        &self,
        id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Option<Uuid>,
    ) -> Result<(), PrStoreError> {
        sqlx::query(pidash_services::app_issues::pr_links::REVIEW_LINK_SOFT_DELETE_SQL)
            .bind(now)
            .bind(actor_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|error| pr_store_error("review link delete", error))?;
        Ok(())
    }
}

/// Parse a snapshot ISO datetime (`GithubAdapter.parse_dt` format) for a
/// timestamptz bind.
fn parse_snapshot_dt(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|aware| aware.with_timezone(&chrono::Utc))
}

use pidash_db::integrations::git_models::git_code_review_link::GitCodeReviewLink;
use pidash_db::integrations::git_models::git_provider_account::GitProviderAccount;
use pidash_db::integrations::git_models::git_repository::GitRepository;
use pidash_db::integrations::github_models::github_pull_request_link::GithubPullRequestLink;
use pidash_services::integrations::code_reviews::{LegacyLinkUpdate, LinkWriteError};

/// Map a sqlx failure into the integrations store error.
fn git_store_error(context: &str, error: sqlx::Error) -> GitStoreError {
    GitStoreError::Db(format!("{context}: {error}"))
}

/// The live `CodeReviewStore`: pool + request actor (CRUM). Audit
/// columns follow `BaseModel.save` — the port passes `None` audit and
/// leaves stamping to the executing layer, exactly where Python does it
/// — and the delete methods soft-delete (`SoftDeleteModel.delete`) with
/// the `soft_delete_related_objects` fan-out inline, preserving the
/// update-then-enqueue order of `mixins.py:71-78`.
struct ReviewStore {
    pool: sqlx::PgPool,
    actor: uuid::Uuid,
}

impl ReviewStore {
    fn new(pool: sqlx::PgPool, actor: uuid::Uuid) -> Self {
        Self { pool, actor }
    }
}

fn review_link_from_row(row: &sqlx::postgres::PgRow) -> Result<GitCodeReviewLink, GitStoreError> {
    use sqlx::Row as _;
    macro_rules! req {
        ($key:literal, $ty:ty) => {
            row.try_get::<$ty, _>($key)
                .map_err(|error| git_store_error("review link map", error))
        };
    }
    Ok(GitCodeReviewLink {
        id: req!("id", Uuid)?,
        created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
        updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
        created_by_id: req!("created_by_id", Option<Uuid>)?,
        updated_by_id: req!("updated_by_id", Option<Uuid>)?,
        deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
        project_id: req!("project_id", Uuid)?,
        workspace_id: req!("workspace_id", Uuid)?,
        issue_id: req!("issue_id", Uuid)?,
        provider: req!("provider", String)?,
        host_url: req!("host_url", String)?,
        namespace: req!("namespace", String)?,
        repo_name: req!("repo_name", String)?,
        repo_external_id: req!("repo_external_id", String)?,
        external_id: req!("external_id", String)?,
        external_iid: req!("external_iid", String)?,
        url: req!("url", String)?,
        title: req!("title", String)?,
        state: req!("state", String)?,
        merged: req!("merged", bool)?,
        draft: req!("draft", bool)?,
        remote_updated_at: req!("remote_updated_at", Option<chrono::DateTime<chrono::Utc>>)?,
        metadata: req!("metadata", Value)?,
    })
}

fn legacy_link_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<GithubPullRequestLink, GitStoreError> {
    use sqlx::Row as _;
    macro_rules! req {
        ($key:literal, $ty:ty) => {
            row.try_get::<$ty, _>($key)
                .map_err(|error| git_store_error("legacy link map", error))
        };
    }
    Ok(GithubPullRequestLink {
        id: req!("id", Uuid)?,
        created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
        updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
        created_by_id: req!("created_by_id", Option<Uuid>)?,
        updated_by_id: req!("updated_by_id", Option<Uuid>)?,
        deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
        project_id: req!("project_id", Uuid)?,
        workspace_id: req!("workspace_id", Uuid)?,
        issue_id: req!("issue_id", Uuid)?,
        repo_owner: req!("repo_owner", String)?,
        repo_name: req!("repo_name", String)?,
        pr_number: req!("pr_number", i32)?,
        url: req!("url", String)?,
        title: req!("title", String)?,
        state: req!("state", String)?,
        merged: req!("merged", bool)?,
        draft: req!("draft", bool)?,
        pr_updated_at: req!("pr_updated_at", Option<chrono::DateTime<chrono::Utc>>)?,
    })
}

fn provider_account_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<GitProviderAccount, GitStoreError> {
    use sqlx::Row as _;
    macro_rules! req {
        ($key:literal, $ty:ty) => {
            row.try_get::<$ty, _>($key)
                .map_err(|error| git_store_error("account map", error))
        };
    }
    Ok(GitProviderAccount {
        id: req!("id", Uuid)?,
        created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
        updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
        created_by_id: req!("created_by_id", Option<Uuid>)?,
        updated_by_id: req!("updated_by_id", Option<Uuid>)?,
        deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
        workspace_id: req!("workspace_id", Uuid)?,
        provider: req!("provider", String)?,
        host_url: req!("host_url", String)?,
        auth_type: req!("auth_type", String)?,
        external_account_id: req!("external_account_id", String)?,
        external_account_login: req!("external_account_login", String)?,
        display_name: req!("display_name", String)?,
        capabilities: req!("capabilities", Value)?,
        credential_config: req!("credential_config", Value)?,
        workspace_integration_id: req!("workspace_integration_id", Option<Uuid>)?,
        status: req!("status", String)?,
        verified_at: req!("verified_at", Option<chrono::DateTime<chrono::Utc>>)?,
        last_check_error: req!("last_check_error", String)?,
        metadata: req!("metadata", Value)?,
    })
}

fn repository_from_row(row: &sqlx::postgres::PgRow) -> Result<GitRepository, GitStoreError> {
    use sqlx::Row as _;
    macro_rules! req {
        ($key:literal, $ty:ty) => {
            row.try_get::<$ty, _>($key)
                .map_err(|error| git_store_error("repository map", error))
        };
    }
    Ok(GitRepository {
        id: req!("id", Uuid)?,
        created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
        updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
        created_by_id: req!("created_by_id", Option<Uuid>)?,
        updated_by_id: req!("updated_by_id", Option<Uuid>)?,
        deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
        provider: req!("provider", String)?,
        host_url: req!("host_url", String)?,
        external_id: req!("external_id", String)?,
        namespace: req!("namespace", String)?,
        name: req!("name", String)?,
        full_name: req!("full_name", String)?,
        web_url: req!("web_url", String)?,
        clone_url_http: req!("clone_url_http", String)?,
        clone_url_ssh: req!("clone_url_ssh", String)?,
        default_branch: req!("default_branch", String)?,
        is_private: req!("is_private", bool)?,
        metadata: req!("metadata", Value)?,
    })
}

impl CodeReviewStore for ReviewStore {
    async fn review_issue_exists(
        &self,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<bool, GitStoreError> {
        let row: Option<(i32,)> =
            sqlx::query_as(pidash_services::integrations::code_reviews::ISSUE_EXISTS_SQL)
                .bind(issue_id)
                .bind(project_id)
                .bind(workspace_slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("review issue exists", error))?;
        Ok(row.is_some())
    }

    async fn find_legacy_link(
        &self,
        repo_owner_lower: &str,
        repo_name_lower: &str,
        pr_number: i32,
    ) -> Result<Option<GithubPullRequestLink>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::LEGACY_LINK_FIND_SQL)
                .bind(repo_owner_lower)
                .bind(repo_name_lower)
                .bind(pr_number)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("legacy link find", error))?;
        row.map(|row| legacy_link_from_row(&row)).transpose()
    }

    async fn find_path_review_link(
        &self,
        provider: &str,
        host_url: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
        external_iid: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::PATH_REVIEW_LINK_FIND_SQL)
                .bind(provider)
                .bind(host_url)
                .bind(namespace_lower)
                .bind(repo_name_lower)
                .bind(external_iid)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("path review link find", error))?;
        row.map(|row| review_link_from_row(&row)).transpose()
    }

    async fn find_repo_scope_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        repo_external_id: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::REPO_SCOPE_LINK_FIND_SQL)
                .bind(provider)
                .bind(host_url)
                .bind(external_iid)
                .bind(repo_external_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("repo scope link find", error))?;
        row.map(|row| review_link_from_row(&row)).transpose()
    }

    async fn find_path_empty_repo_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::PATH_EMPTY_REPO_LINK_FIND_SQL)
                .bind(provider)
                .bind(host_url)
                .bind(external_iid)
                .bind(namespace_lower)
                .bind(repo_name_lower)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("path empty repo link find", error))?;
        row.map(|row| review_link_from_row(&row)).transpose()
    }

    async fn find_binding_review(
        &self,
        workspace_slug: &str,
        project_id: Uuid,
        provider: &str,
        host_url: &str,
        namespace: &str,
        repo_name: &str,
    ) -> Result<Option<(GitProviderAccount, GitRepository)>, GitStoreError> {
        // `SELECT pa.*, r.*`: duplicate names resolve to the account
        // half by name, so the account maps by name and the repository
        // by position (past the 20 account columns, in table order —
        // pinned against `information_schema`).
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::BINDING_FOR_REVIEW_SQL)
                .bind(workspace_slug)
                .bind(project_id)
                .bind(provider)
                .bind(host_url)
                .bind(namespace)
                .bind(repo_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("binding review find", error))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let account = provider_account_from_row(&row)?;
        let repo = {
            use sqlx::Row as _;
            macro_rules! at {
                ($pos:literal, $ty:ty) => {
                    row.try_get::<$ty, _>(20 + $pos)
                        .map_err(|error| git_store_error("binding repo map", error))
                };
            }
            // `git_repositories` table order: created_at, updated_at,
            // deleted_at, id, provider, host_url, external_id,
            // namespace, name, full_name, web_url, clone_url_http,
            // clone_url_ssh, default_branch, is_private, metadata,
            // created_by_id, updated_by_id.
            GitRepository {
                created_at: at!(0, chrono::DateTime<chrono::Utc>)?,
                updated_at: at!(1, chrono::DateTime<chrono::Utc>)?,
                deleted_at: at!(2, Option<chrono::DateTime<chrono::Utc>>)?,
                id: at!(3, Uuid)?,
                provider: at!(4, String)?,
                host_url: at!(5, String)?,
                external_id: at!(6, String)?,
                namespace: at!(7, String)?,
                name: at!(8, String)?,
                full_name: at!(9, String)?,
                web_url: at!(10, String)?,
                clone_url_http: at!(11, String)?,
                clone_url_ssh: at!(12, String)?,
                default_branch: at!(13, String)?,
                is_private: at!(14, bool)?,
                metadata: at!(15, Value)?,
                created_by_id: at!(16, Option<Uuid>)?,
                updated_by_id: at!(17, Option<Uuid>)?,
            }
        };
        Ok(Some((account, repo)))
    }

    async fn find_review_issue_workspace(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<Option<Uuid>, GitStoreError> {
        let row: Option<(Uuid,)> =
            sqlx::query_as(pidash_services::integrations::code_reviews::REVIEW_ISSUE_WORKSPACE_SQL)
                .bind(project_id)
                .bind(workspace_slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("review issue workspace", error))?;
        Ok(row.map(|row| row.0))
    }

    async fn find_review_repository(
        &self,
        provider: &str,
        host_url: &str,
        namespace: &str,
        repo_name: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::code_reviews::REVIEW_REPOSITORY_FIND_SQL)
                .bind(provider)
                .bind(host_url)
                .bind(namespace)
                .bind(repo_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("review repository find", error))?;
        row.map(|row| repository_from_row(&row)).transpose()
    }

    async fn find_review_project_workspace(
        &self,
        project_id: Uuid,
    ) -> Result<Option<Uuid>, GitStoreError> {
        let row: Option<(Uuid,)> = sqlx::query_as(
            pidash_services::integrations::code_reviews::REVIEW_PROJECT_WORKSPACE_SQL,
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("review project workspace", error))?;
        Ok(row.map(|row| row.0))
    }

    async fn insert_code_review_link(
        &self,
        row: NewCodeReviewLink,
    ) -> Result<GitCodeReviewLink, LinkWriteError> {
        let outcome =
            sqlx::query(pidash_services::integrations::code_reviews::CODE_REVIEW_LINK_INSERT_SQL)
                .bind(row.id)
                .bind(row.created_at)
                .bind(row.updated_at)
                .bind(Some(self.actor))
                .bind(None::<Uuid>)
                .bind(row.project_id)
                .bind(row.workspace_id)
                .bind(row.issue_id)
                .bind(&row.provider)
                .bind(&row.host_url)
                .bind(&row.namespace)
                .bind(&row.repo_name)
                .bind(&row.repo_external_id)
                .bind(&row.external_id)
                .bind(&row.external_iid)
                .bind(&row.url)
                .bind(&row.title)
                .bind(&row.state)
                .bind(row.merged)
                .bind(row.draft)
                .bind(row.remote_updated_at)
                .bind(&row.metadata)
                .execute(&self.pool)
                .await;
        match outcome {
            Ok(_) => Ok(GitCodeReviewLink {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                created_by_id: Some(self.actor),
                updated_by_id: None,
                deleted_at: None,
                project_id: row.project_id,
                workspace_id: row.workspace_id,
                issue_id: row.issue_id,
                provider: row.provider,
                host_url: row.host_url,
                namespace: row.namespace,
                repo_name: row.repo_name,
                repo_external_id: row.repo_external_id,
                external_id: row.external_id,
                external_iid: row.external_iid,
                url: row.url,
                title: row.title,
                state: row.state,
                merged: row.merged,
                draft: row.draft,
                remote_updated_at: row.remote_updated_at,
                metadata: row.metadata,
            }),
            Err(error) if is_unique_violation(&error) => Err(LinkWriteError::Conflict),
            Err(error) => Err(LinkWriteError::Store(GitStoreError::Db(format!(
                "code review link insert: {error}"
            )))),
        }
    }

    async fn insert_legacy_link(
        &self,
        row: NewLegacyLink,
    ) -> Result<GithubPullRequestLink, GitStoreError> {
        sqlx::query(pidash_services::integrations::code_reviews::LEGACY_LINK_INSERT_SQL)
            .bind(row.id)
            .bind(row.created_at)
            .bind(row.updated_at)
            .bind(Some(self.actor))
            .bind(None::<Uuid>)
            .bind(row.project_id)
            .bind(row.workspace_id)
            .bind(row.issue_id)
            .bind(&row.repo_owner)
            .bind(&row.repo_name)
            .bind(row.pr_number)
            .bind(&row.url)
            .bind(&row.title)
            .bind(&row.state)
            .bind(row.merged)
            .bind(row.draft)
            .bind(row.pr_updated_at)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("legacy link insert", error))?;
        Ok(GithubPullRequestLink {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            created_by_id: Some(self.actor),
            updated_by_id: None,
            deleted_at: None,
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            issue_id: row.issue_id,
            repo_owner: row.repo_owner,
            repo_name: row.repo_name,
            pr_number: row.pr_number,
            url: row.url,
            title: row.title,
            state: row.state,
            merged: row.merged,
            draft: row.draft,
            pr_updated_at: row.pr_updated_at,
        })
    }

    async fn update_legacy_link(
        &self,
        id: Uuid,
        update: LegacyLinkUpdate,
    ) -> Result<GithubPullRequestLink, GitStoreError> {
        sqlx::query(pidash_services::integrations::code_reviews::LEGACY_LINK_UPDATE_SQL)
            .bind(id)
            .bind(&update.url)
            .bind(&update.title)
            .bind(&update.state)
            .bind(update.merged)
            .bind(update.draft)
            .bind(update.pr_updated_at)
            .bind(update.updated_at)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("legacy link update", error))?;
        // `existing.save(update_fields=[...])` mutates in place; reread
        // the row for the caller.
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                    project_id, workspace_id, issue_id, repo_owner, repo_name, pr_number,
                    url, title, state, merged, draft, pr_updated_at
             FROM github_pull_request_links WHERE id = $1 LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("legacy link reread", error))?;
        row.map(|row| legacy_link_from_row(&row))
            .transpose()?
            .ok_or(GitStoreError::NotFound("legacy link"))
    }

    async fn delete_code_review_link(&self, id: Uuid) -> Result<(), GitStoreError> {
        // Instance `.delete()` (`mixins.py:71-78`): soft row, then the
        // fan-out — the port delegates the write mechanics here.
        let now = chrono::Utc::now();
        sqlx::query(
            "UPDATE git_code_review_links
             SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3",
        )
        .bind(now)
        .bind(self.actor)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|error| git_store_error("review link delete", error))?;
        enqueue_soft_delete(&self.pool, "gitcodereviewlink", &id).await;
        Ok(())
    }

    async fn delete_legacy_link(&self, id: Uuid) -> Result<(), GitStoreError> {
        let now = chrono::Utc::now();
        sqlx::query(
            "UPDATE github_pull_request_links
             SET deleted_at = $1, updated_at = $1, updated_by_id = $2 WHERE id = $3",
        )
        .bind(now)
        .bind(self.actor)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|error| git_store_error("legacy link delete", error))?;
        enqueue_soft_delete(&self.pool, "githubpullrequestlink", &id).await;
        Ok(())
    }
}

impl GitStore for ReviewStore {
    async fn list_provider_accounts(
        &self,
        workspace_id: Uuid,
        provider: &str,
        host_url: &str,
    ) -> Result<Vec<GitProviderAccount>, GitStoreError> {
        let rows: Vec<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::accounts::PROVIDER_ACCOUNT_LIST_SQL)
                .bind(workspace_id)
                .bind(provider)
                .bind(host_url)
                .fetch_all(&self.pool)
                .await
                .map_err(|error| git_store_error("account list", error))?;
        rows.iter().map(provider_account_from_row).collect()
    }

    async fn get_provider_account(
        &self,
        workspace_id: Uuid,
        provider: &str,
        host_url: &str,
        account_id: Uuid,
    ) -> Result<Option<GitProviderAccount>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::accounts::PROVIDER_ACCOUNT_GET_SQL)
                .bind(workspace_id)
                .bind(provider)
                .bind(host_url)
                .bind(account_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("account get", error))?;
        row.map(|row| provider_account_from_row(&row)).transpose()
    }

    async fn insert_provider_account(
        &self,
        row: NewProviderAccount,
    ) -> Result<GitProviderAccount, GitStoreError> {
        sqlx::query(pidash_services::integrations::accounts::PROVIDER_ACCOUNT_INSERT_SQL)
            .bind(row.id)
            .bind(row.created_at)
            .bind(row.updated_at)
            .bind(row.created_by_id)
            .bind(row.updated_by_id)
            .bind(row.workspace_id)
            .bind(&row.provider)
            .bind(&row.host_url)
            .bind(&row.auth_type)
            .bind(&row.external_account_id)
            .bind(&row.external_account_login)
            .bind(&row.display_name)
            .bind(&row.capabilities)
            .bind(&row.credential_config)
            .bind(&row.status)
            .bind(row.verified_at)
            .bind(&row.metadata)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("account insert", error))?;
        Ok(GitProviderAccount {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            created_by_id: row.created_by_id,
            updated_by_id: row.updated_by_id,
            deleted_at: None,
            workspace_id: row.workspace_id,
            provider: row.provider,
            host_url: row.host_url,
            auth_type: row.auth_type,
            external_account_id: row.external_account_id,
            external_account_login: row.external_account_login,
            display_name: row.display_name,
            capabilities: row.capabilities,
            credential_config: row.credential_config,
            workspace_integration_id: None,
            status: row.status,
            verified_at: Some(row.verified_at),
            last_check_error: String::new(),
            metadata: row.metadata,
        })
    }

    async fn find_repository_by_external(
        &self,
        provider: &str,
        host_url: &str,
        external_id: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            pidash_services::integrations::repositories::REPOSITORY_FIND_BY_EXTERNAL_SQL,
        )
        .bind(provider)
        .bind(host_url)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("repository find external", error))?;
        row.map(|row| repository_from_row(&row)).transpose()
    }

    async fn find_repository_by_full_name(
        &self,
        provider: &str,
        host_url: &str,
        full_name: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            pidash_services::integrations::repositories::REPOSITORY_FIND_BY_FULL_NAME_SQL,
        )
        .bind(provider)
        .bind(host_url)
        .bind(full_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("repository find full name", error))?;
        row.map(|row| repository_from_row(&row)).transpose()
    }

    async fn update_repository(
        &self,
        id: Uuid,
        defaults: RepositoryDefaults,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<GitRepository, GitStoreError> {
        sqlx::query(pidash_services::integrations::repositories::REPOSITORY_UPDATE_SQL)
            .bind(id)
            .bind(&defaults.external_id)
            .bind(&defaults.namespace)
            .bind(&defaults.name)
            .bind(&defaults.full_name)
            .bind(&defaults.web_url)
            .bind(&defaults.clone_url_http)
            .bind(&defaults.clone_url_ssh)
            .bind(&defaults.default_branch)
            .bind(defaults.is_private)
            .bind(&defaults.metadata)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("repository update", error))?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                    provider, host_url, external_id, namespace, name, full_name, web_url,
                    clone_url_http, clone_url_ssh, default_branch, is_private, metadata
             FROM git_repositories WHERE id = $1 LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("repository reread", error))?;
        row.map(|row| repository_from_row(&row))
            .transpose()?
            .ok_or(GitStoreError::NotFound("repository"))
    }

    async fn insert_repository(&self, row: NewRepository) -> Result<GitRepository, GitStoreError> {
        sqlx::query(pidash_services::integrations::repositories::REPOSITORY_INSERT_SQL)
            .bind(row.id)
            .bind(row.created_at)
            .bind(row.updated_at)
            .bind(&row.provider)
            .bind(&row.host_url)
            .bind(&row.defaults.external_id)
            .bind(&row.defaults.namespace)
            .bind(&row.defaults.name)
            .bind(&row.defaults.full_name)
            .bind(&row.defaults.web_url)
            .bind(&row.defaults.clone_url_http)
            .bind(&row.defaults.clone_url_ssh)
            .bind(&row.defaults.default_branch)
            .bind(row.defaults.is_private)
            .bind(&row.defaults.metadata)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("repository insert", error))?;
        Ok(GitRepository {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            provider: row.provider,
            host_url: row.host_url,
            external_id: row.defaults.external_id,
            namespace: row.defaults.namespace,
            name: row.defaults.name,
            full_name: row.defaults.full_name,
            web_url: row.defaults.web_url,
            clone_url_http: row.defaults.clone_url_http,
            clone_url_ssh: row.defaults.clone_url_ssh,
            default_branch: row.defaults.default_branch,
            is_private: row.defaults.is_private,
            metadata: row.defaults.metadata,
        })
    }

    async fn find_workspace_id(&self, slug: &str) -> Result<Option<Uuid>, GitStoreError> {
        let row: Option<(Uuid,)> =
            sqlx::query_as(pidash_services::integrations::repositories::WORKSPACE_ID_SQL)
                .bind(slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("workspace id", error))?;
        Ok(row.map(|row| row.0))
    }

    async fn find_project(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
    ) -> Result<Option<pidash_services::integrations::repositories::ProjectRef>, GitStoreError>
    {
        let row: Option<(Uuid, Uuid, String, String)> =
            sqlx::query_as(pidash_services::integrations::repositories::PROJECT_GET_SQL)
                .bind(project_id)
                .bind(workspace_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("project get", error))?;
        Ok(row.map(|(id, workspace_id, repo_url, base_branch)| {
            pidash_services::integrations::repositories::ProjectRef {
                id,
                workspace_id,
                repo_url,
                base_branch,
            }
        }))
    }

    async fn apply_bind(
        &self,
        plan: pidash_services::integrations::repositories::BindPlan,
    ) -> Result<
        pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding,
        GitStoreError,
    > {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| git_store_error("bind tx", error))?;
        sqlx::query(pidash_services::integrations::repositories::BINDING_DELETE_SQL)
            .bind(plan.project_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| git_store_error("bind delete", error))?;
        sqlx::query(pidash_services::integrations::repositories::GITHUB_SYNC_DELETE_SQL)
            .bind(plan.project_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| git_store_error("bind sync delete", error))?;
        let id = Uuid::new_v4();
        let mut metadata = Map::new();
        metadata.insert("raw_url".to_owned(), Value::String(plan.raw_url.clone()));
        sqlx::query(pidash_services::integrations::repositories::BINDING_INSERT_SQL)
            .bind(id)
            .bind(plan.now)
            .bind(plan.now)
            .bind(plan.actor_id)
            .bind(plan.actor_id)
            .bind(plan.project_id)
            .bind(plan.workspace_id)
            .bind(plan.repository_id)
            .bind(plan.provider_account_id)
            .bind(plan.actor_id)
            .bind(false)
            .bind(&plan.clone_auth_mode)
            .bind(Value::Object(metadata.clone()))
            .execute(&mut *tx)
            .await
            .map_err(|error| git_store_error("bind insert", error))?;
        // `project.save(update_fields=...)`: only the changed columns
        // plus `updated_at` (`services.py:341-348`).
        match (&plan.repo_url_update, &plan.base_branch_update) {
            (None, None) => {}
            (repo_url, base_branch) => {
                let mut sets: Vec<String> = Vec::new();
                if repo_url.is_some() {
                    sets.push("repo_url = $2".to_owned());
                }
                if base_branch.is_some() {
                    sets.push(format!("base_branch = ${}", sets.len() + 3));
                }
                sets.push(format!("updated_at = ${}", sets.len() + 3));
                let sql = format!("UPDATE projects SET {} WHERE id = $1", sets.join(", "));
                let mut query = sqlx::query(&sql).bind(plan.project_id);
                if let Some(url) = repo_url {
                    query = query.bind(url);
                }
                if let Some(branch) = base_branch {
                    query = query.bind(branch);
                }
                query = query.bind(plan.now);
                query
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| git_store_error("bind project update", error))?;
            }
        }
        tx.commit()
            .await
            .map_err(|error| git_store_error("bind commit", error))?;
        Ok(
            pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding {
                id,
                created_at: plan.now,
                updated_at: plan.now,
                created_by_id: plan.actor_id,
                updated_by_id: plan.actor_id,
                deleted_at: None,
                project_id: plan.project_id,
                workspace_id: plan.workspace_id,
                repository_id: plan.repository_id,
                provider_account_id: plan.provider_account_id,
                actor_id: plan.actor_id.ok_or(GitStoreError::NotFound("actor"))?,
                is_sync_enabled: false,
                clone_auth_mode: plan.clone_auth_mode,
                last_synced_at: None,
                last_sync_error: String::new(),
                metadata: Value::Object(metadata),
            },
        )
    }

    async fn get_binding(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<Option<pidash_services::integrations::repositories::BindingView>, GitStoreError>
    {
        let row: Option<sqlx::postgres::PgRow> =
            sqlx::query(pidash_services::integrations::repositories::BINDING_GET_SQL)
                .bind(project_id)
                .bind(workspace_slug)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| git_store_error("binding get", error))?;
        let Some(row) = row else {
            return Ok(None);
        };
        use sqlx::Row as _;
        macro_rules! req {
            ($key:literal, $ty:ty) => {
                row.try_get::<$ty, _>($key)
                    .map_err(|error| git_store_error("binding map", error))
            };
        }
        let binding =
            pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding {
                id: req!("b_id", Uuid)?,
                created_at: req!("b_created_at", chrono::DateTime<chrono::Utc>)?,
                updated_at: req!("b_updated_at", chrono::DateTime<chrono::Utc>)?,
                created_by_id: req!("b_created_by_id", Option<Uuid>)?,
                updated_by_id: req!("b_updated_by_id", Option<Uuid>)?,
                deleted_at: req!("b_deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
                project_id: req!("b_project_id", Uuid)?,
                workspace_id: req!("b_workspace_id", Uuid)?,
                repository_id: req!("b_repository_id", Uuid)?,
                provider_account_id: req!("b_provider_account_id", Uuid)?,
                actor_id: req!("b_actor_id", Uuid)?,
                is_sync_enabled: req!("b_is_sync_enabled", bool)?,
                clone_auth_mode: req!("b_clone_auth_mode", String)?,
                last_synced_at: req!("b_last_synced_at", Option<chrono::DateTime<chrono::Utc>>)?,
                last_sync_error: req!("b_last_sync_error", String)?,
                metadata: req!("b_metadata", Value)?,
            };
        let repository = GitRepository {
            id: req!("r_id", Uuid)?,
            created_at: req!("r_created_at", chrono::DateTime<chrono::Utc>)?,
            updated_at: req!("r_updated_at", chrono::DateTime<chrono::Utc>)?,
            created_by_id: req!("r_created_by_id", Option<Uuid>)?,
            updated_by_id: req!("r_updated_by_id", Option<Uuid>)?,
            deleted_at: req!("r_deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
            provider: req!("r_provider", String)?,
            host_url: req!("r_host_url", String)?,
            external_id: req!("r_external_id", String)?,
            namespace: req!("r_namespace", String)?,
            name: req!("r_name", String)?,
            full_name: req!("r_full_name", String)?,
            web_url: req!("r_web_url", String)?,
            clone_url_http: req!("r_clone_url_http", String)?,
            clone_url_ssh: req!("r_clone_url_ssh", String)?,
            default_branch: req!("r_default_branch", String)?,
            is_private: req!("r_is_private", bool)?,
            metadata: req!("r_metadata", Value)?,
        };
        let account = GitProviderAccount {
            id: req!("a_id", Uuid)?,
            created_at: req!("a_created_at", chrono::DateTime<chrono::Utc>)?,
            updated_at: req!("a_updated_at", chrono::DateTime<chrono::Utc>)?,
            created_by_id: req!("a_created_by_id", Option<Uuid>)?,
            updated_by_id: req!("a_updated_by_id", Option<Uuid>)?,
            deleted_at: req!("a_deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
            workspace_id: req!("a_workspace_id", Uuid)?,
            provider: req!("a_provider", String)?,
            host_url: req!("a_host_url", String)?,
            auth_type: req!("a_auth_type", String)?,
            external_account_id: req!("a_external_account_id", String)?,
            external_account_login: req!("a_external_account_login", String)?,
            display_name: req!("a_display_name", String)?,
            capabilities: req!("a_capabilities", Value)?,
            credential_config: req!("a_credential_config", Value)?,
            workspace_integration_id: req!("a_workspace_integration_id", Option<Uuid>)?,
            status: req!("a_status", String)?,
            verified_at: req!("a_verified_at", Option<chrono::DateTime<chrono::Utc>>)?,
            last_check_error: req!("a_last_check_error", String)?,
            metadata: req!("a_metadata", Value)?,
        };
        Ok(Some(
            pidash_services::integrations::repositories::BindingView {
                binding,
                repository,
                account,
            },
        ))
    }

    async fn set_binding_sync(
        &self,
        binding_id: Uuid,
        enabled: bool,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<
        pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding,
        GitStoreError,
    > {
        sqlx::query(pidash_services::integrations::repositories::BINDING_SET_SYNC_SQL)
            .bind(binding_id)
            .bind(enabled)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("binding set sync", error))?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(
            "SELECT id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
                    project_id, workspace_id, repository_id, provider_account_id, actor_id,
                    is_sync_enabled, clone_auth_mode, last_synced_at, last_sync_error, metadata
             FROM git_repository_bindings WHERE id = $1 LIMIT 1",
        )
        .bind(binding_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| git_store_error("binding reread", error))?;
        let Some(row) = row else {
            return Err(GitStoreError::NotFound("binding"));
        };
        use sqlx::Row as _;
        macro_rules! req {
            ($key:literal, $ty:ty) => {
                row.try_get::<$ty, _>($key)
                    .map_err(|error| git_store_error("binding map", error))
            };
        }
        Ok(
            pidash_db::integrations::git_models::git_repository_binding::GitRepositoryBinding {
                id: req!("id", Uuid)?,
                created_at: req!("created_at", chrono::DateTime<chrono::Utc>)?,
                updated_at: req!("updated_at", chrono::DateTime<chrono::Utc>)?,
                created_by_id: req!("created_by_id", Option<Uuid>)?,
                updated_by_id: req!("updated_by_id", Option<Uuid>)?,
                deleted_at: req!("deleted_at", Option<chrono::DateTime<chrono::Utc>>)?,
                project_id: req!("project_id", Uuid)?,
                workspace_id: req!("workspace_id", Uuid)?,
                repository_id: req!("repository_id", Uuid)?,
                provider_account_id: req!("provider_account_id", Uuid)?,
                actor_id: req!("actor_id", Uuid)?,
                is_sync_enabled: req!("is_sync_enabled", bool)?,
                clone_auth_mode: req!("clone_auth_mode", String)?,
                last_synced_at: req!("last_synced_at", Option<chrono::DateTime<chrono::Utc>>)?,
                last_sync_error: req!("last_sync_error", String)?,
                metadata: req!("metadata", Value)?,
            },
        )
    }

    async fn set_github_syncs_enabled(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
        enabled: bool,
    ) -> Result<u64, GitStoreError> {
        let done =
            sqlx::query(pidash_services::integrations::repositories::GITHUB_SYNC_SET_ENABLED_SQL)
                .bind(enabled)
                .bind(project_id)
                .bind(workspace_slug)
                .execute(&self.pool)
                .await
                .map_err(|error| git_store_error("github syncs set enabled", error))?;
        Ok(done.rows_affected())
    }

    async fn delete_binding(&self, binding_id: Uuid) -> Result<(), GitStoreError> {
        sqlx::query(pidash_services::integrations::repositories::BINDING_DELETE_ONE_SQL)
            .bind(binding_id)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("binding delete", error))?;
        Ok(())
    }

    async fn delete_github_syncs_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<u64, GitStoreError> {
        let done = sqlx::query(pidash_services::integrations::repositories::GITHUB_SYNC_DELETE_SQL)
            .bind(project_id)
            .execute(&self.pool)
            .await
            .map_err(|error| git_store_error("github syncs delete", error))?;
        Ok(done.rows_affected())
    }
}

// ---------------------------------------------------------------------------
// Transports: production `GithubClient` + `GitLabTransport`
// ---------------------------------------------------------------------------

use pidash_db::app_integrations::queries_github::{build_app_jwt_claims, normalize_private_key};
use pidash_db::config::encryption::Keyring;
use pidash_services::integrations::adapters_github::{
    ClientAuth, GitHubAdapter, GithubClient, GithubError,
};
use pidash_services::integrations::adapters_gitlab::{
    GitLabAdapter, GitLabConfig, GitLabRequest, GitLabResponse, GitLabTransport,
};
use pidash_types::integrations::registry::UnknownProvider;
use pidash_types::integrations::GitProviderError;

/// Run an async future to completion from the sync transport seams: a
/// fresh current-thread runtime on a spawned thread. The seams
/// (`GithubClient`, `GitLabTransport`) are sync because Python's
/// `requests` is blocking; this keeps the async executor unblocked
/// without new Cargo features.
fn run_sync<F>(future: F) -> F::Output
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("relations transport runtime")
            .block_on(future)
    })
    .join()
    .expect("relations transport thread")
}

/// What the transports need beyond the request: pool (app-config
/// reads), cache (installation-token cache), instance secret (config
/// decrypt + PAT keyring source).
#[derive(Clone)]
struct TransportContext {
    pool: sqlx::PgPool,
    redis: Option<pidash_db::redis::RedisHandle>,
    secret: String,
}

/// Read app-config values through the instance-configurations
/// registry (`get_configuration_value`), like the GitHub App handler.
async fn app_config_values(
    ctx: &TransportContext,
    keys: &[&str],
) -> Option<HashMap<String, pidash_db::config::ConfigValue>> {
    let store = pidash_db::config::PgConfigStore::new(ctx.pool.clone());
    let registry = pidash_db::config::registry::global();
    let keyring = pidash_services::license::encryption::Keyring::from_secret(&ctx.secret);
    pidash_db::config::accessor::get_many_in(registry, &store, &keyring, keys)
        .await
        .ok()
}

/// `require_github_app_config()` (`github_app_auth.py:63-74`): app id +
/// slug + private key, each stripped, missing → the config error.
async fn github_app_config(
    ctx: &TransportContext,
) -> Result<(String, String, String), GithubError> {
    let values = app_config_values(
        ctx,
        &["GITHUB_APP_ID", "GITHUB_APP_SLUG", "GITHUB_APP_PRIVATE_KEY"],
    )
    .await
    .unwrap_or_default();
    let string = |key: &str| match values.get(key) {
        Some(pidash_db::config::ConfigValue::Str(value)) => value.clone(),
        Some(pidash_db::config::ConfigValue::Int(value)) => value.to_string(),
        Some(pidash_db::config::ConfigValue::Float(value)) => value.to_string(),
        Some(pidash_db::config::ConfigValue::Bool(true)) => "True".to_owned(),
        Some(pidash_db::config::ConfigValue::Bool(false)) => "False".to_owned(),
        _ => String::new(),
    };
    let app_id = string("GITHUB_APP_ID");
    let app_slug = string("GITHUB_APP_SLUG");
    let private_key = string("GITHUB_APP_PRIVATE_KEY");
    let config = (
        app_id.trim().to_owned(),
        app_slug.trim().to_owned(),
        normalize_private_key(Some(&private_key)),
    );
    let mut missing: Vec<&str> = Vec::new();
    if config.0.is_empty() {
        missing.push("app_id");
    }
    if config.1.is_empty() {
        missing.push("app_slug");
    }
    if config.2.is_empty() {
        missing.push("private_key");
    }
    if !missing.is_empty() {
        return Err(GithubError::Transport(format!(
            "GitHub App config missing: {}",
            missing.join(", ")
        )));
    }
    Ok(config)
}

/// `build_app_jwt` (`github_app_auth.py:77-89`): RS256 over
/// `iat=now-60, exp=now+540, iss=app_id`.
fn build_app_jwt_token(app_id: &str, private_key: &str) -> Result<String, GithubError> {
    let now = chrono::Utc::now().timestamp();
    let claims = build_app_jwt_claims(now, app_id);
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key.as_bytes())
        .map_err(|error| GithubError::Transport(format!("GitHub App JWT: {error}")))?;
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.typ = Some("JWT".to_owned());
    #[derive(serde::Serialize)]
    struct Claims {
        iat: i64,
        exp: i64,
        iss: String,
    }
    jsonwebtoken::encode(
        &header,
        &Claims {
            iat: claims.iat,
            exp: claims.exp,
            iss: claims.iss,
        },
        &key,
    )
    .map_err(|error| GithubError::Transport(format!("GitHub App JWT: {error}")))
}

/// `installation_token` (`github_app_auth.py:174-193`): cached token or
/// a fresh mint. Without a Redis handle the mint runs through (the cache
/// only saves the POST; a mint failure still fails the connect either
/// way).
async fn installation_token(
    ctx: &TransportContext,
    installation_id: i64,
) -> Result<String, GithubError> {
    let cache_key = format!("github_app_installation_token:{installation_id}");
    if let Some(redis) = ctx.redis.as_ref() {
        match redis.get_string(&cache_key).await {
            Ok(Some(token)) if !token.is_empty() => return Ok(token),
            Ok(_) => {}
            Err(error) => {
                return Err(GithubError::Transport(format!(
                    "installation token cache: {error}"
                )))
            }
        }
    }
    let (app_id, _slug, private_key) = github_app_config(ctx).await?;
    let jwt = build_app_jwt_token(&app_id, &private_key)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| GithubError::Transport(error.to_string()))?;
    let response = client
        .post(format!(
            "https://api.github.com/app/installations/{installation_id}/access_tokens"
        ))
        .header("Authorization", format!("Bearer {jwt}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "pi-dash-github-app")
        .send()
        .await
        .map_err(|error| GithubError::Transport(error.to_string()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(GithubError::Transport(format!(
            "installation token mint: HTTP {}: {text}",
            status.as_u16()
        )));
    }
    let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let token = payload
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if token.is_empty() {
        return Err(GithubError::Transport(
            "GitHub did not return an installation token".to_owned(),
        ));
    }
    if let Some(redis) = ctx.redis.as_ref() {
        // `ttl = max(60, expires-now-60)`, default 55 minutes
        // (`github_app_auth.py:184-188`).
        let mut ttl: u64 = 55 * 60;
        if let Some(expires) = payload.get("expires_at").and_then(Value::as_str) {
            let normalized = expires.replace('Z', "+00:00");
            if let Ok(when) = chrono::DateTime::parse_from_rfc3339(&normalized) {
                let seconds =
                    (when.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds() - 60;
                ttl = u64::try_from(seconds.max(60)).unwrap_or(60);
            }
        }
        if let Err(error) = redis.set_ex(&cache_key, &token, ttl).await {
            return Err(GithubError::Transport(format!(
                "installation token cache: {error}"
            )));
        }
    }
    Ok(token)
}

/// The production `GithubClient` (`github_client.py:38-220`): a bearer
/// token plus the shared context for installation mints.
struct LiveGithubClient {
    token: String,
    ctx: TransportContext,
}

impl LiveGithubClient {
    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Authorization", format!("Bearer {}", self.token)),
            ("Accept", "application/vnd.github+json".to_owned()),
            ("X-GitHub-Api-Version", "2022-11-28".to_owned()),
            ("User-Agent", "pi-dash-github-sync".to_owned()),
        ]
    }

    async fn request_json(
        &self,
        method: reqwest::Method,
        url: &str,
        json: Option<&Value>,
    ) -> Result<(Value, Option<String>), GithubError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|error| GithubError::Transport(error.to_string()))?;
        let mut request = client.request(method, url);
        for (name, value) in self.headers() {
            request = request.header(name, value);
        }
        if let Some(body) = json {
            let text = serde_json::to_string(body).unwrap_or_default();
            request = request
                .header("Content-Type", "application/json")
                .body(text);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GithubError::Transport(error.to_string()))?;
        let status = response.status().as_u16();
        let link = response
            .headers()
            .get("link")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let text = response.text().await.unwrap_or_default();
        // `_request` (`github_client.py:59-71`): 401/403/404 map by
        // status with the response text; anything else failing raises.
        if status == 401 {
            return Err(GithubError::Auth(text));
        }
        if status == 403 {
            return Err(GithubError::Permission(text));
        }
        if status == 404 {
            return Err(GithubError::NotFound(text));
        }
        if !(200..300).contains(&status) {
            return Err(GithubError::Transport(format!("HTTP {status}: {text}")));
        }
        let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((payload, link))
    }

    /// `_next_url`: the `rel="next"` link, if any.
    fn next_url(link: Option<&str>) -> Option<String> {
        let link = link?;
        for part in link.split(',') {
            let part = part.trim_start();
            if !part.starts_with('<') {
                continue;
            }
            let Some(end) = part.find('>') else {
                continue;
            };
            let url = &part[1..end];
            let rest = part[end + 1..].trim_start();
            if let Some(rest) = rest.strip_prefix(';') {
                let rest = rest.trim_start();
                if rest.starts_with("rel=\"next\"") {
                    return Some(url.to_owned());
                }
            }
        }
        None
    }

    async fn paginate(&self, mut url: String) -> Result<Vec<Value>, GithubError> {
        let mut items = Vec::new();
        loop {
            let (payload, link) = self.request_json(reqwest::Method::GET, &url, None).await?;
            match payload {
                Value::Array(page) => items.extend(page),
                Value::Null => {}
                // A mapping payload iterates its keys, like Python's
                // `for item in response.json()` (the API never sends
                // one here, but the iteration is the ported behavior).
                Value::Object(map) => {
                    items.extend(map.keys().map(|key| Value::String(key.clone())));
                }
                _ => {}
            }
            match Self::next_url(link.as_deref()) {
                Some(next) => url = next,
                None => return Ok(items),
            }
        }
    }
}

impl GithubClient for LiveGithubClient {
    fn connect(auth: &ClientAuth) -> Result<Self, GithubError> {
        // The context rides a thread-local: the sync seam has no
        // parameter for it, and `attach` always runs on the request
        // task that installed it just before calling into the port.
        let ctx = TRANSPORT_CONTEXT.with(|slot| slot.borrow().clone());
        let ctx =
            ctx.ok_or_else(|| GithubError::Transport("github transport context".to_owned()))?;
        match auth {
            ClientAuth::Token(token) => {
                if token.is_empty() {
                    return Err(GithubError::Auth("empty token".to_owned()));
                }
                Ok(Self {
                    token: token.clone(),
                    ctx,
                })
            }
            ClientAuth::Installation(id) => {
                let id = *id;
                let minted = ctx.clone();
                let token = run_sync(async move { installation_token(&minted, id).await })?;
                Ok(Self { token, ctx })
            }
        }
    }

    fn get_authenticated_user(&self) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        run_sync(async move {
            this.request_json(reqwest::Method::GET, "https://api.github.com/user", None)
                .await
                .map(|(payload, _)| payload)
        })
    }

    fn list_user_repos(&self, page: i64) -> Result<(Vec<Value>, bool), GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        run_sync(async move {
            let url = format!(
                "https://api.github.com/user/repos?affiliation=owner%2Ccollaborator%2Corganization_member&per_page=100&sort=updated&page={page}"
            );
            let (payload, link) = this.request_json(reqwest::Method::GET, &url, None).await?;
            let repos = match payload {
                Value::Array(repos) => repos,
                _ => Vec::new(),
            };
            Ok((repos, Self::next_url(link.as_deref()).is_some()))
        })
    }

    fn get_repo(&self, owner: &str, name: &str) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        let url = format!("https://api.github.com/repos/{owner}/{name}");
        run_sync(async move {
            this.request_json(reqwest::Method::GET, &url, None)
                .await
                .map(|(payload, _)| payload)
        })
    }

    fn list_all_open_issues(&self, owner: &str, name: &str) -> Result<Vec<Value>, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        // `{"state": "open", "per_page": 100, "sort": "updated",
        // "direction": "desc"}` in insertion order (`:136-140`).
        let url = format!(
            "https://api.github.com/repos/{owner}/{name}/issues?state=open&per_page=100&sort=updated&direction=desc"
        );
        run_sync(async move { this.paginate(url).await })
    }

    fn list_issue_comments(
        &self,
        owner: &str,
        name: &str,
        number: i64,
    ) -> Result<Vec<Value>, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        // `{"per_page": 100, "sort": "created", "direction": "asc"}`
        // (`:149-153`).
        let url = format!(
            "https://api.github.com/repos/{owner}/{name}/issues/{number}/comments?per_page=100&sort=created&direction=asc"
        );
        run_sync(async move { this.paginate(url).await })
    }

    fn post_issue_comment(
        &self,
        owner: &str,
        name: &str,
        number: i64,
        body: &str,
    ) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        let url = format!("https://api.github.com/repos/{owner}/{name}/issues/{number}/comments");
        let payload = serde_json::json!({"body": body});
        run_sync(async move {
            this.request_json(reqwest::Method::POST, &url, Some(&payload))
                .await
                .map(|(payload, _)| payload)
        })
    }

    fn get_pull_request(&self, owner: &str, name: &str, number: i64) -> Result<Value, GithubError> {
        let this = Self {
            token: self.token.clone(),
            ctx: self.ctx.clone(),
        };
        let url = format!("https://api.github.com/repos/{owner}/{name}/pulls/{number}");
        run_sync(async move {
            this.request_json(reqwest::Method::GET, &url, None)
                .await
                .map(|(payload, _)| payload)
        })
    }
}

std::thread_local! {
    /// The ambient transport context for the sync `GithubClient` seam.
    static TRANSPORT_CONTEXT: std::cell::RefCell<Option<TransportContext>> =
        const { std::cell::RefCell::new(None) };
}

/// Install the ambient transport context around one sync-seam call.
/// The guard restores the previous value on drop.
struct TransportGuard {
    previous: Option<TransportContext>,
}

impl TransportGuard {
    fn install(ctx: TransportContext) -> Self {
        let previous = TRANSPORT_CONTEXT.with(|slot| slot.borrow_mut().replace(ctx));
        Self { previous }
    }
}

impl Drop for TransportGuard {
    fn drop(&mut self) {
        TRANSPORT_CONTEXT.with(|slot| *slot.borrow_mut() = self.previous.take());
    }
}

/// The production `GitLabTransport`: `requests.request` over async
/// reqwest, redirects surfaced (never followed).
struct LiveGitlabTransport;

impl GitLabTransport for LiveGitlabTransport {
    fn request(&self, request: &GitLabRequest) -> Result<GitLabResponse, GitProviderError> {
        let request = GitLabRequest {
            method: request.method.clone(),
            url: request.url.clone(),
            headers: request.headers.clone(),
            query: request.query.clone(),
            form: request.form.clone(),
            timeout_secs: request.timeout_secs,
        };
        run_sync(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(request.timeout_secs))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| GitProviderError::General(error.to_string()))?;
            let method: reqwest::Method =
                request
                    .method
                    .parse()
                    .map_err(|error: http::method::InvalidMethod| {
                        GitProviderError::General(error.to_string())
                    })?;
            let mut call = client.request(method, &request.url);
            for (name, value) in &request.headers {
                call = call.header(name.as_str(), value.as_str());
            }
            if !request.query.is_empty() {
                call = call.query(&request.query);
            }
            if !request.form.is_empty() {
                // The pair vector serializes in order (repeats kept),
                // like `requests`' `data=` list of tuples.
                call = call.form(&request.form);
            }
            let response = call
                .send()
                .await
                .map_err(|error| GitProviderError::General(error.to_string()))?;
            let status = response.status().as_u16();
            let mut headers = Vec::new();
            for (name, value) in response.headers() {
                headers.push((name.to_string(), value.to_str().unwrap_or("").to_owned()));
            }
            let body = response.text().await.unwrap_or_default();
            Ok(GitLabResponse {
                status,
                headers,
                body,
            })
        })
    }
}

/// `decrypt_data` (`license/utils/encryption.py:34-44`) as the
/// context-free decryptor the GitLab seam takes: Fernet over
/// `SECRET_KEY`, failures (and empties) → `None` so the adapter falls
/// back to the raw token (`gitlab.py:191-198`).
fn gitlab_decrypt(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let decrypted = Keyring::from_env().decrypt(value);
    if decrypted.is_empty() {
        None
    } else {
        Some(decrypted)
    }
}

/// The live [`AdapterSource`]: GitHub (keyring over the instance
/// secret) + GitLab (production transport, Django-effective config),
/// in registry order. The GitLab transport is owned by the caller and
/// borrowed here (the adapter borrows its transport).
struct RelationsAdapters<'t> {
    github: GitHubAdapter<LiveGithubClient>,
    gitlab: GitLabAdapter<'t, LiveGitlabTransport>,
}

impl<'t> RelationsAdapters<'t> {
    fn new(
        transport: &'t LiveGitlabTransport,
        keyring: Keyring,
        allowed_hosts: Vec<String>,
    ) -> Self {
        // `getattr(settings, "GITLAB_HOST", "")`: Django never defines
        // the setting, so the effective host is always `""`.
        let config = GitLabConfig {
            gitlab_host: String::new(),
            allowed_hosts,
        };
        Self {
            github: GitHubAdapter::new(keyring),
            gitlab: GitLabAdapter::with_decrypt(config, transport, gitlab_decrypt),
        }
    }
}

impl AdapterSource for RelationsAdapters<'_> {
    fn adapter_for(&self, provider: &str) -> Result<&dyn ReviewAdapter, UnknownProvider> {
        let key = pidash_types::integrations::registry::resolve_adapter_key(provider)?;
        // `PROVIDERS` is exactly github + gitlab, so the key is one of
        // the two (Python's `KeyError` arm is the `Err` above).
        match key {
            "github" => Ok(&self.github as &dyn ReviewAdapter),
            "gitlab" => Ok(&self.gitlab as &dyn ReviewAdapter),
            _ => unreachable!("adapter keys are github|gitlab"),
        }
    }
}

// ---------------------------------------------------------------------------
// PR links + code reviews: create / destroy
// ---------------------------------------------------------------------------

/// `POST github-pull-requests/` (`github_pr.py:50-64`): `request.data`
/// must be an object (else the `.get` 500); the `url` scalar must be a
/// string (else `.strip` 500s); missing/null → `""` → the invalid-URL
/// 400. Attach runs pinned to this thread (`block_in_place`) so the
/// sync `GithubClient` seam sees the installed transport context.
async fn pr_create(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_entity_gate(&pool, &slug, &project_id, &user_id, "POST").await {
        return denial.into_response();
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    // `(raw_url or "").strip()` (`github_pr.py:54`): missing/null →
    // `""` (the invalid-URL 400 below); non-strings 500 on `.strip`.
    let raw_url = match body.map.get("url").unwrap_or(&Value::Null) {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        _ => return Denial::ServerError.into_response(),
    };
    let ctx = TransportContext {
        pool: pool.clone(),
        redis: state.redis().cloned(),
        secret: state.settings().secret_key.clone(),
    };
    let _guard = TransportGuard::install(ctx);
    let store = PrStore::new(pool.clone());
    let now = chrono::Utc::now();
    let outcome = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            pidash_services::app_issues::pr_links::attach_pull_request::<PrStore, LiveGithubClient>(
                &store,
                project_id,
                issue_id,
                &slug,
                &raw_url,
                &now,
                Some(user_id),
            ),
        )
    });
    drop(_guard);
    let (link, created) = match outcome {
        Ok(attached) => (attached.link, attached.created),
        Err(PrAttachError::InvalidUrl(_)) => {
            return Denial::BadError("A valid github.com pull request URL is required.".to_owned())
                .into_response()
        }
        Err(PrAttachError::IssueNotFound(_)) => return Denial::WorkItemNotFound.into_response(),
        Err(PrAttachError::AlreadyLinked(conflict)) => {
            return Denial::Conflict(format!(
                "This pull request is already linked to issue {}.",
                conflict.issue_id
            ))
            .into_response()
        }
        Err(PrAttachError::Store(PrStoreError(message))) => {
            if message == PROJECT_WORKSPACE_MISS {
                return Denial::NotFound.into_response();
            }
            if message.starts_with(PR_PAYLOAD_INVALID) || message.contains(PR_RACE_MISS) {
                return Denial::BadError("The payload is not valid".to_owned()).into_response();
            }
            return Denial::ServerError.into_response();
        }
    };
    let tz = match actor_timezone(&pool, &user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let owned = match fetch_pr_owned(&pool, &link.id).await {
        Ok(owned) => owned,
        Err(denial) => return denial.into_response(),
    };
    let creator_ids: Vec<uuid::Uuid> = owned.created_by_id.into_iter().collect();
    let lites = match fetch_user_lites(&pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    let shape = render_pr_row(&owned, &lites, &tz);
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    json_response(status, shape.to_string())
}

/// Re-read one PR link for the create response (the serializer reads
/// the saved instance, whose audit columns the store stamped).
async fn fetch_pr_owned(pool: &sqlx::PgPool, id: &uuid::Uuid) -> Result<PrLinkOwned, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT id, issue_id, repo_owner, repo_name, pr_number, url, title, state,
                  merged, draft, pr_updated_at, created_at, updated_at, created_by_id
           FROM github_pull_request_links WHERE id = $1 LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    use sqlx::Row as _;
    // Raw RFC3339 instants; `render_pr_row` applies the actor zone once.
    let opt_dt = |key: &str| -> Result<Option<String>, Denial> {
        row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(key)
            .map(|stamp| {
                stamp.map(|stamp| stamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
            })
            .map_err(|_| Denial::ServerError)
    };
    let req_dt = |key: &str| -> Result<String, Denial> {
        row.try_get::<chrono::DateTime<chrono::Utc>, _>(key)
            .map(|stamp| stamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
            .map_err(|_| Denial::ServerError)
    };
    Ok(PrLinkOwned {
        id: row
            .try_get::<Uuid, _>("id")
            .map_err(|_| Denial::ServerError)?
            .to_string(),
        issue_id: row
            .try_get::<Uuid, _>("issue_id")
            .map_err(|_| Denial::ServerError)?
            .to_string(),
        repo_owner: row.try_get("repo_owner").map_err(|_| Denial::ServerError)?,
        repo_name: row.try_get("repo_name").map_err(|_| Denial::ServerError)?,
        pr_number: row.try_get("pr_number").map_err(|_| Denial::ServerError)?,
        url: row.try_get("url").map_err(|_| Denial::ServerError)?,
        title: row.try_get("title").map_err(|_| Denial::ServerError)?,
        state: row.try_get("state").map_err(|_| Denial::ServerError)?,
        merged: row.try_get("merged").map_err(|_| Denial::ServerError)?,
        draft: row.try_get("draft").map_err(|_| Denial::ServerError)?,
        pr_updated_at: opt_dt("pr_updated_at")?,
        created_at: req_dt("created_at")?,
        updated_at: req_dt("updated_at")?,
        created_by_id: row
            .try_get("created_by_id")
            .map_err(|_| Denial::ServerError)?,
    })
}

/// `DELETE github-pull-requests/{pk}/` (`github_pr.py:66-73`):
/// scoped `.get(pk)` (404 `{"error": ...}`), detach through the port
/// (mirror culled only when it hangs off the same issue), 204. No
/// activity row on this path. The port's `DetachOutcome.enqueues` fan
/// out in Python order after the soft deletes.
async fn pr_destroy(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    if path_uuid(
        params
            .get("issue_id")
            .map(String::as_str)
            .unwrap_or_default(),
    )
    .is_err()
        || path_uuid(params.get("pk").map(String::as_str).unwrap_or_default()).is_err()
    {
        return crate::edge::proxy(State(state), req).await;
    }
    let ctx = match detail_context(&state, &params, extension, "DELETE").await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_scoped_pr(
        &ctx.pool,
        &ctx.slug,
        &ctx.project_id,
        &ctx.issue_id,
        &ctx.pk,
    )
    .await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(row) = row else {
        return Denial::NotFound.into_response();
    };
    let store = PrStore::new(ctx.pool.clone());
    let link = PrLinkRow {
        id: ctx.pk,
        project_id: ctx.project_id,
        issue_id: ctx.issue_id,
        repo_owner: row.repo_owner.clone(),
        repo_name: row.repo_name.clone(),
        pr_number: i64::from(row.pr_number),
        url: row.url.clone(),
        title: row.title.clone(),
        state: row.state.clone(),
        merged: row.merged,
        draft: row.draft,
        pr_updated_at: row.pr_updated_at.clone(),
    };
    let now = chrono::Utc::now();
    let outcome = match pidash_services::app_issues::pr_links::detach_pull_request_link(
        &store,
        &link,
        &now,
        Some(ctx.user_id),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // The recorded fan-out, verbatim and in Python order.
    for task in outcome.enqueues {
        let message =
            pidash_jobs::celery::CeleryTaskMessage::new(task.task, task.args, task.kwargs);
        enqueue_message(&ctx.pool, message).await;
    }
    empty_response(StatusCode::NO_CONTENT)
}

/// Scoped PR-link detail: the list scope plus the pk.
async fn fetch_scoped_pr(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<PrLinkOwned>, Denial> {
    let mut binder = super::Binder::new();
    let list = pr_link_list_sql(&mut binder, slug, project_id, issue_id);
    // Wraps: the list SQL ends in `ORDER BY`.
    let marker = binder.bind_uuid(*pk);
    let sql = format!("SELECT * FROM ({list}) AS scoped WHERE scoped.id = {marker} LIMIT 1");
    let rows = super::fetch_json_rows(pool, &sql, binder.values())
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.first().map(pr_owned_from_json).transpose()
}

/// Scoped review-link detail: the list scope plus the pk.
async fn fetch_scoped_review(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<ReviewLinkOwned>, Denial> {
    let mut binder = super::Binder::new();
    let list = review_link_list_sql(&mut binder, slug, project_id, issue_id);
    // Wraps: the list SQL ends in `ORDER BY`.
    let marker = binder.bind_uuid(*pk);
    let sql = format!("SELECT * FROM ({list}) AS scoped WHERE scoped.id = {marker} LIMIT 1");
    let rows = super::fetch_json_rows(pool, &sql, binder.values())
        .await
        .map_err(|_| Denial::ServerError)?;
    rows.first().map(review_owned_from_json).transpose()
}

/// `POST code-reviews/` (`git_code_review.py:46-65`): same body rules
/// as the PR create; attach through the D-05 port with the live
/// adapters, pinned to this thread for the sync seams.
async fn review_create(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    let slug = params.get("slug").cloned().unwrap_or_default();
    let project_raw = params.get("project_id").cloned().unwrap_or_default();
    let issue_raw = params.get("issue_id").cloned().unwrap_or_default();
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let user_id = match actor_user_id(&pool, extension).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let project_id = match resolve_project_id(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = check_entity_gate(&pool, &slug, &project_id, &user_id, "POST").await {
        return denial.into_response();
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    let raw_url = match body.map.get("url").unwrap_or(&Value::Null) {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        _ => return Denial::ServerError.into_response(),
    };
    let ctx = TransportContext {
        pool: pool.clone(),
        redis: state.redis().cloned(),
        secret: state.settings().secret_key.clone(),
    };
    let keyring = Keyring::from_secret(&state.settings().secret_key);
    let allowed_hosts = state.settings().gitlab_allowed_hosts.clone();
    let _guard = TransportGuard::install(ctx);
    let store = ReviewStore::new(pool.clone(), user_id);
    let transport = LiveGitlabTransport;
    let outcome = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let adapters = RelationsAdapters::new(&transport, keyring, allowed_hosts);
            let parsers: Vec<&dyn ReviewParser> = vec![&adapters.github, &adapters.gitlab];
            pidash_services::integrations::code_reviews::attach_code_review(
                &store,
                &parsers,
                &adapters,
                &AttachRequest {
                    project_id,
                    issue_id,
                    workspace_slug: slug.clone(),
                    raw_url: raw_url.clone(),
                },
                chrono::Utc::now(),
            )
            .await
        })
    });
    drop(_guard);
    let (link, created) = match outcome {
        Ok(attached) => attached,
        Err(CodeReviewError::InvalidUrl) => {
            return Denial::BadError(
                "A supported GitHub pull request or GitLab merge request URL is required."
                    .to_owned(),
            )
            .into_response()
        }
        Err(CodeReviewError::IssueNotFound) => return Denial::WorkItemNotFound.into_response(),
        Err(CodeReviewError::AlreadyLinked { issue_id }) => {
            return Denial::Conflict(
                pidash_services::integrations::code_reviews::already_linked_message(&issue_id),
            )
            .into_response()
        }
        Err(CodeReviewError::ProviderNotFound(_)) => {
            return Denial::NotFoundError("Code review not found.".to_owned()).into_response()
        }
        Err(CodeReviewError::BadPrNumber(_)) => return Denial::ServerError.into_response(),
        Err(CodeReviewError::UnknownProvider(_)) => return Denial::ServerError.into_response(),
        Err(CodeReviewError::Store(GitStoreError::NotFound(_))) => {
            return Denial::NotFound.into_response()
        }
        Err(CodeReviewError::Store(_)) => return Denial::ServerError.into_response(),
    };
    let tz = match actor_timezone(&pool, &user_id).await {
        Ok(tz) => tz,
        Err(denial) => return denial.into_response(),
    };
    let owned = match fetch_review_owned(&pool, &link.id).await {
        Ok(owned) => owned,
        Err(denial) => return denial.into_response(),
    };
    let creator_ids: Vec<uuid::Uuid> = owned.created_by_id.into_iter().collect();
    let lites = match fetch_user_lites(&pool, &creator_ids).await {
        Ok(lites) => lites,
        Err(denial) => return denial.into_response(),
    };
    let shape = render_review_row(&owned, &lites, &tz);
    // Attach answers 201 on create, 200 on re-attach
    // (`git_code_review.py:63-65`, mirroring the PR view).
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    json_response(status, shape.to_string())
}

/// Re-read one review link for the create response.
async fn fetch_review_owned(
    pool: &sqlx::PgPool,
    id: &uuid::Uuid,
) -> Result<ReviewLinkOwned, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT id, issue_id, provider, host_url, namespace, repo_name, repo_external_id,
                  external_id, external_iid, url, title, state, merged, draft,
                  remote_updated_at, metadata, created_at, updated_at, created_by_id
           FROM git_code_review_links WHERE id = $1 LIMIT 1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::ServerError);
    };
    use sqlx::Row as _;
    let opt_dt = |key: &str| -> Result<Option<String>, Denial> {
        row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(key)
            .map(|stamp| {
                stamp.map(|stamp| stamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
            })
            .map_err(|_| Denial::ServerError)
    };
    let req_dt = |key: &str| -> Result<String, Denial> {
        row.try_get::<chrono::DateTime<chrono::Utc>, _>(key)
            .map(|stamp| stamp.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
            .map_err(|_| Denial::ServerError)
    };
    Ok(ReviewLinkOwned {
        id: row
            .try_get::<Uuid, _>("id")
            .map_err(|_| Denial::ServerError)?
            .to_string(),
        issue_id: row
            .try_get::<Uuid, _>("issue_id")
            .map_err(|_| Denial::ServerError)?
            .to_string(),
        provider: row.try_get("provider").map_err(|_| Denial::ServerError)?,
        host_url: row.try_get("host_url").map_err(|_| Denial::ServerError)?,
        namespace: row.try_get("namespace").map_err(|_| Denial::ServerError)?,
        repo_name: row.try_get("repo_name").map_err(|_| Denial::ServerError)?,
        repo_external_id: row
            .try_get("repo_external_id")
            .map_err(|_| Denial::ServerError)?,
        external_id: row
            .try_get("external_id")
            .map_err(|_| Denial::ServerError)?,
        external_iid: row
            .try_get("external_iid")
            .map_err(|_| Denial::ServerError)?,
        url: row.try_get("url").map_err(|_| Denial::ServerError)?,
        title: row.try_get("title").map_err(|_| Denial::ServerError)?,
        state: row.try_get("state").map_err(|_| Denial::ServerError)?,
        merged: row.try_get("merged").map_err(|_| Denial::ServerError)?,
        draft: row.try_get("draft").map_err(|_| Denial::ServerError)?,
        remote_updated_at: opt_dt("remote_updated_at")?,
        metadata: row.try_get("metadata").map_err(|_| Denial::ServerError)?,
        created_at: req_dt("created_at")?,
        updated_at: req_dt("updated_at")?,
        created_by_id: row
            .try_get("created_by_id")
            .map_err(|_| Denial::ServerError)?,
    })
}

/// `DELETE code-reviews/{pk}/` (`git_code_review.py:67-74`): scoped
/// `.get(pk)`, detach through the D-05 port (legacy culled only when it
/// hangs off the same issue; the store soft-deletes + fans out), 204.
async fn review_destroy(
    State(state): State<AppState>,
    Path(params): Path<HashMap<String, String>>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> Response {
    if path_uuid(
        params
            .get("issue_id")
            .map(String::as_str)
            .unwrap_or_default(),
    )
    .is_err()
        || path_uuid(params.get("pk").map(String::as_str).unwrap_or_default()).is_err()
    {
        return crate::edge::proxy(State(state), req).await;
    }
    let ctx = match detail_context(&state, &params, extension, "DELETE").await {
        Ok(ctx) => ctx,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_scoped_review(
        &ctx.pool,
        &ctx.slug,
        &ctx.project_id,
        &ctx.issue_id,
        &ctx.pk,
    )
    .await
    {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(row) = row else {
        return Denial::NotFound.into_response();
    };
    let store = ReviewStore::new(ctx.pool.clone(), ctx.user_id);
    let link = GitCodeReviewLink {
        id: ctx.pk,
        created_at: parse_rfc3339(&row.created_at),
        updated_at: parse_rfc3339(&row.updated_at),
        created_by_id: row.created_by_id,
        updated_by_id: None,
        deleted_at: None,
        project_id: ctx.project_id,
        workspace_id: Uuid::nil(),
        issue_id: ctx.issue_id,
        provider: row.provider.clone(),
        host_url: row.host_url.clone(),
        namespace: row.namespace.clone(),
        repo_name: row.repo_name.clone(),
        repo_external_id: row.repo_external_id.clone(),
        external_id: row.external_id.clone(),
        external_iid: row.external_iid.clone(),
        url: row.url.clone(),
        title: row.title.clone(),
        state: row.state.clone(),
        merged: row.merged,
        draft: row.draft,
        remote_updated_at: row.remote_updated_at.as_deref().map(parse_rfc3339),
        metadata: row.metadata.clone(),
    };
    if pidash_services::integrations::code_reviews::detach_code_review_link(&store, &link)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    empty_response(StatusCode::NO_CONTENT)
}

fn parse_rfc3339(raw: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|aware| aware.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::DateTime::<chrono::Utc>::MIN_UTC)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denied_status(denial: &Denial) -> (StatusCode, String) {
        denial.status_and_body()
    }

    fn is_server_error(denial: Denial) -> bool {
        matches!(
            denial.status_and_body().0,
            StatusCode::INTERNAL_SERVER_ERROR
        )
    }

    // -- bodies -----------------------------------------------------------

    #[test]
    fn json_body_objects_pass_through() {
        let map = parse_json_body(r#"{"relation_type": "x", "issues": []}"#).expect("object");
        assert_eq!(map.get("relation_type").and_then(Value::as_str), Some("x"));
    }

    #[test]
    fn json_body_non_objects_are_server_errors() {
        for text in ["[1, 2]", "null", "1", "\"x\"", "true"] {
            assert!(
                is_server_error(parse_json_body(text).expect_err(text)),
                "{text}"
            );
        }
    }

    #[test]
    fn json_body_malformed_is_parse_detail() {
        let denial = parse_json_body("{oops").expect_err("malformed");
        let (status, body) = denied_status(&denial);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.starts_with(r#"{"detail":"JSON parse error - "#),
            "{body}"
        );
    }

    // -- path uuids --------------------------------------------------------

    #[test]
    fn path_uuid_accepts_strict_lowercase() {
        let raw = "12345678-1234-1234-1234-1234567890ab";
        assert_eq!(path_uuid(raw).expect("valid").to_string(), raw);
    }

    #[test]
    fn path_uuid_rejects_anything_else() {
        for raw in [
            "12345678-1234-1234-1234-1234567890AB",
            "123456781234123412341234567890ab",
            "{12345678-1234-1234-1234-1234567890ab}",
            "urn:uuid:12345678-1234-1234-1234-1234567890ab",
            "not-a-uuid",
            "",
        ] {
            assert!(path_uuid(raw).is_err(), "{raw}");
        }
    }

    // -- relation item prep -----------------------------------------------

    #[test]
    fn uuid_prep_strings_and_uuid_int() {
        let id = "12345678-1234-1234-1234-1234567890ab";
        assert_eq!(
            uuid_prep(&Value::String(id.to_owned())).expect("uuid"),
            Some(id.parse::<uuid::Uuid>().expect("parse"))
        );
        assert!(uuid_prep(&Value::String("nope".to_owned())).is_err());
        // `UUID(int=123)`.
        assert_eq!(
            uuid_prep(&serde_json::json!(123)).expect("int"),
            Some(uuid::Uuid::from_u128(123))
        );
        assert_eq!(
            uuid_prep(&Value::Bool(true)).expect("bool"),
            Some(uuid::Uuid::from_u128(1))
        );
        assert_eq!(uuid_prep(&Value::Null).expect("null"), None);
    }

    #[test]
    fn uuid_prep_rejects_with_the_detail_message() {
        for value in [
            Value::String("bad".to_owned()),
            serde_json::json!(1.5),
            serde_json::json!({"a": 1}),
            serde_json::json!([1]),
            serde_json::json!(-1),
        ] {
            let denial = uuid_prep(&value).expect_err("rejects");
            assert_eq!(
                denied_status(&denial).1,
                r#"{"error":"Please provide valid detail"}"#
            );
        }
    }

    #[test]
    fn py_str_matches_python() {
        assert_eq!(py_str(&Value::Bool(true)), "True");
        assert_eq!(py_str(&Value::Bool(false)), "False");
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_str(&serde_json::json!(123)), "123");
        assert_eq!(py_str(&serde_json::json!(1.5)), "1.5");
        assert_eq!(py_str(&Value::String("x".to_owned())), "x");
    }

    // -- link validation ---------------------------------------------------

    fn body(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn link_create_requires_url() {
        let denial = validate_link_body(&body(&[]), false).expect_err("required");
        assert_eq!(
            denied_status(&denial).1,
            r#"{"url":["This field is required."]}"#
        );
        // Partial writes skip absent keys.
        assert!(validate_link_body(&body(&[]), true).is_ok());
    }

    #[test]
    fn link_url_null_blank_and_nested_invalid() {
        let denial = validate_link_body(&body(&[("url", Value::Null)]), false).expect_err("null");
        assert_eq!(
            denied_status(&denial).1,
            r#"{"url":["This field may not be null."]}"#
        );
        let denial = validate_link_body(&body(&[("url", Value::String(String::new()))]), false)
            .expect_err("blank");
        assert_eq!(
            denied_status(&denial).1,
            r#"{"url":["This field may not be blank."]}"#
        );
        let denial = validate_link_body(
            &body(&[("url", Value::String("http://".to_owned()))]),
            false,
        )
        .expect_err("invalid");
        assert_eq!(
            denied_status(&denial).1,
            r#"{"url":{"error":"Invalid URL format."}}"#
        );
    }

    #[test]
    fn link_url_truthy_non_string_is_server_error() {
        for value in [serde_json::json!(123), Value::Bool(true)] {
            let denial = validate_link_body(&body(&[("url", value)]), false).expect_err("500");
            assert!(is_server_error(denial));
        }
    }

    #[test]
    fn link_url_prepends_scheme() {
        let fields = validate_link_body(
            &body(&[("url", Value::String("example.com/x".to_owned()))]),
            false,
        )
        .expect("prepend");
        assert_eq!(fields.url.as_deref(), Some("http://example.com/x"));
    }

    #[test]
    fn link_title_coerces_and_caps() {
        let fields = validate_link_body(
            &body(&[
                ("title", serde_json::json!(123)),
                ("url", Value::String("https://example.com/".to_owned())),
            ]),
            false,
        )
        .expect("coerce");
        assert_eq!(fields.title, Some(Some("123".to_owned())));
        let denial = validate_link_body(
            &body(&[
                ("title", Value::Bool(true)),
                ("url", Value::String("https://example.com/".to_owned())),
            ]),
            false,
        )
        .expect_err("bool");
        assert_eq!(
            denied_status(&denial).1,
            r#"{"title":["Not a valid string."]}"#
        );
        let denial = validate_link_body(&body(&[("title", Value::String("é".repeat(256)))]), true)
            .expect_err("cap");
        assert!(denied_status(&denial)
            .1
            .contains("no more than 255 characters"));
        assert!(
            validate_link_body(&body(&[("title", Value::String("é".repeat(255)))]), true).is_ok()
        );
    }

    #[test]
    fn link_metadata_any_but_null() {
        for value in [
            serde_json::json!("str"),
            serde_json::json!([1]),
            serde_json::json!(123),
            serde_json::json!(true),
            serde_json::json!({}),
        ] {
            assert!(validate_link_body(&body(&[("metadata", value)]), true).is_ok());
        }
        let denial =
            validate_link_body(&body(&[("metadata", Value::Null)]), true).expect_err("null");
        assert_eq!(
            denied_status(&denial).1,
            r#"{"metadata":["This field may not be null."]}"#
        );
    }

    // -- url validator ------------------------------------------------------

    #[test]
    fn url_validator_accepts_and_rejects() {
        for url in [
            "http://example.com/",
            "https://example.com/docs?a=1#frag",
            "http://user:pass@example.com:8080/x",
            "http://127.0.0.1/x",
            "http://[::1]/x",
            "http://localhost/x",
            "https://example.com.",
            "https://münchen.de/x",
            "ftp://example.com/x",
            "ftps://example.com/x",
        ] {
            assert!(django_url_valid(url), "{url}");
        }
        for url in [
            "http://",
            "http://exa mple.com/",
            "http://example.com:999999/",
            "http://foo_bar.com/",
            "http://example.com:ab/",
            "http://[::1/x",
            "gopher://example.com/",
            "example.com/x",
            "http://foo..com/",
            "http://-bad.com/",
        ] {
            assert!(!django_url_valid(url), "{url}");
        }
    }

    // -- dumps + floats -----------------------------------------------------

    #[test]
    fn python_dumps_matches_separators_and_ascii() {
        let value = serde_json::json!({"b": [1, true, null], "a": "é"});
        assert_eq!(
            python_dumps(&value),
            r#"{"b": [1, true, null], "a": "\u00e9"}"#
        );
        // Astral planes escape as surrogate pairs.
        assert_eq!(python_dumps(&serde_json::json!("𝄞")), r#""\ud834\udd1e""#);
        // Controls escape short or long.
        assert_eq!(
            python_dumps(&serde_json::json!("a\nb\tc\x01")),
            r#""a\nb\tc\u0001""#
        );
    }

    #[test]
    fn py_float_repr_matches_cpython() {
        for (value, expected) in [
            (0.001f64, "0.001"),
            (1e-3, "0.001"),
            (1.5, "1.5"),
            (100.0, "100.0"),
            (1e16, "1e+16"),
            (1e-4, "0.0001"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-0.0, "-0.0"),
            (123456789.0, "123456789.0"),
            (1.23e-5, "1.23e-05"),
            (2.5e20, "2.5e+20"),
            (5e-7, "5e-07"),
        ] {
            assert_eq!(py_float_repr(value), expected, "{value}");
        }
    }

    #[test]
    fn normalize_parse_floats_rewrites_literals() {
        let mut value: Value = serde_json::from_str(r#"{"t": 1e-3}"#).expect("parse");
        normalize_parse_floats(&mut value);
        assert_eq!(value["t"].to_string(), "0.001");
    }

    // -- relation shapes ----------------------------------------------------

    fn sample_issue() -> RelatedIssueInfo {
        RelatedIssueInfo {
            id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            sequence_id: 7,
            name: "N".to_owned(),
            state_id: Some(uuid::Uuid::nil()),
            priority: "high".to_owned(),
        }
    }

    #[test]
    fn relation_create_key_orders() {
        let tz: Tz = "UTC".parse().expect("tz");
        let user = uuid::Uuid::nil();
        let stamp = chrono::DateTime::parse_from_rfc3339("2026-10-05T05:08:56.003889Z")
            .expect("stamp")
            .with_timezone(&chrono::Utc);
        let normal = render_relation_create_row(
            &sample_issue(),
            "relates_to",
            &user,
            &stamp,
            &stamp,
            &tz,
            false,
        );
        let keys: Vec<&str> = normal
            .as_object()
            .expect("obj")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "project_id",
                "sequence_id",
                "relation_type",
                "name",
                "state_id",
                "priority",
                "created_by",
                "created_at",
                "updated_at",
                "updated_by"
            ]
        );
        let flipped = render_relation_create_row(
            &sample_issue(),
            "blocking",
            &user,
            &stamp,
            &stamp,
            &tz,
            true,
        );
        let keys: Vec<&str> = flipped
            .as_object()
            .expect("obj")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "id",
                "project_id",
                "sequence_id",
                "relation_type",
                "name",
                "state_id",
                "priority",
                "created_by",
                "created_at",
                "updated_by",
                "updated_at"
            ]
        );
        assert_eq!(
            normal.get("created_at").and_then(Value::as_str),
            Some("2026-10-05T05:08:56.003889Z")
        );
    }

    #[test]
    fn relation_create_null_state_drops_key() {
        let tz: Tz = "UTC".parse().expect("tz");
        let user = uuid::Uuid::nil();
        let stamp = chrono::Utc::now();
        let mut issue = sample_issue();
        issue.state_id = None;
        let row =
            render_relation_create_row(&issue, "relates_to", &user, &stamp, &stamp, &tz, false);
        assert!(!row.as_object().expect("obj").contains_key("state_id"));
    }

    #[test]
    fn relation_row_shape_follows_field_order() {
        let mut row = Map::new();
        for field in super::super::queries_core::RELATION_ROW_FIELDS {
            let value = match *field {
                "created_at" | "updated_at" => {
                    Value::String("2026-10-05T05:08:56.003889+00:00".to_owned())
                }
                "sort_order" => serde_json::json!(0.0),
                "sequence_id" => serde_json::json!(3),
                "label_ids" | "assignee_ids" => serde_json::json!([]),
                "state_id" => Value::Null,
                _ => Value::String("00000000-0000-0000-0000-000000000000".to_owned()),
            };
            row.insert((*field).to_owned(), value);
        }
        // `name`/`priority`/`relation_type` are strings, not uuids.
        row.insert("name".to_owned(), Value::String("N".to_owned()));
        row.insert("priority".to_owned(), Value::String("high".to_owned()));
        row.insert(
            "relation_type".to_owned(),
            Value::String("relates_to".to_owned()),
        );
        // The union emits raw `*_id` actor columns, not the wire keys.
        row.remove("created_by");
        row.remove("updated_by");
        row.insert(
            "created_by_id".to_owned(),
            Value::String("11111111-1111-1111-1111-111111111111".to_owned()),
        );
        row.insert("updated_by_id".to_owned(), Value::Null);
        let shaped = shape_relation_row(&row);
        let keys: Vec<&str> = shaped
            .as_object()
            .expect("obj")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, super::super::queries_core::RELATION_ROW_FIELDS);
        assert_eq!(
            shaped.get("created_at").and_then(Value::as_str),
            Some("2026-10-05T05:08:56.003889Z")
        );
        assert_eq!(shaped.get("sort_order").and_then(Value::as_f64), Some(0.0));
        assert_eq!(
            shaped.get("sort_order").map(Value::to_string).as_deref(),
            Some("0.0")
        );
        assert_eq!(
            shaped.get("created_by").and_then(Value::as_str),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(shaped.get("updated_by"), Some(&Value::Null));
    }

    // -- activity kwargs ------------------------------------------------------

    #[test]
    fn activity_kwargs_key_order_and_values() {
        let actor = uuid::Uuid::nil();
        let issue = uuid::Uuid::nil();
        let project = uuid::Uuid::nil();
        let kwargs = issue_activity_kwargs(
            "link.activity.deleted",
            "{}",
            Some("[]".to_owned()),
            &actor,
            &issue,
            &project,
            "http://app",
        );
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch",
                "notification",
                "origin"
            ]
        );
        assert_eq!(kwargs.get("notification"), Some(&Value::Bool(true)));
        assert_eq!(
            kwargs.get("type").and_then(Value::as_str),
            Some("link.activity.deleted")
        );
        assert!(!kwargs.contains_key("subscriber"));
    }

    // -- github pagination ----------------------------------------------------

    #[test]
    fn next_url_parses_link_headers() {
        assert_eq!(
            LiveGithubClient::next_url(Some(
                r#"<https://api.github.com/x?page=2>; rel="next", <https://api.github.com/x?page=5>; rel="last""#
            ))
            .as_deref(),
            Some("https://api.github.com/x?page=2")
        );
        assert_eq!(
            LiveGithubClient::next_url(Some(r#"<https://x>; rel="last""#)),
            None
        );
        assert_eq!(LiveGithubClient::next_url(None), None);
    }

    // -- denials -----------------------------------------------------------------

    #[test]
    fn denial_bodies_match_drf() {
        assert_eq!(
            denied_status(&Denial::LinkNotFound).1,
            r#"{"detail":"No IssueLink matches the given query."}"#
        );
        assert_eq!(
            denied_status(&Denial::ForbiddenDetail).1,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(
            denied_status(&Denial::WorkItemNotFound).1,
            r#"{"error":"Work item not found."}"#
        );
        assert_eq!(
            denied_status(&Denial::BadMessage(
                "Issue relation type is required".to_owned()
            ))
            .1,
            r#"{"message":"Issue relation type is required"}"#
        );
        assert_eq!(
            denied_status(&Denial::LinkNotFound).0,
            StatusCode::NOT_FOUND
        );
    }
}
