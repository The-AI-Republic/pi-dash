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
//! * A list/dict `relation_type` 500s (`dict.get` on an unhashable key)
//!   whenever `issues` is non-empty — but an empty list never evaluates
//!   the mapper, so that 201s.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono_tz::Tz;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::edge;
use crate::middleware::body_limit::read_request_bytes;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;

use pidash_compat::db::is_unique_violation;
use pidash_compat::response::{empty_response_no_content_type as empty_response, json_response};
use pidash_compat::validate::push_pairs_error as push_error;
use pidash_services::app_issues::relation_mapper::get_actual_relation;
use pidash_services::domain::relations as relation_domain;

use pidash_db::entity::{
    git_code_review_links, github_pull_request_links, issue_links, issue_relations, issues,
    project_members, projects, users, workspaces,
};
use sea_orm::sea_query::{Condition, Expr};
use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, FromQueryResult, JoinType, QueryFilter,
    QueryOrder, QuerySelect, QueryTrait, RelationTrait, Set,
};

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
            edge::owned(
                axum::routing::get(relation_list).post(relation_create),
                &["GET", "POST"],
            ),
        )
        .route(
            REMOVE_RELATION_PATH,
            edge::owned(axum::routing::post(remove_relation), &["POST"]),
        )
        .route(
            LINKS_PATH,
            edge::owned(
                axum::routing::get(link_list).post(link_create),
                &["GET", "POST"],
            ),
        )
        .route(
            LINK_PATH,
            edge::owned(
                axum::routing::get(link_retrieve)
                    .put(link_update)
                    .patch(link_partial_update)
                    .delete(link_destroy),
                &["GET", "PUT", "PATCH", "DELETE"],
            ),
        )
        .route(
            PR_LINKS_PATH,
            edge::owned(
                axum::routing::get(pr_list).post(pr_create),
                &["GET", "POST"],
            ),
        )
        .route(
            PR_LINK_PATH,
            edge::owned(axum::routing::delete(pr_destroy), &["DELETE"]),
        )
        .route(
            REVIEWS_PATH,
            edge::owned(
                axum::routing::get(review_list).post(review_create),
                &["GET", "POST"],
            ),
        )
        .route(
            REVIEW_PATH,
            edge::owned(axum::routing::delete(review_destroy), &["DELETE"]),
        )
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
    /// 502, truncated request-body read (the shared bad-gateway shape).
    TruncatedBody,
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
            Denial::WorkItemNotFound => {
                (StatusCode::NOT_FOUND, WORK_ITEM_NOT_FOUND_BODY.to_owned())
            }
            Denial::Conflict(message) => (
                StatusCode::CONFLICT,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::TruncatedBody => (
                StatusCode::BAD_GATEWAY,
                crate::edge::BAD_GATEWAY_BODY.to_owned(),
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

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// The SeaORM connection over the request pool (no I/O).
fn orm(pool: &sqlx::PgPool) -> DatabaseConnection {
    pidash_db::pool::sea_orm_connection(pool)
}

/// Any read failure is the generic 500.
fn db_denial(_: DbErr) -> Denial {
    Denial::ServerError
}

/// [`integrity_denial`] for a SeaORM write failure.
fn orm_integrity_denial(error: DbErr) -> Denial {
    if pidash_db::orm_tx::is_integrity_violation(&error) {
        return Denial::BadError("The payload is not valid".to_owned());
    }
    Denial::ServerError
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
    let exists: Option<Uuid> = users::Entity::find()
        .select_only()
        .column(users::Column::Id)
        .filter(users::Column::Id.eq(id))
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
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
    super::access::project_id_by_identifier(&orm(pool), slug, &upper)
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ProjectNotFound)
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
    let role = super::access::project_role(&orm(pool), slug, *project_id, *user_id)
        .await
        .map_err(db_denial)?;
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
            .map(|role| i32::from(role) == ROLE_ADMIN)
            .unwrap_or(false),
        has_project_admin_or_member: role
            .map(|role| [ROLE_ADMIN, ROLE_MEMBER].contains(&i32::from(role)))
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
    let name = super::access::user_timezone(&orm(pool), *user_id)
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ServerError)?;
    name.parse().map_err(|_| Denial::ServerError)
}

