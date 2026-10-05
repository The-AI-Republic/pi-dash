//! Project member handlers (D-25, stage 5, PIDASHCONV-572).
//!
//! Ports `ProjectMemberViewSet` + the me / roles / preference endpoints from
//! `apps/api/pi_dash/app/views/project/member.py:27-385`:
//!
//! * `create` (`:47-154`): ADMIN gate, empty-400, workspace-role-vs-project-role
//!   guards, `bulk_update` reactivation + `bulk_create` of new rows (exact
//!   write order), `ProjectUserProperty` backfill (`min - 10000`, else
//!   `65535`), one `project_add_user_email` emit per member, 201 role shape.
//! * `list` (`:157-169`): ADMIN/MEMBER/GUEST gate, active-members queryset,
//!   full 6-key role shape (the `fields=` allowlist is ignored — ported bug).
//! * `retrieve` (`:172-203`): explicit 404, admin-vs-guest serializer switch
//!   on the *requesting* member's role.
//! * `partial_update` (`:206-265`): self-role 400 (fires for ANY field —
//!   ported bug), the role-comparison 403 matrix, the workspace-role
//!   ceiling, then the `ProjectMemberSerializer` partial write.
//! * `destroy` (`:268-298`): self-remove 400, higher-role 400 (not 403),
//!   `is_active=False`, 204 with no content type.
//! * `leave` (`:301-326`): only-admin 400, `is_active=False`, 204.
//! * `ProjectMemberUserEndpoint.get` (`:330-339`): own row, full shape.
//! * `UserProjectRolesEndpoint.get` (`:343-356`): `{project_id: role}` dict
//!   in newest-first row order (insertion order, not key-sorted).
//! * `ProjectMemberPreferenceEndpoint` (`:360-385`): get renders the stored
//!   row (JSONB order); patch shallow-merges the whole body over
//!   `preferences` and renders the in-memory merge (stored order + appended
//!   keys), never a DB re-read.
//!
//! Routes (`app/urls/project.py`): the members collection, `<uuid:pk>`
//! detail, `leave/`, `project-members/me/`, `users/me/.../project-roles/`,
//! and `preferences/member/<uuid:member_id>/`. Registration is the cutover
//! granularity (Porting guide cutover row): owned methods serve from Rust,
//! every other method on those paths proxies to Django through
//! [`crate::edge::proxy`], as do `<uuid:>` path params Django's resolver
//! would never route (its `{"error":"Page not found."}` 404 is preserved
//! byte for byte via [`proxy_through`]).
//!
//! Request order mirrors DRF `initial()`: session authN (401), the
//! `project_id` rewrite (UUIDs pass through unchecked; other identifiers
//! resolve `UPPER(strip)` in the workspace, else 404 `Project not found`),
//! `TimezoneMixin` activation (unknown zones are the `KeyError` 400),
//! then the gate (403), then the body. Anonymous callers skip the rewrite.
//! `project-roles` is the exception: its class-level `WorkspaceUserPermission`
//! runs inside `super().initial()`, before activation, so its gate stays first.
//!
//! Layering: gates in `super::gates` (PIDASHCONV-569), serializers in
//! `pidash_services::app_project::ser_member` (PIDASHCONV-564), queries in
//! `::queries` (PIDASHCONV-568), emits + `handle_exception` in `::tasks`
//! (PIDASHCONV-570), tables + defaults in `pidash_db::app_project::models`
//! (PIDASHCONV-567). Bodies negotiate through the shared D-20
//! `v1_cycles_modules::body` kernel with CPython-faithful JSON errors from
//! `v1_cycles_modules::json_cpython`. Asset URLs reuse
//! `v1_projects::handlers_members` (same `FileAsset.asset_url` property —
//! reuse, never fork); `ProjectUserProperty` filter defaults reuse
//! `pidash_db::app_issues::models_core` (D-26 owns them).
//!
//! # Ported bugs and quirks (translate, don't redesign — also listed in the PR)
//!
//! * BUG-fields (`serializers/base.py:17-20`): list + guest-retrieve pass
//!   `fields=("id","member","role")` but `DynamicBaseSerializer` overwrites
//!   it with `expand`, so the full 6-key role shape renders (via
//!   [`member_role_fields_alias`]).
//! * BUG-self-patch (`member.py:216`): the "cannot update your own role"
//!   400 fires for ANY field, and the `is_workspace_admin` bypass reads the
//!   *target's* workspace role, not the requester's.
//! * QUIRK-destroy-400 (`member.py:290-294`): the in-view higher-role check
//!   answers 400, not 403 (reachable via the ws-admin gate override).
//! * QUIRK-no-txn: `bulk_update` and the two `bulk_create`s are three
//!   separate atomic statements, not one transaction — a later failure
//!   leaves the earlier writes committed. Mirrored statement-for-statement.
//! * QUIRK-no-save: the bulk paths never call `save()`, so reactivated and
//!   created rows keep `created_by`/`updated_by` NULL and no
//!   `ProjectUserProperty` side effects fire from `save()`; the props rows
//!   come only from the explicit second `bulk_create`.
//! * QUIRK-bulk-cast: the reactivation `UPDATE` carries Django's
//!   `CAST(CASE ... AS smallint)` while the member `INSERT` has no cast —
//!   the same exotic role value can 201 on one path and 500 on the other.
//! * QUIRK-fanout: the member-list and roles JOINs carry no `deleted_at`
//!   guard on the joined `workspace_members` rows (L6 bug 9), so a stale
//!   soft-deleted row duplicates list output. No dedup, as Django.
//! * QUIRK-pref-order: patch renders the in-memory `dict.update` merge
//!   (stored order + appended keys); get renders the DB re-read (Postgres
//!   JSONB order). Both orders are preserved exactly.
//! * QUIRK-role-int: `int(request.data["role"])` runs before serializer
//!   validation with full CPython semantics (`"15"` ok, `"15.0"`/`null`/
//!   missing-key-missing 500, `15.0` truncates then 400s in the ChoiceField).
//!
//! Fixture ids: FX-APROJ-09 (`members` slice,
//! `rust-api/fixtures/app_project/FX-APROJ-09.handlers_project.json`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{Datelike, Offset, TimeZone};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::Row;

use pidash_db::app_project::models::{project_member, project_user_property};
use pidash_services::app_project::{ser_member, ser_shared, tasks};
use pidash_services::v1_projects::ser_collab;

use super::gates;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json_cpython::{
    parse_request_data, py_str, to_serde_publish, JObject, JStr, JVal, JsonFail, JSON_PARSE_PREFIX,
};
use crate::v1_projects::handlers_members as v1members;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Members collection (`urls/project.py`, `ProjectMemberViewSet` list/create).
pub const MEMBERS_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/members/";
/// Member detail (`<uuid:pk>` retrieve / partial_update / destroy).
pub const MEMBER_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/members/{pk}/";
/// Leave action (`POST` only).
pub const MEMBER_LEAVE_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/members/leave/";
/// Own membership (`ProjectMemberUserEndpoint`).
pub const MEMBER_ME_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/project-members/me/";
/// Role dict (`UserProjectRolesEndpoint`).
pub const PROJECT_ROLES_PATH: &str = "/api/users/me/workspaces/{slug}/project-roles/";
/// Preference get/patch (`<uuid:member_id>`).
pub const MEMBER_PREF_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/preferences/member/{member_id}/";

/// Owned methods serve from Rust; every other method on these paths proxies
/// to Django (its 405s, OPTIONS metadata and resolver 404s live there),
/// including unknown methods via the `MethodRouter` fallback.
pub fn routes() -> Router<AppState> {
    let proxy = crate::edge::proxy;
    Router::new()
        .route(
            MEMBERS_PATH,
            axum::routing::get(member_list)
                .post(member_create)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .options(proxy)
                .trace(proxy)
                .fallback(proxy),
        )
        .route(
            MEMBER_PATH,
            axum::routing::get(member_retrieve)
                .patch(member_partial_update)
                .delete(member_destroy)
                .post(proxy)
                .put(proxy)
                .options(proxy)
                .trace(proxy)
                .fallback(proxy),
        )
        .route(
            MEMBER_LEAVE_PATH,
            axum::routing::post(member_leave)
                .get(proxy)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .options(proxy)
                .trace(proxy)
                .fallback(proxy),
        )
        .route(
            MEMBER_ME_PATH,
            axum::routing::get(member_me)
                .post(proxy)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .options(proxy)
                .trace(proxy)
                .fallback(proxy),
        )
        .route(
            PROJECT_ROLES_PATH,
            axum::routing::get(project_roles)
                .post(proxy)
                .put(proxy)
                .patch(proxy)
                .delete(proxy)
                .options(proxy)
                .trace(proxy)
                .fallback(proxy),
        )
        .route(
            MEMBER_PREF_PATH,
            axum::routing::get(preference_get)
                .patch(preference_patch)
                .post(proxy)
                .put(proxy)
                .delete(proxy)
                .options(proxy)
                .trace(proxy)
                .fallback(proxy),
        )
}

// ---------------------------------------------------------------------------
// Exact bodies
// ---------------------------------------------------------------------------

/// `ProjectMemberViewSet.retrieve` explicit miss (`member.py:193-196`).
pub const MEMBER_NOT_FOUND_BODY: &str = r#"{"error":"Project member not found"}"#;
/// Resolver 404 (`app/views/error_404.py`): served by Django via
/// [`proxy_through`] for `<uuid:>` params it would never route.
pub const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found."}"#;

// ---------------------------------------------------------------------------
// Failure type
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 403, `WorkspaceUserPermission` class body.
    ClassDenied,
    /// 404, `handle_exception` `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, retrieve's explicit miss.
    MemberNotFound,
    /// 404, `{"detail":"Project not found"}` (project-kwarg rewrite miss).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (body `ParseError`).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline).
    BadError(String),
    /// 403, `{"error": ...}` (view-inline role matrix).
    ForbiddenError(String),
    /// 400, serializer `errors` object as-is.
    BadJson(Value),
    /// 500, generic branch.
    ServerError,
    /// 415, unsupported request content type.
    UnsupportedMediaType(String),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, gates::ANON_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, gates::FORBIDDEN_BODY.to_owned()),
            Denial::ClassDenied => (StatusCode::FORBIDDEN, gates::CLASS_DENIED_BODY.to_owned()),
            Denial::NotFound => (
                StatusCode::NOT_FOUND,
                tasks::OBJECT_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::MemberNotFound => (StatusCode::NOT_FOUND, MEMBER_NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                crate::app_issues::PROJECT_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::ForbiddenError(message) => (
                StatusCode::FORBIDDEN,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::BadJson(value) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(value).expect("errors serialize"),
            ),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                tasks::SERVER_ERROR_BODY.to_owned(),
            ),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
        }
    }
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        let (status, body) = self.status_and_body();
        json_response(status, body)
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Map a `sqlx` failure through `handle_exception`: `IntegrityError`
/// (SQLSTATE class 23) is the 400 payload body, everything else the
/// generic 500.
fn db_denial(error: sqlx::Error) -> Denial {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().is_some_and(|code| code.starts_with("23")) {
            return Denial::BadError("The payload is not valid".to_owned());
        }
    }
    Denial::ServerError
}

// ---------------------------------------------------------------------------
// Shared request plumbing
// ---------------------------------------------------------------------------

type HandlerResult = Result<Response, Denial>;

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

/// 204 shape: empty body with NO content type (verified live against Django
/// — DRF dataless responses carry no `Content-Type` here).
fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .body(axum::body::Body::empty())
        .expect("empty response")
}

fn ok_json(body: String) -> Response {
    json_response(StatusCode::OK, body)
}

fn created_json(body: String) -> Response {
    json_response(StatusCode::CREATED, body)
}

/// `request.user` from the Django session: missing session, missing key,
/// or a non-UUID id is anonymous → 401.
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

/// `_rewrite_project_kwarg` (`app/views/base.py:49-81`): authenticated
/// callers only (callers 401 first); UUID-looking input passes through
/// unchecked; other input resolves `UPPER(STRIP(identifier))` in the
/// workspace, else `Http404("Project not found")`.
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    if raw.parse::<uuid::Uuid>().is_ok() {
        // NB: the raw text passes through, not the normalized form —
        // Django keeps `kwargs["project_id"]` verbatim and lets the ORM
        // coerce per statement. Re-parse below only yields the same UUID.
        return raw.parse::<uuid::Uuid>().map_err(|_| Denial::ServerError);
    }
    let upper = raw.trim_matches(is_python_space).to_uppercase();
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

/// Django's `<uuid:>` converter (`django/urls/converters.py:26`):
/// `[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`
/// matched case-sensitively — only lowercase hyphenated UUIDs reach the
/// view; uppercase/braced/`urn:`/simple-hex forms resolver-404 before
/// auth. `uuid::Uuid::parse` accepts all of those, so the `<uuid:pk>`
/// / `<uuid:member_id>` shells gate on this and proxy otherwise.
fn is_django_uuid(raw: &str) -> bool {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut parts = raw.split('-');
    for want in GROUPS {
        match parts.next() {
            Some(part)
                if part.len() == want
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) => {}
            _ => return false,
        }
    }
    parts.next().is_none()
}