/// `Project.objects.get(pk)` over the default manager: soft-deleted rows
/// miss → 404 `{"error": ...}`. Returns the project's workspace id (what
/// `ProjectBaseModel.save` stamps).
async fn project_workspace(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<uuid::Uuid, Denial> {
    let row: Option<Uuid> = projects::Entity::find()
        .select_only()
        .column(projects::Column::WorkspaceId)
        .filter(projects::Column::Id.eq(*project_id))
        .filter(projects::Column::DeletedAt.is_null())
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    row.ok_or(Denial::NotFound)
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

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
    // The serve-wide `BodyLimitLayer` already 413'd over-limit bodies;
    // the read below cannot re-cap. A mid-stream transport failure fails
    // closed with the truncated-body 502.
    let bytes = read_request_bytes(body)
        .await
        .map_err(|_| Denial::TruncatedBody)?;
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

// ---------------------------------------------------------------------------
// Tasks: best-effort publishing through the queue
// ---------------------------------------------------------------------------

// Canonical task name lives in `pidash_services::domain::activity`.
use pidash_services::domain::activity::ISSUE_ACTIVITY_TASK;
/// `crawl_work_item_link_title` wire name (`bgtasks/work_item_link_task.py:262`).
const CRAWL_LINK_TASK: &str = "pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title";

/// Enqueue a worker message for the worker to forward to the broker
/// (Python-owned task). Best-effort: without a queue the response still
/// stands (the intake-handler precedent).
async fn enqueue_message(pool: &sqlx::PgPool, message: pidash_jobs::TaskMessage) {
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
    let message = pidash_jobs::TaskMessage::new(task, vec![], kwargs);
    enqueue_message(pool, message).await;
}

/// Best-effort positional-args publish (`crawl_work_item_link_title`
/// takes `(id, url)` positionally, `link.py:52,79`).
async fn enqueue_args(pool: &sqlx::PgPool, task: &str, args: Vec<Value>) {
    let message = pidash_jobs::TaskMessage::new(task, args, Map::new());
    enqueue_message(pool, message).await;
}

/// `soft_delete_related_objects.delay("db", <model>, pk, using=None)`
/// (`db/mixins.py:77`): three positional args, `using` stays a null
/// kwarg. Fires on every instance `.delete()` in these views.
async fn enqueue_soft_delete(pool: &sqlx::PgPool, model: &str, pk: &uuid::Uuid) {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    let message = pidash_jobs::TaskMessage::new(
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
/// origin for task kwargs, never the inbound host. Empty strings fall
/// through like Python's `or` (PIDASHCONV-1587).
fn app_origin(state: &AppState) -> String {
    let urls = &state.settings().urls;
    if let Some(url) = urls.app_base_url.as_deref().filter(|s| !s.is_empty()) {
        return url.to_owned();
    }
    if let Some(url) = urls.web_url.as_deref().filter(|s| !s.is_empty()) {
        return url.to_owned();
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
fn render_dt_utc(stamp: &sea_orm::prelude::DateTimeWithTimeZone) -> String {
    crate::serializer::render_datetime(&stamp.with_timezone(&chrono::Utc))
}

/// A stored instant as the RFC 3339 text the PR / review read shapes
/// carry (`render_dt_str` applies the actor zone once).
fn rfc3339(stamp: &sea_orm::prelude::DateTimeWithTimeZone) -> String {
    stamp
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

// ---------------------------------------------------------------------------
// User-lite nests (`created_by_detail`)
// ---------------------------------------------------------------------------

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
    // The asset FK traversal is `_base_manager` (a soft-deleted asset
    // still renders its URL), so the read is unguarded.
    let row = super::handlers_engage::fetch_asset_url_row(&orm(pool), asset_id)
        .await
        .map_err(db_denial)?
        .ok_or(Denial::ServerError)?;
    asset_url(
        &row.id.to_string(),
        row.entity_type.as_deref(),
        row.workspace_slug.as_deref(),
        row.project_id.map(|id| id.to_string()).as_deref(),
        row.issue_id.map(|id| id.to_string()).as_deref(),
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

/// The `users` columns behind [`UserLiteOwned`].
#[derive(Debug, FromQueryResult)]
struct UserLiteDb {
    id: Uuid,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_asset_id: Option<Uuid>,
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
    let rows = users::Entity::find()
        .select_only()
        .columns([
            users::Column::Id,
            users::Column::FirstName,
            users::Column::LastName,
            users::Column::Avatar,
            users::Column::AvatarAssetId,
            users::Column::IsBot,
            users::Column::DisplayName,
        ])
        .filter(users::Column::Id.is_in(user_ids.iter().copied()))
        .into_model::<UserLiteDb>()
        .all(&orm(pool))
        .await
        .map_err(db_denial)?;
    for row in rows {
        let avatar_url = avatar_url_of(pool, Some(row.avatar.clone()), row.avatar_asset_id).await?;
        out.insert(
            row.id,
            UserLiteOwned {
                id: row.id.to_string(),
                first_name: row.first_name,
                last_name: row.last_name,
                avatar: row.avatar,
                avatar_url,
                is_bot: row.is_bot,
                display_name: row.display_name,
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
    let db = orm(&pool);
    let mut body = Map::with_capacity(8);
    for bucket in super::queries_core::RELATION_BUCKETS {
        // The bucket builder scopes slug + issue sides only
        // (`relation.py:42-50`): the path `project_id` never filters
        // the list (it only gates + stamps), so related rows from any
        // project in the workspace show — there is no wrapper.
        let rows = match super::queries_core::relation_bucket_select(&slug, issue_id, *bucket)
            .into_model::<super::queries_core::RelationBucketRow>()
            .all(&db)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let shaped: Vec<Value> = rows
            .iter()
            .map(|row| shape_relation_row(row, bucket.label))
            .collect();
        body.insert(bucket.label.to_owned(), Value::Array(shaped));
    }
    json_response(StatusCode::OK, Value::Object(body).to_string())
}

/// Shape one relation `.values()` dict in `RELATION_ROW_FIELDS` order:
/// plain-encoder UTC datetimes, `py_float` sort order, UUIDs as strings,
/// the two id lists sorted (Postgres sorts a `DISTINCT` aggregate's input)
/// and `[]` when empty, and the bucket's `relation_type` literal.
fn shape_relation_row(row: &super::queries_core::RelationBucketRow, label: &str) -> Value {
    let uuid = |id: &Uuid| Value::String(id.to_string());
    let opt_uuid = |id: &Option<Uuid>| id.as_ref().map(uuid).unwrap_or(Value::Null);
    let ids = |ids: &Option<Vec<Uuid>>| {
        let sorted: std::collections::BTreeSet<Uuid> = ids.iter().flatten().copied().collect();
        Value::Array(sorted.iter().map(uuid).collect())
    };
    let mut out = Map::with_capacity(super::queries_core::RELATION_ROW_FIELDS.len());
    for field in super::queries_core::RELATION_ROW_FIELDS {
        let value = match *field {
            "id" => uuid(&row.id),
            "name" => Value::String(row.name.clone()),
            "state_id" => opt_uuid(&row.state_id),
            "sort_order" => {
                serde_json::from_str::<Value>(&crate::paginator::py_float_str(row.sort_order))
                    .unwrap_or(Value::Null)
            }
            "priority" => Value::String(row.priority.clone()),
            "sequence_id" => Value::Number(row.sequence_id.into()),
            "project_id" => uuid(&row.project_id),
            "created_at" => Value::String(render_dt_utc(&row.created_at)),
            "updated_at" => Value::String(render_dt_utc(&row.updated_at)),
            // The `.values()` wire keys drop the `_id` suffix on the two
            // actor FKs.
            "created_by" => opt_uuid(&row.created_by_id),
            "updated_by" => opt_uuid(&row.updated_by_id),
            "label_ids" => ids(&row.label_ids),
            "assignee_ids" => ids(&row.assignee_ids),
            "relation_type" => Value::String(label.to_owned()),
            other => unreachable!("relation row field {other}"),
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
    // Unhashable `relation_type` 500s on the first comprehension item,
    // before any item prep runs.
    if relation_type_unhashable(relation_raw, !items.is_empty()) {
        return Denial::ServerError.into_response();
    }
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
        // Domain batch twin: one chunk-atomic `UNNEST ... ON CONFLICT
        // DO NOTHING` per chunk (bare — no arbiter is possible: the
        // dedup index is a partial unique *index*, which `ON CONFLICT
        // ON CONSTRAINT` rejects with SQLSTATE 42P10).
        let mut rows = Vec::with_capacity(chunk.len());
        for slot in chunk {
            // `auto_now_add`/`auto_now` stamp each object in Python
            // before the `INSERT`.
            let stamp = chrono::Utc::now();
            let item = slot.expect("null items rejected above");
            let (issue_side, related_side) = if flipped {
                (item, issue_id)
            } else {
                (issue_id, item)
            };
            stamps.push((item, stamp, stamp));
            rows.push(relation_domain::RelationRow {
                id: uuid::Uuid::new_v4(),
                created_at: stamp,
                updated_at: stamp,
                created_by: Some(user_id),
                updated_by: Some(user_id),
                project_id,
                workspace_id,
                issue_id: issue_side,
                related_issue_id: related_side,
                relation_type: stored_type,
            });
        }
        if let Err(error) = relation_domain::relate_rows_or_nothing(
            &pidash_db::pool::sea_orm_connection(&pool),
            &rows,
        )
        .await
        {
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

/// `get_actual_relation` (`issue_relation_mapper.py`) runs once per
/// bulk-comprehension item: an unhashable `relation_type` (list/dict)
/// raises `TypeError` → 500 — but an empty `issues` list never
/// evaluates it, so that 201s.
fn relation_type_unhashable(raw: &Value, has_items: bool) -> bool {
    has_items && matches!(raw, Value::Array(_) | Value::Object(_))
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
    let rows: Vec<(Uuid, Uuid, i32, String, Option<Uuid>, String)> = issues::Entity::find()
        .select_only()
        .columns([
            issues::Column::Id,
            issues::Column::ProjectId,
            issues::Column::SequenceId,
            issues::Column::Name,
            issues::Column::StateId,
            issues::Column::Priority,
        ])
        .filter(issues::Column::Id.is_in(ids.iter().copied()))
        .into_tuple()
        .all(&orm(pool))
        .await
        .map_err(db_denial)?;
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
        Value::String(crate::serializer::render_datetime_in(&edge.created_at, &tz)),
    );
    shape.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(&edge.updated_at, &tz)),
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
    if relation_domain::soft_delete_relation_row(
        &pidash_db::pool::sea_orm_connection(&pool),
        edge.id,
        user_id,
        now,
        now,
    )
    .await
    .is_err()
    {
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
#[derive(Debug, FromQueryResult)]
struct RelationEdgeRow {
    id: uuid::Uuid,
    related_issue_id: uuid::Uuid,
    relation_type: String,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    created_at: sea_orm::prelude::DateTimeWithTimeZone,
    updated_at: sea_orm::prelude::DateTimeWithTimeZone,
}

/// `<column> IS NOT DISTINCT FROM <id>`: equality, or `IS NULL` for a
/// `None` filter value.
fn same_issue(column: issue_relations::Column, id: Option<Uuid>) -> sea_orm::sea_query::SimpleExpr {
    match id {
        Some(id) => column.eq(id),
        None => column.is_null(),
    }
}

/// Newest live edge in either direction (`Meta.ordering = -created_at`,
/// `.first()`).
fn relation_edge_select(
    slug: &str,
    issue_id: Uuid,
    related_id: Option<Uuid>,
) -> sea_orm::Select<issue_relations::Entity> {
    issue_relations::Entity::find()
        .select_only()
        .columns([
            issue_relations::Column::Id,
            issue_relations::Column::RelatedIssueId,
            issue_relations::Column::RelationType,
            issue_relations::Column::CreatedById,
            issue_relations::Column::UpdatedById,
            issue_relations::Column::CreatedAt,
            issue_relations::Column::UpdatedAt,
        ])
        .join(
            JoinType::InnerJoin,
            issue_relations::Relation::Workspaces.def(),
        )
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_relations::Column::DeletedAt.is_null())
        .filter(
            Condition::any()
                .add(
                    Condition::all()
                        .add(issue_relations::Column::IssueId.eq(issue_id))
                        .add(same_issue(
                            issue_relations::Column::RelatedIssueId,
                            related_id,
                        )),
                )
                .add(
                    Condition::all()
                        .add(same_issue(issue_relations::Column::IssueId, related_id))
                        .add(issue_relations::Column::RelatedIssueId.eq(issue_id)),
                ),
        )
        .order_by_desc(issue_relations::Column::CreatedAt)
        .limit(1)
}

async fn fetch_relation_edge(
    pool: &sqlx::PgPool,
    slug: &str,
    issue_id: &uuid::Uuid,
    related_id: Option<uuid::Uuid>,
) -> Result<Option<RelationEdgeRow>, Denial> {
    relation_edge_select(slug, *issue_id, related_id)
        .into_model::<RelationEdgeRow>()
        .one(&orm(pool))
        .await
        .map_err(db_denial)
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
    // `URLValidator.__call__` rejects past `max_length = 2048` (code
    // points) before anything else; the link `url` is a `TextField`,
    // so no model `max_length` shadows this arm.
    if url.chars().count() > 2048 {
        return false;
    }
    let scheme = url.split("://").next().unwrap_or("").to_lowercase();
    if !["http", "https", "ftp", "ftps"].contains(&scheme.as_str()) {
        return false;
    }
    // `unsafe_chars` (`\t\r\n`) are rejected before the regex runs.
    if url.chars().any(|c| matches!(c, '\t' | '\r' | '\n')) {
        return false;
    }
    // `urlsplit` NFKC parity (`urllib/parse.py::_checknetloc`): Django
    // raises `ValueError` before the regex runs, so a failure here is
    // final (no IDN retry).
    if !url_netloc_nfkc_ok(url) {
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
/// path/query/fragment without whitespace. The userinfo group is greedy
/// but backtracks: the userinfo reading is tried first, then the
/// no-userinfo reading. An `@` past a `?`/`#` is resource text
/// (`example.com?email=user@…` and `example.com:80?x@y/` validate), while
/// `?`/`#` may still sit inside real userinfo (`user?x@host/` validates
/// — all verified against Django's `URLValidator`).
fn url_host_pattern_valid(url: &str) -> bool {
    let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    // Greedy userinfo attempt (`[^\s:@/]+(?::[^\s:@/]*)?@`): the pattern
    // cannot cross `/`, so `@` past the first `/` starts the resource
    // (verified live: `/a@b` validates as a path).
    if let Some(at) = after_scheme.find('@') {
        if after_scheme.find('/').is_none_or(|slash| at < slash) {
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
            if user_ok && pass_ok && !rest.is_empty() && url_host_tail_valid(rest) {
                return true;
            }
        }
    }
    // No-userinfo reading (also the backtracked fallback).
    url_host_tail_valid(after_scheme)
}

/// Host | `:port` | resource match over one authority-plus-resource
/// string: IPv4 | IPv6 | hostname, optional `:port` (1-5 digits), then an
/// optional resource starting with `/`, `?` or `#` without whitespace.
fn url_host_tail_valid(host_part: &str) -> bool {
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

/// Codepoints whose NFKC normalization introduces one of `/?#@:`
/// (`urllib/parse.py::_checknetloc`, reached from every `urlsplit` call).
/// Same table as `ser_extras::NFKC_FORBIDDEN` (per-module copies are the
/// codebase precedent): enumerated once with CPython's
/// `unicodedata.normalize` over all 0x110000 codepoints (single-char
/// table; canonical composition can never produce ASCII `/ ? # @ :`, so
/// per-char membership is exactly the whole-string check Django runs).
const NFKC_FORBIDDEN: [char; 19] = [
    '\u{2047}', '\u{2048}', '\u{2049}', '\u{2100}', '\u{2101}', '\u{2105}', '\u{2106}', '\u{2A74}',
    '\u{FE13}', '\u{FE16}', '\u{FE55}', '\u{FE56}', '\u{FE5F}', '\u{FE6B}', '\u{FF03}', '\u{FF0F}',
    '\u{FF1A}', '\u{FF1F}', '\u{FF20}',
];

/// `urlsplit()` NFKC parity (`urllib/parse.py::_checknetloc`): the
/// authority (to the first `/?#`, like `_splitnetloc`) must hold no char
/// whose NFKC form introduces `/?#@:`. Any failure maps to invalid
/// (Django catches `ValueError` at `validators.py:126-128`).
fn url_netloc_nfkc_ok(url: &str) -> bool {
    let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    let netloc_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let netloc = &after_scheme[..netloc_end];
    // `_checknetloc`: early return for empty/ASCII; literal `@:?#` are
    // stripped before Django's check, but every table entry is non-ASCII,
    // so no stripping is needed here.
    netloc.is_ascii()
        || !netloc
            .chars()
            .any(|c| NFKC_FORBIDDEN.binary_search(&c).is_ok())
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

/// Python `str.strip()` for `CharField(trim_whitespace=True)` (the
/// DRF default): strips the same set `is_python_space` matches.
fn py_strip(text: &str) -> &str {
    text.trim_matches(is_python_space)
}

/// `ProhibitNullCharactersValidator` (a default `CharField` validator):
/// NUL fails even when every other check passes.
const NULL_CHARACTERS_MESSAGE: &str = "Null characters are not allowed.";

/// Validated link fields (`None` = key absent from the input).
#[derive(Debug)]
struct LinkFields {
    title: Option<Option<String>>,
    url: Option<String>,
    metadata: Option<Value>,
}

/// `IssueLinkSerializer` validation (`issue.py:804-877`): the
/// `to_internal_value` scheme step (with its truthy-non-string 500),
/// then `CharField` rules — strip first (`trim_whitespace`, the DRF
/// default), blank on the stripped value, `max_length` + NUL validators
/// on it, and for `url` the `URLValidator` (`issue.py:826-830`) whose
/// nested dict error keeps its `{"error": ...}` shape under the key.
/// `title` (nullable + blank, 255 chars, int/float coerced, bools
/// rejected), `url` (required unless partial, non-nullable), `metadata`
/// (any JSON but null). Unknown and read-only keys are ignored. Errors
/// collect in writable-field order.
fn validate_link_body(body: &Map<String, Value>, partial: bool) -> Result<LinkFields, Denial> {
    let mut errors: FieldErrors = Vec::new();
    // `to_internal_value` (`issue.py:817-823`) runs before field
    // validation: truthy non-string `url` 500s on `.startswith`;
    // strings take the prepend; falsy numbers fall through to the
    // field, which coerces them (`str(data)`) instead of rejecting.
    let mut url_raw: Option<Value> = body.get("url").cloned();
    if let Some(raw) = url_raw.clone() {
        if !matches!(raw, Value::String(_)) && is_python_truthy(&raw) {
            return Err(Denial::ServerError);
        }
        match &raw {
            Value::String(text)
                if !text.is_empty()
                    && !text.starts_with("http://")
                    && !text.starts_with("https://") =>
            {
                url_raw = Some(Value::String(format!("http://{text}")));
            }
            Value::Number(number) => {
                url_raw = Some(Value::String(number.to_string()));
            }
            _ => {}
        }
    }
    // `title`: `CharField(max_length=255, null=True, blank=True)` →
    // `required=False`. Absent on full writes too (model null). The
    // blank check, the length cap and the NUL check all run on the
    // stripped value; a whitespace-only title validates to `""`.
    let mut title: Option<Option<String>> = None;
    if let Some(raw) = body.get("title") {
        let coerced: Option<String> = match raw {
            Value::Null => {
                title = Some(None);
                None
            }
            Value::String(text) => Some(py_strip(text).to_owned()),
            Value::Number(number) => Some(py_strip(&number.to_string()).to_owned()),
            Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
                push_error(
                    &mut errors,
                    "title",
                    Value::String("Not a valid string.".to_owned()),
                );
                None
            }
        };
        if let Some(candidate) = coerced {
            if candidate.is_empty() {
                title = Some(Some(String::new()));
            } else {
                // Field validators collect: length first, then NUL.
                let mut bad = false;
                if candidate.chars().count() > 255 {
                    push_error(
                        &mut errors,
                        "title",
                        Value::String(
                            "Ensure this field has no more than 255 characters.".to_owned(),
                        ),
                    );
                    bad = true;
                }
                if candidate.contains('\0') {
                    push_error(
                        &mut errors,
                        "title",
                        Value::String(NULL_CHARACTERS_MESSAGE.to_owned()),
                    );
                    bad = true;
                }
                if !bad {
                    title = Some(Some(candidate));
                }
            }
        }
    }
    // `url`: required (unless partial), non-nullable, non-blank, then
    // the NUL validator and `URLValidator`, all on the stripped value.
    // The NUL failure is a plain list entry and pre-empts the nested
    // `{"error": ...}` shape (field validators run first).
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
            let stripped = py_strip(&text).to_owned();
            if stripped.is_empty() {
                push_error(
                    &mut errors,
                    "url",
                    Value::String("This field may not be blank.".to_owned()),
                );
            } else if stripped.contains('\0') {
                push_error(
                    &mut errors,
                    "url",
                    Value::String(NULL_CHARACTERS_MESSAGE.to_owned()),
                );
            } else if !django_url_valid(&stripped) {
                // Bare dict, not a one-list: DRF renders a dict
                // detail as-is (`{"url": {"error": ...}}`).
                let mut nested = Map::new();
                nested.insert(
                    "error".to_owned(),
                    Value::String("Invalid URL format.".to_owned()),
                );
                set_error(&mut errors, "url", Value::Object(nested));
            } else {
                url = Some(stripped);
            }
        }
        // Falsy non-strings (`false`, empty composites): no
        // `.startswith` call, so field validation rejects them as
        // non-strings. (Falsy numbers coerce above and never land here.)
        Some(Value::Bool(_))
        | Some(Value::Number(_))
        | Some(Value::Array(_) | Value::Object(_)) => push_error(
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

impl From<issue_links::Model> for IssueLinkOwned {
    fn from(row: issue_links::Model) -> Self {
        let utc = |stamp: sea_orm::prelude::DateTimeWithTimeZone| stamp.with_timezone(&chrono::Utc);
        Self {
            id: row.id,
            created_by_id: row.created_by_id,
            created_at: utc(row.created_at),
            updated_at: utc(row.updated_at),
            deleted_at: row.deleted_at.map(utc),
            title: row.title,
            url: row.url,
            metadata: row.metadata,
            updated_by_id: row.updated_by_id,
            project_id: row.project_id,
            workspace_id: row.workspace_id,
            issue_id: row.issue_id,
        }
    }
}

/// Scoped link list (`link.py:26-35` + `queries_core::link_list_select`).
async fn fetch_scoped_links(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Vec<IssueLinkOwned>, Denial> {
    let rows = super::queries_core::link_list_select(slug, *project_id, *issue_id, *user_id)
        .all(&orm(pool))
        .await
        .map_err(db_denial)?;
    Ok(rows.into_iter().map(IssueLinkOwned::from).collect())
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
    let row = super::queries_core::link_list_select(slug, *project_id, *issue_id, *user_id)
        .filter(issue_links::Column::Id.eq(*pk))
        .limit(1)
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    Ok(row.map(IssueLinkOwned::from))
}

/// Unscoped link read (`IssueLink.objects.get(workspace__slug,
/// project_id=, issue_id=, id=)` on the default manager): what the
/// custom `partial_update`/`destroy` use (member/archived-blind).
fn plain_link_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    pk: Uuid,
) -> sea_orm::Select<issue_links::Entity> {
    issue_links::Entity::find()
        .join(JoinType::InnerJoin, issue_links::Relation::Workspaces.def())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(issue_links::Column::ProjectId.eq(project_id))
        .filter(issue_links::Column::IssueId.eq(issue_id))
        .filter(issue_links::Column::Id.eq(pk))
        .filter(issue_links::Column::DeletedAt.is_null())
        .limit(1)
}

async fn fetch_plain_link(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<IssueLinkOwned>, Denial> {
    let row = plain_link_select(slug, *project_id, *issue_id, *pk)
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    Ok(row.map(IssueLinkOwned::from))
}

/// The link save (`serializer.save()` on update / partial update): the
/// writable fields plus the `auto_now` / CRUM audit.
async fn save_link(
    pool: &sqlx::PgPool,
    row: &IssueLinkOwned,
    now: chrono::DateTime<chrono::Utc>,
    actor: Uuid,
    pk: Uuid,
) -> Result<(), Denial> {
    issue_links::Entity::update_many()
        .col_expr(issue_links::Column::Title, Expr::value(row.title.clone()))
        .col_expr(issue_links::Column::Url, Expr::value(row.url.clone()))
        .col_expr(
            issue_links::Column::Metadata,
            Expr::value(row.metadata.clone()),
        )
        .col_expr(issue_links::Column::UpdatedAt, Expr::value(now))
        .col_expr(issue_links::Column::UpdatedById, Expr::value(actor))
        .filter(issue_links::Column::Id.eq(pk))
        .exec(&orm(pool))
        .await
        .map_err(orm_integrity_denial)?;
    Ok(())
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
    match link_dupe_exists(&pool, &url, &issue_id, None).await {
        Ok(true) => {
            return Denial::BadError("URL already exists for this Issue".to_owned()).into_response()
        }
        Ok(false) => {}
        Err(denial) => return denial.into_response(),
    }
    let now = chrono::Utc::now();
    let id = uuid::Uuid::new_v4();
    let metadata = metadata.unwrap_or(Value::Object(Map::new()));
    // `deleted_at` keeps its column default; `updated_by` is `NULL`
    // on create (CRUM stamps only `created_by`).
    if let Err(error) = issue_links::Entity::insert(issue_links::ActiveModel {
        id: Set(id),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        title: Set(title.clone().unwrap_or(None)),
        url: Set(url.clone()),
        metadata: Set(metadata.clone()),
        created_by_id: Set(Some(user_id)),
        updated_by_id: Set(None),
        issue_id: Set(issue_id),
        project_id: Set(project_id),
        workspace_id: Set(workspace_id),
        ..Default::default()
    })
    .exec_without_returning(&orm(&pool))
    .await
    {
        return orm_integrity_denial(error).into_response();
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
    if let Err(denial) = save_link(&ctx.pool, &row, now, ctx.user_id, ctx.pk).await {
        return denial.into_response();
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
    if let Err(denial) = save_link(&ctx.pool, &row, now, ctx.user_id, ctx.pk).await {
        return denial.into_response();
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
    if issue_links::Entity::update_many()
        .col_expr(issue_links::Column::DeletedAt, Expr::value(now))
        .col_expr(issue_links::Column::UpdatedAt, Expr::value(now))
        .col_expr(issue_links::Column::UpdatedById, Expr::value(ctx.user_id))
        .filter(issue_links::Column::Id.eq(ctx.pk))
        .exec(&orm(&ctx.pool))
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    enqueue_soft_delete(&ctx.pool, "issuelink", &ctx.pk).await;
    empty_response(StatusCode::NO_CONTENT)
}

/// The dup probe statement: same URL + issue, live rows, optionally
/// excluding one pk.
fn link_dupe_select(
    url: &str,
    issue_id: Uuid,
    exclude: Option<Uuid>,
) -> sea_orm::Select<issue_links::Entity> {
    issue_links::Entity::find()
        .select_only()
        .column(issue_links::Column::Id)
        .filter(issue_links::Column::Url.eq(url))
        .filter(issue_links::Column::IssueId.eq(issue_id))
        .apply_if(exclude, |query, pk| {
            query.filter(issue_links::Column::Id.ne(pk))
        })
        .filter(issue_links::Column::DeletedAt.is_null())
        .limit(1)
}

/// The `IssueLinkSerializer` dup probe: same URL + live issue row,
/// optionally excluding one pk.
async fn link_dupe_exists(
    pool: &sqlx::PgPool,
    url: &str,
    issue_id: &uuid::Uuid,
    exclude: Option<&uuid::Uuid>,
) -> Result<bool, Denial> {
    let row: Option<Uuid> = link_dupe_select(url, *issue_id, exclude.copied())
        .into_tuple()
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// PR links + code reviews: list SQL + shapes
// ---------------------------------------------------------------------------

/// `GithubPullRequestLinkViewSet.get_queryset` (`github_pr.py:37-48`):
/// every link column, `DISTINCT` over an unfiltered project-member join
/// (it only requires the project to have a membership row; no
/// `deleted_at` guard — bug 7), archived guard, `-created_at`. Same scope
/// shape as `queries_core::link_list_select`.
fn pr_link_list_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
) -> sea_orm::Select<github_pull_request_links::Entity> {
    github_pull_request_links::Entity::find()
        .distinct()
        .join(
            JoinType::InnerJoin,
            github_pull_request_links::Entity::belongs_to(project_members::Entity)
                .from(github_pull_request_links::Column::ProjectId)
                .to(project_members::Column::ProjectId)
                .into(),
        )
        .join(
            JoinType::InnerJoin,
            github_pull_request_links::Relation::Projects.def(),
        )
        .join(
            JoinType::InnerJoin,
            github_pull_request_links::Relation::Workspaces.def(),
        )
        .filter(github_pull_request_links::Column::DeletedAt.is_null())
        .filter(github_pull_request_links::Column::IssueId.eq(issue_id))
        .filter(github_pull_request_links::Column::ProjectId.eq(project_id))
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .order_by_desc(github_pull_request_links::Column::CreatedAt)
}

/// `GitCodeReviewLinkViewSet.get_queryset` (`git_code_review.py:33-44`),
/// same scope shape.
fn review_link_list_select(
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
) -> sea_orm::Select<git_code_review_links::Entity> {
    git_code_review_links::Entity::find()
        .distinct()
        .join(
            JoinType::InnerJoin,
            git_code_review_links::Entity::belongs_to(project_members::Entity)
                .from(git_code_review_links::Column::ProjectId)
                .to(project_members::Column::ProjectId)
                .into(),
        )
        .join(
            JoinType::InnerJoin,
            git_code_review_links::Relation::Projects.def(),
        )
        .join(
            JoinType::InnerJoin,
            git_code_review_links::Relation::Workspaces.def(),
        )
        .filter(git_code_review_links::Column::DeletedAt.is_null())
        .filter(git_code_review_links::Column::IssueId.eq(issue_id))
        .filter(git_code_review_links::Column::ProjectId.eq(project_id))
        .filter(projects::Column::ArchivedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .order_by_desc(git_code_review_links::Column::CreatedAt)
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

impl From<github_pull_request_links::Model> for PrLinkOwned {
    fn from(row: github_pull_request_links::Model) -> Self {
        Self {
            id: row.id.to_string(),
            issue_id: row.issue_id.to_string(),
            repo_owner: row.repo_owner,
            repo_name: row.repo_name,
            pr_number: row.pr_number,
            url: row.url,
            title: row.title,
            state: row.state,
            merged: row.merged,
            draft: row.draft,
            pr_updated_at: row.pr_updated_at.as_ref().map(rfc3339),
            created_at: rfc3339(&row.created_at),
            updated_at: rfc3339(&row.updated_at),
            created_by_id: row.created_by_id,
        }
    }
}

impl From<git_code_review_links::Model> for ReviewLinkOwned {
    fn from(row: git_code_review_links::Model) -> Self {
        Self {
            id: row.id.to_string(),
            issue_id: row.issue_id.to_string(),
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
            remote_updated_at: row.remote_updated_at.as_ref().map(rfc3339),
            metadata: row.metadata,
            created_at: rfc3339(&row.created_at),
            updated_at: rfc3339(&row.updated_at),
            created_by_id: row.created_by_id,
        }
    }
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
    let owned: Vec<PrLinkOwned> = match pr_link_list_select(&slug, project_id, issue_id)
        .all(&orm(&pool))
        .await
    {
        Ok(rows) => rows.into_iter().map(PrLinkOwned::from).collect(),
        Err(_) => return Denial::ServerError.into_response(),
    };
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
    let owned: Vec<ReviewLinkOwned> = match review_link_list_select(&slug, project_id, issue_id)
        .all(&orm(&pool))
        .await
    {
        Ok(rows) => rows.into_iter().map(ReviewLinkOwned::from).collect(),
        Err(_) => return Denial::ServerError.into_response(),
    };
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
    AttachError as PrAttachError, PrLinkRow, StoreError as PrStoreError,
};
use pidash_services::integrations::accounts::{
    GitStore, NewProviderAccount, StoreError as GitStoreError,
};
use pidash_services::integrations::code_reviews::{
    AdapterSource, AttachRequest, CodeReviewError, CodeReviewStore, NewCodeReviewLink,
    NewLegacyLink, ReviewAdapter, ReviewParser,
};
use pidash_services::integrations::repositories::{NewRepository, RepositoryDefaults};

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

use pidash_db::integrations::git_models::git_code_review_link::GitCodeReviewLink;
use pidash_db::integrations::git_models::git_provider_account::GitProviderAccount;
use pidash_db::integrations::git_models::git_repository::GitRepository;
use pidash_db::integrations::github_models::github_pull_request_link::GithubPullRequestLink;
use pidash_services::integrations::code_reviews::{LegacyLinkUpdate, LinkWriteError};

/// Map a SeaORM failure into the integrations store error.
fn git_store_error(context: &str, error: DbErr) -> GitStoreError {
    GitStoreError::Db(format!("{context}: {error}"))
}

/// True when a SeaORM write failed on a unique violation (SQLSTATE 23505).
fn is_orm_unique_violation(error: &DbErr) -> bool {
    match error {
        DbErr::Exec(sea_orm::RuntimeErr::SqlxError(inner))
        | DbErr::Query(sea_orm::RuntimeErr::SqlxError(inner)) => is_unique_violation(inner),
        _ => false,
    }
}

/// The live `CodeReviewStore`: pool + request actor (CRUM). Audit
/// columns follow `BaseModel.save` — the port passes `None` audit and
/// leaves stamping to the executing layer, exactly where Python does it
/// — and the delete methods soft-delete (`SoftDeleteModel.delete`) with
/// the `soft_delete_related_objects` fan-out inline, preserving the
/// update-then-enqueue order of `mixins.py:71-78`. Every statement is the
/// SeaORM twin beside its SQL constant in `pidash_services::integrations`.
struct ReviewStore {
    pool: sqlx::PgPool,
    db: DatabaseConnection,
    actor: uuid::Uuid,
}

impl ReviewStore {
    fn new(pool: sqlx::PgPool, actor: uuid::Uuid) -> Self {
        let db = orm(&pool);
        Self { pool, db, actor }
    }
}

use pidash_services::integrations::accounts::orm as accounts_orm;
use pidash_services::integrations::code_reviews::orm as reviews_orm;
use pidash_services::integrations::repositories::orm as repos_orm;

impl CodeReviewStore for ReviewStore {
    async fn review_issue_exists(
        &self,
        issue_id: Uuid,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<bool, GitStoreError> {
        reviews_orm::issue_exists(&self.db, issue_id, project_id, workspace_slug)
            .await
            .map_err(|error| git_store_error("review issue exists", error))
    }

    async fn find_legacy_link(
        &self,
        repo_owner_lower: &str,
        repo_name_lower: &str,
        pr_number: i32,
    ) -> Result<Option<GithubPullRequestLink>, GitStoreError> {
        reviews_orm::find_legacy_link(&self.db, repo_owner_lower, repo_name_lower, pr_number)
            .await
            .map_err(|error| git_store_error("legacy link find", error))
    }

    async fn find_path_review_link(
        &self,
        provider: &str,
        host_url: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
        external_iid: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        reviews_orm::find_path_review_link(
            &self.db,
            provider,
            host_url,
            namespace_lower,
            repo_name_lower,
            external_iid,
        )
        .await
        .map_err(|error| git_store_error("path review link find", error))
    }

    async fn find_repo_scope_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        repo_external_id: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        reviews_orm::find_repo_scope_link(
            &self.db,
            provider,
            host_url,
            external_iid,
            repo_external_id,
        )
        .await
        .map_err(|error| git_store_error("repo scope link find", error))
    }

    async fn find_path_empty_repo_link(
        &self,
        provider: &str,
        host_url: &str,
        external_iid: &str,
        namespace_lower: &str,
        repo_name_lower: &str,
    ) -> Result<Option<GitCodeReviewLink>, GitStoreError> {
        reviews_orm::find_path_empty_repo_link(
            &self.db,
            provider,
            host_url,
            external_iid,
            namespace_lower,
            repo_name_lower,
        )
        .await
        .map_err(|error| git_store_error("path empty repo link find", error))
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
        // The account and repository halves decode by prefixed name
        // (`pa_` / `r_`), never by position.
        reviews_orm::find_binding_review(
            &self.db,
            workspace_slug,
            project_id,
            provider,
            host_url,
            namespace,
            repo_name,
        )
        .await
        .map_err(|error| git_store_error("binding review find", error))
    }

    async fn find_review_issue_workspace(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
    ) -> Result<Option<Uuid>, GitStoreError> {
        reviews_orm::review_issue_workspace(&self.db, project_id, workspace_slug)
            .await
            .map_err(|error| git_store_error("review issue workspace", error))
    }

    async fn find_review_repository(
        &self,
        provider: &str,
        host_url: &str,
        namespace: &str,
        repo_name: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        reviews_orm::find_review_repository(&self.db, provider, host_url, namespace, repo_name)
            .await
            .map_err(|error| git_store_error("review repository find", error))
    }

    async fn find_review_project_workspace(
        &self,
        project_id: Uuid,
    ) -> Result<Option<Uuid>, GitStoreError> {
        reviews_orm::review_project_workspace(&self.db, project_id)
            .await
            .map_err(|error| git_store_error("review project workspace", error))
    }

    async fn insert_code_review_link(
        &self,
        row: NewCodeReviewLink,
    ) -> Result<GitCodeReviewLink, LinkWriteError> {
        let link = GitCodeReviewLink {
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
        };
        match reviews_orm::insert_code_review_link(&self.db, &link).await {
            Ok(()) => Ok(link),
            Err(error) if is_orm_unique_violation(&error) => Err(LinkWriteError::Conflict),
            Err(error) => Err(LinkWriteError::Store(GitStoreError::Db(format!(
                "code review link insert: {error}"
            )))),
        }
    }

    async fn insert_legacy_link(
        &self,
        row: NewLegacyLink,
    ) -> Result<GithubPullRequestLink, GitStoreError> {
        let link = GithubPullRequestLink {
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
        };
        reviews_orm::insert_legacy_link(&self.db, &link)
            .await
            .map_err(|error| git_store_error("legacy link insert", error))?;
        Ok(link)
    }

    async fn update_legacy_link(
        &self,
        id: Uuid,
        update: LegacyLinkUpdate,
    ) -> Result<GithubPullRequestLink, GitStoreError> {
        reviews_orm::update_legacy_link(&self.db, id, &update)
            .await
            .map_err(|error| git_store_error("legacy link update", error))?;
        // `existing.save(update_fields=[...])` mutates in place; reread
        // the row for the caller.
        reviews_orm::legacy_link_by_id(&self.db, id)
            .await
            .map_err(|error| git_store_error("legacy link reread", error))?
            .ok_or(GitStoreError::NotFound("legacy link"))
    }

    async fn delete_code_review_link(&self, id: Uuid) -> Result<(), GitStoreError> {
        // Instance `.delete()` (`mixins.py:71-78`): soft row, then the
        // fan-out — the port delegates the write mechanics here.
        reviews_orm::soft_delete_code_review_link(&self.db, id, self.actor, chrono::Utc::now())
            .await
            .map_err(|error| git_store_error("review link delete", error))?;
        enqueue_soft_delete(&self.pool, "gitcodereviewlink", &id).await;
        Ok(())
    }

    async fn delete_legacy_link(&self, id: Uuid) -> Result<(), GitStoreError> {
        reviews_orm::soft_delete_legacy_link(&self.db, id, self.actor, chrono::Utc::now())
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
        accounts_orm::list_provider_accounts(&self.db, workspace_id, provider, host_url)
            .await
            .map_err(|error| git_store_error("account list", error))
    }

    async fn get_provider_account(
        &self,
        workspace_id: Uuid,
        provider: &str,
        host_url: &str,
        account_id: Uuid,
    ) -> Result<Option<GitProviderAccount>, GitStoreError> {
        accounts_orm::get_provider_account(&self.db, workspace_id, provider, host_url, account_id)
            .await
            .map_err(|error| git_store_error("account get", error))
    }

    async fn insert_provider_account(
        &self,
        row: NewProviderAccount,
    ) -> Result<GitProviderAccount, GitStoreError> {
        accounts_orm::insert_provider_account(&self.db, &row)
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
        repos_orm::find_repository_by_external(&self.db, provider, host_url, external_id)
            .await
            .map_err(|error| git_store_error("repository find external", error))
    }

    async fn find_repository_by_full_name(
        &self,
        provider: &str,
        host_url: &str,
        full_name: &str,
    ) -> Result<Option<GitRepository>, GitStoreError> {
        repos_orm::find_repository_by_full_name(&self.db, provider, host_url, full_name)
            .await
            .map_err(|error| git_store_error("repository find full name", error))
    }

    async fn update_repository(
        &self,
        id: Uuid,
        defaults: RepositoryDefaults,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<GitRepository, GitStoreError> {
        repos_orm::update_repository(&self.db, id, &defaults, now)
            .await
            .map_err(|error| git_store_error("repository update", error))?;
        repos_orm::repository_by_id(&self.db, id)
            .await
            .map_err(|error| git_store_error("repository reread", error))?
            .ok_or(GitStoreError::NotFound("repository"))
    }

    async fn insert_repository(&self, row: NewRepository) -> Result<GitRepository, GitStoreError> {
        repos_orm::insert_repository(&self.db, &row)
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
        repos_orm::workspace_id(&self.db, slug)
            .await
            .map_err(|error| git_store_error("workspace id", error))
    }

    async fn find_project(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
    ) -> Result<Option<pidash_services::integrations::repositories::ProjectRef>, GitStoreError>
    {
        repos_orm::project(&self.db, workspace_id, project_id)
            .await
            .map_err(|error| git_store_error("project get", error))
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
            .map_err(|error| GitStoreError::Db(format!("bind tx: {error}")))?;
        let id = Uuid::new_v4();
        let mut metadata = Map::new();
        metadata.insert("raw_url".to_owned(), Value::String(plan.raw_url.clone()));
        {
            let conn = pidash_db::orm_tx::TxConn::new(&mut tx);
            repos_orm::delete_project_bindings(&conn, plan.project_id)
                .await
                .map_err(|error| git_store_error("bind delete", error))?;
            repos_orm::delete_project_github_syncs(&conn, plan.project_id)
                .await
                .map_err(|error| git_store_error("bind sync delete", error))?;
            repos_orm::insert_binding(
                &conn,
                repos_orm::NewBinding {
                    id,
                    now: plan.now,
                    created_by_id: plan.actor_id,
                    updated_by_id: plan.actor_id,
                    project_id: plan.project_id,
                    workspace_id: plan.workspace_id,
                    repository_id: plan.repository_id,
                    provider_account_id: plan.provider_account_id,
                    actor_id: plan.actor_id,
                    is_sync_enabled: false,
                    clone_auth_mode: &plan.clone_auth_mode,
                    metadata: Value::Object(metadata.clone()),
                },
            )
            .await
            .map_err(|error| git_store_error("bind insert", error))?;
        }
        // `project.save(update_fields=...)`: only the changed columns
        // plus `updated_at` (`services.py:341-348`).
        pidash_services::domain::projects::save_bind_project_update(
            &pidash_db::orm_tx::TxConn::new(&mut tx),
            plan.project_id,
            plan.repo_url_update.as_deref(),
            plan.base_branch_update.as_deref(),
            plan.now,
        )
        .await
        .map_err(|error| GitStoreError::Db(format!("bind project update: {error}")))?;
        tx.commit()
            .await
            .map_err(|error| GitStoreError::Db(format!("bind commit: {error}")))?;
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
        repos_orm::binding_view(&self.db, project_id, workspace_slug)
            .await
            .map_err(|error| git_store_error("binding get", error))
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
        repos_orm::set_binding_sync(&self.db, binding_id, enabled, now)
            .await
            .map_err(|error| git_store_error("binding set sync", error))?;
        repos_orm::binding_by_id(&self.db, binding_id)
            .await
            .map_err(|error| git_store_error("binding reread", error))?
            .ok_or(GitStoreError::NotFound("binding"))
    }

    async fn set_github_syncs_enabled(
        &self,
        project_id: Uuid,
        workspace_slug: &str,
        enabled: bool,
    ) -> Result<u64, GitStoreError> {
        repos_orm::set_project_github_syncs_enabled(&self.db, project_id, workspace_slug, enabled)
            .await
            .map_err(|error| git_store_error("github syncs set enabled", error))
    }

    async fn delete_binding(&self, binding_id: Uuid) -> Result<(), GitStoreError> {
        repos_orm::delete_binding(&self.db, binding_id)
            .await
            .map_err(|error| git_store_error("binding delete", error))?;
        Ok(())
    }

    async fn delete_github_syncs_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<u64, GitStoreError> {
        repos_orm::delete_project_github_syncs(&self.db, project_id)
            .await
            .map_err(|error| git_store_error("github syncs delete", error))
    }
}

// ---------------------------------------------------------------------------
// Transports: production `GitLabTransport` (the GitHub twin moved to
// pidash_services::integrations::github_client)
// ---------------------------------------------------------------------------

use pidash_compat::py::{layout_py_float, python_dumps};
use pidash_db::config::encryption::Keyring;
use pidash_services::integrations::adapters_github::GitHubAdapter;
use pidash_services::integrations::adapters_gitlab::{
    GitLabAdapter, GitLabConfig, GitLabRequest, GitLabResponse, GitLabTransport,
};
use pidash_services::integrations::github_client::{
    LiveGithubClient, PoolPrStore, PrLinksSurface, TransportContext, TransportGuard,
};
use pidash_types::integrations::registry::UnknownProvider;
use pidash_types::integrations::GitProviderError;

/// Run an async future to completion from the sync `GitLabTransport`
/// seam: a fresh current-thread runtime on a spawned thread. The seam
/// is sync because Python's `requests` is blocking; this keeps the
/// async executor unblocked without new Cargo features. (The GitHub
/// twin moved to `pidash_services::integrations::github_client` with
/// its own copy; OVL-04 canonicalizes the pair.)
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
    // `(raw_url or "").strip()` (`github_pr.py:54`): missing/null
    // and falsy non-strings (`0`, `false`, `[]`, `{}`) collapse to `""`
    // (the invalid-URL 400 below); truthy non-strings 500 on `.strip`;
    // strings strip inside the port's parser.
    let raw_url = match body.map.get("url").unwrap_or(&Value::Null) {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other if !is_python_truthy(other) => String::new(),
        _ => return Denial::ServerError.into_response(),
    };
    let ctx = TransportContext {
        pool: pool.clone(),
        redis: state.redis().cloned(),
        secret: state.settings().secret_key.clone(),
    };
    let _guard = TransportGuard::install(ctx);
    let store = PoolPrStore::new(pool.clone(), PrLinksSurface::Relations);
    let now = chrono::Utc::now();
    let outcome = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(
            pidash_services::app_issues::pr_links::attach_pull_request::<
                PoolPrStore,
                LiveGithubClient,
            >(
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
    github_pull_request_links::Entity::find_by_id(*id)
        .one(&orm(pool))
        .await
        .map_err(db_denial)?
        .map(PrLinkOwned::from)
        .ok_or(Denial::ServerError)
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
    let store = PoolPrStore::new(ctx.pool.clone(), PrLinksSurface::Relations);
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
        let message = pidash_jobs::TaskMessage::new(task.task, task.args, task.kwargs);
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
    let row = pr_link_list_select(slug, *project_id, *issue_id)
        .filter(github_pull_request_links::Column::Id.eq(*pk))
        .limit(1)
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    Ok(row.map(PrLinkOwned::from))
}

/// Scoped review-link detail: the list scope plus the pk.
async fn fetch_scoped_review(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    issue_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Option<ReviewLinkOwned>, Denial> {
    let row = review_link_list_select(slug, *project_id, *issue_id)
        .filter(git_code_review_links::Column::Id.eq(*pk))
        .limit(1)
        .one(&orm(pool))
        .await
        .map_err(db_denial)?;
    Ok(row.map(ReviewLinkOwned::from))
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
    // `(raw_url or "").strip()` (`code_reviews.py:167`): same falsy
    // collapse as the PR create; strings strip in the port's parser.
    let raw_url = match body.map.get("url").unwrap_or(&Value::Null) {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other if !is_python_truthy(other) => String::new(),
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
    git_code_review_links::Entity::find_by_id(*id)
        .one(&orm(pool))
        .await
        .map_err(db_denial)?
        .map(ReviewLinkOwned::from)
        .ok_or(Denial::ServerError)
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

    fn origin_state(app_base_url: Option<&str>, web_url: Option<&str>) -> AppState {
        let mut settings = pidash_db::config::Settings::test_defaults();
        settings.urls.app_base_url = app_base_url.map(str::to_owned);
        settings.urls.web_url = web_url.map(str::to_owned);
        AppState::with_settings("test", settings)
    }

    /// `base_host(request, is_app=True)` treats `""` as missing
    /// (PIDASHCONV-1587): an empty `APP_BASE_URL` falls through to
    /// `WEB_URL`. (The localhost fallback for unset-everywhere is a
    /// separate, deliberate-looking deviation — out of scope.)
    #[test]
    fn app_origin_falls_through_empty_strings() {
        let web = "https://web.example.invalid";
        let state = origin_state(Some(""), Some(web));
        assert_eq!(app_origin(&state), web);
        let state = origin_state(Some("https://app.example.invalid"), Some(web));
        assert_eq!(app_origin(&state), "https://app.example.invalid");
        let state = origin_state(Some(""), Some(""));
        assert_eq!(app_origin(&state), "http://localhost");
    }

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
    fn relation_type_unhashable_only_with_items() {
        // A list/dict `relation_type` 500s (unhashable `dict.get` key),
        // but only when the comprehension runs it: empty `issues` 201s.
        assert!(relation_type_unhashable(&serde_json::json!([]), true));
        assert!(relation_type_unhashable(&serde_json::json!({"a": 1}), true));
        assert!(!relation_type_unhashable(&serde_json::json!([]), false));
        assert!(!relation_type_unhashable(&serde_json::json!({}), false));
        assert!(!relation_type_unhashable(
            &Value::String("relates_to".to_owned()),
            true
        ));
        assert!(!relation_type_unhashable(&serde_json::json!(123), true));
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
    fn link_url_falsy_number_coerces_then_fails_validation() {
        // `CharField` coerces falsy numbers (`str(data)`) instead of
        // rejecting: `"0"` then fails `URLValidator` like any string.
        for value in [serde_json::json!(0), serde_json::json!(0.0)] {
            let denial = validate_link_body(&body(&[("url", value)]), false).expect_err("invalid");
            assert_eq!(
                denied_status(&denial).1,
                r#"{"url":{"error":"Invalid URL format."}}"#
            );
        }
    }

    #[test]
    fn link_fields_strip_whitespace() {
        // `trim_whitespace` (the DRF default): values strip before the
        // blank check, the validators and the store.
        let fields = validate_link_body(
            &body(&[
                ("title", Value::String("  padded  ".to_owned())),
                ("url", Value::String("https://example.com/x ".to_owned())),
            ]),
            false,
        )
        .expect("strip");
        assert_eq!(fields.title, Some(Some("padded".to_owned())));
        assert_eq!(fields.url.as_deref(), Some("https://example.com/x"));
        // Whitespace-only titles validate to `""` (blank is allowed).
        let fields = validate_link_body(&body(&[("title", Value::String("   ".to_owned()))]), true)
            .expect("blank");
        assert_eq!(fields.title, Some(Some(String::new())));
        // The length cap runs on the stripped value.
        let padded = format!("{} ", "x".repeat(255));
        assert!(validate_link_body(&body(&[("title", Value::String(padded))]), true).is_ok());
    }

    #[test]
    fn link_fields_reject_nul() {
        let denial =
            validate_link_body(&body(&[("title", Value::String("a\0b".to_owned()))]), true)
                .expect_err("nul");
        assert_eq!(
            denied_status(&denial).1,
            r#"{"title":["Null characters are not allowed."]}"#
        );
        // The NUL failure is a plain list entry and pre-empts the
        // nested `URLValidator` shape (field validators run first).
        let denial = validate_link_body(
            &body(&[("url", Value::String("https://example.com/a\0b".to_owned()))]),
            false,
        )
        .expect_err("nul");
        assert_eq!(
            denied_status(&denial).1,
            r#"{"url":["Null characters are not allowed."]}"#
        );
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
            // PIDASHCONV-1425: `@` past the netloc is resource text
            // (the userinfo group backtracks to the no-userinfo
            // reading); fullwidth chars past the netloc are valid.
            // Verdicts from Django 4.2.30 `URLValidator`.
            "https://example.com?email=user@gmail.com&ref=x",
            "https://example.com#user@gmail.com",
            "http://example.com:8080?x=a@b",
            "http://user?x@example.com/",
            "http://example.com:80?x@y/",
            "https://example.com/\u{FF0F}evil",
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
            // PIDASHCONV-1425: no backtracked reading validates these;
            // NFKC-forbidden chars in the netloc fail the urlsplit gate.
            "http://a:b@c@d/",
            "http://ab?xy@zw/",
            "https://example\u{FF0F}evil.com",
            "https://a.com\u{FF20}b.com",
        ] {
            assert!(!django_url_valid(url), "{url}");
        }
    }

    #[test]
    fn url_validator_enforces_max_length() {
        // `URLValidator.max_length = 2048` counts code points.
        let ok = format!("http://example.com/{}", "é".repeat(2048 - 19));
        assert_eq!(ok.chars().count(), 2048);
        assert!(django_url_valid(&ok));
        let over = format!("http://example.com/{}", "é".repeat(2048 - 19 + 1));
        assert_eq!(over.chars().count(), 2049);
        assert!(!django_url_valid(&over));
    }

    // -- dumps + floats -----------------------------------------------------

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
        let stamp = chrono::DateTime::parse_from_rfc3339("2026-10-05T05:08:56.003889+00:00")
            .expect("stamp");
        let (low, high) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let row = super::super::queries_core::RelationBucketRow {
            id: Uuid::nil(),
            name: "N".to_owned(),
            state_id: None,
            sort_order: 0.0,
            priority: "high".to_owned(),
            sequence_id: 3,
            project_id: Uuid::nil(),
            created_at: stamp,
            updated_at: stamp,
            created_by_id: Some(Uuid::from_u128(0x11111111_1111_1111_1111_111111111111)),
            updated_by_id: None,
            label_ids: Some(vec![high, low]),
            assignee_ids: None,
        };
        let shaped = shape_relation_row(&row, "relates_to");
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
        assert_eq!(shaped.get("state_id"), Some(&Value::Null));
        assert_eq!(shaped.get("sequence_id"), Some(&serde_json::json!(3)));
        // Sorted like Postgres's DISTINCT aggregate; NULL aggregate → [].
        assert_eq!(
            shaped.get("label_ids"),
            Some(&serde_json::json!([low.to_string(), high.to_string()]))
        );
        assert_eq!(shaped.get("assignee_ids"), Some(&serde_json::json!([])));
        assert_eq!(
            shaped.get("relation_type").and_then(Value::as_str),
            Some("relates_to")
        );
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
        // The truncated-body 502 reuses the shared bad-gateway bytes;
        // the 415 shape matches DRF content negotiation.
        assert_eq!(
            denied_status(&Denial::TruncatedBody).1,
            crate::edge::BAD_GATEWAY_BODY
        );
        assert_eq!(
            denied_status(&Denial::UnsupportedMediaType(
                "Unsupported media type \"text/plain\" in request.".to_owned()
            ))
            .1,
            r#"{"detail":"Unsupported media type \"text/plain\" in request."}"#
        );
    }

    // -- live SeaORM statements ------------------------------------------------

    /// A one-connection pool with constraint-free temp copies of `tables`
    /// shadowing `public`, or `None` (skip) without `DATABASE_URL`.
    async fn live_temp_pool(tables: &[&str]) -> Option<sqlx::PgPool> {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return None;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in tables {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        Some(pool)
    }

    async fn seed(pool: &sqlx::PgPool, statements: &[String]) {
        for sql in statements {
            sqlx::query(sql).execute(pool).await.expect(sql);
        }
    }

    /// Relation buckets, the removal edge, and the user-lite nests.
    #[tokio::test]
    async fn live_relation_reads() {
        let Some(pool) = live_temp_pool(&[
            "workspaces",
            "projects",
            "states",
            "issues",
            "issue_relations",
            "issue_labels",
            "issue_assignees",
            "users",
            "project_members",
            "file_assets",
        ])
        .await
        else {
            return;
        };
        let (ws, other_ws, project, archived) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (backlog, triage) = (Uuid::new_v4(), Uuid::new_v4());
        let (me, blocker, dup_a, dup_b, triaged, drafted, in_archived) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (active_user, inactive_user, asset) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (label_a, label_b) = (Uuid::from_u128(5), Uuid::from_u128(3));
        let issue_cols = "id, created_at, updated_at, name, priority, sequence_id, sort_order, \
                          project_id, workspace_id, state_id, is_draft, deleted_at, archived_at";
        let rel = |id: Uuid,
                   issue: Uuid,
                   related: Uuid,
                   kind: &str,
                   ws: Uuid,
                   ago: i32,
                   gone: bool| {
            format!(
                "INSERT INTO issue_relations (id, created_at, updated_at, relation_type, issue_id, \
                 related_issue_id, project_id, workspace_id, created_by_id, deleted_at) VALUES \
                 ('{id}', now() - interval '{ago} minutes', now(), '{kind}', '{issue}', '{related}', \
                 '{project}', '{ws}', '{active_user}', {})",
                if gone { "now()" } else { "NULL" }
            )
        };
        let (edge_old, edge_new) = (Uuid::new_v4(), Uuid::new_v4());
        seed(
            &pool,
            &[
                format!(
                    "INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme'), ('{other_ws}', 'other')"
                ),
                format!(
                    "INSERT INTO projects (id, workspace_id, archived_at) VALUES ('{project}', '{ws}', NULL), \
                     ('{archived}', '{ws}', now())"
                ),
                format!(
                    "INSERT INTO states (id, \"group\") VALUES ('{backlog}', 'backlog'), ('{triage}', 'triage')"
                ),
                format!(
                    "INSERT INTO issues ({issue_cols}) VALUES \
                     ('{me}', now(), now(), 'Me', 'none', 1, 1.0, '{project}', '{ws}', '{backlog}', false, NULL, NULL), \
                     ('{blocker}', '2026-10-05T05:08:56.003889+00', now(), 'Blocker', 'high', 2, 65535.5, '{project}', '{ws}', NULL, false, NULL, NULL), \
                     ('{dup_a}', now(), now(), 'DupA', 'low', 3, 2.0, '{project}', '{ws}', '{backlog}', false, NULL, NULL), \
                     ('{dup_b}', now(), now(), 'DupB', 'low', 4, 3.0, '{project}', '{ws}', '{backlog}', false, NULL, NULL), \
                     ('{triaged}', now(), now(), 'Tri', 'low', 5, 4.0, '{project}', '{ws}', '{triage}', false, NULL, NULL), \
                     ('{drafted}', now(), now(), 'Draft', 'low', 6, 5.0, '{project}', '{ws}', '{backlog}', true, NULL, NULL), \
                     ('{in_archived}', now(), now(), 'Arch', 'low', 7, 6.0, '{archived}', '{ws}', '{backlog}', false, NULL, NULL)"
                ),
                // `blocker` is blocked by `me`: the `blocking` bucket lists
                // `issue_id` where `related_issue_id` is the current issue.
                rel(Uuid::new_v4(), blocker, me, "blocked_by", ws, 5, false),
                // Duplicates in both directions, plus a tombstone and a
                // cross-workspace row that never count.
                rel(edge_old, me, dup_a, "duplicate", ws, 30, false),
                rel(edge_new, dup_b, me, "duplicate", ws, 1, false),
                rel(Uuid::new_v4(), me, blocker, "duplicate", ws, 2, true),
                rel(Uuid::new_v4(), me, blocker, "duplicate", other_ws, 3, false),
                // Filtered targets: triage state, draft, archived project.
                rel(Uuid::new_v4(), me, triaged, "relates_to", ws, 4, false),
                rel(Uuid::new_v4(), me, drafted, "relates_to", ws, 4, false),
                rel(Uuid::new_v4(), me, in_archived, "relates_to", ws, 4, false),
                format!(
                    "INSERT INTO issue_labels (id, issue_id, label_id, deleted_at) VALUES \
                     (gen_random_uuid(), '{blocker}', '{label_a}', NULL), \
                     (gen_random_uuid(), '{blocker}', '{label_a}', NULL), \
                     (gen_random_uuid(), '{blocker}', '{label_b}', NULL), \
                     (gen_random_uuid(), '{blocker}', '{}', now()), \
                     (gen_random_uuid(), '{blocker}', NULL, NULL)",
                    Uuid::new_v4()
                ),
                format!(
                    "INSERT INTO issue_assignees (id, issue_id, assignee_id, deleted_at) VALUES \
                     (gen_random_uuid(), '{blocker}', '{active_user}', NULL), \
                     (gen_random_uuid(), '{blocker}', '{inactive_user}', NULL)"
                ),
                format!(
                    "INSERT INTO users (id, first_name, last_name, avatar, avatar_asset_id, is_bot, \
                     display_name) VALUES \
                     ('{active_user}', 'A', 'U', 'raw.png', NULL, false, 'au'), \
                     ('{inactive_user}', 'I', 'U', '', '{asset}', true, 'iu')"
                ),
                // Two active rows (any project) fan the old join out; the
                // inactive user's only row is inactive.
                format!(
                    "INSERT INTO project_members (id, project_id, member_id, is_active) VALUES \
                     (gen_random_uuid(), '{project}', '{active_user}', true), \
                     (gen_random_uuid(), '{archived}', '{active_user}', true), \
                     (gen_random_uuid(), '{project}', '{inactive_user}', false)"
                ),
                format!(
                    "INSERT INTO file_assets (id, entity_type, workspace_id) VALUES \
                     ('{asset}', 'USER_AVATAR', '{ws}')"
                ),
            ],
        )
        .await;
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let bucket = |label: &str| {
            *super::super::queries_core::RELATION_BUCKETS
                .iter()
                .find(|bucket| bucket.label == label)
                .expect("bucket")
        };
        let rows = |label: &'static str| {
            let db = db.clone();
            async move {
                super::super::queries_core::relation_bucket_select("acme", me, bucket(label))
                    .into_model::<super::super::queries_core::RelationBucketRow>()
                    .all(&db)
                    .await
                    .expect("bucket rows")
            }
        };
        let blocking = rows("blocking").await;
        assert_eq!(blocking.len(), 1);
        let shaped = shape_relation_row(&blocking[0], "blocking");
        assert_eq!(shaped["id"], Value::String(blocker.to_string()));
        assert_eq!(shaped["state_id"], Value::Null);
        assert_eq!(shaped["sort_order"].to_string(), "65535.5");
        assert_eq!(shaped["created_at"], "2026-10-05T05:08:56.003889Z");
        assert_eq!(
            shaped["label_ids"],
            serde_json::json!([label_b.to_string(), label_a.to_string()])
        );
        assert_eq!(
            shaped["assignee_ids"],
            serde_json::json!([active_user.to_string()])
        );
        assert!(rows("blocked_by").await.is_empty());
        let mut dups: Vec<Uuid> = rows("duplicate").await.iter().map(|row| row.id).collect();
        dups.sort();
        let mut want = vec![dup_a, dup_b];
        want.sort();
        assert_eq!(dups, want);
        let dup = rows("duplicate").await;
        assert!(dup
            .iter()
            .all(|row| row.label_ids.is_none() && row.assignee_ids.is_none()));
        assert!(rows("relates_to").await.is_empty());
        // The wrong slug sees nothing.
        assert!(super::super::queries_core::relation_bucket_select(
            "other",
            me,
            bucket("duplicate")
        )
        .into_model::<super::super::queries_core::RelationBucketRow>()
        .all(&db)
        .await
        .expect("other slug")
        .is_empty());

        // Removal edge: either direction, newest first; a NULL target misses.
        let edge = fetch_relation_edge(&pool, "acme", &me, Some(dup_a))
            .await
            .expect("edge")
            .expect("found");
        assert_eq!((edge.id, edge.related_issue_id), (edge_old, dup_a));
        assert_eq!(edge.relation_type, "duplicate");
        assert_eq!(edge.created_by_id, Some(active_user));
        let reverse = fetch_relation_edge(&pool, "acme", &me, Some(dup_b))
            .await
            .expect("edge")
            .expect("found");
        assert_eq!((reverse.id, reverse.related_issue_id), (edge_new, me));
        assert!(fetch_relation_edge(&pool, "acme", &me, None)
            .await
            .expect("null")
            .is_none());
        // The tombstoned and cross-workspace duplicates never match.
        assert!(fetch_relation_edge(&pool, "acme", &me, Some(blocker))
            .await
            .expect("blocker")
            .is_some_and(|edge| edge.relation_type == "blocked_by"));

        // Issues by id and the user-lite nests (raw avatar vs asset URL).
        let infos = fetch_issues_by_id(&pool, &[blocker, dup_a])
            .await
            .expect("issues");
        assert_eq!(infos[&blocker].sequence_id, 2);
        assert_eq!(infos[&blocker].state_id, None);
        assert_eq!(infos[&dup_a].priority, "low");
        let lites = fetch_user_lites(&pool, &[active_user, inactive_user])
            .await
            .expect("lites");
        assert_eq!(lites[&active_user].avatar_url.as_deref(), Some("raw.png"));
        assert_eq!(
            lites[&inactive_user].avatar_url,
            Some(format!("/api/assets/v2/static/{asset}/"))
        );
        assert!(lites[&inactive_user].is_bot);
        assert_eq!(
            project_workspace(&pool, &project).await.expect("workspace"),
            ws
        );
    }

    /// Link, PR-link and review-link scopes plus the link writes.
    #[tokio::test]
    async fn live_link_reads_and_writes() {
        let Some(pool) = live_temp_pool(&[
            "workspaces",
            "projects",
            "project_members",
            "issue_links",
            "github_pull_request_links",
            "git_code_review_links",
        ])
        .await
        else {
            return;
        };
        let (ws, project, archived, lonely) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (member, stranger, issue) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (old_link, new_link, gone_link, arch_link) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (pr, pr_lonely, review) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let link = |id: Uuid, project: Uuid, url: &str, ago: i32, gone: bool| {
            format!(
                "INSERT INTO issue_links (id, created_at, updated_at, title, url, metadata, \
                 created_by_id, issue_id, project_id, workspace_id, deleted_at) VALUES ('{id}', \
                 now() - interval '{ago} minutes', now(), NULL, '{url}', '{{\"k\": 1.0}}', \
                 '{member}', '{issue}', '{project}', '{ws}', {})",
                if gone { "now()" } else { "NULL" }
            )
        };
        let pr_row = |id: Uuid, project: Uuid| {
            format!(
                "INSERT INTO github_pull_request_links (id, created_at, updated_at, repo_owner, \
                 repo_name, pr_number, url, title, state, merged, draft, pr_updated_at, \
                 created_by_id, issue_id, project_id, workspace_id) VALUES ('{id}', now(), now(), \
                 'o', 'r', 7, 'https://github.com/o/r/pull/7', 'T', 'open', false, true, \
                 '2026-01-02T03:04:05+00', '{member}', '{issue}', '{project}', '{ws}')"
            )
        };
        seed(
            &pool,
            &[
                format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme')"),
                format!(
                    "INSERT INTO projects (id, archived_at) VALUES ('{project}', NULL), \
                     ('{archived}', now()), ('{lonely}', NULL)"
                ),
                // Two active rows for the member (one tombstoned: bug 7) fan
                // the join out; DISTINCT folds them.
                format!(
                    "INSERT INTO project_members (id, project_id, member_id, is_active, deleted_at) \
                     VALUES (gen_random_uuid(), '{project}', '{member}', true, NULL), \
                     (gen_random_uuid(), '{project}', '{member}', true, now()), \
                     (gen_random_uuid(), '{project}', '{stranger}', false, NULL), \
                     (gen_random_uuid(), '{archived}', '{member}', true, NULL)"
                ),
                link(old_link, project, "https://a.example", 10, false),
                link(new_link, project, "https://b.example", 1, false),
                link(gone_link, project, "https://c.example", 2, true),
                link(arch_link, archived, "https://d.example", 3, false),
                pr_row(pr, project),
                pr_row(pr_lonely, lonely),
                format!(
                    "INSERT INTO git_code_review_links (id, created_at, updated_at, provider, \
                     host_url, namespace, repo_name, repo_external_id, external_id, external_iid, \
                     url, title, state, merged, draft, remote_updated_at, metadata, created_by_id, \
                     issue_id, project_id, workspace_id) VALUES ('{review}', now(), now(), \
                     'gitlab', 'https://gitlab.com', 'g', 'r', '1', '2', '3', 'https://gitlab.com/g/r/-/merge_requests/3', \
                     'MR', 'open', false, false, NULL, '{{\"a\": [1]}}', NULL, '{issue}', '{project}', '{ws}')"
                ),
            ],
        )
        .await;

        let listed = fetch_scoped_links(&pool, "acme", &project, &issue, &member)
            .await
            .expect("links");
        assert_eq!(
            listed.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![new_link, old_link]
        );
        assert_eq!(listed[0].metadata, serde_json::json!({"k": 1.0}));
        assert_eq!(listed[0].title, None);
        assert_eq!(listed[0].created_by_id, Some(member));
        assert!(
            fetch_scoped_links(&pool, "acme", &project, &issue, &stranger)
                .await
                .expect("stranger")
                .is_empty()
        );
        assert!(
            fetch_scoped_links(&pool, "acme", &archived, &issue, &member)
                .await
                .expect("archived")
                .is_empty()
        );
        assert!(
            fetch_scoped_link(&pool, "acme", &project, &issue, &old_link, &member)
                .await
                .expect("one")
                .is_some()
        );
        assert!(
            fetch_scoped_link(&pool, "acme", &project, &issue, &gone_link, &member)
                .await
                .expect("gone")
                .is_none()
        );
        // The plain read is member/archived-blind.
        assert!(
            fetch_plain_link(&pool, "acme", &archived, &issue, &arch_link)
                .await
                .expect("plain")
                .is_some()
        );
        assert!(
            fetch_plain_link(&pool, "nope", &archived, &issue, &arch_link)
                .await
                .expect("plain slug")
                .is_none()
        );
        // Dup probe: live rows only, optionally excluding self.
        assert!(link_dupe_exists(&pool, "https://a.example", &issue, None)
            .await
            .expect("dupe"));
        assert!(
            !link_dupe_exists(&pool, "https://a.example", &issue, Some(&old_link))
                .await
                .expect("self")
        );
        assert!(!link_dupe_exists(&pool, "https://c.example", &issue, None)
            .await
            .expect("tombstone"));
        // Save: writable fields plus audit.
        let mut row = fetch_plain_link(&pool, "acme", &project, &issue, &old_link)
            .await
            .expect("plain")
            .expect("found");
        row.title = Some("New".to_owned());
        row.url = "https://e.example".to_owned();
        row.metadata = serde_json::json!({});
        let now = chrono::Utc::now();
        save_link(&pool, &row, now, stranger, old_link)
            .await
            .expect("save");
        let saved = fetch_plain_link(&pool, "acme", &project, &issue, &old_link)
            .await
            .expect("reread")
            .expect("found");
        assert_eq!(saved.title.as_deref(), Some("New"));
        assert_eq!(saved.url, "https://e.example");
        assert_eq!(saved.updated_by_id, Some(stranger));
        assert_eq!(saved.metadata, serde_json::json!({}));

        // PR / review scopes: any membership row in the project, DISTINCT.
        let db = pidash_db::pool::sea_orm_connection(&pool);
        let prs = pr_link_list_select("acme", project, issue)
            .all(&db)
            .await
            .expect("prs");
        assert_eq!(prs.len(), 1);
        let owned = PrLinkOwned::from(prs[0].clone());
        assert_eq!(owned.pr_number, 7);
        assert_eq!(
            owned.pr_updated_at.as_deref(),
            Some("2026-01-02T03:04:05.000000Z")
        );
        assert!(pr_link_list_select("acme", lonely, issue)
            .all(&db)
            .await
            .expect("lonely")
            .is_empty());
        assert!(fetch_scoped_pr(&pool, "acme", &project, &issue, &pr)
            .await
            .expect("scoped pr")
            .is_some());
        assert_eq!(
            fetch_pr_owned(&pool, &pr_lonely).await.expect("owned").id,
            pr_lonely.to_string()
        );
        let reviews = review_link_list_select("acme", project, issue)
            .all(&db)
            .await
            .expect("reviews");
        assert_eq!(reviews.len(), 1);
        let review_owned = fetch_scoped_review(&pool, "acme", &project, &issue, &review)
            .await
            .expect("scoped review")
            .expect("found");
        assert_eq!(review_owned.metadata, serde_json::json!({"a": [1]}));
        assert_eq!(review_owned.remote_updated_at, None);
        assert!(
            fetch_scoped_review(&pool, "acme", &archived, &issue, &review)
                .await
                .expect("archived")
                .is_none()
        );
    }
}