/// Membership facts for one `(user, slug, project)` over the same rows the
/// decorator reads (`app/permissions/base.py:44-78`), with the
/// allowed-role flags computed against the calling gate's roles.
async fn fetch_allow_facts(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    allowed: &[i32],
) -> Result<pidash_auth::permissions::allow::AllowFacts, Denial> {
    let project_role: Option<(i16,)> = sqlx::query_as(
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
    let workspace_role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(pidash_auth::permissions::allow::AllowFacts {
        workspace: pidash_types::WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: workspace_role.is_some(),
        has_allowed_workspace_role: workspace_role
            .map(|(role,)| allowed.contains(&i32::from(role)))
            .unwrap_or(false),
        is_creator: false,
        has_allowed_project_role: project_role
            .map(|(role,)| allowed.contains(&i32::from(role)))
            .unwrap_or(false),
        is_project_member: project_role.is_some(),
        is_workspace_admin: workspace_role
            .map(|(role,)| i32::from(role) == pidash_auth::permissions::ROLE_ADMIN)
            .unwrap_or(false),
    })
}

/// Roles of one allow-gate (`Project` gates check project roles;
/// anything else checks none here).
fn gate_roles(gate: &gates::Gate) -> &[i32] {
    match gate {
        gates::Gate::Project { roles } | gates::Gate::Workspace { roles } => roles,
        _ => &[],
    }
}

/// Run one `super::gates` row: `Allow` runs the body, `Deny` answers the
/// gate's 403, anonymous already 401'd.
fn check_gate(
    gate: &gates::Gate,
    slug: &str,
    facts: &pidash_auth::permissions::allow::AllowFacts,
) -> Result<(), Denial> {
    match gates::decide_gate(gate, &gates::tenant_context(slug), facts) {
        gates::GateOutcome::Allow => Ok(()),
        gates::GateOutcome::Deny => Err(Denial::Forbidden),
        gates::GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

fn gate_for(method: &str, path: &str) -> Result<&'static gates::Gate, Denial> {
    gates::gate_for(method, path)
        .map(|entry| &entry.gate)
        .ok_or(Denial::ServerError)
}

/// `request.user.user_timezone` (`TimezoneMixin.initial`, after
/// `super().initial()` but before the `@allow_permission` action gates):
/// `ZoneInfo` activation raises `ZoneInfoNotFoundError`, a `KeyError`
/// subclass, so unknown zones are the `KeyError` 400, not the 500.
async fn actor_timezone(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<Tz, Denial> {
    let row: Option<(String,)> =
        sqlx::query_as(r#"SELECT u.user_timezone FROM users u WHERE u.id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let (name,) = row.ok_or(Denial::ServerError)?;
    name.parse()
        .map_err(|_| body_error("The required key does not exist."))
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Rebuild a request from its parts and proxy it to Django (the resolver
/// 404 for a `<uuid:>` param Django itself would never route to the view,
/// plus unowned-method fallback — all byte-exact from Django).
async fn proxy_through(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::from(body))
        .expect("rebuild proxy request");
    *req.headers_mut() = headers;
    crate::edge::proxy(State(state), req).await
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-67`): the app
/// origin for the email emits, never the inbound host. Both settings
/// empty is `ImproperlyConfigured` → the generic 500 (members already
/// persisted — the call site sits after the writes, like Python's).
fn current_site(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    if let Some(url) = urls.app_base_url.as_deref() {
        if !url.is_empty() {
            return Ok(url.to_owned());
        }
    }
    if let Some(url) = urls.web_url.as_deref() {
        if !url.is_empty() {
            return Ok(url.to_owned());
        }
    }
    Err(Denial::ServerError)
}

/// Best-effort deferred publish of the `.delay(...)` calls (handlers
/// precedent): enqueue failures never change the response.
async fn enqueue_email(pool: &sqlx::PgPool, emit: &tasks::ProjectAddUserEmailEmit) {
    let message =
        pidash_jobs::celery::CeleryTaskMessage::new(emit.task_name(), emit.args(), emit.kwargs());
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task, "task enqueue failed; response stands");
    }
}

// ---------------------------------------------------------------------------
// Request bodies (`request.data`)
// ---------------------------------------------------------------------------

/// Member write paths accept JSON, form and multipart (DRF default
/// parsers). No key is list-shaped (create reads `members` with `.get`,
/// i.e. last-wins, never `getlist`), and no scalar treats a
/// present-but-empty form value as absent (`role` blank dies in `int()`,
/// JSONFields `json.loads` the blank, `comment`/FKs/datetimes keep their
/// `''`→`''`/`None` rules inside the validators below).
const MEMBER_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// `request.data` after content negotiation: empty is `{}`, JSON keeps
/// the CPython value path, form/multipart arrives as its text map
/// (uploads per key ride alongside for the `in` checks).
struct RequestData {
    value: JVal,
    files: shared_body::FilesMap,
    is_html: bool,
}

fn negotiate_data(headers: &HeaderMap, body: &[u8]) -> Result<RequestData, Denial> {
    let map_error = |error: shared_body::BodyError| match error {
        shared_body::BodyError::UnsupportedMediaType(message) => {
            Denial::UnsupportedMediaType(message)
        }
        shared_body::BodyError::ParseDetail(message) => Denial::BadDetail(message),
        shared_body::BodyError::ServerError => Denial::ServerError,
    };
    match shared_body::negotiate_body(headers, body, &MEMBER_BODY_SPEC).map_err(map_error)? {
        shared_body::NegotiatedBody::Empty => Ok(RequestData {
            value: JVal::Object(JObject::new()),
            files: Default::default(),
            is_html: false,
        }),
        shared_body::NegotiatedBody::JsonText(text) => parse_request_data(text.as_bytes())
            .map(|value| RequestData {
                value,
                files: Default::default(),
                is_html: false,
            })
            .map_err(|fail| match fail {
                JsonFail::Message(detail) => {
                    Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{detail}"))
                }
                JsonFail::Recursion => Denial::ServerError,
            }),
        shared_body::NegotiatedBody::Form { map, files } => {
            let mut object = JObject::new();
            for (key, item) in map.iter() {
                object.insert(JStr::from_text(key), json_value_to_jval(item));
            }
            Ok(RequestData {
                value: JVal::Object(object),
                files,
                is_html: true,
            })
        }
    }
}

/// Lift shared-kernel form text values into `JVal` (form parsing never
/// emits numbers, booleans or null — only strings and empty arrays for
/// file-only list keys, and our spec has no list fields).
fn json_value_to_jval(value: &Value) -> JVal {
    match value {
        Value::Null => JVal::Null,
        Value::Bool(flag) => JVal::Bool(*flag),
        Value::Number(number) => {
            let text = number.to_string();
            if text.contains(['.', 'e', 'E']) {
                JVal::Num(crate::v1_cycles_modules::json_cpython::JNum::float(text))
            } else {
                JVal::Num(crate::v1_cycles_modules::json_cpython::JNum::int(text))
            }
        }
        Value::String(text) => JVal::Str(JStr::from_text(text)),
        Value::Array(items) => JVal::Array(items.iter().map(json_value_to_jval).collect()),
        Value::Object(map) => {
            let mut object = JObject::new();
            for (key, item) in map.iter() {
                object.insert(JStr::from_text(key), json_value_to_jval(item));
            }
            JVal::Object(object)
        }
    }
}

fn body_error(body: &str) -> Denial {
    Denial::BadError(body.to_owned())
}

fn forbidden_error(body: &str) -> Denial {
    Denial::ForbiddenError(body.to_owned())
}

// ---------------------------------------------------------------------------
// CPython `int()` over request values (QUIRK-role-int)
// ---------------------------------------------------------------------------

/// What CPython `int(value)` yields, or `None` when it raises
/// (`ValueError`/`TypeError`/`OverflowError` → the generic 500).
/// Arbitrary precision: values beyond `i128` saturate by sign, which is
/// exact for every comparison against small role ints.
fn python_int(value: &JVal) -> Option<i128> {
    match value {
        JVal::Bool(flag) => Some(i128::from(*flag)),
        JVal::Num(number) => {
            if number.is_float() {
                let float = number.as_f64();
                if !float.is_finite() {
                    return None;
                }
                // `int()` truncates toward zero and never overflows on
                // finite input (unbounded ints) — saturate by sign past
                // `i128`, exact for small-int comparisons.
                if float >= 170_141_183_460_469_231_731_687_303_715_884_105_728.0 {
                    return Some(i128::MAX);
                }
                if float < -170_141_183_460_469_231_731_687_303_715_884_105_728.0 {
                    return Some(i128::MIN);
                }
                Some(float.trunc() as i128)
            } else if let Ok(int) = number.text().parse::<i128>() {
                Some(int)
            } else if number.text().starts_with('-') {
                Some(i128::MIN)
            } else {
                Some(i128::MAX)
            }
        }
        JVal::Str(text) => {
            let clean = text.to_clean_string()?;
            python_int_str(&clean)
        }
        JVal::Null | JVal::Array(_) | JVal::Object(_) => None,
    }
}

/// CPython `int(str)`: ASCII-whitespace strip, optional sign, base-10
/// digits (underscores allowed between digits), arbitrary precision.
fn python_int_str(text: &str) -> Option<i128> {
    let stripped =
        text.trim_matches(|ch: char| ch.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&ch));
    let (negative, digits) = match stripped.strip_prefix(['+', '-']) {
        Some(rest) => (stripped.starts_with('-'), rest),
        None => (false, stripped),
    };
    if digits.is_empty() {
        return None;
    }
    // Underscores: allowed only singly between digits.
    let mut canonical = String::with_capacity(digits.len());
    let mut prev_underscore = true; // leading '_' rejected
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if ch.is_ascii_digit() {
            canonical.push(ch);
            prev_underscore = false;
        } else {
            return None;
        }
    }
    if prev_underscore {
        return None;
    }
    let magnitude = canonical.trim_start_matches('0');
    if magnitude.is_empty() {
        return Some(0);
    }
    // 39 digits always overflow i128 (max 170141... = 39 digits but the
    // comparison below is exact for the boundary).
    if magnitude.len() > 39 {
        return Some(if negative { i128::MIN } else { i128::MAX });
    }
    match magnitude.parse::<i128>() {
        Ok(value) => Some(if negative { -value } else { value }),
        Err(_) => Some(if negative { i128::MIN } else { i128::MAX }),
    }
}

// ---------------------------------------------------------------------------
// Rows + rendering
// ---------------------------------------------------------------------------

/// Python float spellings for the JSON wire (`repr(float)`): `inf`,
/// `-inf`, `nan` — `serde_json` cannot emit these, so `sort_order` is
/// spliced post-serialization when non-finite (only reachable via an
/// explicit `inf`/`nan` PATCH; Postgres `float8` stores them fine).
fn py_float_wire(value: f64) -> Option<&'static str> {
    if value.is_nan() {
        Some("NaN")
    } else if value == f64::INFINITY {
        Some("Infinity")
    } else if value == f64::NEG_INFINITY {
        Some("-Infinity")
    } else {
        None
    }
}

/// Splice a non-finite top-level `sort_order` into serialized member JSON.
/// The scanner tracks string/depth state so a nested `"sort_order"` inside
/// `view_props` can never match: only the depth-1 key is replaced.
fn splice_non_finite_sort_order(mut text: String, wire: &str) -> String {
    const KEY: &str = "\"sort_order\"";
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' => {
                if depth == 1
                    && text[index..].starts_with(KEY)
                    && text[index + KEY.len()..].starts_with(':')
                {
                    let value_start = index + KEY.len() + 1;
                    let mut end = value_start;
                    while end < bytes.len() && !matches!(bytes[end], b',' | b'}') {
                        end += 1;
                    }
                    text.replace_range(value_start..end, wire);
                    return text;
                }
                in_string = true;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ => {}
        }
        index += 1;
    }
    text
}

/// One nested workspace lite row, resolved (logo precedence applied).
struct WorkspaceLite {
    name: String,
    slug: String,
    id: String,
    logo_url: Option<String>,
}

/// One nested project lite row, resolved (cover precedence applied).
struct ProjectLite {
    id: String,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_image_url: Option<String>,
    logo_props: Value,
    description: String,
    is_default: bool,
}

/// One nested member user row: the lite columns plus the admin-only
/// `email` / `last_login_medium` (rendered only by the admin shape).
struct MemberUser {
    id: String,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
    email: Option<String>,
    last_login_medium: String,
}

/// One fully-fetched `ProjectMember` row: datetimes pre-rendered in the
/// request timezone, JSONB parsed order-preserving from `::text`.
struct FullMember {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    comment: Option<String>,
    role: i64,
    view_props: Value,
    default_props: Value,
    preferences: Value,
    sort_order: f64,
    is_active: bool,
    created_by: Option<String>,
    updated_by: Option<String>,
    project_id: String,
    workspace_id: String,
    member_id: Option<String>,
    workspace: WorkspaceLite,
    project: ProjectLite,
    member: Option<MemberUser>,
}

/// Full-shape column list, selected with the `select_related` joins
/// (`member.py:166`-style: project + member + workspace; Django joins
/// carry no `deleted_at` guards on the joined tables).
const FULL_SELECT: &str = r#"SELECT pm.id, pm.created_at, pm.updated_at, pm.deleted_at,
       pm.comment, pm.role, pm.view_props::text AS view_props, pm.default_props::text AS default_props,
       pm.preferences::text AS preferences, pm.sort_order, pm.is_active,
       pm.created_by_id, pm.updated_by_id, pm.project_id, pm.workspace_id, pm.member_id,
       w.name AS w_name, w.slug AS w_slug, w.id AS w_id, w.logo AS w_logo, w.logo_asset_id AS w_logo_asset,
       p.id AS p_id, p.identifier AS p_identifier, p.name AS p_name, p.cover_image AS p_cover,
       p.cover_image_asset_id AS p_cover_asset, p.logo_props::text AS p_logo_props,
       p.description AS p_description, p.is_default AS p_is_default,
       u.id AS u_id, u.first_name AS u_first, u.last_name AS u_last, u.avatar AS u_avatar,
       u.avatar_asset_id AS u_avatar_asset, u.is_bot AS u_is_bot, u.display_name AS u_display,
       u.email AS u_email, u.last_login_medium AS u_medium
       FROM project_members pm
       JOIN workspaces w ON w.id = pm.workspace_id
       JOIN projects p ON p.id = pm.project_id
       LEFT JOIN users u ON u.id = pm.member_id"#;

/// Asset ids referenced by one decoded row (workspace logo + project
/// cover + member avatar), for the batched `file_assets` read.
fn row_asset_ids(row: &sqlx::postgres::PgRow) -> Vec<uuid::Uuid> {
    let mut ids = Vec::new();
    for column in ["w_logo_asset", "p_cover_asset", "u_avatar_asset"] {
        if let Ok(Some(id)) = row.try_get::<Option<uuid::Uuid>, _>(column) {
            ids.push(id);
        }
    }
    ids
}

fn parse_jsonb(text: &str) -> Result<Value, Denial> {
    serde_json::from_str(text).map_err(|_| Denial::ServerError)
}

/// Decode one [`FULL_SELECT`] row: datetimes render in the request
/// timezone, JSONB parses order-preserving, nested URLs resolve through
/// the L2 precedence kernels over the batched asset map.
fn decode_full(
    row: &sqlx::postgres::PgRow,
    tz: &Tz,
    assets: &HashMap<uuid::Uuid, String>,
) -> Result<FullMember, Denial> {
    let render_dt =
        |value: chrono::DateTime<chrono::Utc>| crate::serializer::render_datetime_in(&value, tz);
    let created_at: chrono::DateTime<chrono::Utc> =
        row.try_get("created_at").map_err(|_| Denial::ServerError)?;
    let updated_at: chrono::DateTime<chrono::Utc> =
        row.try_get("updated_at").map_err(|_| Denial::ServerError)?;
    let deleted_at: Option<chrono::DateTime<chrono::Utc>> =
        row.try_get("deleted_at").map_err(|_| Denial::ServerError)?;
    let view_props: String = row.try_get("view_props").map_err(|_| Denial::ServerError)?;
    let default_props: String = row
        .try_get("default_props")
        .map_err(|_| Denial::ServerError)?;
    let preferences: String = row
        .try_get("preferences")
        .map_err(|_| Denial::ServerError)?;
    let logo_props: String = row
        .try_get("p_logo_props")
        .map_err(|_| Denial::ServerError)?;
    let id: uuid::Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let project_id: uuid::Uuid = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
    let workspace_id: uuid::Uuid = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    let member_id: Option<uuid::Uuid> =
        row.try_get("member_id").map_err(|_| Denial::ServerError)?;
    let created_by: Option<uuid::Uuid> = row
        .try_get("created_by_id")
        .map_err(|_| Denial::ServerError)?;
    let updated_by: Option<uuid::Uuid> = row
        .try_get("updated_by_id")
        .map_err(|_| Denial::ServerError)?;
    let role: i16 = row.try_get("role").map_err(|_| Denial::ServerError)?;
    let w_id: uuid::Uuid = row.try_get("w_id").map_err(|_| Denial::ServerError)?;
    let w_logo_asset: Option<uuid::Uuid> = row
        .try_get("w_logo_asset")
        .map_err(|_| Denial::ServerError)?;
    let p_id: uuid::Uuid = row.try_get("p_id").map_err(|_| Denial::ServerError)?;
    let p_cover_asset: Option<uuid::Uuid> = row
        .try_get("p_cover_asset")
        .map_err(|_| Denial::ServerError)?;
    let u_id: Option<uuid::Uuid> = row.try_get("u_id").map_err(|_| Denial::ServerError)?;
    let u_avatar_asset: Option<uuid::Uuid> = row
        .try_get("u_avatar_asset")
        .map_err(|_| Denial::ServerError)?;

    let w_logo: Option<String> = row.try_get("w_logo").map_err(|_| Denial::ServerError)?;
    let logo_url = ser_member::resolve_workspace_logo_url(
        w_logo_asset.is_some(),
        w_logo_asset
            .as_ref()
            .and_then(|id| assets.get(id).map(String::as_str)),
        w_logo.as_deref(),
    )
    .map(str::to_owned);
    let p_cover: Option<String> = row.try_get("p_cover").map_err(|_| Denial::ServerError)?;
    let cover_image_url = ser_member::resolve_project_cover_image_url(
        p_cover_asset.is_some(),
        p_cover_asset
            .as_ref()
            .and_then(|id| assets.get(id).map(String::as_str)),
        p_cover.as_deref(),
    )
    .map(str::to_owned);
    let u_avatar: Option<String> = row.try_get("u_avatar").unwrap_or(None);
    let avatar_url = match u_id {
        None => None,
        Some(_) => ser_collab::resolve_avatar_url(
            u_avatar_asset.is_some(),
            u_avatar_asset
                .as_ref()
                .and_then(|id| assets.get(id).map(String::as_str)),
            u_avatar.as_deref().unwrap_or(""),
        )
        .map(str::to_owned),
    };

    let workspace = WorkspaceLite {
        name: row.try_get("w_name").map_err(|_| Denial::ServerError)?,
        slug: row.try_get("w_slug").map_err(|_| Denial::ServerError)?,
        id: w_id.to_string(),
        logo_url,
    };
    let project = ProjectLite {
        id: p_id.to_string(),
        identifier: row
            .try_get("p_identifier")
            .map_err(|_| Denial::ServerError)?,
        name: row.try_get("p_name").map_err(|_| Denial::ServerError)?,
        cover_image: p_cover,
        cover_image_url,
        logo_props: parse_jsonb(&logo_props)?,
        description: row
            .try_get("p_description")
            .map_err(|_| Denial::ServerError)?,
        is_default: row
            .try_get("p_is_default")
            .map_err(|_| Denial::ServerError)?,
    };
    let member = match u_id {
        None => None,
        Some(uid) => Some(MemberUser {
            id: uid.to_string(),
            first_name: row.try_get("u_first").map_err(|_| Denial::ServerError)?,
            last_name: row.try_get("u_last").map_err(|_| Denial::ServerError)?,
            avatar: u_avatar.unwrap_or_default(),
            avatar_url,
            is_bot: row.try_get("u_is_bot").map_err(|_| Denial::ServerError)?,
            display_name: row.try_get("u_display").map_err(|_| Denial::ServerError)?,
            email: row.try_get("u_email").map_err(|_| Denial::ServerError)?,
            last_login_medium: row.try_get("u_medium").map_err(|_| Denial::ServerError)?,
        }),
    };
    Ok(FullMember {
        id: id.to_string(),
        created_at: render_dt(created_at),
        updated_at: render_dt(updated_at),
        deleted_at: deleted_at.map(render_dt),
        comment: row.try_get("comment").map_err(|_| Denial::ServerError)?,
        role: i64::from(role),
        view_props: parse_jsonb(&view_props)?,
        default_props: parse_jsonb(&default_props)?,
        preferences: parse_jsonb(&preferences)?,
        sort_order: row.try_get("sort_order").map_err(|_| Denial::ServerError)?,
        is_active: row.try_get("is_active").map_err(|_| Denial::ServerError)?,
        created_by: created_by.map(|id| id.to_string()),
        updated_by: updated_by.map(|id| id.to_string()),
        project_id: project_id.to_string(),
        workspace_id: workspace_id.to_string(),
        member_id: member_id.map(|id| id.to_string()),
        workspace,
        project,
        member,
    })
}

/// Fetch asset URLs for a set of rows (batched `file_assets` read over
/// the v1 kernel — the same `asset_url` property, reused).
async fn fetch_assets(
    pool: &sqlx::PgPool,
    rows: &[sqlx::postgres::PgRow],
) -> Result<HashMap<uuid::Uuid, String>, Denial> {
    let mut ids: Vec<uuid::Uuid> = rows.iter().flat_map(row_asset_ids).collect();
    ids.sort();
    ids.dedup();
    v1members::fetch_asset_urls(pool, &ids)
        .await
        .map_err(|_| Denial::ServerError)
}

/// Django `.get()` cardinality: zero rows is the 404 branch, more than
/// one is `MultipleObjectsReturned` → the generic 500.
fn one<T>(mut rows: Vec<T>) -> Result<T, Denial> {
    if rows.len() > 1 {
        return Err(Denial::ServerError);
    }
    rows.pop().ok_or(Denial::NotFound)
}

/// Render the `ProjectMemberSerializer` shape (me / partial_update).
fn render_full(member: &FullMember) -> String {
    let workspace = ser_member::WorkspaceLiteRow {
        name: &member.workspace.name,
        slug: &member.workspace.slug,
        id: &member.workspace.id,
        logo_url: member.workspace.logo_url.as_deref(),
    };
    let project = ser_member::ProjectLiteRow {
        id: &member.project.id,
        identifier: &member.project.identifier,
        name: &member.project.name,
        cover_image: member.project.cover_image.as_deref(),
        cover_image_url: member.project.cover_image_url.as_deref(),
        logo_props: &member.project.logo_props,
        description: &member.project.description,
        is_default: member.project.is_default,
    };
    let user = member.member.as_ref().map(|user| ser_shared::UserLiteRow {
        id: &user.id,
        first_name: &user.first_name,
        last_name: &user.last_name,
        avatar: &user.avatar,
        avatar_url: user.avatar_url.as_deref(),
        is_bot: user.is_bot,
        display_name: &user.display_name,
    });
    let core = ser_member::ProjectMemberCore {
        id: &member.id,
        created_at: &member.created_at,
        updated_at: &member.updated_at,
        deleted_at: member.deleted_at.as_deref(),
        comment: member.comment.as_deref(),
        role: member.role,
        view_props: &member.view_props,
        default_props: &member.default_props,
        preferences: &member.preferences,
        sort_order: member.sort_order,
        is_active: member.is_active,
        created_by: member.created_by.as_deref(),
        updated_by: member.updated_by.as_deref(),
    };
    let mut row = ser_member::ProjectMemberRow {
        core,
        workspace,
        project,
        member: user,
    };
    // `serde_json` cannot emit non-finite floats; Python renders
    // `Infinity` / `-Infinity` / `NaN` — serialize a placeholder and
    // splice the exact bytes (only reachable via explicit inf/nan PATCH).
    if py_float_wire(member.sort_order).is_some() {
        row.core.sort_order = 0.0;
    }
    let view = ser_member::member_to_representation(&row);
    let text = serde_json::to_string(&view).expect("member view serializes");
    match py_float_wire(member.sort_order) {
        None => text,
        Some(wire) => splice_non_finite_sort_order(text, wire),
    }
}

/// Render the `ProjectMemberAdminSerializer` shape (admin retrieve).
fn render_full_admin(member: &FullMember) -> String {
    let workspace = ser_member::WorkspaceLiteRow {
        name: &member.workspace.name,
        slug: &member.workspace.slug,
        id: &member.workspace.id,
        logo_url: member.workspace.logo_url.as_deref(),
    };
    let project = ser_member::ProjectLiteRow {
        id: &member.project.id,
        identifier: &member.project.identifier,
        name: &member.project.name,
        cover_image: member.project.cover_image.as_deref(),
        cover_image_url: member.project.cover_image_url.as_deref(),
        logo_props: &member.project.logo_props,
        description: &member.project.description,
        is_default: member.project.is_default,
    };
    let user = member
        .member
        .as_ref()
        .map(|user| ser_member::UserAdminLiteRow {
            id: &user.id,
            first_name: &user.first_name,
            last_name: &user.last_name,
            avatar: &user.avatar,
            avatar_url: user.avatar_url.as_deref(),
            is_bot: user.is_bot,
            display_name: &user.display_name,
            email: user.email.as_deref(),
            last_login_medium: &user.last_login_medium,
        });
    let core = ser_member::ProjectMemberCore {
        id: &member.id,
        created_at: &member.created_at,
        updated_at: &member.updated_at,
        deleted_at: member.deleted_at.as_deref(),
        comment: member.comment.as_deref(),
        role: member.role,
        view_props: &member.view_props,
        default_props: &member.default_props,
        preferences: &member.preferences,
        sort_order: member.sort_order,
        is_active: member.is_active,
        created_by: member.created_by.as_deref(),
        updated_by: member.updated_by.as_deref(),
    };
    let mut row = ser_member::ProjectMemberAdminRow {
        core,
        workspace,
        project,
        member: user,
    };
    if py_float_wire(member.sort_order).is_some() {
        row.core.sort_order = 0.0;
    }
    let view = ser_member::member_admin_to_representation(&row);
    let text = serde_json::to_string(&view).expect("member view serializes");
    match py_float_wire(member.sort_order) {
        None => text,
        Some(wire) => splice_non_finite_sort_order(text, wire),
    }
}

/// The `fields=` allowlist the list / guest-retrieve call sites pass and
/// `DynamicBaseSerializer` ignores (BUG-fields, ported as-is).
fn member_role_fields_to_json(
    id: &str,
    role: i64,
    member: Option<&str>,
    project: &str,
    created_at: &str,
) -> String {
    let row = ser_member::ProjectMemberRoleRow {
        id,
        role,
        member,
        project,
        created_at,
    };
    let view = ser_member::member_role_fields_to_representation(
        &row,
        pidash_services::app_project::queries::MEMBER_LIST_FIELDS,
    );
    serde_json::to_string(&view).expect("role view serializes")
}

/// Render the create shape (no `fields=` argument at that call site).
fn member_role_to_json(
    id: &str,
    role: i64,
    member: Option<&str>,
    project: &str,
    created_at: &str,
) -> String {
    let row = ser_member::ProjectMemberRoleRow {
        id,
        role,
        member,
        project,
        created_at,
    };
    let view = ser_member::member_role_to_representation(&row);
    serde_json::to_string(&view).expect("role view serializes")
}

// ---------------------------------------------------------------------------
// `ProjectMemberSerializer` partial validation (`partial_update`)
// ---------------------------------------------------------------------------

/// Writable-field order for the 400 body: the serializer wire order
/// restricted to validating fields.
const PATCH_FIELD_ORDER: [&str; 10] = [
    "deleted_at",
    "comment",
    "role",
    "view_props",
    "default_props",
    "preferences",
    "sort_order",
    "is_active",
    "created_by",
    "updated_by",
];

const DATETIME_FORMAT_HINT: &str = "YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]";

/// One validated partial write: `None` per field means "not present".
#[derive(Default)]
struct MemberPatch {
    deleted_at: Option<Option<ParsedMemberDatetime>>,
    comment: Option<Option<String>>,
    role: Option<i64>,
    view_props: Option<Value>,
    default_props: Option<Value>,
    preferences: Option<Value>,
    sort_order: Option<f64>,
    is_active: Option<bool>,
    created_by: Option<Option<uuid::Uuid>>,
    updated_by: Option<Option<uuid::Uuid>>,
}

/// Field failure: message list(s) for the 400 body, or the 500 arm
/// (only the `str()`-depth cap on exotic FK echoes).
enum FieldFail {
    Messages(Vec<String>),
    ServerError,
}

impl FieldFail {
    fn one(message: String) -> Self {
        FieldFail::Messages(vec![message])
    }
}

/// PATCH validation outcome: the write, the collected 400 errors, or
/// the 500 arm.
type PatchOutcome = Result<MemberPatch, Result<serde_json::Map<String, Value>, Denial>>;

/// Validate one PATCH body (partial `ProjectMemberSerializer`): unknown
/// and read-only keys are silently ignored, present fields validate in
/// [`PATCH_FIELD_ORDER`], and any error aborts the write with the
/// collected `{"field": [...]}` body.
async fn validate_member_patch(
    pool: &sqlx::PgPool,
    object: &JObject,
    files: &shared_body::FilesMap,
    is_html: bool,
    tz: &Tz,
    tz_name: &str,
) -> PatchOutcome {
    let mut patch = MemberPatch::default();
    let mut errors = serde_json::Map::new();
    for field in PATCH_FIELD_ORDER {
        let file = files.get(field).and_then(|parts| parts.last());
        let present = object.contains_key(field) || file.is_some();
        if !present {
            continue;
        }
        let outcome: Result<(), FieldFail> = async {
            match field {
                "deleted_at" => {
                    patch.deleted_at =
                        Some(validate_deleted_at(object.get(field), file, tz, tz_name)?);
                }
                "comment" => {
                    patch.comment = Some(validate_comment(object.get(field), file)?);
                }
                "role" => {
                    patch.role = Some(validate_role_choice(object.get(field), file)?);
                }
                "view_props" => {
                    patch.view_props = Some(validate_json_field(object.get(field), file, is_html)?);
                }
                "default_props" => {
                    patch.default_props =
                        Some(validate_json_field(object.get(field), file, is_html)?);
                }
                "preferences" => {
                    patch.preferences =
                        Some(validate_json_field(object.get(field), file, is_html)?);
                }
                "sort_order" => {
                    patch.sort_order = Some(validate_sort_order(object.get(field), file)?);
                }
                "is_active" => {
                    patch.is_active = Some(validate_is_active(object.get(field), file)?);
                }
                "created_by" => {
                    patch.created_by = Some(validate_fk(object.get(field), file, pool).await?);
                }
                "updated_by" => {
                    patch.updated_by = Some(validate_fk(object.get(field), file, pool).await?);
                }
                _ => unreachable!("field order lists validating fields only"),
            }
            Ok(())
        }
        .await;
        match outcome {
            Ok(()) => {}
            Err(FieldFail::Messages(messages)) => {
                errors.insert(
                    field.to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
            Err(FieldFail::ServerError) => return Err(Err(Denial::ServerError)),
        }
    }
    if errors.is_empty() {
        Ok(patch)
    } else {
        Err(Ok(errors))
    }
}

fn datetime_invalid() -> FieldFail {
    FieldFail::one(format!(
        "Datetime has wrong format. Use one of these formats instead: {DATETIME_FORMAT_HINT}."
    ))
}

/// `DateTimeField` (`deleted_at`, nullable): ISO-8601 via the
/// `fromisoformat`-then-fallback grammar, naive values made aware in the
/// request timezone, aware values converted into it. The parsed value
/// carries the gap arm's verbatim echo (if any) through to the PATCH
/// response renderer.
fn validate_deleted_at(
    value: Option<&JVal>,
    file: Option<&shared_body::FilePart>,
    tz: &Tz,
    tz_name: &str,
) -> Result<Option<ParsedMemberDatetime>, FieldFail> {
    if file.is_some() {
        return Err(datetime_invalid());
    }
    match value {
        None => Ok(None),
        Some(JVal::Null) => Ok(None),
        Some(JVal::Str(text)) => {
            let clean = text.to_clean_string().ok_or_else(datetime_invalid)?;
            parse_member_datetime(&clean, tz, tz_name).map(Some)
        }
        Some(_) => Err(datetime_invalid()),
    }
}

/// `CharField` (`comment`, `allow_blank` + `allow_null`, trimmed):
/// numerics coerce via `str()`, booleans and composites fail, `\x00`
/// and surrogates fail their validators.
fn validate_comment(
    value: Option<&JVal>,
    file: Option<&shared_body::FilePart>,
) -> Result<Option<String>, FieldFail> {
    if let Some(part) = file {
        // The upload object is not a string — only a whitespace-only
        // name reaches the blank rule via `str()`.
        if part.filename.trim_matches(is_python_space).is_empty() {
            return Ok(Some(String::new()));
        }
        return Err(FieldFail::one("Not a valid string.".to_owned()));
    }
    match value {
        None => Ok(None),
        Some(JVal::Null) => Ok(None),
        Some(JVal::Str(text)) => {
            if text.is_empty() || text.trim().is_empty() {
                return Ok(Some(String::new()));
            }
            let trimmed = text.trim();
            let mut failures = Vec::new();
            if trimmed.contains_nul() {
                failures.push("Null characters are not allowed.".to_owned());
            }
            if let Some(code) = trimmed.first_surrogate() {
                failures.push(format!("Surrogate characters are not allowed: U+{code:X}."));
            }
            if !failures.is_empty() {
                return Err(FieldFail::Messages(failures));
            }
            trimmed
                .to_clean_string()
                .map(Some)
                .ok_or_else(|| FieldFail::one("Not a valid string.".to_owned()))
        }
        Some(JVal::Num(number)) => Ok(Some(number.py_string())),
        Some(_) => Err(FieldFail::one("Not a valid string.".to_owned())),
    }
}

fn is_python_space(ch: char) -> bool {
    ch.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&ch)
}

/// `ChoiceField` (`role`, 20/15/5): stringified lookup — only reached
/// for values that survived the view's `int()` gates, plus defensive
/// arms for anything else.
fn validate_role_choice(
    value: Option<&JVal>,
    file: Option<&shared_body::FilePart>,
) -> Result<i64, FieldFail> {
    if file.is_some() {
        // Unreachable: the view's `int()` gates 500 on uploads first.
        return Err(FieldFail::ServerError);
    }
    let key = match value {
        None => return Err(FieldFail::one("\"None\" is not a valid choice.".to_owned())),
        Some(JVal::Null) => "None".to_owned(),
        Some(JVal::Bool(true)) => "True".to_owned(),
        Some(JVal::Bool(false)) => "False".to_owned(),
        Some(JVal::Num(number)) => number.py_string(),
        Some(JVal::Str(text)) => text
            .to_clean_string()
            .unwrap_or_else(|| text.to_lossy_string()),
        Some(other) => match py_str(other) {
            Ok(rendered) => rendered
                .to_clean_string()
                .unwrap_or_else(|| rendered.to_lossy_string()),
            Err(_) => return Err(FieldFail::ServerError),
        },
    };
    match key.as_str() {
        "20" => Ok(20),
        "15" => Ok(15),
        "5" => Ok(5),
        _ => Err(FieldFail::one(format!("\"{key}\" is not a valid choice."))),
    }
}

/// `JSONField` (`view_props` / `default_props` / `preferences`): any JSON
/// value passes through as-is; HTML-form strings are `json.loads`-parsed.
fn validate_json_field(
    value: Option<&JVal>,
    file: Option<&shared_body::FilePart>,
    is_html: bool,
) -> Result<Value, FieldFail> {
    const INVALID: &str = "Value must be valid JSON.";
    if file.is_some() {
        return Err(FieldFail::one(INVALID.to_owned()));
    }
    match value {
        None => Err(FieldFail::one("This field may not be null.".to_owned())),
        Some(JVal::Null) => Err(FieldFail::one("This field may not be null.".to_owned())),
        Some(JVal::Str(text)) if is_html => {
            let clean = text
                .to_clean_string()
                .unwrap_or_else(|| text.to_lossy_string());
            match crate::v1_cycles_modules::json_cpython::parse_json_text(&clean) {
                Ok(parsed) => Ok(to_serde_publish(&parsed)),
                Err(JsonFail::Message(_)) => Err(FieldFail::one(INVALID.to_owned())),
                Err(JsonFail::Recursion) => Err(FieldFail::ServerError),
            }
        }
        Some(other) => Ok(to_serde_publish(other)),
    }
}

/// `FloatField` (`sort_order`): booleans coerce (`True` → `1.0`),
/// strings parse with the CPython `float()` grammar, over-large integer
/// literals overflow.
fn validate_sort_order(
    value: Option<&JVal>,
    file: Option<&shared_body::FilePart>,
) -> Result<f64, FieldFail> {
    const INVALID: &str = "A valid number is required.";
    if file.is_some() {
        return Err(FieldFail::one(INVALID.to_owned()));
    }
    match value {
        None => Err(FieldFail::one("This field may not be null.".to_owned())),
        Some(JVal::Null) => Err(FieldFail::one("This field may not be null.".to_owned())),
        Some(JVal::Bool(true)) => Ok(1.0),
        Some(JVal::Bool(false)) => Ok(0.0),
        Some(JVal::Num(number)) => {
            if number.is_float() {
                Ok(number.as_f64())
            } else {
                let parsed: f64 = number.text().parse().unwrap_or(f64::INFINITY);
                if parsed.is_infinite() {
                    Err(FieldFail::one(
                        "Integer value too large to convert to float".to_owned(),
                    ))
                } else {
                    Ok(parsed)
                }
            }
        }
        Some(JVal::Str(text)) => {
            if text.len_chars() > 1000 {
                return Err(FieldFail::one("String value too large.".to_owned()));
            }
            let clean = text
                .to_clean_string()
                .ok_or_else(|| FieldFail::one(INVALID.to_owned()))?;
            py_float(&clean).ok_or_else(|| FieldFail::one(INVALID.to_owned()))
        }
        Some(_) => Err(FieldFail::one(INVALID.to_owned())),
    }
}

/// CPython `float(str)`: whitespace strip, digit-pair underscores,
/// case-insensitive `inf`/`infinity`/`nan`, else the decimal grammar.
fn py_float(text: &str) -> Option<f64> {
    let stripped = text.trim_matches(is_python_space);
    if stripped.is_empty() {
        return None;
    }
    // Underscores must sit between ASCII digits.
    let chars: Vec<char> = stripped.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '_' {
            let left = index.checked_sub(1).and_then(|i| chars.get(i));
            let right = chars.get(index + 1);
            if !left.is_some_and(|c| c.is_ascii_digit())
                || !right.is_some_and(|c| c.is_ascii_digit())
            {
                return None;
            }
        }
    }
    let compact: String = chars.into_iter().filter(|ch| *ch != '_').collect();
    let (negative, rest) = match compact.strip_prefix(['+', '-']) {
        Some(tail) => (compact.starts_with('-'), tail),
        None => (false, compact.as_str()),
    };
    let lowered = rest.to_ascii_lowercase();
    if lowered == "inf" || lowered == "infinity" {
        return Some(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if lowered == "nan" {
        // `float()` never yields a signed NaN payload here.
        return Some(f64::NAN);
    }
    // Reject non-ASCII digits (CPython accepts some; untestable corner).
    if !rest.is_ascii() {
        return None;
    }
    let parsed: f64 = compact.parse().ok()?;
    Some(parsed)
}

/// `BooleanField` (`is_active`): strict sets — `1`/`1.0` true,
/// `0`/`-0`/`0.0` false, strings lowered before set membership.
fn validate_is_active(
    value: Option<&JVal>,
    file: Option<&shared_body::FilePart>,
) -> Result<bool, FieldFail> {
    const INVALID: &str = "Must be a valid boolean.";
    if file.is_some() {
        return Err(FieldFail::one(INVALID.to_owned()));
    }
    match value {
        None => Err(FieldFail::one("This field may not be null.".to_owned())),
        Some(JVal::Null) => Err(FieldFail::one("This field may not be null.".to_owned())),
        Some(JVal::Bool(flag)) => Ok(*flag),
        Some(JVal::Num(number)) => {
            if number.is_float() {
                let float = number.as_f64();
                if float == 1.0 {
                    Ok(true)
                } else if float == 0.0 {
                    Ok(false)
                } else {
                    Err(FieldFail::one(INVALID.to_owned()))
                }
            } else {
                match number.text() {
                    "1" => Ok(true),
                    "0" | "-0" => Ok(false),
                    _ => Err(FieldFail::one(INVALID.to_owned())),
                }
            }
        }
        Some(JVal::Str(text)) => {
            let lowered = text
                .to_clean_string()
                .map(|clean| clean.to_ascii_lowercase());
            match lowered.as_deref() {
                Some("1" | "t" | "y" | "yes" | "true" | "on") => Ok(true),
                Some("0" | "f" | "n" | "no" | "false" | "off") => Ok(false),
                _ => Err(FieldFail::one(INVALID.to_owned())),
            }
        }
        Some(_) => Err(FieldFail::one(INVALID.to_owned())),
    }
}

/// `PrimaryKeyRelatedField` (`created_by` / `updated_by`, nullable):
/// booleans are an incorrect type, UUID-shaped input checks existence,
/// everything else is the curly-quote UUID message.
async fn validate_fk(
    value: Option<&JVal>,
    file: Option<&shared_body::FilePart>,
    pool: &sqlx::PgPool,
) -> Result<Option<uuid::Uuid>, FieldFail> {
    if let Some(part) = file {
        return Err(uuid_invalid(&part.filename));
    }
    match value {
        None | Some(JVal::Null) => Ok(None),
        Some(JVal::Bool(_)) => Err(FieldFail::one(
            "Incorrect type. Expected pk value, received bool.".to_owned(),
        )),
        Some(JVal::Str(text)) => {
            let clean = text
                .to_clean_string()
                .unwrap_or_else(|| text.to_lossy_string());
            match clean.parse::<uuid::Uuid>() {
                Ok(id) => fk_exists(pool, id, &clean).await,
                Err(_) => Err(uuid_invalid(&clean)),
            }
        }
        Some(JVal::Num(number)) => {
            if number.is_float() {
                Err(uuid_invalid(&number.py_string()))
            } else {
                match number.text().parse::<u128>() {
                    Ok(raw) => fk_exists(pool, uuid::Uuid::from_u128(raw), number.text()).await,
                    Err(_) => Err(uuid_invalid(number.text())),
                }
            }
        }
        Some(other) => match py_str(other) {
            Ok(rendered) => {
                let text = rendered
                    .to_clean_string()
                    .unwrap_or_else(|| rendered.to_lossy_string());
                Err(uuid_invalid(&text))
            }
            Err(_) => Err(FieldFail::ServerError),
        },
    }
}

fn uuid_invalid(input: &str) -> FieldFail {
    FieldFail::one(format!("\u{201c}{input}\u{201d} is not a valid UUID."))
}

/// The `queryset.get(pk=...)` existence probe: a miss is the invalid-pk
/// message echoing the caller's original input.
async fn fk_exists(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
    echo: &str,
) -> Result<Option<uuid::Uuid>, FieldFail> {
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(r#"SELECT u.id FROM users u WHERE u.id = $1"#)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| FieldFail::ServerError)?;
    if row.is_some() {
        Ok(Some(id))
    } else {
        Err(FieldFail::one(format!(
            "Invalid pk \"{echo}\" - object does not exist."
        )))
    }
}

// ---------------------------------------------------------------------------
// `parse_datetime` (`deleted_at` input grammar)
// ---------------------------------------------------------------------------

/// Split an ISO-8601 input into (naive wall time, optional fixed offset),
/// following `datetime.fromisoformat` (3.11+) with the `datetime_re`
/// fallback: week dates, basic + extended calendars, 1–2 digit fields,
/// any-length fractions (truncated to 6), `Z`/numeric offsets.
fn split_member_datetime(
    text: &str,
) -> Option<(chrono::NaiveDateTime, Option<chrono::FixedOffset>)> {
    // Week date: extended `YYYY-Www[-d]` or basic `YYYYWww[d]`
    // (strict widths, like `fromisoformat`).
    let bytes = text.as_bytes();
    if bytes.len() >= 7
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && (bytes[4] == b'W' || (bytes.len() >= 8 && bytes[4] == b'-' && bytes[5] == b'W'))
    {
        return split_week_datetime(text);
    }
    // Calendar date: extended `YYYY-MM-DD` (1–2 digit M/D via the
    // fallback) or basic `YYYYMMDD`.
    let (date, rest) = split_calendar_date(text)?;
    if rest.is_empty() {
        // Date-only input means midnight (`fromisoformat` accepts it).
        return Some((date.and_hms_opt(0, 0, 0)?, None));
    }
    // Exactly one separator character (Django accepts `T`/space here;
    // `fromisoformat` accepts any single non-digit — the realistic
    // inputs are `T`/`t`/space, which is what this port matches).
    let mut chars = rest.char_indices();
    let (_, separator) = chars.next()?;
    if !matches!(separator, 'T' | 't' | ' ') {
        return None;
    }
    let time_text = chars.as_str();
    if time_text.is_empty() {
        return None;
    }
    let (time, offset) = split_time_offset(time_text)?;
    Some((chrono::NaiveDateTime::new(date, time), offset))
}

/// Week dates: extended `YYYY-Www[-d]` or basic `YYYYWww[d]`, then
/// the optional time part. The weekday defaults to Monday; dashes
/// must not mix (`2024-W012` and `2024W01-2` both fail).
fn split_week_datetime(text: &str) -> Option<(chrono::NaiveDateTime, Option<chrono::FixedOffset>)> {
    if !text.is_ascii() {
        return None;
    }
    let bytes = text.as_bytes();
    if text.len() < 4 || !bytes[0..4].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let extended = bytes.get(4) == Some(&b'-');
    if extended && bytes.get(5) != Some(&b'W') {
        return None;
    }
    if !extended && bytes.get(4) != Some(&b'W') {
        return None;
    }
    let year: i32 = text[0..4].parse().ok()?;
    let (week_text, tail) = if extended {
        if text.len() < 8 {
            return None;
        }
        (&text[6..8], &text[8..])
    } else {
        if text.len() < 7 {
            return None;
        }
        (&text[5..7], &text[7..])
    };
    if !week_text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let week: u32 = week_text.parse().ok()?;
    let (weekday, rest) = if extended {
        match tail.strip_prefix('-') {
            Some(day_tail) => {
                let digit = day_tail.chars().next()?;
                if !digit.is_ascii_digit() {
                    return None;
                }
                (digit.to_digit(10)?, &day_tail[1..])
            }
            None => (1, tail),
        }
    } else {
        match tail.chars().next() {
            Some(digit) if digit.is_ascii_digit() => (digit.to_digit(10)?, &tail[1..]),
            _ => (1, tail),
        }
    };
    if !weekday_is_valid(weekday) {
        return None;
    }
    let date = chrono::NaiveDate::from_isoywd_opt(year, week, day_from_u32(weekday))?;
    if rest.is_empty() {
        return Some((date.and_hms_opt(0, 0, 0)?, None));
    }
    let mut chars = rest.char_indices();
    let (_, separator) = chars.next()?;
    if !matches!(separator, 'T' | 't' | ' ') {
        return None;
    }
    let (time, offset) = split_time_offset(chars.as_str())?;
    Some((chrono::NaiveDateTime::new(date, time), offset))
}

fn weekday_is_valid(day: u32) -> bool {
    (1..=7).contains(&day)
}

fn day_from_u32(day: u32) -> chrono::Weekday {
    match day {
        1 => chrono::Weekday::Mon,
        2 => chrono::Weekday::Tue,
        3 => chrono::Weekday::Wed,
        4 => chrono::Weekday::Thu,
        5 => chrono::Weekday::Fri,
        6 => chrono::Weekday::Sat,
        _ => chrono::Weekday::Sun,
    }
}

/// Extended `YYYY-MM-DD` (1–2 digit month/day) or basic `YYYYMMDD`,
/// returning the date plus the unparsed remainder.
fn split_calendar_date(text: &str) -> Option<(chrono::NaiveDate, &str)> {
    let bytes = text.as_bytes();
    if bytes.len() >= 8 && bytes[0..4].iter().all(|b| b.is_ascii_digit()) {
        // Basic form has no dashes in the first 8 chars.
        if bytes.len() >= 8 && bytes[4..8].iter().all(|b| b.is_ascii_digit()) {
            let year: i32 = text[0..4].parse().ok()?;
            let month: u32 = text[4..6].parse().ok()?;
            let day: u32 = text[6..8].parse().ok()?;
            let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
            return Some((date, &text[8..]));
        }
        if bytes.len() >= 10 && bytes[4] == b'-' {
            let dash2 = bytes[5..].iter().position(|b| *b == b'-')? + 5;
            let month_text = &text[5..dash2];
            if !(1..=2).contains(&month_text.len())
                || !month_text.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let day_start = dash2 + 1;
            let day_end = text[day_start..]
                .find(|ch: char| !ch.is_ascii_digit())
                .map(|pos| day_start + pos)
                .unwrap_or(text.len());
            let day_text = &text[day_start..day_end];
            if !(1..=2).contains(&day_text.len()) {
                return None;
            }
            let year: i32 = text[0..4].parse().ok()?;
            let month: u32 = month_text.parse().ok()?;
            let day: u32 = day_text.parse().ok()?;
            let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
            return Some((date, &text[day_end..]));
        }
    }
    None
}

/// `HH[:MM[:SS[.ffffff]]]` plus the optional `Z`/numeric offset.
/// Single-digit fields come from the `datetime_re` fallback; hour-only
/// input means `:00:00`.
fn split_time_offset(text: &str) -> Option<(chrono::NaiveTime, Option<chrono::FixedOffset>)> {
    // Peel an optional trailing offset first (`Z` or `±HH[:]MM[:SS]`).
    let (core, offset) = split_tz_suffix(text)?;
    // Basic time: `HHMMSS` / `HHMM` / `HH`, optional fraction.
    if !core.contains(':') {
        let (digits, micro) = match core.split_once(['.', ',']) {
            None => (core, 0),
            Some((head, frac)) => {
                if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let mut padded = frac.to_owned();
                padded.truncate(6);
                while padded.len() < 6 {
                    padded.push('0');
                }
                (head, padded.parse::<u32>().ok()?)
            }
        };
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let (hour, minute, second) = match digits.len() {
            2 => (digits.parse::<u32>().ok()?, 0, 0),
            4 => (
                digits[0..2].parse::<u32>().ok()?,
                digits[2..4].parse::<u32>().ok()?,
                0,
            ),
            6 => (
                digits[0..2].parse::<u32>().ok()?,
                digits[2..4].parse::<u32>().ok()?,
                digits[4..6].parse::<u32>().ok()?,
            ),
            _ => return None,
        };
        let time = chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micro)?;
        return Some((time, offset));
    }
    let mut parts = core.split(':');
    let hour_text = parts.next()?;
    if !(1..=2).contains(&hour_text.len()) || !hour_text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hour: u32 = hour_text.parse().ok()?;
    let (minute, second, micro) = match parts.next() {
        None => (0, 0, 0),
        Some(minute_text) => {
            if !(1..=2).contains(&minute_text.len())
                || !minute_text.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let minute: u32 = minute_text.parse().ok()?;
            match parts.next() {
                None => (minute, 0, 0),
                Some(second_text) => {
                    if parts.next().is_some() {
                        return None;
                    }
                    let (sec_text, micro) = match second_text.split_once(['.', ',']) {
                        None => (second_text, 0),
                        Some((sec, frac)) => {
                            if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
                                return None;
                            }
                            let mut digits = frac.to_owned();
                            digits.truncate(6);
                            while digits.len() < 6 {
                                digits.push('0');
                            }
                            (sec, digits.parse::<u32>().ok()?)
                        }
                    };
                    if !(1..=2).contains(&sec_text.len())
                        || !sec_text.bytes().all(|b| b.is_ascii_digit())
                    {
                        return None;
                    }
                    (minute, sec_text.parse::<u32>().ok()?, micro)
                }
            }
        }
    };
    let time = chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micro)?;
    Some((time, offset))
}

/// Peel `Z` / `±HH[:]MM` (seconds accepted and ignored, like
/// `fromisoformat`) off the end of a time string.
fn split_tz_suffix(text: &str) -> Option<(&str, Option<chrono::FixedOffset>)> {
    if let Some(core) = text.strip_suffix(['Z', 'z']) {
        // Lowercase `z` fails `fromisoformat` AND the fallback regex.
        if text.ends_with('z') {
            return None;
        }
        return Some((core, Some(chrono::FixedOffset::east_opt(0)?)));
    }
    // A numeric offset starts at the last `+`/`-` past the date part; the
    // time core itself never contains one.
    let mut split_at = None;
    for (index, ch) in text.char_indices() {
        if (ch == '+' || ch == '-') && index > 0 {
            split_at = Some(index);
        }
    }
    let Some(at) = split_at else {
        return Some((text, None));
    };
    let (core, zone) = text.split_at(at);
    if core.is_empty() {
        return None;
    }
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let digits: String = zone[1..].chars().filter(|ch| *ch != ':').collect();
    if !digits.is_ascii() {
        return None;
    }
    // `±HH` short form or `±HHMM` / `±HH:MM`, optional ignored `:SS`.
    let (hour_text, minute_text) = if digits.len() == 2 {
        (&digits[0..2], "00")
    } else if digits.len() == 4 || digits.len() == 6 {
        (&digits[0..2], &digits[2..4])
    } else {
        return None;
    };
    if !hour_text.bytes().all(|b| b.is_ascii_digit())
        || !minute_text.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let hours: i32 = hour_text.parse().ok()?;
    let minutes: i32 = minute_text.parse().ok()?;
    // Neither minutes nor seconds are range-checked (`+02:75` is
    // +11700s, `+02:00:99` is +7299s): `timedelta` normalizes, and
    // only the strict ±24h bound rejects — exactly `east_opt`.
    let seconds: i32 = if digits.len() == 6 {
        if !digits[4..6].bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits[4..6].parse().ok()?
    } else {
        0
    };
    let total = hours * 3600 + minutes * 60 + seconds;
    let offset = chrono::FixedOffset::east_opt(sign * total)?;
    Some((core, Some(offset)))
}

/// Full `deleted_at` input pipeline: grammar, then `enforce_timezone`
/// (naive → request zone with the DST-gap arm, aware → converted).
/// The offset in force immediately before a DST gap: Python `fold=0`
/// semantics for `MappedLocalTime::None` wall times. Steps back in
/// 10-minute increments (gaps run 1h typically, 24h for date-line
/// skips); `None` is unreachable on real zone data.
fn offset_before_gap(tz: &Tz, naive: &chrono::NaiveDateTime) -> Option<chrono::FixedOffset> {
    let mut probe = *naive;
    for _ in 0..200 {
        probe = probe.checked_sub_signed(chrono::Duration::minutes(10))?;
        match tz.offset_from_local_datetime(&probe) {
            chrono::MappedLocalTime::Single(offset) => return Some(offset.fix()),
            chrono::MappedLocalTime::Ambiguous(first, _) => return Some(first.fix()),
            chrono::MappedLocalTime::None => {}
        }
    }
    None
}

/// A parsed `deleted_at` input: the stored instant, plus the echo
/// override for the DST-gap arm only (`None` everywhere else, where the
/// normalized render is already correct).
struct ParsedMemberDatetime {
    utc: chrono::DateTime<chrono::Utc>,
    echo: Option<String>,
}

/// Render a gap wall time exactly like DRF renders the in-memory
/// `make_aware` value: wall parts plus the pre-transition offset, with
/// `+00:00` rewritten to `Z`. DRF's `enforce_timezone` calls
/// `value.astimezone(field_timezone)`, and CPython's `astimezone`
/// returns `self` unchanged when `tzinfo is tz` — which always holds
/// here, since `make_aware` attached the cached `ZoneInfo` object —
/// so the echo is the non-normalized local form, never the instant
/// re-rendered (`2026-03-08T02:30:00-05:00`, not `03:30:00-04:00`).
fn render_gap_echo(naive: &chrono::NaiveDateTime, offset: &chrono::FixedOffset) -> String {
    let suffix = offset
        .from_local_datetime(naive)
        .single()
        .map(|local| local.format("%:z").to_string())
        .unwrap_or_else(|| "+00:00".to_owned());
    let suffix = if suffix == "+00:00" { "Z" } else { &suffix };
    let base = naive.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = naive.and_utc().timestamp_subsec_nanos();
    if nanos == 0 {
        format!("{base}{suffix}")
    } else if nanos.is_multiple_of(1000) {
        format!("{base}.{:06}{suffix}", nanos / 1000)
    } else {
        format!("{base}.{nanos:09}{suffix}")
    }
}

fn parse_member_datetime(
    text: &str,
    tz: &Tz,
    tz_name: &str,
) -> Result<ParsedMemberDatetime, FieldFail> {
    let (naive, offset) = split_member_datetime(text).ok_or_else(datetime_invalid)?;
    match offset {
        Some(fixed) => {
            let aware = fixed
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(datetime_invalid)?;
            // `astimezone` overflows past year 9999 (or before year 1):
            // checked add, since `naive_local` would panic there.
            let zoned = aware.with_timezone(tz);
            let shift =
                chrono::Duration::seconds(i64::from(zoned.offset().fix().local_minus_utc()));
            let local = zoned
                .naive_utc()
                .checked_add_signed(shift)
                .ok_or_else(|| FieldFail::one("Datetime value out of range.".to_owned()))?;
            if !(1..=9999).contains(&local.date().year()) {
                return Err(FieldFail::one("Datetime value out of range.".to_owned()));
            }
            Ok(ParsedMemberDatetime {
                utc: zoned.with_timezone(&chrono::Utc),
                echo: None,
            })
        }
        None => match tz.from_local_datetime(&naive) {
            chrono::MappedLocalTime::Single(local) => Ok(ParsedMemberDatetime {
                utc: local.with_timezone(&chrono::Utc),
                echo: None,
            }),
            chrono::MappedLocalTime::Ambiguous(first, _) => Ok(ParsedMemberDatetime {
                utc: first.with_timezone(&chrono::Utc),
                echo: None,
            }),
            // Spring-forward gaps: Django's `make_aware` under ZoneInfo
            // is a bare `replace(tzinfo)` (never raises) and DRF's
            // `valid_datetime` only rejects ambiguous times, so gap wall
            // times 200 and store with the pre-transition (fold=0)
            // offset — never the `make_aware` 400. The PATCH echo must
            // render that same non-normalized local form (D6).
            chrono::MappedLocalTime::None => {
                let offset = offset_before_gap(tz, &naive).ok_or_else(|| {
                    FieldFail::one(format!("Invalid datetime for the timezone \"{tz_name}\"."))
                })?;
                let utc = naive
                    .checked_sub_signed(chrono::Duration::seconds(i64::from(
                        offset.local_minus_utc(),
                    )))
                    .map(|naive_utc| {
                        chrono::DateTime::from_naive_utc_and_offset(naive_utc, chrono::Utc)
                    })
                    .ok_or_else(|| FieldFail::one("Datetime value out of range.".to_owned()))?;
                let echo = render_gap_echo(&naive, &offset);
                Ok(ParsedMemberDatetime {
                    utc,
                    echo: Some(echo),
                })
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn into_response(result: HandlerResult) -> Response {
    match result {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

// --- list ---------------------------------------------------------------

async fn member_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    into_response(member_list_inner(&state, &slug, &project_raw, extension).await)
}

async fn member_list_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let gate = gate_for("GET", "workspaces/<slug>/projects/<project_id>/members/")?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    // `TimezoneMixin.initial` activates before the `@allow_permission`
    // action gate: unknown zones 400 even where the gate would 403.
    let tz = actor_timezone(&pool, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    // Custom `list()` queryset (`member.py:159-166`): the get_queryset
    // scope plus `is_active` and the member's live workspace membership
    // (no `deleted_at` guard on the joined rows — QUIRK-fanout, so no
    // dedup either), `Meta.ordering` newest-first.
    let rows = sqlx::query(
        r#"SELECT pm.id, pm.role, pm.member_id, pm.project_id, pm.created_at
           FROM project_members pm
           INNER JOIN workspaces w ON w.id = pm.workspace_id
           INNER JOIN users u ON u.id = pm.member_id
           INNER JOIN workspace_members wm ON wm.member_id = u.id
           INNER JOIN workspaces w2 ON w2.id = wm.workspace_id
           WHERE pm.deleted_at IS NULL AND w.slug = $1 AND pm.project_id = $2
             AND NOT u.is_bot AND pm.is_active AND wm.is_active AND w2.slug = $1
           ORDER BY pm.created_at DESC"#,
    )
    .bind(slug)
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: uuid::Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
        let role: i16 = row.try_get("role").map_err(|_| Denial::ServerError)?;
        let member_id: Option<uuid::Uuid> =
            row.try_get("member_id").map_err(|_| Denial::ServerError)?;
        let project: uuid::Uuid = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
        let created_at: chrono::DateTime<chrono::Utc> =
            row.try_get("created_at").map_err(|_| Denial::ServerError)?;
        items.push(member_role_fields_to_json(
            &id.to_string(),
            i64::from(role),
            member_id.map(|id| id.to_string()).as_deref(),
            &project.to_string(),
            &crate::serializer::render_datetime_in(&created_at, &tz),
        ));
    }
    Ok(ok_json(format!("[{}]", items.join(","))))
}

// --- create --------------------------------------------------------------

/// Dedup key for the `member_roles` dict: tagged by JSON type with
/// Python equality inside each tag (notably `True == 1` and `1 == 1.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum MemberKey {
    Null,
    Int(String),
    Str(String),
}

/// Canonicalize one `member_id` for dict dedup: unhashable composites
/// (list/dict) are the `TypeError` 500; floats integral to an int fold
/// onto the int key, exactly like CPython dicts.
fn member_dict_key(value: &JVal) -> Result<MemberKey, Denial> {
    match value {
        JVal::Null => Ok(MemberKey::Null),
        JVal::Bool(true) => Ok(MemberKey::Int("1".to_owned())),
        JVal::Bool(false) => Ok(MemberKey::Int("0".to_owned())),
        JVal::Num(number) => {
            if number.is_float() {
                let float = number.as_f64();
                if float.is_finite() && float.fract() == 0.0 {
                    // Integral floats hash with ints (`1.0 == 1`).
                    Ok(MemberKey::Int(format!("{}", float.trunc() as i128)))
                } else {
                    Ok(MemberKey::Str(format!("float:{}", number.py_string())))
                }
            } else {
                // Normalize sign + leading zeros (`-0` → `0`).
                let negative = number.text().starts_with('-');
                let digits = number.text().trim_start_matches(['+', '-']);
                let digits = digits.trim_start_matches('0');
                let digits = if digits.is_empty() { "0" } else { digits };
                if negative && digits != "0" {
                    Ok(MemberKey::Int(format!("-{digits}")))
                } else {
                    Ok(MemberKey::Int(digits.to_owned()))
                }
            }
        }
        JVal::Str(text) => Ok(MemberKey::Str(
            text.to_clean_string()
                .unwrap_or_else(|| text.to_lossy_string()),
        )),
        JVal::Array(_) | JVal::Object(_) => Err(Denial::ServerError),
    }
}

/// Python `value in [choices]` for small-int choice lists (guards only
/// ever compare against role ints).
fn int_choice_contains(value: &JVal, choices: &[i32]) -> bool {
    match value {
        JVal::Bool(flag) => {
            let as_int = i32::from(*flag);
            choices.contains(&as_int)
        }
        JVal::Num(number) => {
            if number.is_float() {
                let float = number.as_f64();
                float.is_finite()
                    && float.fract() == 0.0
                    && choices.iter().any(|choice| f64::from(*choice) == float)
            } else {
                number
                    .text()
                    .parse::<i128>()
                    .map(|int| choices.iter().any(|choice| i128::from(*choice) == int))
                    .unwrap_or(false)
            }
        }
        _ => false,
    }
}

async fn member_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    headers: HeaderMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> Response {
    into_response(
        member_create_inner(&state, &slug, &project_raw, &headers, &body, extension).await,
    )
}

async fn member_create_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    headers: &HeaderMap,
    body: &[u8],
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let gate = gate_for("POST", "workspaces/<slug>/projects/<project_id>/members/")?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    // `TimezoneMixin.initial` activates before the `@allow_permission`
    // action gate: unknown zones 400 even where the gate would 403.
    let tz = actor_timezone(&pool, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    let data = negotiate_data(headers, body)?;
    // `request.data.get("members", [])`: only mappings have `.get`.
    let members = match &data.value {
        JVal::Object(object) => {
            if data.files.contains_key("members") {
                // The merged value is the upload, which has no `.get`.
                return Err(Denial::ServerError);
            }
            match object.get("members") {
                None => Vec::new(),
                Some(JVal::Array(items)) => items.clone(),
                Some(JVal::Object(map)) if map.is_empty() => Vec::new(),
                Some(JVal::Str(text)) if text.is_empty() => Vec::new(),
                _ => {
                    // `len()` works on non-empty str/dict (then `.get`
                    // fails per item); every other type fails `len()`.
                    match object.get("members") {
                        Some(JVal::Object(_)) | Some(JVal::Str(_)) => {
                            return Err(Denial::ServerError)
                        }
                        _ => return Err(Denial::ServerError),
                    }
                }
            }
        }
        _ => return Err(Denial::ServerError),
    };
    // The project lookup precedes the empty check (`member.py:52-59`).
    let project_rows: Vec<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
        r#"SELECT p.id, p.workspace_id FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (_project_uuid, workspace_uuid) = one(project_rows)?;
    member_create_write(
        state,
        &pool,
        slug,
        &project_id,
        &workspace_uuid,
        &user_id,
        &tz,
        members,
    )
    .await
}

/// How a `role` value travels to Postgres on the bulk paths.
/// `SmallIntegerField.get_prep_value` runs `int(value)` in Python
/// before anything reaches Postgres, so the port normalizes the same
/// way: bools become 0/1 (a 201 — `True` stores role 1, verified
/// live), floats truncate toward zero, strings parse (or the
/// `ValueError` 500), composites are the `TypeError` 500. Out-of-int2
/// values fail at the DB exactly like Django's (huge ints ride `f64`,
/// still out of range). `None` (missing role on an existing row) is
/// SQL NULL → the `IntegrityError` 400.
#[derive(Debug, PartialEq)]
enum RoleBind {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
}

fn role_bind(value: &JVal) -> RoleBind {
    match value {
        JVal::Null => RoleBind::Null,
        JVal::Bool(flag) => RoleBind::Int(i64::from(*flag)),
        JVal::Num(number) => {
            if number.is_float() {
                let float = number.as_f64();
                if float.is_finite() {
                    // `int()` truncates toward zero; `as` saturates
                    // huge magnitudes to `i64::{MAX, MIN}`, still out
                    // of int2 range → the same 500.
                    RoleBind::Int(float as i64)
                } else {
                    // NaN/inf: `int()` raises → 500; Postgres errors
                    // on the float8→int2 conversion the same way.
                    RoleBind::Float(float)
                }
            } else {
                match number.as_i64() {
                    Some(int) => RoleBind::Int(int),
                    None => RoleBind::Float(number.as_f64()),
                }
            }
        }
        JVal::Str(_) => match python_int(value) {
            Some(int) if int <= i128::from(i64::MAX) && int >= i128::from(i64::MIN) => {
                RoleBind::Int(int as i64)
            }
            Some(int) => RoleBind::Float(int as f64),
            // Unparseable text: a typed TEXT param has no cast to
            // smallint (42804) — the 500 `int()`'s `ValueError` maps
            // to. (The content is irrelevant: every TEXT fails.)
            None => RoleBind::Text(to_serde_publish(value).to_string()),
        },
        JVal::Array(_) | JVal::Object(_) => RoleBind::Text(to_serde_publish(value).to_string()),
    }
}

/// The guard-loop workspace lookup (`member.py:72-83`):
/// `WorkspaceMember.objects.get(workspace__slug, member, is_active)`.
/// Missing rows are the 404 branch, unparseable ids the `ValidationError`
/// 400; `member=None` never matches (Django queries `IS NULL`).
async fn guard_workspace_role(
    pool: &sqlx::PgPool,
    slug: &str,
    member: &JVal,
) -> Result<i32, Denial> {
    let id = match member {
        JVal::Null => return Err(Denial::NotFound),
        JVal::Bool(true) => uuid::Uuid::from_u128(1),
        JVal::Bool(false) => uuid::Uuid::from_u128(0),
        JVal::Num(number) => {
            if number.is_float() {
                return Err(body_error("Please provide valid detail"));
            }
            match number.text().parse::<u128>() {
                Ok(raw) => uuid::Uuid::from_u128(raw),
                Err(_) => return Err(body_error("Please provide valid detail")),
            }
        }
        JVal::Str(text) => {
            let clean = text
                .to_clean_string()
                .unwrap_or_else(|| text.to_lossy_string());
            match clean.parse::<uuid::Uuid>() {
                Ok(id) => id,
                Err(_) => return Err(body_error("Please provide valid detail")),
            }
        }
        JVal::Array(_) | JVal::Object(_) => return Err(Denial::ServerError),
    };
    let rows: Vec<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(id)
    .bind(slug)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    one(rows).map(|row| i32::from(row.0))
}

/// How a request `member_id` reaches Postgres past the guard loop:
/// Django's `UUIDField` prep maps bools and ints via `UUID(int=...)`
/// and parses strings; floats, unparseables and `None` never survive
/// the guard, so `None` here is unreachable.
fn request_member_uuid(member: &JVal) -> Option<uuid::Uuid> {
    match member {
        JVal::Bool(flag) => Some(uuid::Uuid::from_u128(u128::from(*flag))),
        JVal::Num(number) => {
            if number.is_float() {
                None
            } else {
                number
                    .text()
                    .parse::<u128>()
                    .ok()
                    .map(uuid::Uuid::from_u128)
            }
        }
        JVal::Str(text) => text
            .to_clean_string()
            .unwrap_or_else(|| text.to_lossy_string())
            .parse::<uuid::Uuid>()
            .ok(),
        JVal::Null | JVal::Array(_) | JVal::Object(_) => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn member_create_write(
    state: &AppState,
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    tz: &Tz,
    members: Vec<JVal>,
) -> HandlerResult {
    if members.is_empty() {
        return Err(body_error("At least one member is required"));
    }
    // `member_roles` (`member.py:62-66`): first position wins, last role
    // wins; non-dict items and unhashable ids are the 500 arms.
    let mut roles: Vec<(MemberKey, JVal, JVal)> = Vec::new();
    for item in &members {
        let JVal::Object(object) = item else {
            return Err(Denial::ServerError);
        };
        let member_id = object.get("member_id").cloned().unwrap_or(JVal::Null);
        let role = object.get("role").cloned().unwrap_or(JVal::Null);
        let key = member_dict_key(&member_id)?;
        match roles.iter_mut().find(|entry| entry.0 == key) {
            Some(entry) => entry.2 = role,
            None => roles.push((key, member_id, role)),
        }
    }
    // Workspace-role-vs-project-role guards (`member.py:69-83`).
    for (_, member_id, role) in &roles {
        let workspace_role = guard_workspace_role(pool, slug, member_id).await?;
        if workspace_role == 20 && int_choice_contains(role, &[5, 15]) {
            return Err(body_error(
                "You cannot add a user with role lower than the workspace role",
            ));
        }
        if workspace_role == 5 && int_choice_contains(role, &[15, 20]) {
            return Err(body_error(
                "You cannot add a user with role higher than the workspace role",
            ));
        }
    }
    // Every surviving id maps to a UUID here exactly as in the guard
    // loop. Bools and ints ride `UUID(int=...)`; they then miss the
    // canonical-string role lookup (the `KeyError` 400) unless a
    // canonical-string twin was requested alongside.
    let mut member_ids: Vec<uuid::Uuid> = Vec::with_capacity(roles.len());
    for (_, member_id, _) in &roles {
        member_ids.push(request_member_uuid(member_id).ok_or(Denial::ServerError)?);
    }
    // Reactivation (`member.py:86-97`): existing rows iterate newest
    // first; the role lookup keys on `str(member_id)` — a request id in
    // non-canonical case misses and is the `KeyError` 400.
    let existing: Vec<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
        r#"SELECT pm.id, pm.member_id FROM project_members pm
           WHERE pm.project_id = $1 AND pm.member_id = ANY($2) AND pm.deleted_at IS NULL
           ORDER BY pm.created_at DESC"#,
    )
    .bind(project_id)
    .bind(&member_ids)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut role_by_member: HashMap<String, &JVal> = HashMap::new();
    for (key, _, role) in &roles {
        if let MemberKey::Str(text) = key {
            role_by_member.insert(text.clone(), role);
        }
    }
    let mut updates: Vec<(uuid::Uuid, RoleBind)> = Vec::new();
    for (id, member_id) in &existing {
        let Some(role) = role_by_member.get(&member_id.to_string()) else {
            return Err(body_error("The required key does not exist."));
        };
        updates.push((*id, role_bind(role)));
    }
    // `bulk_update(["is_active", "role"])` (`member.py:97`): one atomic
    // statement with Django's `CAST(CASE ... AS smallint)` shape
    // (QUIRK-bulk-cast); skipped entirely when there is nothing to
    // reactivate.
    if !updates.is_empty() {
        let mut sql = String::from("UPDATE project_members SET role = CAST(CASE id");
        let mut index = 0;
        for _ in &updates {
            index += 1;
            let id_arg = index;
            index += 1;
            let role_arg = index;
            sql.push_str(&format!(" WHEN ${id_arg} THEN ${role_arg}"));
        }
        sql.push_str(" END AS smallint), is_active = CASE id");
        for _ in &updates {
            index += 1;
            let id_arg = index;
            index += 1;
            let active_arg = index;
            sql.push_str(&format!(" WHEN ${id_arg} THEN ${active_arg}"));
        }
        sql.push_str(" END WHERE id IN (");
        for (position, _) in updates.iter().enumerate() {
            if position > 0 {
                sql.push_str(", ");
            }
            index += 1;
            sql.push_str(&format!("${index}"));
        }
        sql.push(')');
        let mut query = sqlx::query(&sql);
        for (id, role) in &updates {
            query = query.bind(id);
            query = match role {
                RoleBind::Null => query.bind(None::<String>),
                RoleBind::Int(int) => query.bind(int),
                RoleBind::Float(float) => query.bind(float),
                RoleBind::Text(text) => query.bind(text),
            };
        }
        for (id, _) in &updates {
            query = query.bind(id).bind(true);
        }
        for (id, _) in &updates {
            query = query.bind(id);
        }
        query.execute(pool).await.map_err(db_denial)?;
    }
    // Per-user `MIN(sort_order)` over the workspace (`member.py:99-111`).
    let sort_rows: Vec<(uuid::Uuid, Option<f64>)> = sqlx::query_as(
        r#"SELECT pup.user_id, MIN(pup.sort_order) FROM project_user_properties pup
           JOIN workspaces w ON w.id = pup.workspace_id
           WHERE w.slug = $1 AND pup.user_id = ANY($2) AND pup.deleted_at IS NULL
           GROUP BY pup.user_id"#,
    )
    .bind(slug)
    .bind(&member_ids)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sort_min: HashMap<String, f64> = sort_rows
        .into_iter()
        .filter_map(|(user, min)| min.map(|min| (user.to_string(), min)))
        .collect();
    // Append rows in request order, duplicates included
    // (`member.py:113-138`): `bulk_create` skips conflicts, so extra
    // rows for reactivated members simply do not insert.
    // JSONB columns: bind the `Value` itself — psycopg sends jsonb
    // params, while TEXT has no assignment cast to jsonb (42804).
    let member_view = project_member::default_props();
    let member_prefs = project_member::default_preferences();
    let props_filters =
        pidash_db::app_issues::models_core::project_user_property::default_filters();
    let props_display_filters =
        pidash_db::app_issues::models_core::project_user_property::default_display_filters();
    let props_display_properties =
        pidash_db::app_issues::models_core::project_user_property::default_display_properties();
    let props_rich = project_user_property::default_rich_filters();
    let props_prefs = project_user_property::default_preferences();
    let mut new_members: Vec<(uuid::Uuid, uuid::Uuid, RoleBind, f64)> = Vec::new();
    for item in &members {
        let JVal::Object(object) = item else {
            return Err(Denial::ServerError);
        };
        let raw_member_id = object.get("member_id").cloned().unwrap_or(JVal::Null);
        let member_id = request_member_uuid(&raw_member_id).ok_or(Denial::ServerError)?;
        // `member.get("role", 5)`: absent means Guest, present-but-null
        // stays NULL (the `IntegrityError` 400 on insert).
        let role = match object.get("role") {
            None => RoleBind::Int(5),
            Some(value) => role_bind(value),
        };
        // `str(member.get("member_id"))`: the RAW request value, so a
        // bool/int id (`True`, `15`) never matches a canonical-uuid
        // sort row and falls back to 65535.
        let raw_key = py_str(&raw_member_id).map_err(|_| Denial::ServerError)?;
        let raw_key = raw_key
            .to_clean_string()
            .unwrap_or_else(|| raw_key.to_lossy_string());
        let sort_order = project_member::sort_order_on_create(sort_min.get(&raw_key).copied());
        new_members.push((uuid::Uuid::new_v4(), member_id, role, sort_order));
    }
    if !new_members.is_empty() {
        let mut sql = String::from(
            "INSERT INTO project_members (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, project_id, workspace_id, member_id, comment, role, view_props, default_props, preferences, sort_order, is_active) VALUES ",
        );
        let mut index = 0;
        for (position, _) in new_members.iter().enumerate() {
            if position > 0 {
                sql.push_str(", ");
            }
            sql.push('(');
            for column in 0..16 {
                if column > 0 {
                    sql.push_str(", ");
                }
                index += 1;
                sql.push_str(&format!("${index}"));
            }
            sql.push(')');
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        let mut query = sqlx::query(&sql);
        for (id, member_id, role, _) in &new_members {
            // `auto_now_add` / `auto_now` each call `now()` (the fixture
            // shows the two stamps microseconds apart).
            let created_at = chrono::Utc::now();
            let updated_at = chrono::Utc::now();
            query = query
                .bind(created_at)
                .bind(updated_at)
                .bind(None::<uuid::Uuid>)
                .bind(None::<uuid::Uuid>)
                .bind(None::<chrono::DateTime<chrono::Utc>>)
                .bind(id)
                .bind(project_id)
                .bind(workspace_id)
                .bind(member_id)
                .bind(None::<String>);
            query = match role {
                RoleBind::Null => query.bind(None::<String>),
                RoleBind::Int(int) => query.bind(int),
                RoleBind::Float(float) => query.bind(float),
                RoleBind::Text(text) => query.bind(text),
            };
            query = query
                .bind(&member_view)
                .bind(&member_view)
                .bind(&member_prefs)
                .bind(project_member::DEFAULT_SORT_ORDER)
                .bind(true);
        }
        query.execute(pool).await.map_err(db_denial)?;
        let mut sql = String::from(
            "INSERT INTO project_user_properties (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, project_id, workspace_id, user_id, filters, display_filters, display_properties, rich_filters, preferences, sort_order) VALUES ",
        );
        let mut index = 0;
        for (position, _) in new_members.iter().enumerate() {
            if position > 0 {
                sql.push_str(", ");
            }
            sql.push('(');
            for column in 0..15 {
                if column > 0 {
                    sql.push_str(", ");
                }
                index += 1;
                sql.push_str(&format!("${index}"));
            }
            sql.push(')');
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        let mut query = sqlx::query(&sql);
        for (_, member_id, _, sort_order) in &new_members {
            let created_at = chrono::Utc::now();
            let updated_at = chrono::Utc::now();
            query = query
                .bind(created_at)
                .bind(updated_at)
                .bind(None::<uuid::Uuid>)
                .bind(None::<uuid::Uuid>)
                .bind(None::<chrono::DateTime<chrono::Utc>>)
                .bind(uuid::Uuid::new_v4())
                .bind(project_id)
                .bind(workspace_id)
                .bind(member_id)
                .bind(&props_filters)
                .bind(&props_display_filters)
                .bind(&props_display_properties)
                .bind(&props_rich)
                .bind(&props_prefs)
                .bind(sort_order);
        }
        query.execute(pool).await.map_err(db_denial)?;
    }
    // Re-read newest-first for the emits + 201 body (`member.py:140-150`).
    type CreatedRow = (
        uuid::Uuid,
        i16,
        Option<uuid::Uuid>,
        uuid::Uuid,
        chrono::DateTime<chrono::Utc>,
    );
    let created: Vec<CreatedRow> = sqlx::query_as(
        r#"SELECT pm.id, pm.role, pm.member_id, pm.project_id, pm.created_at
           FROM project_members pm
           WHERE pm.project_id = $1 AND pm.member_id = ANY($2) AND pm.deleted_at IS NULL
           ORDER BY pm.created_at DESC"#,
    )
    .bind(project_id)
    .bind(&member_ids)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    // The app origin resolves after the writes (a missing setting 500s
    // with the members already persisted, like Python's).
    let site = current_site(state)?;
    let mut items = Vec::with_capacity(created.len());
    for (id, role, member_id, project, created_at) in &created {
        let emit = tasks::ProjectAddUserEmailEmit {
            current_site: site.clone(),
            project_member_id: id.to_string(),
            invitor_id: user_id.to_string(),
        };
        enqueue_email(pool, &emit).await;
        items.push(member_role_to_json(
            &id.to_string(),
            i64::from(*role),
            member_id.map(|id| id.to_string()).as_deref(),
            &project.to_string(),
            &crate::serializer::render_datetime_in(created_at, tz),
        ));
    }
    Ok(created_json(format!("[{}]", items.join(","))))
}

// --- retrieve ------------------------------------------------------------

async fn member_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> Response {
    // The `<uuid:pk>` converter rejects before auth (Django resolver).
    if !is_django_uuid(&pk_raw) {
        return proxy_through(state, method, uri, headers, body).await;
    }
    into_response(member_retrieve_inner(&state, &slug, &project_raw, &pk_raw, extension).await)
}

async fn member_retrieve_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    pk_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let gate = gate_for(
        "GET",
        "workspaces/<slug>/projects/<project_id>/members/<pk>/",
    )?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    // `TimezoneMixin.initial` activates before the `@allow_permission`
    // action gate: unknown zones 400 even where the gate would 403.
    let tz = actor_timezone(&pool, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    let pk = pk_raw
        .parse::<uuid::Uuid>()
        .map_err(|_| Denial::ServerError)?;
    // The requesting membership (`member.py:177-180`) 404s first.
    let requester: Vec<(uuid::Uuid, i16)> = sqlx::query_as(
        r#"SELECT pm.id, pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (_, requester_role) = one(requester)?;
    // The target (`.filter(...).first()`, `member.py:181-196`): explicit
    // miss body, newest-first.
    let rows = sqlx::query(&format!(
        "{FULL_SELECT} WHERE pm.id = $1 AND pm.project_id = $2 AND w.slug = $3
             AND NOT u.is_bot AND pm.is_active AND pm.deleted_at IS NULL
             ORDER BY pm.created_at DESC LIMIT 1"
    ))
    .bind(pk)
    .bind(project_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let assets = fetch_assets(&pool, &rows).await?;
    let Some(row) = rows.into_iter().next() else {
        return Err(Denial::MemberNotFound);
    };
    let member = decode_full(&row, &tz, &assets)?;
    // Admin-vs-guest switch on the REQUESTING role (`member.py:198-203`).
    if i32::from(requester_role) > pidash_auth::permissions::ROLE_GUEST {
        Ok(ok_json(render_full_admin(&member)))
    } else {
        Ok(ok_json(member_role_fields_to_json(
            &member.id,
            member.role,
            member.member_id.as_deref(),
            &member.project_id,
            &member.created_at,
        )))
    }
}

// --- partial_update -------------------------------------------------------

async fn member_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> Response {
    if !is_django_uuid(&pk_raw) {
        return proxy_through(state, method, uri, headers, body).await;
    }
    into_response(
        member_partial_update_inner(
            &state,
            &slug,
            &project_raw,
            &pk_raw,
            &headers,
            &body,
            extension,
        )
        .await,
    )
}

async fn member_partial_update_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    pk_raw: &str,
    headers: &HeaderMap,
    body: &[u8],
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let gate = gate_for(
        "PATCH",
        "workspaces/<slug>/projects/<project_id>/members/<pk>/",
    )?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    // `TimezoneMixin.initial` activates before the `@allow_permission`
    // action gate: unknown zones 400 even where the gate would 403.
    let tz = actor_timezone(&pool, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    let tz_name = actor_timezone_name(&pool, &user_id).await?;
    let pk = pk_raw
        .parse::<uuid::Uuid>()
        .map_err(|_| Denial::ServerError)?;
    // Target, then the target's workspace role, then the self-role 400
    // (any field — BUG-self-patch), then the requesting membership
    // (`member.py:211-227`) — all before the body parses.
    let rows = sqlx::query(&format!(
        "{FULL_SELECT} WHERE pm.id = $1 AND pm.project_id = $2 AND w.slug = $3
         AND pm.is_active AND pm.deleted_at IS NULL"
    ))
    .bind(pk)
    .bind(project_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let assets = fetch_assets(&pool, &rows).await?;
    let target_row = one(rows)?;
    let target = decode_full(&target_row, &tz, &assets)?;
    let target_member_id = target
        .member_id
        .as_ref()
        .and_then(|id| id.parse::<uuid::Uuid>().ok())
        .ok_or(Denial::NotFound)?;
    let ws_rows: Vec<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(target_member_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (ws_role,) = one(ws_rows)?;
    let ws_role = i32::from(ws_role);
    let is_workspace_admin = ws_role == pidash_auth::permissions::ROLE_ADMIN;
    if target_member_id == user_id && !is_workspace_admin {
        return Err(body_error("You cannot update your own role"));
    }
    let requester: Vec<(uuid::Uuid, i16)> = sqlx::query_as(
        r#"SELECT pm.id, pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (_, requester_role) = one(requester)?;
    let requester_role = i32::from(requester_role);
    // First `request.data` touch: parse (and 415/400) errors land here,
    // after the lookups (`member.py:229`).
    let data = negotiate_data(headers, body)?;
    // `"role" in request.data` on non-dict bodies: `in` over lists and
    // strings, `TypeError` (500) everywhere else.
    let role_present;
    let role_value: Option<JVal>;
    match &data.value {
        JVal::Object(object) => {
            // Uploads win the text-then-files merge; `int()` on the
            // upload raises in the gate stage below.
            let file_present = data.files.contains_key("role");
            role_present = object.contains_key("role") || file_present;
            role_value = if file_present {
                None
            } else {
                object.get("role").cloned()
            };
        }
        JVal::Array(items) => {
            let contains = items
                .iter()
                .any(|item| matches!(item, JVal::Str(text) if text.eq_str("role")));
            if !contains {
                return Err(non_dict_body("list"));
            }
            // `"role" in [...]` is true: the 403 matrix runs before the
            // `.get` AttributeError 500 (`member.py:230-245`), so fall
            // through with a missing value.
            role_present = true;
            role_value = None;
        }
        JVal::Str(text) => {
            if !text.contains_str("role") {
                return Err(non_dict_body("str"));
            }
            // Substring `"role"`: same 403-then-500 order as lists.
            role_present = true;
            role_value = None;
        }
        JVal::Num(_) | JVal::Bool(_) | JVal::Null => return Err(Denial::ServerError),
    }
    if role_present {
        // The 403 matrix (`member.py:230-250`): inline 403s with the
        // `{"error": ...}` shape, not the gate's `detail` body.
        if requester_role < pidash_auth::permissions::ROLE_ADMIN && !is_workspace_admin {
            return Err(forbidden_error(
                "You do not have permission to update roles",
            ));
        }
        if i128::from(target.role) >= i128::from(requester_role) && !is_workspace_admin {
            return Err(forbidden_error(
                "You cannot update the role of a member with a role equal to or higher than your own",
            ));
        }
        let new_role = match role_value.as_ref() {
            Some(value) => python_int(value).ok_or(Denial::ServerError)?,
            None => return Err(Denial::ServerError),
        };
        if new_role >= i128::from(requester_role) && !is_workspace_admin {
            return Err(forbidden_error(
                "You cannot assign a role equal to or higher than your own",
            ));
        }
        if ws_role == pidash_auth::permissions::ROLE_GUEST && (new_role == 15 || new_role == 20) {
            return Err(body_error(
                "You cannot add a user with role higher than the workspace role",
            ));
        }
    }
    // The serializer stage (`member.py:252-265`).
    let JVal::Object(object) = &data.value else {
        return Err(Denial::ServerError);
    };
    let patch = match validate_member_patch(&pool, object, &data.files, data.is_html, &tz, &tz_name)
        .await
    {
        Ok(patch) => patch,
        Err(Ok(errors)) => {
            return Err(Denial::BadJson(Value::Object(errors)));
        }
        Err(Err(denial)) => return Err(denial),
    };
    // `save()`: `updated_at`/`updated_by` stamp like `BaseModel.save`;
    // note `save()` overwrites any validated `updated_by` with the
    // request user, while a validated `created_by` persists.
    let saved_at = chrono::Utc::now();
    let mut sets = vec![
        "updated_at = $1".to_owned(),
        "updated_by_id = $2".to_owned(),
    ];
    let mut index = 2;
    if patch.deleted_at.is_some() {
        index += 1;
        sets.push(format!("deleted_at = ${index}"));
    }
    if patch.comment.is_some() {
        index += 1;
        sets.push(format!("comment = ${index}"));
    }
    if patch.role.is_some() {
        index += 1;
        sets.push(format!("role = ${index}"));
    }
    if patch.view_props.is_some() {
        index += 1;
        sets.push(format!("view_props = ${index}"));
    }
    if patch.default_props.is_some() {
        index += 1;
        sets.push(format!("default_props = ${index}"));
    }
    if patch.preferences.is_some() {
        index += 1;
        sets.push(format!("preferences = ${index}"));
    }
    if patch.sort_order.is_some() {
        index += 1;
        sets.push(format!("sort_order = ${index}"));
    }
    if patch.is_active.is_some() {
        index += 1;
        sets.push(format!("is_active = ${index}"));
    }
    if patch.created_by.is_some() {
        index += 1;
        sets.push(format!("created_by_id = ${index}"));
    }
    index += 1;
    let id_arg = index;
    let sql = format!(
        "UPDATE project_members SET {} WHERE id = ${id_arg}",
        sets.join(", ")
    );
    let mut query = sqlx::query(&sql).bind(saved_at).bind(user_id);
    if let Some(deleted_at) = patch.deleted_at.as_ref() {
        query = query.bind(deleted_at.as_ref().map(|parsed| parsed.utc));
    }
    if let Some(comment) = patch.comment.as_ref() {
        query = query.bind(comment);
    }
    if let Some(role) = patch.role {
        query = query.bind(role as i16);
    }
    // JSONB columns bind the `Value` (TEXT has no cast to jsonb).
    if let Some(view_props) = patch.view_props.as_ref() {
        query = query.bind(view_props);
    }
    if let Some(default_props) = patch.default_props.as_ref() {
        query = query.bind(default_props);
    }
    if let Some(preferences) = patch.preferences.as_ref() {
        query = query.bind(preferences);
    }
    if let Some(sort_order) = patch.sort_order {
        query = query.bind(sort_order);
    }
    if let Some(is_active) = patch.is_active {
        query = query.bind(is_active);
    }
    if let Some(created_by) = patch.created_by {
        query = query.bind(created_by);
    }
    query = query.bind(pk);
    query.execute(&pool).await.map_err(db_denial)?;
    // The response renders the in-memory instance (validated values in
    // request order for JSONB), never a DB re-read.
    let mut merged = target;
    merged.updated_at = crate::serializer::render_datetime_in(&saved_at, &tz);
    merged.updated_by = Some(user_id.to_string());
    if let Some(deleted_at) = patch.deleted_at {
        merged.deleted_at = deleted_at.map(|parsed| {
            parsed
                .echo
                .unwrap_or_else(|| crate::serializer::render_datetime_in(&parsed.utc, &tz))
        });
    }
    if let Some(comment) = patch.comment {
        merged.comment = comment;
    }
    if let Some(role) = patch.role {
        merged.role = role;
    }
    if let Some(view_props) = patch.view_props {
        merged.view_props = view_props;
    }
    if let Some(default_props) = patch.default_props {
        merged.default_props = default_props;
    }
    if let Some(preferences) = patch.preferences {
        merged.preferences = preferences;
    }
    if let Some(sort_order) = patch.sort_order {
        merged.sort_order = sort_order;
    }
    if let Some(is_active) = patch.is_active {
        merged.is_active = is_active;
    }
    if let Some(created_by) = patch.created_by {
        merged.created_by = created_by.map(|id| id.to_string());
    }
    Ok(ok_json(render_full(&merged)))
}

/// The serializer's non-dict body (`serializers.py:504-510`).
fn non_dict_body(datatype: &str) -> Denial {
    let mut errors = serde_json::Map::new();
    errors.insert(
        "non_field_errors".to_owned(),
        Value::Array(vec![Value::String(format!(
            "Invalid data. Expected a dictionary, but got {datatype}."
        ))]),
    );
    Denial::BadJson(Value::Object(errors))
}

async fn actor_timezone_name(pool: &sqlx::PgPool, user_id: &uuid::Uuid) -> Result<String, Denial> {
    let row: Option<(String,)> =
        sqlx::query_as(r#"SELECT u.user_timezone FROM users u WHERE u.id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

// --- destroy -------------------------------------------------------------

async fn member_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> Response {
    if !is_django_uuid(&pk_raw) {
        return proxy_through(state, method, uri, headers, body).await;
    }
    into_response(member_destroy_inner(&state, &slug, &project_raw, &pk_raw, extension).await)
}

async fn member_destroy_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    pk_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let gate = gate_for(
        "DELETE",
        "workspaces/<slug>/projects/<project_id>/members/<pk>/",
    )?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    // `TimezoneMixin.initial` activates before the `@allow_permission`
    // action gate: unknown zones 400 even where the gate would 403.
    let _tz = actor_timezone(&pool, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    let pk = pk_raw
        .parse::<uuid::Uuid>()
        .map_err(|_| Denial::ServerError)?;
    // Target, then the requesting membership (`member.py:273-283`).
    let targets: Vec<(uuid::Uuid, i16)> = sqlx::query_as(
        r#"SELECT pm.id, pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           JOIN users u ON u.id = pm.member_id
           WHERE pm.id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND NOT u.is_bot AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(pk)
    .bind(project_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (target_id, target_role) = one(targets)?;
    let requesters: Vec<(uuid::Uuid, i16)> = sqlx::query_as(
        r#"SELECT pm.id, pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (requester_id, requester_role) = one(requesters)?;
    // Self-remove 400, then the higher-role 400 — not 403
    // (QUIRK-destroy-400, reachable via the ws-admin gate override).
    if target_id == requester_id {
        return Err(body_error(
            "You cannot remove yourself from the workspace. Please use leave workspace",
        ));
    }
    if i32::from(requester_role) < i32::from(target_role) {
        return Err(body_error(
            "You cannot remove a user having role higher than you",
        ));
    }
    sqlx::query(
        r#"UPDATE project_members SET is_active = false, updated_at = $1, updated_by_id = $2
           WHERE id = $3"#,
    )
    .bind(chrono::Utc::now())
    .bind(user_id)
    .bind(target_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// --- leave ---------------------------------------------------------------

async fn member_leave(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    into_response(member_leave_inner(&state, &slug, &project_raw, extension).await)
}

async fn member_leave_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let gate = gate_for(
        "POST",
        "workspaces/<slug>/projects/<project_id>/members/leave/",
    )?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    // `TimezoneMixin.initial` activates before the `@allow_permission`
    // action gate: unknown zones 400 even where the gate would 403.
    let _tz = actor_timezone(&pool, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    let rows: Vec<(uuid::Uuid, i16)> = sqlx::query_as(
        r#"SELECT pm.id, pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (member_id, role) = one(rows)?;
    // Only-admin 400: `not (admins > 1)` with self an admin
    // (`member.py:310-318`).
    if i32::from(role) == pidash_auth::permissions::ROLE_ADMIN {
        let admins: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM project_members pm
               JOIN workspaces w ON w.id = pm.workspace_id
               WHERE w.slug = $1 AND pm.project_id = $2 AND pm.role = $3
               AND pm.is_active AND pm.deleted_at IS NULL"#,
        )
        .bind(slug)
        .bind(project_id)
        .bind(pidash_auth::permissions::ROLE_ADMIN)
        .fetch_one(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
        if !(admins.0 > 1) {
            return Err(body_error(
                "You cannot leave the project as your the only admin of the project you will have to either delete the project or create an another admin",
            ));
        }
    }
    sqlx::query(
        r#"UPDATE project_members SET is_active = false, updated_at = $1, updated_by_id = $2
           WHERE id = $3"#,
    )
    .bind(chrono::Utc::now())
    .bind(user_id)
    .bind(member_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// --- me ------------------------------------------------------------------

async fn member_me(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    into_response(member_me_inner(&state, &slug, &project_raw, extension).await)
}

async fn member_me_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let gate = gate_for(
        "GET",
        "workspaces/<slug>/projects/<project_id>/project-members/me/",
    )?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    // `TimezoneMixin.initial` activates before the `@allow_permission`
    // action gate: unknown zones 400 even where the gate would 403.
    let tz = actor_timezone(&pool, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    // Own row (`endpoint:333-336`): 404 when not an active member.
    let rows = sqlx::query(&format!(
        "{FULL_SELECT} WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
         AND pm.is_active AND pm.deleted_at IS NULL"
    ))
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let assets = fetch_assets(&pool, &rows).await?;
    let row = one(rows)?;
    let member = decode_full(&row, &tz, &assets)?;
    Ok(ok_json(render_full(&member)))
}

// --- roles ---------------------------------------------------------------

async fn project_roles(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Response {
    into_response(project_roles_inner(&state, &slug, extension).await)
}

async fn project_roles_inner(
    state: &AppState,
    slug: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    // No `project_id` kwarg, so no rewrite; the class-level
    // `WorkspaceUserPermission` gate answers its own 403 body.
    let membership: Vec<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let role = membership.first().map(|row| i32::from(row.0));
    let unfiltered_admin: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT wm.id FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2 AND wm.role = $3 AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .bind(pidash_auth::permissions::ROLE_ADMIN)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let facts = pidash_auth::permissions::workspace::WorkspaceFacts {
        workspace: pidash_types::WorkspaceId::from(slug),
        authenticated: true,
        has_admin_or_member_role: role.is_some_and(|role| role == 20 || role == 15),
        has_admin_role: role.is_some_and(|role| role == 20),
        is_member: role.is_some(),
        is_admin_unfiltered: !unfiltered_admin.is_empty(),
    };
    match gates::decide_workspace_user_gate(&gates::tenant_context(slug), &facts) {
        gates::GateOutcome::Allow => {}
        gates::GateOutcome::Deny => return Err(Denial::ClassDenied),
        gates::GateOutcome::Unauthenticated => return Err(Denial::Unauthorized),
    }
    // Class-level `WorkspaceUserPermission` runs inside
    // `super().initial()`, before `TimezoneMixin` activates — so here,
    // unlike the `@allow_permission` actions, the gate stays first.
    let _tz = actor_timezone(&pool, &user_id).await?;
    // `{project_id: role}` newest-first (`endpoint:349-353`): duplicate
    // rows fold last-wins at first position, exactly like the dict
    // comprehension — insertion order, never key-sorted.
    // No `deleted_at` guard on the joined rows: the ORM join never
    // applies the related default manager (verified in the generated
    // SQL — only the base `project_members.deleted_at IS NULL`
    // appears), so deleted workspace rows still fan out here and fold
    // in the dict below (QUIRK-fanout, same as `member_list`).
    let rows: Vec<(uuid::Uuid, i16)> = sqlx::query_as(
        r#"SELECT pm.project_id, pm.role FROM project_members pm
           INNER JOIN users u ON u.id = pm.member_id
           INNER JOIN workspace_members wm ON wm.member_id = u.id
           INNER JOIN workspaces w ON w.id = wm.workspace_id
           INNER JOIN workspaces w2 ON w2.id = pm.workspace_id
           WHERE pm.deleted_at IS NULL AND pm.is_active AND wm.is_active
             AND w.slug = $1 AND pm.member_id = $2 AND w2.slug = $1
           ORDER BY pm.created_at DESC"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut dict = serde_json::Map::new();
    for (project_id, role) in &rows {
        dict.insert(
            project_id.to_string(),
            Value::Number(i32::from(*role).into()),
        );
    }
    Ok(ok_json(
        serde_json::to_string(&Value::Object(dict)).expect("roles dict serializes"),
    ))
}

// --- preferences ----------------------------------------------------------

async fn preference_get(
    State(state): State<AppState>,
    Path((slug, project_raw, member_raw)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> Response {
    if !is_django_uuid(&member_raw) {
        return proxy_through(state, method, uri, headers, body).await;
    }
    into_response(preference_get_inner(&state, &slug, &project_raw, &member_raw, extension).await)
}

async fn preference_get_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    member_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let gate = gate_for(
        "GET",
        "workspaces/<slug>/projects/<project_id>/preferences/member/<member_id>/",
    )?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    // `TimezoneMixin.initial` activates before the `@allow_permission`
    // action gate: unknown zones 400 even where the gate would 403.
    let tz = actor_timezone(&pool, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    let member_id = member_raw
        .parse::<uuid::Uuid>()
        .map_err(|_| Denial::ServerError)?;
    // No `is_active` filter on this lookup (`endpoint:363-365`).
    let rows = sqlx::query(&format!(
        "{FULL_SELECT} WHERE pm.project_id = $1 AND pm.member_id = $2 AND w.slug = $3
         AND pm.deleted_at IS NULL"
    ))
    .bind(project_id)
    .bind(member_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let assets = fetch_assets(&pool, &rows).await?;
    let row = one(rows)?;
    let member = decode_full(&row, &tz, &assets)?;
    let pref_row = ser_member::ProjectMemberPreferenceRow {
        preferences: &member.preferences,
        project_id: &member.project_id,
        member_id: member.member_id.as_deref(),
        workspace_id: &member.workspace_id,
    };
    let view = ser_member::preference_to_representation(&pref_row);
    Ok(ok_json(
        serde_json::to_string(&view).expect("preference view serializes"),
    ))
}

async fn preference_patch(
    State(state): State<AppState>,
    Path((slug, project_raw, member_raw)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: Bytes,
) -> Response {
    if !is_django_uuid(&member_raw) {
        return proxy_through(state, method, uri, headers, body).await;
    }
    into_response(
        preference_patch_inner(
            &state,
            &slug,
            &project_raw,
            &member_raw,
            &headers,
            &body,
            extension,
        )
        .await,
    )
}

async fn preference_patch_inner(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    member_raw: &str,
    headers: &HeaderMap,
    body: &[u8],
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = actor_user_id(extension)?;
    let project_id = resolve_project_id(&pool, slug, project_raw).await?;
    let gate = gate_for(
        "PATCH",
        "workspaces/<slug>/projects/<project_id>/preferences/member/<member_id>/",
    )?;
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate)).await?;
    // `TimezoneMixin.initial` activates before the `@allow_permission`
    // action gate: unknown zones 400 even where the gate would 403.
    let tz = actor_timezone(&pool, &user_id).await?;
    check_gate(gate, slug, &facts)?;
    let member_id = member_raw
        .parse::<uuid::Uuid>()
        .map_err(|_| Denial::ServerError)?;
    // The lookup precedes the body parse (`endpoint:372-374` runs
    // before `is_valid` touches `request.data`).
    let rows = sqlx::query(&format!(
        "{FULL_SELECT} WHERE pm.project_id = $1 AND pm.member_id = $2 AND w.slug = $3
         AND pm.deleted_at IS NULL"
    ))
    .bind(project_id)
    .bind(member_id)
    .bind(slug)
    .fetch_all(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let assets = fetch_assets(&pool, &rows).await?;
    let row = one(rows)?;
    let member = decode_full(&row, &tz, &assets)?;
    let data = negotiate_data(headers, body)?;
    // `{"preferences": request.data}` (`endpoint:376-380`): a form body
    // wraps the `QueryDict`, which is not JSON-serializable.
    if data.is_html {
        let mut errors = serde_json::Map::new();
        errors.insert(
            "preferences".to_owned(),
            Value::Array(vec![Value::String("Value must be valid JSON.".to_owned())]),
        );
        return Err(Denial::BadJson(Value::Object(errors)));
    }
    if matches!(data.value, JVal::Null) {
        let mut errors = serde_json::Map::new();
        errors.insert(
            "preferences".to_owned(),
            Value::Array(vec![Value::String(
                "This field may not be null.".to_owned(),
            )]),
        );
        return Err(Denial::BadJson(Value::Object(errors)));
    }
    // `validate_preferences` (`serializers/project.py:226-240`): the
    // shallow merge — non-dict stored values or patches are the
    // `AttributeError`/`TypeError` 500s.
    let Value::Object(mut stored) = member.preferences.clone() else {
        return Err(Denial::ServerError);
    };
    let patch_value = to_serde_publish(&data.value);
    // `dict.update` accepts a vacuous empty string (no-op 200); every
    // other non-dict raises (`ValueError`/`TypeError` → 500). A
    // list-of-pairs would also merge in Python, but no caller sends
    // one — see the workpad.
    let patch_map = match patch_value {
        Value::Object(map) => map,
        Value::String(text) if text.is_empty() => serde_json::Map::new(),
        _ => return Err(Denial::ServerError),
    };
    ser_member::merge_preferences(&mut stored, &patch_map);
    let merged = Value::Object(stored);
    sqlx::query(
        r#"UPDATE project_members SET preferences = $1, updated_at = $2, updated_by_id = $3
           WHERE id = $4"#,
    )
    .bind(&merged)
    .bind(chrono::Utc::now())
    .bind(user_id)
    .bind(
        member
            .id
            .parse::<uuid::Uuid>()
            .map_err(|_| Denial::ServerError)?,
    )
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    // The response renders the in-memory merge (stored order + appended
    // keys), never a DB re-read (`endpoint:383-385`).
    let mut response = serde_json::Map::new();
    response.insert("preferences".to_owned(), merged);
    Ok(ok_json(
        serde_json::to_string(&Value::Object(response)).expect("preference patch serializes"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1_cycles_modules::json_cpython::parse_json_text;

    fn jval(text: &str) -> JVal {
        parse_json_text(text).expect("test json parses")
    }

    fn denial_body(denial: &Denial) -> (StatusCode, String) {
        denial.status_and_body()
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            denial_body(&Denial::Unauthorized),
            (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":"Authentication credentials were not provided."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(&Denial::Forbidden),
            (
                StatusCode::FORBIDDEN,
                r#"{"error":"You don't have the required permissions."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(&Denial::ClassDenied),
            (
                StatusCode::FORBIDDEN,
                r#"{"detail":"You do not have permission to perform this action."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(&Denial::NotFound),
            (
                StatusCode::NOT_FOUND,
                r#"{"error":"The required object does not exist."}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(&Denial::MemberNotFound),
            (
                StatusCode::NOT_FOUND,
                r#"{"error":"Project member not found"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(&Denial::ProjectNotFound),
            (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(&Denial::BadDetail("boom".to_owned())),
            (StatusCode::BAD_REQUEST, r#"{"detail":"boom"}"#.to_owned())
        );
        assert_eq!(
            denial_body(&Denial::BadError("nope".to_owned())),
            (StatusCode::BAD_REQUEST, r#"{"error":"nope"}"#.to_owned())
        );
        assert_eq!(
            denial_body(&Denial::ServerError),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":"Something went wrong please try again later"}"#.to_owned()
            )
        );
        assert_eq!(
            denial_body(&Denial::UnsupportedMediaType("text/plain".to_owned())),
            (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                r#"{"detail":"text/plain"}"#.to_owned()
            )
        );
    }

    #[test]
    fn empty_response_has_no_content_type() {
        let response = empty_response(StatusCode::NO_CONTENT);
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(response.headers().get(header::CONTENT_TYPE).is_none());
    }

    #[test]
    fn python_int_matches_cpython() {
        // Ints, bools, strings.
        assert_eq!(python_int(&jval("15")), Some(15));
        assert_eq!(python_int(&jval("-0")), Some(0));
        assert_eq!(python_int(&jval("true")), Some(1));
        assert_eq!(python_int(&jval("false")), Some(0));
        assert_eq!(python_int(&jval("\"15\"")), Some(15));
        assert_eq!(python_int(&jval("\"  -15  \"")), Some(-15));
        assert_eq!(python_int(&jval("\"1_5\"")), Some(15));
        assert_eq!(python_int(&jval("\"+0\"")), Some(0));
        // Floats truncate toward zero.
        assert_eq!(python_int(&jval("15.9")), Some(15));
        assert_eq!(python_int(&jval("-15.9")), Some(-15));
        assert_eq!(python_int(&jval("1e3")), Some(1000));
        // Unbounded ints saturate by sign past i128.
        assert_eq!(
            python_int(&jval(&format!("\"{}\"", "9".repeat(60)))),
            Some(i128::MAX)
        );
        assert_eq!(
            python_int(&jval(&format!("\"-{}\"", "9".repeat(60)))),
            Some(i128::MIN)
        );
        assert_eq!(python_int(&jval("1e400")), None); // int(inf) raises
        assert!(python_int(&jval("1e30")).is_some_and(|v| v > 0));
        // Raising inputs → None (the 500 arm).
        assert_eq!(python_int(&jval("\"15.0\"")), None);
        assert_eq!(python_int(&jval("\"\"")), None);
        assert_eq!(python_int(&jval("\"abc\"")), None);
        assert_eq!(python_int(&jval("\"1__5\"")), None);
        assert_eq!(python_int(&jval("\"_15\"")), None);
        assert_eq!(python_int(&jval("\"15_\"")), None);
        assert_eq!(python_int(&jval("null")), None);
        assert_eq!(python_int(&jval("[15]")), None);
        assert_eq!(python_int(&jval("{\"a\":1}")), None);
    }

    #[test]
    fn int_choice_follows_python_equality() {
        assert!(int_choice_contains(&jval("15"), &[5, 15]));
        assert!(int_choice_contains(&jval("15.0"), &[5, 15]));
        assert!(!int_choice_contains(&jval("\"15\""), &[5, 15]));
        assert!(!int_choice_contains(&jval("true"), &[5, 15]));
        assert!(!int_choice_contains(&jval("null"), &[5, 15]));
        assert!(!int_choice_contains(&jval("15.5"), &[5, 15]));
        assert!(!int_choice_contains(&jval("20"), &[5, 15]));
    }

    #[test]
    fn member_dict_keys_follow_python_hash_equality() {
        assert_eq!(
            member_dict_key(&jval("true")).unwrap(),
            member_dict_key(&jval("1")).unwrap()
        );
        assert_eq!(
            member_dict_key(&jval("1.0")).unwrap(),
            member_dict_key(&jval("1")).unwrap()
        );
        assert_eq!(
            member_dict_key(&jval("-0")).unwrap(),
            member_dict_key(&jval("0")).unwrap()
        );
        assert_ne!(
            member_dict_key(&jval("-5")).unwrap(),
            member_dict_key(&jval("5")).unwrap()
        );
        assert_ne!(
            member_dict_key(&jval("\"1\"")).unwrap(),
            member_dict_key(&jval("1")).unwrap()
        );
        assert!(member_dict_key(&jval("[1]")).is_err());
        assert!(member_dict_key(&jval("{}")).is_err());
    }

    #[test]
    fn role_choice_messages_quote_stringified_input() {
        let check = |text: &str| match validate_role_choice(Some(&jval(text)), None) {
            Ok(role) => format!("ok:{role}"),
            Err(FieldFail::Messages(messages)) => messages.join(";"),
            Err(FieldFail::ServerError) => "500".to_owned(),
        };
        assert_eq!(check("15"), "ok:15");
        assert_eq!(check("\"15\""), "ok:15");
        assert_eq!(check("7"), "\"7\" is not a valid choice.");
        assert_eq!(check("15.0"), "\"15.0\" is not a valid choice.");
        assert_eq!(check("\" 15 \""), "\" 15 \" is not a valid choice.");
        assert_eq!(check("true"), "\"True\" is not a valid choice.");
        assert_eq!(check("null"), "\"None\" is not a valid choice.");
    }

    #[test]
    fn comment_validation_trims_coerces_and_rejects() {
        let check = |text: &str| match validate_comment(Some(&jval(text)), None) {
            Ok(value) => format!("ok:{value:?}"),
            Err(FieldFail::Messages(messages)) => messages.join(";"),
            Err(FieldFail::ServerError) => "500".to_owned(),
        };
        assert_eq!(check("\"  note  \""), "ok:Some(\"note\")");
        assert_eq!(check("\"\""), "ok:Some(\"\")");
        assert_eq!(check("\"   \""), "ok:Some(\"\")");
        assert_eq!(check("null"), "ok:None");
        assert_eq!(check("123"), "ok:Some(\"123\")");
        assert_eq!(check("true"), "Not a valid string.");
        assert_eq!(check("[\"x\"]"), "Not a valid string.");
        assert_eq!(check("\"a\\u0000b\""), "Null characters are not allowed.");
        assert_eq!(
            check("\"\\ud800\""),
            "Surrogate characters are not allowed: U+D800."
        );
    }

    #[test]
    fn sort_order_validation_matches_float_field() {
        let check = |text: &str| match validate_sort_order(Some(&jval(text)), None) {
            Ok(value) => format!("ok:{value:?}"),
            Err(FieldFail::Messages(messages)) => messages.join(";"),
            Err(FieldFail::ServerError) => "500".to_owned(),
        };
        assert_eq!(check("true"), "ok:1.0");
        assert_eq!(check("false"), "ok:0.0");
        assert_eq!(check("15"), "ok:15.0");
        assert_eq!(check("\"1_0\""), "ok:10.0");
        assert_eq!(check("\"inf\""), "ok:inf");
        assert_eq!(check("null"), "This field may not be null.");
        assert_eq!(check("[1]"), "A valid number is required.");
        assert_eq!(check("\"abc\""), "A valid number is required.");
        assert_eq!(check(&format!("\"{}\"", "9".repeat(50))), "ok:1e50");
        assert_eq!(
            check(&"9".repeat(400)),
            "Integer value too large to convert to float"
        );
        assert_eq!(
            check(&format!("\"{}\"", "1".repeat(1001))),
            "String value too large."
        );
    }

    #[test]
    fn bool_validation_uses_strict_sets() {
        let check = |text: &str| match validate_is_active(Some(&jval(text)), None) {
            Ok(value) => format!("ok:{value}"),
            Err(FieldFail::Messages(messages)) => messages.join(";"),
            Err(FieldFail::ServerError) => "500".to_owned(),
        };
        assert_eq!(check("true"), "ok:true");
        assert_eq!(check("1"), "ok:true");
        assert_eq!(check("1.0"), "ok:true");
        assert_eq!(check("0"), "ok:false");
        assert_eq!(check("-0"), "ok:false");
        assert_eq!(check("0.0"), "ok:false");
        assert_eq!(check("\"Yes\""), "ok:true");
        assert_eq!(check("\"off\""), "ok:false");
        assert_eq!(check("2"), "Must be a valid boolean.");
        assert_eq!(check("1.5"), "Must be a valid boolean.");
        assert_eq!(check("\"\""), "Must be a valid boolean.");
        assert_eq!(check("null"), "This field may not be null.");
    }

    #[test]
    fn json_field_accepts_anything_but_null() {
        let check =
            |text: &str, html: bool| match validate_json_field(Some(&jval(text)), None, html) {
                Ok(value) => format!("ok:{value}"),
                Err(FieldFail::Messages(messages)) => messages.join(";"),
                Err(FieldFail::ServerError) => "500".to_owned(),
            };
        assert_eq!(check("{\"b\":1,\"a\":2}", false), "ok:{\"b\":1,\"a\":2}");
        assert_eq!(check("[1,2]", false), "ok:[1,2]");
        assert_eq!(check("\"s\"", false), "ok:\"s\"");
        assert_eq!(check("null", false), "This field may not be null.");
        assert_eq!(check("\"{\\\"a\\\": 1}\"", true), "ok:{\"a\":1}");
        assert_eq!(check("\"nope\"", true), "Value must be valid JSON.");
    }

    #[test]
    fn member_datetime_grammar_matches_django() {
        let tz: Tz = "UTC".parse().unwrap();
        let parse = |text: &str| {
            parse_member_datetime(text, &tz, "UTC")
                .map(|parsed| parsed.utc.to_rfc3339())
                .map_err(|fail| match fail {
                    FieldFail::Messages(messages) => messages.join(";"),
                    FieldFail::ServerError => "500".to_owned(),
                })
        };
        assert_eq!(
            parse("2024-01-02T03:04:05.123456Z").unwrap(),
            "2024-01-02T03:04:05.123456+00:00"
        );
        assert_eq!(
            parse("2024-01-02 03:04").unwrap(),
            "2024-01-02T03:04:00+00:00"
        );
        // Date-only means midnight; single-digit fields come from the
        // fallback; basic + week calendars parse.
        assert_eq!(parse("2024-01-02").unwrap(), "2024-01-02T00:00:00+00:00");
        assert_eq!(parse("2024-1-2T3:4").unwrap(), "2024-01-02T03:04:00+00:00");
        assert_eq!(
            parse("20240102T030405").unwrap(),
            "2024-01-02T03:04:05+00:00"
        );
        assert_eq!(parse("2024-W01-2").unwrap(), "2024-01-02T00:00:00+00:00");
        // Fractions truncate to 6; offsets convert; lowercase z fails.
        assert_eq!(
            parse("2024-01-02T03:04:05.123456789").unwrap(),
            "2024-01-02T03:04:05.123456+00:00"
        );
        assert_eq!(
            parse("2024-01-02T05:04:05+02:00").unwrap(),
            "2024-01-02T03:04:05+00:00"
        );
        assert!(parse("2024-01-02T03:04:05z").is_err());
        assert!(parse("2024-13-02T03:04").is_err());
        assert!(parse("2024-01-02T24:00").is_err());
        assert!(parse("2024-01-02T03:04:05.").is_err());
        assert!(parse("not-a-date").is_err());
        // Bare weeks default to Monday; basic weeks parse; dashes must
        // not mix (all verified against live `parse_datetime`).
        assert_eq!(parse("2024-W01").unwrap(), "2024-01-01T00:00:00+00:00");
        assert_eq!(
            parse("2024-W01T03:04").unwrap(),
            "2024-01-01T03:04:00+00:00"
        );
        assert_eq!(parse("2024W01").unwrap(), "2024-01-01T00:00:00+00:00");
        assert_eq!(parse("2024W012").unwrap(), "2024-01-02T00:00:00+00:00");
        assert_eq!(
            parse("2024W012T03:04").unwrap(),
            "2024-01-02T03:04:00+00:00"
        );
        assert!(parse("2024-W012").is_err());
        assert!(parse("2024W01-2").is_err());
        // Offset minutes/seconds are not range-checked (`timedelta`
        // normalizes); only the strict ±24h bound rejects.
        assert_eq!(
            parse("2024-01-02T03:04:05+02:75").unwrap(),
            "2024-01-01T23:49:05+00:00"
        );
        assert_eq!(
            parse("2024-01-02T03:04:05+02:00:30").unwrap(),
            "2024-01-02T01:03:35+00:00"
        );
        assert_eq!(
            parse("2024-01-02T03:04:05+02:00:99").unwrap(),
            "2024-01-02T01:02:26+00:00"
        );
        assert!(parse("2024-01-02T03:04:05+24:00").is_err());
        assert!(parse("2024-01-02T03:04:05+23:59:99").is_err());
        // Comma fractions and basic date-only, locked in.
        assert_eq!(
            parse("2024-01-02T03:04:05,123").unwrap(),
            "2024-01-02T03:04:05.123+00:00"
        );
        assert_eq!(parse("20240102").unwrap(), "2024-01-02T00:00:00+00:00");
        // `astimezone` overflow is a 400, never a panic: year 9999 past
        // the far-east line, year 1 before the far-west line.
        let east: Tz = "Pacific/Kiritimati".parse().unwrap();
        // New York's LMT (-4:56:02) is exact back to year 1; some
        // zones (e.g. Pacific/Midway) report garbage pre-1900 offsets
        // from chrono-tz's prebuilt spans — out of scope, see workpad.
        let west: Tz = "America/New_York".parse().unwrap();
        let overflow = |text: &str, zone: &Tz| {
            parse_member_datetime(text, zone, "x").map_err(|fail| match fail {
                FieldFail::Messages(messages) => messages.join(";"),
                FieldFail::ServerError => "500".to_owned(),
            })
        };
        assert_eq!(
            overflow("9999-12-31T23:59:59Z", &east).unwrap_err(),
            "Datetime value out of range."
        );
        assert_eq!(
            overflow("0001-01-01T00:00:00Z", &west).unwrap_err(),
            "Datetime value out of range."
        );
        // Same instants stay valid in UTC.
        assert!(overflow("9999-12-31T23:59:59Z", &tz).is_ok());
        assert!(overflow("0001-01-01T00:00:00Z", &tz).is_ok());
    }

    #[test]
    fn uuid_converter_matches_django_resolver() {
        // Lowercase hyphenated only — every other `Uuid::parse` form
        // resolver-404s (`django/urls/converters.py:26`).
        let good = "abcdef01-2345-6789-abcd-ef0123456789";
        assert!(is_django_uuid(good));
        assert!(!is_django_uuid(&good.to_uppercase()));
        assert!(!is_django_uuid(&format!("{{{good}}}")));
        assert!(!is_django_uuid(&format!("urn:uuid:{good}")));
        assert!(!is_django_uuid(&good.replace('-', "")));
        assert!(!is_django_uuid("44444444-4444-4444-4444-44444444440"));
        assert!(!is_django_uuid("44444444-4444-4444-4444-4444444444022"));
        assert!(!is_django_uuid("g4444444-4444-4444-4444-444444444402"));
        assert!(!is_django_uuid(""));
        assert!(!is_django_uuid("not-a-uuid-at-all-ok-fine"));
    }

    #[test]
    fn dst_gap_uses_pre_transition_offset() {
        // Spring-forward gap wall times 200 with the fold=0 offset
        // (verified against live `to_internal_value`: NY 2024-03-10
        // 02:30 stores 07:30Z); ambiguous times keep the first offset.
        let ny: Tz = "America/New_York".parse().unwrap();
        let parse = |text: &str| {
            parse_member_datetime(text, &ny, "America/New_York")
                .map(|parsed| parsed.utc.to_rfc3339())
                .map_err(|fail| match fail {
                    FieldFail::Messages(messages) => messages.join(";"),
                    FieldFail::ServerError => "500".to_owned(),
                })
        };
        assert_eq!(
            parse("2024-03-10T02:30:00").unwrap(),
            "2024-03-10T07:30:00+00:00"
        );
        assert_eq!(
            parse("2024-03-10T02:00:00").unwrap(),
            "2024-03-10T07:00:00+00:00"
        );
        assert_eq!(
            parse("2024-11-03T01:30:00").unwrap(),
            "2024-11-03T05:30:00+00:00"
        );
        assert_eq!(
            parse("2024-01-02T03:04:05").unwrap(),
            "2024-01-02T08:04:05+00:00"
        );
    }

    #[test]
    fn dst_gap_echo_renders_local_form() {
        // D6: the PATCH echo of a gap wall time is the non-normalized
        // local form, not the stored instant re-rendered (DRF's
        // `astimezone` returns `self` when `tzinfo is tz`, and ZoneInfo
        // objects are cached). Live oracle: NY 2026-03-08 02:30 stores
        // 07:30Z and echoes `02:30:00-05:00`.
        let ny: Tz = "America/New_York".parse().unwrap();
        let london: Tz = "Europe/London".parse().unwrap();
        let parse = |text: &str, zone: &Tz| {
            parse_member_datetime(text, zone, "test").map_err(|fail| match fail {
                FieldFail::Messages(messages) => messages.join(";"),
                FieldFail::ServerError => "500".to_owned(),
            })
        };
        let gap = parse("2026-03-08T02:30:00", &ny).unwrap();
        assert_eq!(gap.utc.to_rfc3339(), "2026-03-08T07:30:00+00:00");
        assert_eq!(gap.echo.as_deref(), Some("2026-03-08T02:30:00-05:00"));
        // Fractions survive the echo; a pre-transition `+00:00` offset
        // (London springs forward out of GMT) takes the `Z` rewrite.
        let micro = parse("2026-03-08T02:30:00.5", &ny).unwrap();
        assert_eq!(
            micro.echo.as_deref(),
            Some("2026-03-08T02:30:00.500000-05:00")
        );
        let zed = parse("2026-03-29T01:30:00", &london).unwrap();
        assert_eq!(zed.utc.to_rfc3339(), "2026-03-29T01:30:00+00:00");
        assert_eq!(zed.echo.as_deref(), Some("2026-03-29T01:30:00Z"));
        // Every other arm echoes `None`: the normalized render is
        // already correct there (naive single/ambiguous local forms
        // equal it; aware inputs normalize through `astimezone`).
        for text in [
            "2024-01-02T03:04:05",
            "2024-11-03T01:30:00",
            "2024-01-02T05:04:05+02:00",
        ] {
            let parsed = parse(text, &ny).unwrap();
            assert_eq!(parsed.echo, None, "{text}");
        }
    }

    #[test]
    fn role_bind_matches_get_prep_value() {
        // `SmallIntegerField.get_prep_value` runs `int(value)` in
        // Python first: bools → 0/1 (live-verified: `True` 201s with
        // role 1), floats truncate, strings parse, composites and
        // unparseable text are the 500 arms (typed TEXT has no cast
        // to smallint).
        assert_eq!(role_bind(&jval("15")), RoleBind::Int(15));
        assert_eq!(role_bind(&jval("-0")), RoleBind::Int(0));
        assert_eq!(role_bind(&jval("15.0")), RoleBind::Int(15));
        assert_eq!(role_bind(&jval("15.9")), RoleBind::Int(15));
        assert_eq!(role_bind(&jval("-15.9")), RoleBind::Int(-15));
        assert_eq!(role_bind(&jval("1e3")), RoleBind::Int(1000));
        assert_eq!(role_bind(&jval("0.5")), RoleBind::Int(0));
        assert_eq!(role_bind(&jval("null")), RoleBind::Null);
        assert_eq!(role_bind(&jval("true")), RoleBind::Int(1));
        assert_eq!(role_bind(&jval("false")), RoleBind::Int(0));
        assert_eq!(role_bind(&jval("\"15\"")), RoleBind::Int(15));
        assert_eq!(role_bind(&jval("\" 15 \"")), RoleBind::Int(15));
        assert_eq!(role_bind(&jval("\"1_5\"")), RoleBind::Int(15));
        assert!(matches!(role_bind(&jval("\"15.5\"")), RoleBind::Text(_)));
        assert!(matches!(role_bind(&jval("\"abc\"")), RoleBind::Text(_)));
        assert!(matches!(role_bind(&jval("[15]")), RoleBind::Text(_)));
        assert!(matches!(
            role_bind(&jval("{\"role\": 15}")),
            RoleBind::Text(_)
        ));
        match role_bind(&jval(&"9".repeat(60))) {
            RoleBind::Float(float) => assert!(float.is_finite() && float > 0.0),
            other => panic!("overflow int binds f64, got {other:?}"),
        }
        match role_bind(&jval(&"9".repeat(400))) {
            RoleBind::Float(float) => assert!(float.is_infinite()),
            other => panic!("huge int saturates to inf, got {other:?}"),
        }
        assert_eq!(role_bind(&jval("1e400")), RoleBind::Float(f64::INFINITY));
        match role_bind(&jval(&format!("\"{}\"", "9".repeat(60)))) {
            RoleBind::Float(float) => assert!(float.is_finite() && float > 1e37),
            other => panic!("huge digit-string binds out-of-range f64, got {other:?}"),
        }
    }

    #[test]
    fn splice_only_replaces_top_level_sort_order() {
        let text = r#"{"view_props":{"sort_order":1},"sort_order":0.0,"x":1}"#.to_owned();
        assert_eq!(
            splice_non_finite_sort_order(text, "Infinity"),
            r#"{"view_props":{"sort_order":1},"sort_order":Infinity,"x":1}"#
        );
    }

    #[test]
    fn gate_rows_exist_for_all_member_routes() {
        for (method, path) in [
            ("GET", "workspaces/<slug>/projects/<project_id>/members/"),
            ("POST", "workspaces/<slug>/projects/<project_id>/members/"),
            (
                "GET",
                "workspaces/<slug>/projects/<project_id>/members/<pk>/",
            ),
            (
                "PATCH",
                "workspaces/<slug>/projects/<project_id>/members/<pk>/",
            ),
            (
                "DELETE",
                "workspaces/<slug>/projects/<project_id>/members/<pk>/",
            ),
            (
                "POST",
                "workspaces/<slug>/projects/<project_id>/members/leave/",
            ),
            (
                "GET",
                "workspaces/<slug>/projects/<project_id>/project-members/me/",
            ),
            (
                "GET",
                "workspaces/<slug>/projects/<project_id>/preferences/member/<member_id>/",
            ),
            (
                "PATCH",
                "workspaces/<slug>/projects/<project_id>/preferences/member/<member_id>/",
            ),
        ] {
            assert!(gate_for(method, path).is_ok(), "{method} {path}");
        }
    }
}
