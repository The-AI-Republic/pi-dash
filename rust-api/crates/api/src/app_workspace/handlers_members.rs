//! Workspace member handlers (D-24, stage 5, PIDASHCONV-616).
//!
//! Ports the member family of `app/views/workspace/member.py:30-265` plus
//! the last-visited endpoint of `app/views/workspace/user.py:69-97` with
//! identical URL paths, status codes and JSON bytes:
//!
//! - `GET workspaces/<slug>/members/` (`WorkSpaceMemberViewSet.list`,
//!   `app/urls/workspace.py:113-117`, route W12)
//! - `GET workspaces/<slug>/project-members/`
//!   (`WorkspaceProjectMemberEndpoint.get`, `workspace.py:118-122`, W13)
//! - `GET`/`PATCH`/`DELETE workspaces/<slug>/members/<pk>/`
//!   (`retrieve`/`partial_update`/`destroy`, `workspace.py:123-127`, W14)
//! - `POST workspaces/<slug>/members/leave/` (`leave`,
//!   `workspace.py:128-132`, W15)
//! - `GET users/last-visited-workspace/`
//!   (`UserLastProjectWithWorkspaceEndpoint.get`, `workspace.py:133-137`,
//!   W16)
//! - `GET workspaces/<slug>/workspace-members/me/`
//!   (`WorkspaceMemberUserEndpoint.get`, `workspace.py:138-142`, W17)
//! - `POST workspaces/<slug>/workspace-views/`
//!   (`WorkspaceMemberUserViewsEndpoint.post`, `workspace.py:143-147`, W18)
//!
//! Only these seven routes are registered, so the edge serves exactly this
//! family from Rust while every sibling path keeps proxying to Django —
//! route registration is the cutover granularity, no flag needed.
//!
//! Layering: the gates live in [`super::gates`] (F-W24-13, PIDASHCONV-613),
//! the SQL shapes in [`pidash_services::app_workspace::queries_membership`]
//! (F-W24-10, PIDASHCONV-609), the member shapes in
//! [`pidash_services::app_workspace::ser_workspace`] (F-W24-01,
//! PIDASHCONV-600) composed with the user lite shapes in
//! [`pidash_services::app_workspace::ser_user`] (F-W24-04, PIDASHCONV-603),
//! and the project-member role shape in
//! [`pidash_services::app_project::ser_member`] (D-25, PIDASHCONV-564).
//! This module owns the HTTP shell (routes, session auth, gates), the
//! executed SQL text, the PATCH validation kernels, the response rendering,
//! and the leave cache invalidations.
//!
//! Handler notes (all verified against the Python source at drift baseline
//! `01a93e17`, DRF 3.15.2, Django 4.2):
//! - Gate order: session authN first (anon 401), then the gate, then the
//!   body. List/retrieve carry `@allow_permission([ADMIN, MEMBER, GUEST])`,
//!   partial_update/destroy `@allow_permission([ADMIN])`, leave
//!   `@allow_permission([ADMIN, MEMBER, GUEST])` with the three
//!   `@invalidate_cache` decorators *outside* it (a 403 still invalidates).
//!   Project-members carries `WorkspaceEntityPermission` (safe GET: any
//!   active membership). Views-post, me-get and last-visited carry the
//!   `IsAuthenticated` default only.
//! - The `fields=("id", "member", "role")` argument on list/retrieve is a
//!   silent no-op (`DynamicBaseSerializer.__init__` overwrites `fields`
//!   with `expand`, `serializers/base.py:16-18`): the full 17-key shape
//!   renders. The call sites use the `*_fields_to_representation` kernels
//!   so the quirk stays visible.
//! - List/retrieve branch on `requester.role > 5` (list spells it as a
//!   literal, retrieve as `ROLE.GUEST.value` — same threshold): admins
//!   see the admin-lite nested user, everyone else the plain lite.
//! - `partial_update` runs the guest-demote cascade *before* serializer
//!   validation: a present-but-unparseable `role` 500s (Python `int()`
//!   raising through `handle_exception`) and never reaches the 400 arm,
//!   while a truncated float such as `5.9` demotes and *then* 400s.
//!   `role` validates as a DRF `ChoiceField` over `[20, 15, 5]` (the model
//!   carries `choices`). DRF runs *no* uniqueness validators here:
//!   `get_unique_together_validators` only sees writable fields and
//!   `member` is read-only, so both the `(workspace, member, deleted_at)`
//!   trio and the conditional `(workspace, member)` pair are skipped — a
//!   workspace collision saves, hits the unique constraint, and 400s
//!   through the `IntegrityError` branch instead.
//! - `destroy`'s sole-project-admin guard compares `member_id` (a User FK)
//!   against the WorkspaceMember PK, so it never fires; `leave` uses
//!   `request.user.id` and works. Both shapes are kept, never unified.
//! - `leave` invalidations run after auth but before the gate: anonymous
//!   callers 401 without invalidating, while an authenticated 403 still
//!   deletes the three keys (one missing its leading slash — harmless
//!   inside the `*…*` glob, which still matches the stored keys).
//! - Detail routes carry `<uuid:pk>`, which only matches
//!   lowercase-hyphenated UUIDs; anything else misses the route and 404s
//!   through `custom_404_view` (`JsonResponse` bytes, with the space
//!   after the colon) before auth ever runs.
//! - The me-get serializes a missing row (`None`) to a 14-key all-null
//!   object with `company_role: ""` and `is_active: false` (verified
//!   against live DRF, not derived): read-only fields skip, the rest
//!   null, except the `CharField`/`BooleanField` initials.
//! - Views-post reads `request.data.get("view_props", {})` with no
//!   serializer: a valid non-dict body 500s (`.get` on a list), and an
//!   explicit `view_props: null` 400s (the column is `NOT NULL`).
//! - Last-visited always 500s: `user.last_workspace_id` lives on `Profile`,
//!   not `User`, so every call raises `AttributeError`.
//!
//! Fixture ids: F-W24-15 (these routes; the Done-when oracle) over
//! F-W24-01 (member shapes), F-W24-04 (user lite shapes), F-W24-10
//! (membership SQL) and F-W24-13 (gates), which stay green.
//!
//! Ported bugs (translation, don't redesign; also listed in the PR):
//! - B1: `fields=` is a no-op on all four list/retrieve call sites — the
//!   full shape renders (`serializers/base.py:16-18`).
//! - B2: last-visited always 500s (`user.py:73` reads a `Profile` field
//!   off `User`).
//! - B3: destroy's sole-project-admin guard never fires (`member.py:128`
//!   compares the User FK against the WorkspaceMember PK).
//! - B4: the leave invalidate path `api/users/me/workspaces/` misses its
//!   leading slash (harmless inside the `*…*` glob) (`member.py:159`).
//! - B5: member list has no `is_active` filter — inactive rows are listed
//!   (`member.py:37-43`).
//! - B6: the guest-demote cascade has no `is_active` filter — inactive
//!   project rows are rewritten too (`member.py:89`).
//! - B7: the involved-project-ids query has no slug filter — the ids span
//!   all workspaces (`member.py:245-249`).
//! - B8: equal roles can remove — the higher-role guard is strict `<`
//!   (`member.py:116`).
//! - B9: invalid JSON carries the `serde` reason after DRF's
//!   `JSON parse error - ` prefix (blank input is byte-exact); bodies
//!   with lone surrogates or `NaN`/`Infinity` literals 400 at parse
//!   where Python's `json` accepts them (same shared approximation as
//!   the `app_analytics` handlers).
//!
//! Known edge (naive year-1/9999 `deleted_at` whose request-zone instant
//! leaves Python's year range): Django saves then 500s at render; this
//! port renders through the shared datetime kernel and 200s.
//!
//! Sibling plumbing mirrors `app_analytics::handlers_a`: [`owned`],
//! session [`actor`], exact denial bodies, manual envelope assembly for
//! DRF key order/bytes.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::middleware::SessionHandle;
use crate::state::AppState;

use super::gates::{
    decide_class_entity, decide_gate, gate_for, invalidation_key, invalidations_for, outcome_body,
    tenant_context, GateOutcome, InvalidateAction, ANON_BODY, CLASS_DENIED_BODY, FORBIDDEN_BODY,
};
use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::workspace::WorkspaceFacts;
use pidash_services::app_project::ser_member as d25;
use pidash_services::app_workspace::models_user::user::avatar_url as resolve_avatar_url;
use pidash_services::app_workspace::queries_membership as qm;
use pidash_services::app_workspace::{ser_user, ser_workspace};
use pidash_types::WorkspaceId;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Route path templates for this family, in `app/urls/workspace.py` form
/// (the matching rows also live in [`super::gates::GATES`]).
pub const MEMBERS_PATH: &str = "workspaces/<slug>/members/";
/// Project-members dict path (route W13).
pub const PROJECT_MEMBERS_PATH: &str = "workspaces/<slug>/project-members/";
/// Member detail path: retrieve + partial_update + destroy (route W14).
pub const MEMBER_DETAIL_PATH: &str = "workspaces/<slug>/members/<pk>/";
/// Leave path (route W15).
pub const MEMBERS_LEAVE_PATH: &str = "workspaces/<slug>/members/leave/";
/// Last-visited path (route W16).
pub const LAST_VISITED_PATH: &str = "users/last-visited-workspace/";
/// Member-me path (route W17).
pub const MEMBERS_ME_PATH: &str = "workspaces/<slug>/workspace-members/me/";
/// Workspace-views path (route W18).
pub const WORKSPACE_VIEWS_PATH: &str = "workspaces/<slug>/workspace-views/";

/// Register the seven owned routes. Nothing else: sibling paths stay
/// unmatched and proxy to Django, and every non-owned method on the owned
/// paths falls through to Django too (its 405-after-auth and metadata
/// responses live there). `HEAD` rides axum's `get` handling like Django's
/// `GET`-backed `HEAD`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/members/",
            owned(
                axum::routing::get(list_members),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/project-members/",
            owned(
                axum::routing::get(project_members),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/members/{pk}/",
            owned(
                axum::routing::get(retrieve_member)
                    .patch(partial_update_member)
                    .delete(destroy_member),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/members/leave/",
            owned(
                axum::routing::post(leave_workspace),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/last-visited-workspace/",
            owned(
                axum::routing::get(last_visited),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/workspace-members/me/",
            owned(
                axum::routing::get(member_me),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/workspace-views/",
            owned(
                axum::routing::post(member_views),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
}

/// An owned path: the owned methods serve from Rust, everything else proxies
/// to Django (DRF metadata, 401-anon-before-405).
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
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

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// DRF `IsAuthenticated` / `NotAuthenticated` 401.
pub const UNAUTHENTICATED_BODY: &str = ANON_BODY;
/// `@allow_permission` 403 on the member routes.
pub const PERMISSION_DENIED_BODY: &str = FORBIDDEN_BODY;
/// `WorkspaceEntityPermission` 403 on project-members.
pub const ENTITY_DENIED_BODY: &str = CLASS_DENIED_BODY;
/// Retrieve miss (`member.py:64-68`).
pub const MEMBER_NOT_FOUND_BODY: &str = r#"{"error":"Workspace member not found"}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (`app/views/base.py`):
/// the bare-`.get` misses (write targets, requesters, views-post).
pub const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// Resolver 404 for a `<uuid:pk>` segment that misses the converter
/// (global `handler404`, `pi_dash/urls.py:15` → `custom_404_view`):
/// `JsonResponse` bytes, i.e. `json.dumps` defaults with the space after
/// the colon.
pub const PAGE_NOT_FOUND_BODY: &str = r#"{"error": "Page not found."}"#;
/// `handle_exception`'s `IntegrityError` branch: unique-constraint
/// collisions at save (DRF runs no uniqueness validators for this
/// serializer, so the database is the only guard).
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s generic 500 branch — and the last-visited body.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// Self role change (`member.py:81-85`).
pub const SELF_ROLE_BODY: &str = r#"{"error":"You cannot update your own role"}"#;
/// Self remove (`member.py:110-114`).
pub const SELF_REMOVE_BODY: &str =
    r#"{"error":"You cannot remove yourself from the workspace. Please use leave workspace"}"#;
/// Higher-role remove (`member.py:116-120`).
pub const HIGHER_ROLE_BODY: &str =
    r#"{"error":"You cannot remove a user having role higher than you"}"#;
/// Destroy sole-project-admin guard (`member.py:136-141`).
pub const SOLE_PROJECT_ADMIN_BODY: &str = r#"{"error":"User is a part of some projects where they are the only admin, they should either leave that project or promote another user to admin."}"#;
/// Leave sole-workspace-admin guard (`member.py:169-174`).
pub const SOLE_WORKSPACE_ADMIN_BODY: &str = r#"{"error":"You cannot leave the workspace as you are the only admin of the workspace you will have to either delete the workspace or promote another user to admin."}"#;
/// Leave sole-project-admin guard (`member.py:190-195`).
pub const SOLE_PROJECT_ADMIN_LEAVE_BODY: &str = r#"{"error":"You are a part of some projects where you are the only admin, you should either leave the project or promote another user to admin."}"#;
/// DRF `ParseError` message prefix (`rest_framework/parsers.py`).
pub const JSON_PARSE_PREFIX: &str = "JSON parse error - ";

/// Handler denials with byte-exact bodies.
#[derive(Debug)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403 with the gate's own body (decorator vs entity class).
    Forbidden(&'static str),
    /// 404, retrieve miss (`member.py:64-68`).
    MemberNotFound,
    /// 404, `ObjectDoesNotExist` branch.
    ObjectNotFound,
    /// 404, the URL resolver (`<uuid:pk>` converter miss → `handler404`).
    PageNotFound,
    /// 400, `IntegrityError` branch.
    BadPayload,
    /// 400, malformed JSON body (carries the full `detail` text, reason
    /// included, like DRF's `ParseError`).
    BadJson(String),
    /// 400, an inline handler guard body (self-role, self-remove,
    /// higher-role, sole-admin).
    BadGuard(&'static str),
    /// 400, serializer field errors (`{"role": [...]}`).
    BadFields(Value),
    /// 500, anything Python lets escape (`AttributeError`, `TypeError`,
    /// DB errors, unparseable stored values).
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::Forbidden(body) => (StatusCode::FORBIDDEN, (*body).to_owned()),
            Denial::MemberNotFound => (StatusCode::NOT_FOUND, MEMBER_NOT_FOUND_BODY.to_owned()),
            Denial::ObjectNotFound => (StatusCode::NOT_FOUND, OBJECT_NOT_FOUND_BODY.to_owned()),
            Denial::PageNotFound => (StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY.to_owned()),
            Denial::BadPayload => (StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY.to_owned()),
            Denial::BadJson(detail) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(&serde_json::json!({"detail": detail}))
                    .expect("json parse body"),
            ),
            Denial::BadGuard(body) => (StatusCode::BAD_REQUEST, (*body).to_owned()),
            Denial::BadFields(errors) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(errors).expect("field errors"),
            ),
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
        (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response()
    }
}

fn json_response(status: StatusCode, body: String) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

/// DRF `JSONRenderer` post-pass over this project's settings
/// (`UNICODE_JSON=True`, so raw UTF-8 survives, except U+2028/U+2029 which
/// the renderer escapes; same rule as the `drf_escape` helper in
/// `assistant::common`, restated because that helper is private).
fn drf_escape(rendered: &str) -> String {
    rendered
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

// ---------------------------------------------------------------------------
// Request context: auth + tenant + membership
// ---------------------------------------------------------------------------

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

/// One active workspace-membership row: the id (self-remove compares
/// WorkspaceMember PKs, `member.py:110`), the role (branch + guards), and
/// the user id (self-role compares user ids, `member.py:81`).
#[derive(Debug, Clone, Copy)]
struct Requester {
    id: Uuid,
    role: i32,
    member_id: Uuid,
}

/// Active workspace membership for `(user, slug)`, or `None` (no row).
/// Mirrors the `allow_permission` workspace lookup (`is_active=True`,
/// soft-deleted rows excluded, `app/permissions/base.py:44-51`) and the
/// half-dozen bare `.get(member, slug, active)` requester lookups in
/// `member.py` (`:47`, `:59`, `:106-108`, `:162`, `:210`).
async fn requester_lookup(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    slug: &str,
) -> Result<Option<Requester>, Denial> {
    let row: Option<(Uuid, i16, Uuid)> = sqlx::query_as(
        "SELECT wm.id, wm.role, wm.member_id FROM workspace_members wm \
         WHERE wm.member_id = $1 AND wm.workspace_id = \
         (SELECT id FROM workspaces WHERE slug = $2) AND wm.is_active \
         AND wm.deleted_at IS NULL LIMIT 1",
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(id, role, member_id)| Requester {
        id,
        role: i32::from(role),
        member_id,
    }))
}

/// Membership facts for one decorator gate: the caller passes the route's
/// [`super::gates::Gate`] so the allowed-role set matches the Python
/// source (`[ADMIN, MEMBER, GUEST]` on list/retrieve/leave, `[ADMIN]` on
/// partial_update/destroy).
fn workspace_facts(slug: &str, role: Option<i32>, allowed_roles: &[i32]) -> AllowFacts {
    AllowFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        is_workspace_member: role.is_some(),
        has_allowed_workspace_role: role.is_some_and(|role| allowed_roles.contains(&role)),
        is_creator: false,
        has_allowed_project_role: false,
        is_project_member: false,
        is_workspace_admin: role == Some(qm::ROLE_ADMIN),
    }
}

/// Membership facts for the project-members class gate
/// (`WorkspaceEntityPermission` over a safe GET: only `is_member`
/// decides; `is_admin_unfiltered` is filled from the same active row —
/// immaterial here because an inactive-only admin denies on `is_member`
/// first either way).
fn entity_facts(slug: &str, role: Option<i32>) -> WorkspaceFacts {
    WorkspaceFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        authenticated: true,
        has_admin_or_member_role: role
            .is_some_and(|role| role == qm::ROLE_ADMIN || role == qm::ROLE_MEMBER),
        has_admin_role: role == Some(qm::ROLE_ADMIN),
        is_member: role.is_some(),
        is_admin_unfiltered: role == Some(qm::ROLE_ADMIN),
    }
}

/// Resolve the gate row for `method` + `path`, run it for `(slug, user)`,
/// and return the actor on allow. `path` is the [`gate_for`] template
/// (`workspaces/<slug>/members/` form). Decorator and `Authenticated`
/// rows decide through [`decide_gate`]; the class row (project-members)
/// through [`decide_class_entity`].
#[allow(clippy::result_large_err)]
async fn gated_actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
    method: &str,
    path: &str,
    slug: &str,
) -> Result<crate::license::Actor, Response> {
    let actor = match actor(state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return Err(denial.into_response()),
    };
    let gate = gate_for(method, path).ok_or(Denial::ServerError.into_response())?;
    let pool = pool_of(state).map_err(|denial| denial.into_response())?;
    let requester = requester_lookup(pool, &actor.id, slug)
        .await
        .map_err(|denial| denial.into_response())?;
    let role = requester.map(|requester| requester.role);
    let scope = tenant_context(slug);
    let outcome = match &gate.gate {
        super::gates::Gate::Workspace { roles } => {
            decide_gate(&gate.gate, &scope, &workspace_facts(slug, role, roles))
        }
        super::gates::Gate::ClassEntity => {
            decide_class_entity(method, &scope, &entity_facts(slug, role))
        }
        other => decide_gate(other, &scope, &workspace_facts(slug, role, &[])),
    };
    match outcome {
        GateOutcome::Allow => Ok(actor),
        _ => Err(deny_response(outcome)),
    }
}

fn parse_pk(raw: &str) -> Result<Uuid, Denial> {
    // Django's `<uuid:pk>` converter (`[0-9a-f]{8}-…`, lowercase-only,
    // case-sensitive match): anything else misses the route and 404s
    // through `custom_404_view` before auth ever runs.
    const HYPHENS: [usize; 4] = [8, 13, 18, 23];
    let bytes = raw.as_bytes();
    let valid = bytes.len() == 36
        && HYPHENS.iter().all(|&i| bytes[i] == b'-')
        && bytes.iter().enumerate().all(|(i, &b)| {
            HYPHENS.contains(&i) || (b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        });
    if !valid {
        return Err(Denial::PageNotFound);
    }
    Uuid::parse_str(raw).map_err(|_| Denial::PageNotFound)
}

/// Map a body-parse failure to DRF's `ParseError` shape. Blank input is
/// byte-exact (`Expecting value: line 1 column N+1 (char N)`); anything
/// else carries the parser reason after the same prefix.
fn json_parse_denial(raw: &[u8], error: &serde_json::Error) -> Denial {
    if let Ok(text) = std::str::from_utf8(raw) {
        if text.trim().is_empty() {
            let len = text.len();
            return Denial::BadJson(format!(
                "{JSON_PARSE_PREFIX}Expecting value: line 1 column {} (char {})",
                len + 1,
                len
            ));
        }
    }
    Denial::BadJson(format!("{JSON_PARSE_PREFIX}{error}"))
}

/// Parse a PATCH body's JSON: the cascade pre-check and the dict
/// check both read this value.
fn parse_body_json(raw: &[u8]) -> Result<Value, Denial> {
    serde_json::from_slice(raw).map_err(|error| json_parse_denial(raw, &error))
}

/// The cascade pre-check over raw `request.data` (`member.py:88`): it
/// runs before any dict check, so `"role" in data` raises `TypeError`
/// for scalar bodies (the 500), and a membership hit on a `str`
/// (substring) or a `list` (element equality) 500s on the `.get`
/// subscript; a miss skips the cascade and falls through to the
/// serializer's dict 400. Returns the `role` member for dict bodies,
/// owned so the caller can move the body into the dict check after.
fn cascade_role_lookup(value: &Value) -> Result<Option<Value>, Denial> {
    match value {
        Value::Object(map) => Ok(map.get("role").cloned()),
        Value::Array(items) => {
            if items
                .iter()
                .any(|item| matches!(item, Value::String(text) if text == "role"))
            {
                Err(Denial::ServerError)
            } else {
                Ok(None)
            }
        }
        Value::String(text) => {
            if text.contains("role") {
                Err(Denial::ServerError)
            } else {
                Ok(None)
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => Err(Denial::ServerError),
    }
}

/// The dict half of PATCH body parsing, over an already-parsed value
/// (DRF interpolates `type(data).__name__` for anything else).
fn parse_body_value(value: Value) -> Result<Map<String, Value>, Denial> {
    value.as_object().cloned().ok_or_else(|| {
        let kind = python_type_name(&value);
        let mut errors = Map::with_capacity(1);
        errors.insert(
            "non_field_errors".to_owned(),
            Value::Array(vec![Value::String(format!(
                "Invalid data. Expected a dictionary, but got {kind}."
            ))]),
        );
        Denial::BadFields(Value::Object(errors))
    })
}

/// DRF's `type(data).__name__` over JSON values.
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_i64() || number.is_u64() => "int",
        Value::Number(_) => "float",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// One query value, repeated or not: axum's `Query` backend does not coerce
/// a lone `?key=value` into a sequence, so the extractor uses this untagged
/// shape and callers read the last value — mirroring Django's `QueryDict`,
/// where repeats are legal and `.get` returns the last value.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

type QueryMap = HashMap<String, OneOrMany>;

fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    match query.get(key)? {
        OneOrMany::One(value) => Some(value.clone()),
        OneOrMany::Many(values) => values.last().cloned(),
    }
}

/// `SearchFilter` terms (`rest_framework/filters.py`
/// `search_smart_split` over Django `smart_split`): the last `?search=`
/// value split on whitespace with quoted phrases kept together; commas
/// trim per token, then quoted tokens unescape while the rest split on
/// commas. Empty input filters nothing.
fn search_terms(query: &QueryMap) -> Vec<String> {
    query_last(query, "search")
        .map(|raw| search_smart_split(&raw))
        .unwrap_or_default()
}

/// DRF `search_smart_split`: each token has commas trimmed, then a
/// token wrapped in one quote pair (single char included — `"x"` and a
/// lone `"` both count, the latter unescaping to the empty term) keeps
/// together unescaped, while any other token splits on commas with
/// empty pieces dropped and the rest stripped.
fn search_smart_split(raw: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for token in smart_split_terms(raw) {
        let trimmed = token.trim_matches(',');
        let mut chars = trimmed.chars();
        let Some(first) = chars.next() else {
            // Empty after the comma trim: `"".split(",")` is `[""]`,
            // filtered as empty — contributes nothing.
            continue;
        };
        let last = trimmed.chars().next_back().expect("first exists");
        if (first == '"' || first == '\'') && first == last {
            terms.push(unescape_string_literal(trimmed));
        } else {
            for sub in trimmed.split(',') {
                if !sub.is_empty() {
                    terms.push(py_strip(sub).to_owned());
                }
            }
        }
    }
    terms
}

/// Django `smart_split` (`django/utils/text.py`): split on Python
/// whitespace, keeping quoted phrases (single or double, backslash
/// escapes, quotes retained) together with any adjacent non-space text.
/// A backslash-newline or an unclosed quote breaks the quoted match and
/// the token falls back to a bare non-space run.
fn smart_split_terms(text: &str) -> Vec<&str> {
    let mut terms = Vec::new();
    let mut rest = text.trim_start_matches(py_is_space);
    while !rest.is_empty() {
        let end = smart_token_len(rest);
        terms.push(&rest[..end]);
        rest = rest[end..].trim_start_matches(py_is_space);
    }
    terms
}

/// One `smart_split` token length: the quote-aware alternative wins when
/// at least one complete quoted string parses (a later unclosed quote
/// ends the token instead of failing it, like the regex backtrack),
/// else the bare non-space run (`\S+`).
fn smart_token_len(text: &str) -> usize {
    let mut pos = 0;
    while let Some(ch) = text[pos..].chars().next() {
        if py_is_space(ch) || ch == '"' || ch == '\'' {
            break;
        }
        pos += ch.len_utf8();
    }
    let mut end = pos;
    let mut quoted = 0;
    while text[end..].starts_with(['"', '\'']) {
        let Some(after) = quoted_len(&text[end..]) else {
            break;
        };
        end += after;
        quoted += 1;
        while let Some(ch) = text[end..].chars().next() {
            if py_is_space(ch) || ch == '"' || ch == '\'' {
                break;
            }
            end += ch.len_utf8();
        }
    }
    if quoted > 0 {
        return end;
    }
    let mut len = 0;
    while let Some(ch) = text[len..].chars().next() {
        if py_is_space(ch) {
            break;
        }
        len += ch.len_utf8();
    }
    len
}

/// One complete quoted string length (`"(?:[^"\\]|\\.)*"` or the
/// single-quote form — `.` never matches `\n`), or `None` when the
/// quote never closes.
fn quoted_len(text: &str) -> Option<usize> {
    let quote = text.chars().next()?;
    debug_assert!(quote == '"' || quote == '\'');
    let mut pos = quote.len_utf8();
    loop {
        let ch = text[pos..].chars().next()?;
        if ch == '\\' {
            let next = text[pos + 1..].chars().next()?;
            if next == '\n' {
                return None;
            }
            pos += 1 + next.len_utf8();
        } else if ch == quote {
            return Some(pos + quote.len_utf8());
        } else {
            pos += ch.len_utf8();
        }
    }
}

/// Django `unescape_string_literal`: strip one quote pair, then unescape
/// the quote and the backslash, in that order. The caller guarantees a
/// non-empty string literal (first and last chars the same quote).
fn unescape_string_literal(term: &str) -> String {
    let quote = term.chars().next().expect("quoted term");
    let inner = term
        .strip_prefix(quote)
        .and_then(|rest| rest.strip_suffix(quote))
        .unwrap_or("");
    let escaped = format!("\\{quote}");
    inner
        .replace(&escaped, &quote.to_string())
        .replace("\\\\", "\\")
}

/// `icontains` parameter: LIKE metacharacters escaped, wrapped in `%`.
fn like_param(term: &str) -> String {
    let mut out = String::with_capacity(term.len() + 2);
    out.push('%');
    for ch in term.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('%');
    out
}

/// Python whitespace (`str.strip`, `re \s`, `str.isspace` — all the same
/// 29 chars: Rust's `char::is_whitespace` plus `\x1c`-`\x1f`, verified
/// equal over the whole scalar range).
fn py_is_space(ch: char) -> bool {
    ch.is_whitespace() || ('\x1c'..='\x1f').contains(&ch)
}

/// Python `str.strip()` with no arguments.
fn py_strip(value: &str) -> &str {
    value.trim_matches(py_is_space)
}

/// Python `str()` over JSON values, for the `ChoiceField` display
/// (`'"%s" is not a valid choice.' % data`): strings as-is, `True` /
/// `False` / `None`, ints as digits, floats as `repr`, lists with `", "`
/// joins and dicts with single-quoted `repr` pairs.
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => py_num_str(number),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        python_repr(&Value::String(key.clone())),
                        python_repr(item)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` over JSON values (what `str()` of a container shows
/// for its items): strings quoted, everything else like [`python_str`].
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(text) => py_string_repr(text),
        _ => python_str(value),
    }
}

/// Python `repr()` of a string: single quotes unless the text holds a `'`
/// but no `"`, short escapes, `\xhh` for bare controls, `\uxxxx` for the
/// rest of the non-printables. The non-printable table covers Cc, Zs
/// (other than space), Zl/Zp and the common Cf ranges; rarer format
/// characters render raw (a display-only edge inside an already-invalid
/// choice message).
fn py_string_repr(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let mut out = String::with_capacity(text.len() + 2);
    out.push(if use_double { '"' } else { '\'' });
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\'' if !use_double => out.push_str("\\'"),
            '"' if use_double => out.push_str("\\\""),
            c if c < ' ' || c == '\x7f' => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if !py_printable(c) => {
                if (c as u32) <= 0xffff {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                } else {
                    out.push_str(&format!("\\U{:08x}", c as u32));
                }
            }
            c => out.push(c),
        }
    }
    out.push(if use_double { '"' } else { '\'' });
    out
}

/// Approximation of `str.isprintable` for the `repr` escape table.
fn py_printable(ch: char) -> bool {
    if ch == ' ' {
        return true;
    }
    if ch.is_whitespace() {
        return false;
    }
    // Zl/Zp separators and the common Cf ranges.
    if matches!(ch, '\u{2028}' | '\u{2029}' | '\u{0085}') {
        return false;
    }
    if matches!(ch, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{feff}' | '\u{061c}') {
        return false;
    }
    true
}

/// Python `str()` of a JSON number: ints echo digits, floats render as
/// `repr` (a `.0` suffix when integral, `inf`/`-inf`/`nan` unbounded).
fn py_num_str(number: &serde_json::Number) -> String {
    if let Some(int) = number.as_i64() {
        return int.to_string();
    }
    if let Some(uint) = number.as_u64() {
        return uint.to_string();
    }
    // Beyond `u64`: integer text echoes (Python bigint `str()` renders
    // the same digits); float text renders as `repr()`.
    if !number.to_string().contains(['.', 'e', 'E']) {
        return number.to_string();
    }
    match number.as_f64() {
        Some(float) if float.is_finite() => py_float_repr(float),
        Some(float) if float.is_nan() => "nan".to_owned(),
        Some(_) => {
            if number.as_f64().is_some_and(|f| f.is_sign_negative()) {
                "-inf".to_owned()
            } else {
                "inf".to_owned()
            }
        }
        None => number.to_string(),
    }
}

/// Python `repr()` of a finite float: shortest round-trip digits (what
/// Rust's `{}` already renders, fixed notation), re-pointed into fixed
/// notation for `1e-4 <= abs < 1e16` and `d.dddde±XX` (two-digit signed
/// exponent) otherwise, with a `.0` suffix when integral.
fn py_float_repr(float: f64) -> String {
    debug_assert!(float.is_finite());
    let sign = if float.is_sign_negative() { "-" } else { "" };
    let abs = float.abs();
    if abs == 0.0 {
        return format!("{sign}0.0");
    }
    // Shortest round-trip digits, fixed notation, no exponent.
    let plain = format!("{abs}");
    let (int_part, frac_part) = match plain.split_once('.') {
        Some((int, frac)) => (int, frac),
        None => (plain.as_str(), ""),
    };
    let int_sig = int_part.trim_start_matches('0');
    let (sig, exp): (String, i32) = if !int_sig.is_empty() {
        // 1 <= abs: Rust's `{}` expands large magnitudes to full fixed
        // width (`1e16` → 17 chars), so strip the filler zeros — the
        // shortest digits carry the value, `exp` carries the scale.
        let raw = format!("{int_sig}{}", frac_part.trim_end_matches('0'));
        (
            raw.trim_end_matches('0').to_owned(),
            int_sig.len() as i32 - 1,
        )
    } else {
        // abs < 1: skip the fractional leading zeros.
        let zeros = frac_part.len() - frac_part.trim_start_matches('0').len();
        let digits = frac_part.trim_start_matches('0').trim_end_matches('0');
        (digits.to_owned(), -(zeros as i32) - 1)
    };
    debug_assert!(!sig.is_empty());
    if (-4..16).contains(&exp) {
        if exp < 0 {
            // `0.000<sig>`: one zero per missing place (`-exp - 1`
            // of them; `exp` here is `-4..=-1`, so no underflow).
            let fixed = format!("0.{}{}", "0".repeat((-exp - 1) as usize), sig);
            return format!("{sign}{fixed}");
        }
        let point = (exp + 1) as usize;
        let fixed = if point >= sig.len() {
            format!("{sig}{}.0", "0".repeat(point - sig.len()))
        } else {
            format!("{}.{}", &sig[..point], &sig[point..])
        };
        return format!("{sign}{fixed}");
    }
    let mantissa = if sig.len() == 1 {
        sig
    } else {
        format!("{}.{}", &sig[..1], &sig[1..])
    };
    format!("{sign}{mantissa}e{exp:+03}")
}

/// Python `int(value)`: bools as 0/1, floats truncated toward zero,
/// strings stripped (minus `\x1c`-`\x1f`) with Unicode decimal digits
/// and single internal underscores, ints as-is.
/// `None` arms the caller's 500 (`TypeError`/`ValueError` through
/// `handle_exception`).
fn python_int(value: &Value) -> Option<i64> {
    match value {
        Value::Null => None,
        Value::Bool(true) => Some(1),
        Value::Bool(false) => Some(0),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Some(int);
            }
            if let Some(uint) = number.as_u64() {
                return Some(i64::try_from(uint).unwrap_or(i64::MAX));
            }
            // `int()` of an unbounded int that overflows `i64` is still
            // an int — it just never equals the cascade trigger.
            let float = number.as_f64()?;
            if !float.is_finite() {
                return None;
            }
            // `as_i64` already failed, so this is either a genuine
            // float (truncate toward zero) or an out-of-range int
            // (no truncation applies — report "not the trigger").
            if number.to_string().contains(['.', 'e', 'E']) {
                Some(float.trunc() as i64)
            } else {
                Some(i64::MAX)
            }
        }
        Value::String(text) => py_int_str(text),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// `int()`'s strip class: Python whitespace except `\x1c`-`\x1f`
/// (probed exhaustively over the scalar range: `int()` strips the
/// other 25 `str.strip` chars but raises on the four controls).
fn py_int_is_space(ch: char) -> bool {
    py_is_space(ch) && !('\x1c'..='\x1f').contains(&ch)
}

/// Unicode decimal value (`unicodedata.decimal`, general category Nd),
/// the digit set `int()` accepts: 63 single decades plus the
/// mathematical block (`U+1D7CE..U+1D7FF`, five packed decades, hence
/// the remainder). Table generated from CPython's `unicodedata` and
/// verified value by value; numbering/letter numerics (`No`/`Nl`,
/// e.g. `²`) are rejected, like `int()`.
fn nd_value(ch: char) -> Option<u32> {
    let cp = ch as u32;
    if (0x1D7CE..=0x1D7FF).contains(&cp) {
        return Some((cp - 0x1D7CE) % 10);
    }
    const DECADES: &[u32] = &[
        0x0030, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66,
        0x0CE6, 0x0D66, 0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946,
        0x19D0, 0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0,
        0xA9F0, 0xAA50, 0xABF0, 0xFF10, 0x104A0, 0x10D30, 0x11066, 0x110F0, 0x11136, 0x111D0,
        0x112F0, 0x11450, 0x114D0, 0x11650, 0x116C0, 0x11730, 0x118E0, 0x11950, 0x11C50, 0x11D50,
        0x11DA0, 0x11F50, 0x16A60, 0x16AC0, 0x16B50, 0x1E140, 0x1E2F0, 0x1E4F0, 0x1E950, 0x1FBF0,
    ];
    DECADES
        .iter()
        .find(|start| (**start..**start + 10).contains(&cp))
        .map(|start| cp - start)
}

/// Python `int(s, 10)`: surrounding whitespace stripped (minus
/// `\x1c`-`\x1f`), one optional ASCII sign, Unicode decimal digits
/// with single internal underscores.
fn py_int_str(text: &str) -> Option<i64> {
    let text = text.trim_matches(py_int_is_space);
    let (negative, digits) = match text.strip_prefix(['+', '-']) {
        Some(rest) => (text.starts_with('-'), rest),
        None => (false, text),
    };
    if digits.is_empty() {
        return None;
    }
    let mut cleaned = String::with_capacity(digits.len());
    let mut prev_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else {
            let digit = nd_value(ch)?;
            cleaned.push(char::from_digit(digit, 10).expect("decimal digit"));
            prev_underscore = false;
        }
    }
    if prev_underscore {
        return None;
    }
    // The string is digits-only by construction, so a parse failure is an
    // overflow — Python's bigint still compares (never the trigger).
    let mut value: i64 = match cleaned.parse() {
        Ok(value) => value,
        Err(_) => return Some(if negative { i64::MIN } else { i64::MAX }),
    };
    if negative {
        value = value.checked_neg()?;
    }
    Some(value)
}

// ---------------------------------------------------------------------------
// Django `parse_datetime` (`django/utils/dateparse.py`, Django 4.2.30):
// `datetime.fromisoformat` first, `datetime_re` fallback (CPython 3.12)
// ---------------------------------------------------------------------------

/// Humanized `iso-8601` input format (`rest_framework/utils/
/// humanize_datetime.py`), interpolated into the `deleted_at` invalid message.
const ISO_FORMAT_HINT: &str = "YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]";

/// A parsed `deleted_at` input: aware values carry their instant, naive
/// values the wall time (DRF then attaches the request timezone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedDt {
    Aware(DateTime<Utc>),
    Naive(chrono::NaiveDateTime),
}

/// Parse exactly what Django's `parse_datetime` accepts (pinned against
/// Django 4.2.30 / CPython 3.12 empirically): the `fromisoformat` ∪
/// `datetime_re` union — `fromisoformat` first, the regex fallback when
/// it raises. Anything else is `None` (the caller's invalid arm).
fn parse_django_datetime(text: &str) -> Option<ParsedDt> {
    parse_fromisoformat_datetime(text).or_else(|| parse_regex_datetime(text))
}

/// Arm 1: CPython 3.12 `datetime.fromisoformat` — calendar dates
/// (`YYYY-MM-DD`, `YYYYMMDD`) and ISO week dates (`YYYY-Www[-d]`,
/// `YYYYWww[d]`, weekday default 1), an optional time after any single
/// non-digit separator (parts strictly two digits, seconds optional,
/// `.`/`,` fraction always a seconds fraction truncated to 6 digits),
/// one optional gap byte, and an optional `Z`/numeric offset (`±HH`,
/// `±HHMM`, `±HH:MM`, `±HHMMSS`, `±HH:MM:SS`, optional seconds fraction
/// after any count, total strictly under 24h). Surrounding whitespace
/// rejects (the C parser is ASCII-strict).
fn parse_fromisoformat_datetime(text: &str) -> Option<ParsedDt> {
    if text.is_empty() || py_strip(text).len() != text.len() {
        return None;
    }
    let bytes = text.as_bytes();
    // Date part: 4-digit year first in every accepted shape.
    if bytes.len() < 4 || !bytes[..4].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: i32 = text[..4].parse().ok()?;
    let (date, rest) = parse_iso_date(text, year)?;
    if rest.is_empty() {
        // Date-only renders midnight.
        return Some(ParsedDt::Naive(
            date.and_hms_opt(0, 0, 0).expect("midnight valid"),
        ));
    }
    // Any single non-digit separator (`T`, `X`, space, `t`, even `+`/`-`
    // — the offset only starts after the time part).
    let rest = match rest.strip_prefix('T') {
        Some(tail) => tail,
        None => {
            let mut chars = rest.chars();
            let sep = chars.next()?;
            if sep.is_ascii_digit() {
                return None;
            }
            chars.as_str()
        }
    };
    if rest.is_empty() {
        return None;
    }
    parse_iso_time(date, rest)
}

/// Parse the date head, returning the day and the unparsed tail.
fn parse_iso_date(text: &str, year: i32) -> Option<(chrono::NaiveDate, &str)> {
    let bytes = text.as_bytes();
    // Week dates contain an uppercase `W` (`2024-W03[-1]`, `2024W03[1]`).
    // The basic form takes no dash-day (`2024W03-1` is rejected outright —
    // the dash is not retried as a time separator).
    if bytes.len() > 4 && bytes[4] == b'W' {
        let tail = &text[5..];
        if tail.len() < 2 || !tail.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if tail.as_bytes().get(2) == Some(&b'-') {
            return None;
        }
        let week: u32 = tail[..2].parse().ok()?;
        let (weekday, rest) = match tail.as_bytes().get(2) {
            Some(digit) if digit.is_ascii_digit() => (tail[2..3].parse().ok()?, &tail[3..]),
            _ => (1, &tail[2..]),
        };
        if !(1..=7).contains(&weekday) {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, weekday_as_monday0(weekday))?;
        return Some((date, rest));
    }
    if bytes.len() > 4 && bytes[4] == b'-' {
        // `YYYY-Www` extended week form.
        if bytes.get(5) == Some(&b'W') {
            let tail = &text[6..];
            if tail.len() < 2 || !tail.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let week: u32 = tail[..2].parse().ok()?;
            let (weekday, rest) = match tail[2..].strip_prefix('-') {
                Some(day) => {
                    let digit = day.as_bytes().first()?;
                    if !digit.is_ascii_digit() {
                        return None;
                    }
                    (day[..1].parse().ok()?, &day[1..])
                }
                None => (1, &tail[2..]),
            };
            if !(1..=7).contains(&weekday) {
                return None;
            }
            let date = chrono::NaiveDate::from_isoywd_opt(year, week, weekday_as_monday0(weekday))?;
            return Some((date, rest));
        }
        // `YYYY-MM-DD`, strictly zero-padded (byte-checked before
        // slicing so non-ASCII input rejects instead of panicking).
        if bytes.len() < 10 || bytes[7] != b'-' {
            return None;
        }
        if !bytes[5..7].iter().all(|b| b.is_ascii_digit())
            || !bytes[8..10].iter().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let (month, day): (u32, u32) = (text[5..7].parse().ok()?, text[8..10].parse().ok()?);
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, &text[10..]));
    }
    // Basic `YYYYMMDD`.
    if bytes.len() < 8 || !bytes[4..8].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (month, day): (u32, u32) = (text[4..6].parse().ok()?, text[6..8].parse().ok()?);
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    Some((date, &text[8..]))
}

fn weekday_as_monday0(weekday: u32) -> chrono::Weekday {
    match weekday {
        1 => chrono::Weekday::Mon,
        2 => chrono::Weekday::Tue,
        3 => chrono::Weekday::Wed,
        4 => chrono::Weekday::Thu,
        5 => chrono::Weekday::Fri,
        6 => chrono::Weekday::Sat,
        _ => chrono::Weekday::Sun,
    }
}

/// Parse the time tail (time + optional fraction + optional offset).
/// Every component is strictly two digits (the C parser's 2-char
/// windows); 1-digit times belong to the regex arm, which accepts the
/// regex-reachable ones with identical values.
fn parse_iso_time(date: chrono::NaiveDate, rest: &str) -> Option<ParsedDt> {
    let (hour, rest) = take_time2(rest)?;
    if hour > 23 {
        return None;
    }
    let (minute, second, rest) = if let Some(tail) = rest.strip_prefix(':') {
        let (minute, rest) = take_time2(tail)?;
        let (second, rest) = if let Some(tail) = rest.strip_prefix(':') {
            let (second, rest) = take_time2(tail)?;
            (second, rest)
        } else {
            (0, rest)
        };
        (minute, second, rest)
    } else if rest.len() >= 2 && rest.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
        // Basic `HHMM[SS]` (no colon after the hour).
        let minute: u32 = rest[..2].parse().ok()?;
        let tail = &rest[2..];
        if tail.len() >= 2 && tail.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
            let second: u32 = tail[..2].parse().ok()?;
            (minute, second, &tail[2..])
        } else {
            (minute, 0, tail)
        }
    } else {
        (0, 0, rest)
    };
    if minute > 59 || second > 59 {
        return None;
    }
    // The fraction (`.` or `,`) is always a seconds fraction — even on
    // the hour or minute (`T10.5` is 10:00:00.5, verified) — truncated
    // past 6 digits and right-padded to microseconds.
    let (micros, frac_len, rest) = match rest.strip_prefix(['.', ',']) {
        Some(frac) => {
            let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                return None;
            }
            let mut micros = digits[..digits.len().min(6)].to_owned();
            while micros.len() < 6 {
                micros.push('0');
            }
            (micros.parse().ok()?, digits.len(), &frac[digits.len()..])
        }
        None => (0, 0, rest),
    };
    let naive = date.and_hms_micro_opt(hour, minute, second, micros)?;
    if rest.is_empty() {
        return Some(ParsedDt::Naive(naive));
    }
    // One gap byte before the tz (the C parser ignores a single
    // trailing time char when a tz follows) — but only past a
    // component boundary or 6+ fraction digits: a short fraction must
    // abut the tz, longer gaps and multibyte whitespace reject (all
    // probed).
    let gap_ok = frac_len == 0 || frac_len >= 6;
    let tz_text = match rest.as_bytes().first() {
        Some(b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c' | 0x1c..=0x1f) if gap_ok => {
            &rest[1..]
        }
        _ => rest,
    };
    let offset_micros = parse_iso_offset(tz_text)?;
    let utc = naive.and_utc() - chrono::Duration::microseconds(offset_micros);
    Some(ParsedDt::Aware(utc))
}

/// One strictly-two-digit time component (the C parser reads 2-char
/// windows; unlike the regex arm, nothing here backtracks narrower).
fn take_time2(text: &str) -> Option<(u32, &str)> {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_digit() && bytes[1].is_ascii_digit() {
        return Some((text[..2].parse().ok()?, &text[2..]));
    }
    None
}

/// `Z` (uppercase only) or a numeric offset, returned as signed total
/// microseconds; the total must stay strictly under 24h (CPython raises
/// past it, which DRF suppresses into the invalid arm). A seconds
/// fraction (`.`/`,`, any digit count, truncated to 6) may follow any
/// component count — `+05.5` is 5 hours plus half a second — and a
/// fraction-only offset (zero hours/minutes/seconds) collapses to UTC,
/// sign included (both probed on CPython 3.12, whose C parser differs
/// from `_pydatetime` here).
fn parse_iso_offset(text: &str) -> Option<i64> {
    if text == "Z" {
        return Some(0);
    }
    let bytes = text.as_bytes();
    let sign = match bytes.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits = &text[1..];
    // The fraction attaches to the offset body in any spelling; the
    // remainder after the separator must be ASCII digits only.
    let (clock, micros) = match digits.find(['.', ',']) {
        Some(pos) => {
            let frac = &digits[pos + 1..];
            if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let mut padded = frac[..frac.len().min(6)].to_owned();
            while padded.len() < 6 {
                padded.push('0');
            }
            (&digits[..pos], padded.parse().ok()?)
        }
        None => (digits, 0),
    };
    let (hours, minutes, seconds) = if clock.contains(':') {
        // Extended form: every present part is exactly two digits.
        let mut parts = clock.split(':');
        let hour_text = parts.next()?;
        let minute_text = parts.next()?;
        if hour_text.len() != 2 || minute_text.len() != 2 {
            return None;
        }
        let hours: i64 = hour_text.parse().ok()?;
        let minutes: i64 = minute_text.parse().ok()?;
        let seconds: i64 = match parts.next() {
            Some(part) => {
                if part.len() != 2 {
                    return None;
                }
                part.parse().ok()?
            }
            None => 0,
        };
        if parts.next().is_some() {
            return None;
        }
        (hours, minutes, seconds)
    } else {
        if clock.len() != 2 && clock.len() != 4 && clock.len() != 6 {
            return None;
        }
        if !clock.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let hours: i64 = clock[..2].parse().ok()?;
        let minutes: i64 = clock
            .get(2..4)
            .map(str::parse)
            .transpose()
            .ok()?
            .unwrap_or(0);
        let seconds: i64 = clock
            .get(4..6)
            .map(str::parse)
            .transpose()
            .ok()?
            .unwrap_or(0);
        (hours, minutes, seconds)
    };
    if hours < 0 || minutes < 0 || seconds < 0 {
        return None;
    }
    // A fraction-only offset is UTC (probed: `+00:00:00.5` carries
    // `timezone.utc`, not a half-second offset).
    if hours == 0 && minutes == 0 && seconds == 0 {
        return Some(0);
    }
    // Minutes/seconds are unchecked (`+00:61` is accepted); only the
    // total must stay strictly under a day, in microseconds.
    let total = (hours * 3600 + minutes * 60 + seconds) * 1_000_000 + micros;
    if total >= 86_400_000_000 {
        return None;
    }
    Some(sign * total)
}

/// Arm 2: Django's `datetime_re` fallback (`django/utils/dateparse.py`,
/// tried when `fromisoformat` raises) — `YYYY-M-D[T ]H:M[:S[.ffffff]]`
/// with 1-2 digit parts, `T`/space separator only, `.`/`,` fraction of
/// at most 12 digits (right-padded to microseconds, Django's
/// `ljust(6, "0")`), `re \s*` before an optional `Z`/`±HH`/`±HHMM`/
/// `±HH:MM` zone (parts unchecked, total under 24h), and a `$` end
/// (which also matches one trailing newline — probed).
fn parse_regex_datetime(text: &str) -> Option<ParsedDt> {
    let bytes = text.as_bytes();
    if bytes.len() < 5 || !bytes[..4].iter().all(|b| b.is_ascii_digit()) || bytes[4] != b'-' {
        return None;
    }
    let year: i32 = text[..4].parse().ok()?;
    let (month, rest) = parse_regex_part(&text[5..], b"-")?;
    let (day, rest) = parse_regex_part(rest, b"T ")?;
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let (hour, rest) = parse_regex_part(rest, b":")?;
    if hour > 23 {
        return None;
    }
    // Greedy two digits when present, else one: a narrower retry after
    // a shape failure always fails on its leftover digit, so the
    // regex backtrack is unobservable here (unlike the required
    // followers above). A range failure rejects outright (Django
    // matches greedily, then `datetime()` raises).
    let (minute, rest) = take_regex_greedy(rest)?;
    if minute > 59 {
        return None;
    }
    let (second, micros, rest) = match rest.strip_prefix(':') {
        Some(tail) => {
            let (second, rest) = take_regex_greedy(tail)?;
            if second > 59 {
                return None;
            }
            let (micros, rest) = parse_regex_fraction(rest)?;
            (second, micros, rest)
        }
        None => (0, 0, rest),
    };
    parse_regex_zoned(date, hour, minute, second, micros, rest)
}

/// One 1-2 digit regex part followed by one of `follows` (tries two
/// digits, then one — the regex backtrack). All indexes are
/// ASCII-anchored, so slicing cannot panic.
fn parse_regex_part<'a>(text: &'a str, follows: &[u8]) -> Option<(u32, &'a str)> {
    let bytes = text.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && follows.contains(&bytes[2])
    {
        return Some((text[..2].parse().ok()?, &text[3..]));
    }
    if bytes.len() >= 2 && bytes[0].is_ascii_digit() && follows.contains(&bytes[1]) {
        return Some((text[..1].parse().ok()?, &text[2..]));
    }
    None
}

/// Greedy one-or-two ASCII digits (two when present).
fn take_regex_greedy(text: &str) -> Option<(u32, &str)> {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_digit() && bytes[1].is_ascii_digit() {
        return Some((text[..2].parse().ok()?, &text[2..]));
    }
    if !bytes.is_empty() && bytes[0].is_ascii_digit() {
        return Some((text[..1].parse().ok()?, &text[1..]));
    }
    None
}

/// Optional regex fraction (`.`/`,`, 1-12 ASCII digits, first 6 kept
/// right-padded).
fn parse_regex_fraction(text: &str) -> Option<(u32, &str)> {
    let Some(frac) = text.strip_prefix(['.', ',']) else {
        return Some((0, text));
    };
    let len = frac.bytes().take_while(|b| b.is_ascii_digit()).count();
    if len == 0 || len > 12 {
        return None;
    }
    let mut padded = frac[..len.min(6)].to_owned();
    while padded.len() < 6 {
        padded.push('0');
    }
    Some((padded.parse().ok()?, &frac[len..]))
}

/// The regex tail: `re \s*` (shared [`py_is_space`]), an optional
/// `Z`/`±HH`/`±HHMM`/`±HH:MM` zone, and the `$` end.
fn parse_regex_zoned(
    date: chrono::NaiveDate,
    hour: u32,
    minute: u32,
    second: u32,
    micros: u32,
    rest: &str,
) -> Option<ParsedDt> {
    let naive = date.and_hms_micro_opt(hour, minute, second, micros)?;
    let rest = rest.trim_start_matches(py_is_space);
    // A leading `Z`/sign commits to the zone (the zoneless end can
    // only be empty or one newline, which neither starts).
    if let Some(tail) = rest.strip_prefix('Z') {
        return rest_end_ok(tail).then_some(ParsedDt::Aware(naive.and_utc()));
    }
    if rest.starts_with(['+', '-']) {
        let (offset_micros, tail) = parse_regex_zone(rest)?;
        if !rest_end_ok(tail) {
            return None;
        }
        let utc = naive.and_utc() - chrono::Duration::microseconds(offset_micros);
        return Some(ParsedDt::Aware(utc));
    }
    rest_end_ok(rest).then_some(ParsedDt::Naive(naive))
}

/// Regex zone (`±HH`, `±HHMM`, `±HH:MM`): parts unchecked, total under
/// 24h (`get_fixed_timezone`). Returns signed microseconds.
fn parse_regex_zone(text: &str) -> Option<(i64, &str)> {
    let bytes = text.as_bytes();
    let sign: i64 = match bytes.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let tail = &text[1..];
    let tail_bytes = tail.as_bytes();
    if tail_bytes.len() < 2 || !tail_bytes[..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = tail[..2].parse().ok()?;
    let rest = &tail[2..];
    let (minutes, rest) = match rest.strip_prefix(':') {
        Some(tail) => {
            let tail_bytes = tail.as_bytes();
            if tail_bytes.len() < 2 || !tail_bytes[..2].iter().all(|b| b.is_ascii_digit()) {
                return None;
            }
            (tail[..2].parse().ok()?, &tail[2..])
        }
        None => {
            if rest.len() >= 2 && rest.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
                (rest[..2].parse().ok()?, &rest[2..])
            } else {
                (0, rest)
            }
        }
    };
    let total = hours * 3600 + minutes * 60;
    if total >= 86_400 {
        return None;
    }
    Some((sign * total * 1_000_000, rest))
}

/// Regex `$`: end of input, or one trailing newline.
fn rest_end_ok(rest: &str) -> bool {
    rest.is_empty() || rest == "\n"
}

// ---------------------------------------------------------------------------
// PATCH validation (DRF `WorkSpaceMemberSerializer`, partial)
// ---------------------------------------------------------------------------

/// Field errors in wire order, then `non_field_errors` last (DRF runs
/// object validators after fields).
#[derive(Debug, Default)]
struct FieldErrors {
    deleted_at: Vec<String>,
    role: Vec<String>,
    company_role: Vec<String>,
    view_props: Vec<String>,
    default_props: Vec<String>,
    issue_props: Vec<String>,
    is_active: Vec<String>,
    getting_started_checklist: Vec<String>,
    tips: Vec<String>,
    explored_features: Vec<String>,
    created_by: Vec<String>,
    updated_by: Vec<String>,
    workspace: Vec<String>,
    non_field_errors: Vec<String>,
}

impl FieldErrors {
    fn is_empty(&self) -> bool {
        self.deleted_at.is_empty()
            && self.role.is_empty()
            && self.company_role.is_empty()
            && self.view_props.is_empty()
            && self.default_props.is_empty()
            && self.issue_props.is_empty()
            && self.is_active.is_empty()
            && self.getting_started_checklist.is_empty()
            && self.tips.is_empty()
            && self.explored_features.is_empty()
            && self.created_by.is_empty()
            && self.updated_by.is_empty()
            && self.workspace.is_empty()
            && self.non_field_errors.is_empty()
    }

    fn into_value(self) -> Value {
        let mut errors = Map::new();
        let mut push = |name: &str, messages: Vec<String>| {
            if !messages.is_empty() {
                errors.insert(
                    name.to_owned(),
                    Value::Array(messages.into_iter().map(Value::String).collect()),
                );
            }
        };
        push("deleted_at", self.deleted_at);
        push("role", self.role);
        push("company_role", self.company_role);
        push("view_props", self.view_props);
        push("default_props", self.default_props);
        push("issue_props", self.issue_props);
        push("is_active", self.is_active);
        push("getting_started_checklist", self.getting_started_checklist);
        push("tips", self.tips);
        push("explored_features", self.explored_features);
        push("created_by", self.created_by);
        push("updated_by", self.updated_by);
        push("workspace", self.workspace);
        push("non_field_errors", self.non_field_errors);
        Value::Object(errors)
    }
}

/// Validated PATCH assignments: `None` means the key was absent.
#[derive(Debug, Default)]
struct PatchSets {
    role: Option<i32>,
    company_role: Option<Option<String>>,
    view_props: Option<Value>,
    default_props: Option<Value>,
    issue_props: Option<Value>,
    is_active: Option<bool>,
    getting_started_checklist: Option<Value>,
    tips: Option<Value>,
    explored_features: Option<Value>,
    workspace: Option<Uuid>,
    created_by: Option<Option<Uuid>>,
    updated_by: Option<Option<Uuid>>,
    deleted_at: Option<Option<DateTime<Utc>>>,
}

/// `role` through DRF's `ChoiceField` over `[20, 15, 5]` (the model
/// carries `choices`, `db/models/workspace.py:19`): the lookup key is
/// Python `str()` of the input, so `"15"` coerces but `5.0` does not.
fn validate_role_choice(value: &Value, errors: &mut FieldErrors) -> Option<i32> {
    if *value == Value::Null {
        errors.role.push("This field may not be null.".to_owned());
        return None;
    }
    let display = python_str(value);
    let coerced = match display.as_str() {
        "20" => Some(qm::ROLE_ADMIN),
        "15" => Some(qm::ROLE_MEMBER),
        "5" => Some(qm::ROLE_GUEST),
        _ => None,
    };
    match coerced {
        Some(role) => Some(role),
        None => {
            errors
                .role
                .push(format!("\"{display}\" is not a valid choice."));
            None
        }
    }
}

/// `company_role` through DRF's `CharField` (`TextField(null=True,
/// blank=True)`): null passes through, numerics coerce via `str()`,
/// blank (post-trim) returns `""`, and `\x00` fails the null-characters
/// validator. (`allow_blank` short-circuits before the NUL check, exactly
/// as `CharField.run_validation` orders it.)
fn validate_company_role(value: &Value, errors: &mut FieldErrors) -> Option<Option<String>> {
    if *value == Value::Null {
        return Some(None);
    }
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => py_num_str(number),
        _ => {
            errors.company_role.push("Not a valid string.".to_owned());
            return None;
        }
    };
    let trimmed = py_strip(&text);
    if trimmed.is_empty() {
        return Some(Some(String::new()));
    }
    if trimmed.contains('\0') {
        errors
            .company_role
            .push("Null characters are not allowed.".to_owned());
        return None;
    }
    Some(Some(trimmed.to_owned()))
}

/// `is_active` through DRF's `BooleanField`: the `TRUE_VALUES` /
/// `FALSE_VALUES` sets (case-insensitive for strings, `1`/`1.0` true,
/// `0`/`0.0` false), anything else invalid.
fn validate_bool(value: &Value, errors: &mut FieldErrors) -> Option<bool> {
    if *value == Value::Null {
        errors
            .is_active
            .push("This field may not be null.".to_owned());
        return None;
    }
    match value {
        Value::Bool(flag) => Some(*flag),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return match int {
                    1 => Some(true),
                    0 => Some(false),
                    _ => {
                        errors.is_active.push("Must be a valid boolean.".to_owned());
                        None
                    }
                };
            }
            if let Some(uint) = number.as_u64() {
                return match uint {
                    1 => Some(true),
                    0 => Some(false),
                    _ => {
                        errors.is_active.push("Must be a valid boolean.".to_owned());
                        None
                    }
                };
            }
            match number.as_f64() {
                // `1.0 == 1` and `0.0 == 0` are set members in Python.
                Some(1.0) => Some(true),
                Some(0.0) => Some(false),
                _ => {
                    errors.is_active.push("Must be a valid boolean.".to_owned());
                    None
                }
            }
        }
        Value::String(text) => {
            let lowered = text.to_lowercase();
            match lowered.as_str() {
                "t" | "y" | "yes" | "true" | "on" | "1" => Some(true),
                "f" | "n" | "no" | "false" | "off" | "0" => Some(false),
                _ => {
                    errors.is_active.push("Must be a valid boolean.".to_owned());
                    None
                }
            }
        }
        _ => {
            errors.is_active.push("Must be a valid boolean.".to_owned());
            None
        }
    }
}

/// A JSON props field through DRF's `JSONField` (non-binary, non-HTML):
/// anything but null passes through as-is.
fn validate_json_prop(value: &Value, slot: &mut Vec<String>) -> Option<Value> {
    if *value == Value::Null {
        slot.push("This field may not be null.".to_owned());
        return None;
    }
    Some(value.clone())
}

/// One UUID FK value through `PrimaryKeyRelatedField`: `""` converts
/// to `None` first (`RelatedField.run_validation` → `validate_empty_values`,
/// so `allow_null` decides between a valid `Null` and the null message),
/// bools fail `incorrect_type`, non-negative ints are tiny UUIDs, and
/// every other input is the fancy-quote field error — Django's
/// `UUIDField.to_python` raises `ValidationError`, which
/// `Serializer.to_internal_value` catches per-field
/// (`serializers.py:502-504`), so nothing escapes to `handle_exception`.
/// Note: `arbitrary_precision` keeps big-int text exact, so past-`u64`
/// inputs render their full digits (probed: 2**128 shows all 39
/// digits) — there is no float-repr residual here.
enum FkValue {
    /// Field error already pushed.
    Invalid,
    Null,
    Id(Uuid),
}

/// The `UUIDField.to_python` failure message (`fields/__init__.py`):
/// fancy quotes around the `str()` display.
fn push_invalid_uuid(value: &Value, slot: &mut Vec<String>) -> FkValue {
    slot.push(format!(
        "\u{201c}{}\u{201d} is not a valid UUID.",
        python_str(value)
    ));
    FkValue::Invalid
}

fn validate_fk_uuid(value: &Value, allow_null: bool, slot: &mut Vec<String>) -> FkValue {
    match value {
        Value::Null => {
            if allow_null {
                return FkValue::Null;
            }
            slot.push("This field may not be null.".to_owned());
            FkValue::Invalid
        }
        Value::Bool(_) => {
            slot.push("Incorrect type. Expected pk value, received bool.".to_owned());
            FkValue::Invalid
        }
        Value::Number(number) => {
            // `int` inputs take the `int=` form (`uuid.UUID(int=5)` is a
            // valid tiny UUID); negatives fail `to_python` like every
            // other non-UUID input.
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return push_invalid_uuid(value, slot);
                }
                return FkValue::Id(Uuid::from_u128(int as u128));
            }
            if let Some(uint) = number.as_u64() {
                return FkValue::Id(Uuid::from_u128(u128::from(uint)));
            }
            // Floats take the `hex=` form and fail `to_python`
            // (`AttributeError`, caught alongside `ValueError`); unbounded
            // ints are tiny UUIDs in range, fancy-quote past 2^128.
            match number.as_f64() {
                Some(_) => push_invalid_uuid(value, slot),
                None => match number.to_string().parse::<u128>() {
                    Ok(big) => FkValue::Id(Uuid::from_u128(big)),
                    Err(_) => push_invalid_uuid(value, slot),
                },
            }
        }
        Value::String(raw) => {
            // `""` is `None` before validation (`validate_empty_values`).
            if raw.is_empty() {
                if allow_null {
                    return FkValue::Null;
                }
                slot.push("This field may not be null.".to_owned());
                return FkValue::Invalid;
            }
            match Uuid::parse_str(raw) {
                Ok(id) => FkValue::Id(id),
                Err(_) => push_invalid_uuid(value, slot),
            }
        }
        Value::Array(_) | Value::Object(_) => push_invalid_uuid(value, slot),
    }
}

/// `deleted_at` through DRF's `DateTimeField` (`null=True`): null passes
/// through, strings parse as ISO-8601 (naive values attach the request
/// timezone, aware values convert), anything else fails with the
/// humanized-format message. An aware instant outside Python's year
/// range is the `overflow` field error (400): `enforce_timezone` catches
/// the `OverflowError` from `value.astimezone(field_timezone)`
/// (`fields.py:1154-1157`). CPython raises on the UTC-side subtraction
/// first, so the check covers both representations — a UTC-out-of-range
/// instant overflows in every request timezone, and an in-range instant
/// still overflows when its request-tz wall date leaves the range.
fn validate_deleted_at(
    value: &Value,
    timezone: &chrono_tz::Tz,
    errors: &mut FieldErrors,
) -> Result<Option<Option<DateTime<Utc>>>, Denial> {
    fn invalid(errors: &mut FieldErrors) -> Result<Option<Option<DateTime<Utc>>>, Denial> {
        errors.deleted_at.push(format!(
            "Datetime has wrong format. Use one of these formats instead: {ISO_FORMAT_HINT}."
        ));
        Ok(None)
    }
    if *value == Value::Null {
        return Ok(Some(None));
    }
    let Value::String(text) = value else {
        return invalid(errors);
    };
    match parse_django_datetime(text) {
        Some(ParsedDt::Aware(utc)) => {
            let wall = utc.with_timezone(timezone).date_naive();
            if python_range_contains(&utc.date_naive()) && python_range_contains(&wall) {
                Ok(Some(Some(utc)))
            } else {
                errors
                    .deleted_at
                    .push("Datetime value out of range.".to_owned());
                Ok(None)
            }
        }
        Some(ParsedDt::Naive(naive)) => Ok(Some(Some(attach_request_tz(&naive, timezone)))),
        None => invalid(errors),
    }
}

/// Python `datetime` range (`0001-01-01` through `9999-12-31`): DRF's
/// `astimezone` raises past it, on either representation (the UTC-side
/// subtraction or the request-tz wall). `chrono_tz` offset lookups are
/// infallible (they extrapolate past the transition tables), so an
/// out-of-range wall date is the only overflow signal.
fn python_range_contains(date: &chrono::NaiveDate) -> bool {
    *date >= chrono::NaiveDate::from_ymd_opt(1, 1, 1).expect("min date")
        && *date <= chrono::NaiveDate::from_ymd_opt(9999, 12, 31).expect("max date")
}

/// DRF `enforce_timezone` for naive input: attach the request timezone
/// (`make_aware`, fold 0; a DST-gap wall time takes the pre-transition
/// offset, like zoneinfo's non-raising attach).
fn attach_request_tz(naive: &chrono::NaiveDateTime, timezone: &chrono_tz::Tz) -> DateTime<Utc> {
    use chrono::{MappedLocalTime, TimeZone};
    match timezone.from_local_datetime(naive) {
        MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) => {
            local.with_timezone(&Utc)
        }
        MappedLocalTime::None => {
            // DST gap: take the offset valid just before it (the fold-0
            // attach zoneinfo performs without raising).
            let probe = *naive - chrono::Duration::hours(1);
            match timezone.from_local_datetime(&probe) {
                MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) => {
                    let shift = local.timestamp() - probe.and_utc().timestamp();
                    naive.and_utc() - chrono::Duration::seconds(shift)
                }
                MappedLocalTime::None => naive.and_utc(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rows + fetching + rendering
// ---------------------------------------------------------------------------

/// One list/retrieve row: the member columns plus the nested-user columns
/// (`select_related("member")`) and the avatar-asset context
/// (`select_related("member__avatar_asset")` plus the asset's workspace
/// slug for the attachment/description URL branches).
#[derive(Debug, Clone, sqlx::FromRow)]
struct MemberFullRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    role: i16,
    company_role: Option<String>,
    view_props: Value,
    default_props: Value,
    issue_props: Value,
    is_active: bool,
    getting_started_checklist: Value,
    tips: Value,
    explored_features: Value,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    workspace_id: Uuid,
    u_id: Uuid,
    u_first_name: String,
    u_last_name: String,
    u_avatar: String,
    u_is_bot: bool,
    u_display_name: String,
    u_email: Option<String>,
    u_last_login_medium: String,
    fa_id: Option<Uuid>,
    fa_entity_type: Option<String>,
    fa_workspace_id: Option<Uuid>,
    fa_project_id: Option<Uuid>,
    fa_issue_id: Option<Uuid>,
    fa_workspace_slug: Option<String>,
}

/// The R1 list/retrieve SELECT head: scope + `select_related` joins. No
/// `is_active` filter (ported bug B5); the avatar join is a bare `LEFT`
/// join (the FK is nullable) with no manager scope — `select_related`
/// never applies the related manager, so a soft-deleted avatar asset
/// still attaches and its `asset_url` renders — and the asset-workspace
/// join is likewise unscoped (forward FK descriptors read through the
/// unscoped base manager, so a soft-deleted asset workspace still
/// renders its slug). The head ends right after `slug = ` so
/// [`member_rows_builder`] can bind the slug as `$1` itself — a literal
/// `$1` here would duplicate the bind and 500 every list/retrieve.
const MEMBER_SELECT: &str = "SELECT wm.id AS id, wm.created_at AS created_at, \
     wm.updated_at AS updated_at, wm.deleted_at AS deleted_at, wm.role AS role, \
     wm.company_role AS company_role, wm.view_props AS view_props, \
     wm.default_props AS default_props, wm.issue_props AS issue_props, \
     wm.is_active AS is_active, \
     wm.getting_started_checklist AS getting_started_checklist, wm.tips AS tips, \
     wm.explored_features AS explored_features, wm.created_by_id AS created_by_id, \
     wm.updated_by_id AS updated_by_id, wm.workspace_id AS workspace_id, \
     u.id AS u_id, u.first_name AS u_first_name, \
     u.last_name AS u_last_name, u.avatar AS u_avatar, u.is_bot AS u_is_bot, \
     u.display_name AS u_display_name, u.email AS u_email, \
     u.last_login_medium AS u_last_login_medium, fa.id AS fa_id, \
     fa.entity_type AS fa_entity_type, fa.workspace_id AS fa_workspace_id, \
     fa.project_id AS fa_project_id, fa.issue_id AS fa_issue_id, \
     faw.slug AS fa_workspace_slug FROM workspace_members wm \
     JOIN users u ON u.id = wm.member_id \
     LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id \
     LEFT JOIN workspaces faw ON faw.id = fa.workspace_id \
     WHERE wm.workspace_id = (SELECT id FROM workspaces WHERE slug = ";

/// Build the list/retrieve statement: the slug binds as `$1`, then the
/// soft-delete guard, the optional `?search=` terms (ANDed, `icontains`
/// over display/first name), the optional target, and the `-created_at`
/// ordering. Ported bug B5: no `is_active` filter. Split from execution
/// so tests can assert the exact statement + placeholder numbering via
/// `.sql()`.
fn member_rows_builder<'q>(
    slug: &'q str,
    terms: &[String],
    target: Option<&Uuid>,
    one: bool,
) -> sqlx::QueryBuilder<'q, sqlx::Postgres> {
    let mut qb = sqlx::QueryBuilder::new(MEMBER_SELECT);
    qb.push_bind(slug);
    qb.push(") AND wm.deleted_at IS NULL ");
    for term in terms {
        // `icontains` is `UPPER(col::text) LIKE UPPER($N)` on Postgres
        // (`backends/postgresql/base.py` + `lookups.py`), not `ILIKE`:
        // upper- and lower-case folding disagree on Turkic-I and
        // Greek-sigma pairs, so only the `UPPER` form matches row for
        // row (the `app_pages` search precedent).
        qb.push("AND (UPPER(u.display_name::text) LIKE UPPER(");
        qb.push_bind(like_param(term));
        qb.push(") OR UPPER(u.first_name::text) LIKE UPPER(");
        qb.push_bind(like_param(term));
        qb.push(")) ");
    }
    if let Some(id) = target {
        qb.push("AND wm.id = ");
        qb.push_bind(*id);
        qb.push(" ");
    }
    qb.push("ORDER BY wm.created_at DESC");
    if one {
        qb.push(" LIMIT 1");
    }
    qb
}

/// Fetch list/retrieve rows through [`member_rows_builder`].
async fn fetch_member_rows(
    pool: &sqlx::PgPool,
    slug: &str,
    terms: &[String],
    target: Option<&Uuid>,
    one: bool,
) -> Result<Vec<MemberFullRow>, Denial> {
    member_rows_builder(slug, terms, target, one)
        .build_query_as::<MemberFullRow>()
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)
}

/// Fetch one row by id alone: the post-PATCH re-render reads what was
/// saved (Django renders the in-memory instance unconditionally), so no
/// scope predicate applies — a workspace change or a `deleted_at` stamp
/// still 200s.
async fn fetch_member_row_by_id(
    pool: &sqlx::PgPool,
    id: &Uuid,
) -> Result<Option<MemberFullRow>, Denial> {
    sqlx::query_as(
        "SELECT wm.id AS id, wm.created_at AS created_at, wm.updated_at AS updated_at, \
         wm.deleted_at AS deleted_at, wm.role AS role, wm.company_role AS company_role, \
         wm.view_props AS view_props, wm.default_props AS default_props, \
         wm.issue_props AS issue_props, wm.is_active AS is_active, \
         wm.getting_started_checklist AS getting_started_checklist, wm.tips AS tips, \
         wm.explored_features AS explored_features, wm.created_by_id AS created_by_id, \
         wm.updated_by_id AS updated_by_id, wm.workspace_id AS workspace_id, \
         u.id AS u_id, u.first_name AS u_first_name, u.last_name AS u_last_name, \
         u.avatar AS u_avatar, u.is_bot AS u_is_bot, u.display_name AS u_display_name, \
         u.email AS u_email, u.last_login_medium AS u_last_login_medium, fa.id AS fa_id, \
         fa.entity_type AS fa_entity_type, fa.workspace_id AS fa_workspace_id, \
         fa.project_id AS fa_project_id, fa.issue_id AS fa_issue_id, \
         faw.slug AS fa_workspace_slug FROM workspace_members wm \
         JOIN users u ON u.id = wm.member_id \
         LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id \
         LEFT JOIN workspaces faw ON faw.id = fa.workspace_id \
         WHERE wm.id = $1 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// One me-get row: the member columns plus the draft-count annotation.
#[derive(Debug, Clone, sqlx::FromRow)]
struct MeRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    role: i16,
    company_role: Option<String>,
    view_props: Value,
    default_props: Value,
    issue_props: Value,
    is_active: bool,
    getting_started_checklist: Value,
    tips: Value,
    explored_features: Value,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    workspace_id: Uuid,
    member_id: Uuid,
    draft_issue_count: i64,
}

/// The R7 me SELECT: lookup + `draft_issue_count` annotation (the inner
/// `GROUP BY` yields no row on empty matches, hence the `COALESCE`), no
/// explicit ordering beyond the `-created_at` fallback, first row only.
async fn fetch_me_row(
    pool: &sqlx::PgPool,
    user_id: &Uuid,
    slug: &str,
) -> Result<Option<MeRow>, Denial> {
    sqlx::query_as(
        "SELECT wm.id AS id, wm.created_at AS created_at, wm.updated_at AS updated_at, \
         wm.deleted_at AS deleted_at, wm.role AS role, wm.company_role AS company_role, \
         wm.view_props AS view_props, wm.default_props AS default_props, \
         wm.issue_props AS issue_props, wm.is_active AS is_active, \
         wm.getting_started_checklist AS getting_started_checklist, wm.tips AS tips, \
         wm.explored_features AS explored_features, wm.created_by_id AS created_by_id, \
         wm.updated_by_id AS updated_by_id, wm.workspace_id AS workspace_id, \
         wm.member_id AS member_id, COALESCE((SELECT COUNT(d.id) FROM draft_issues d \
         WHERE d.created_by_id = $1 AND d.workspace_id = wm.workspace_id \
         AND d.deleted_at IS NULL GROUP BY d.workspace_id), 0) AS draft_issue_count \
         FROM workspace_members wm WHERE wm.member_id = $1 AND wm.workspace_id = \
         (SELECT id FROM workspaces WHERE slug = $2) AND wm.is_active \
         AND wm.deleted_at IS NULL ORDER BY wm.created_at DESC LIMIT 1",
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// One write-target row (partial_update/destroy): the PK, the user id,
/// and the role.
#[derive(Debug, Clone, Copy, sqlx::FromRow)]
struct WriteTarget {
    id: Uuid,
    member_id: Uuid,
    role: i16,
}

/// The R4/R5 write-target lookup: `.get(pk, slug, member__is_bot=False,
/// is_active)` (the `JOIN users` carries the bot guard).
async fn fetch_write_target(
    pool: &sqlx::PgPool,
    slug: &str,
    pk: &Uuid,
) -> Result<Option<WriteTarget>, Denial> {
    sqlx::query_as(
        "SELECT wm.id AS id, wm.member_id AS member_id, wm.role AS role \
         FROM workspace_members wm \
         JOIN users u ON u.id = wm.member_id AND u.is_bot = FALSE \
         WHERE wm.id = $1 AND wm.workspace_id = (SELECT id FROM workspaces WHERE slug = $2) \
         AND wm.is_active AND wm.deleted_at IS NULL LIMIT 1",
    )
    .bind(pk)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// One project-members row: the `ProjectMemberRoleSerializer` columns.
#[derive(Debug, Clone, sqlx::FromRow)]
struct ProjectMemberRow {
    id: Uuid,
    role: i16,
    member_id: Option<Uuid>,
    project_id: Uuid,
    created_at: DateTime<Utc>,
}

/// `FileAsset.asset_url` (`db/models/asset.py:79-99`) for an attached
/// avatar asset: the four static types render without context, the
/// attachment/description branches need the asset workspace slug (a NULL
/// FK 500s through the `.slug` attribute access, a dangling FK 404s
/// through the FK descriptor), anything else is `None`.
fn asset_url_for_row(row: &MemberFullRow) -> Result<Option<String>, Denial> {
    let asset_id = row.fa_id.expect("attached asset");
    match row.fa_entity_type.as_deref() {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Ok(Some(format!("/api/assets/v2/static/{asset_id}/")))
        }
        Some("ISSUE_ATTACHMENT") => {
            let slug = asset_workspace_slug(row)?;
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{}/issues/{}/attachments/{asset_id}/",
                opt_uuid(&row.fa_project_id),
                opt_uuid(&row.fa_issue_id),
            )))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let slug = asset_workspace_slug(row)?;
            Ok(Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{}/{asset_id}/",
                opt_uuid(&row.fa_project_id),
            )))
        }
        _ => Ok(None),
    }
}

fn asset_workspace_slug(row: &MemberFullRow) -> Result<&str, Denial> {
    match (&row.fa_workspace_slug, &row.fa_workspace_id) {
        (Some(slug), _) => Ok(slug),
        (None, None) => Err(Denial::ServerError),
        (None, Some(_)) => Err(Denial::ObjectNotFound),
    }
}

/// Python f-string of a nullable id (`None` renders `None`).
fn opt_uuid(id: &Option<Uuid>) -> String {
    id.map(|id| id.to_string())
        .unwrap_or_else(|| "None".to_owned())
}

/// `User.avatar_url` (`db/models/user.py:142-151`) for one row: attached
/// means the avatar join hit — `select_related` ignores the asset
/// manager, so a soft-deleted asset still attaches and its URL renders
/// with no fall-through to the text; only a NULL FK falls back to the
/// text, then null.
fn avatar_for_row(row: &MemberFullRow) -> Result<Option<String>, Denial> {
    if row.fa_id.is_none() {
        return Ok(if row.u_avatar.is_empty() {
            None
        } else {
            Some(row.u_avatar.clone())
        });
    }
    let url = asset_url_for_row(row)?;
    Ok(resolve_avatar_url(true, url.as_deref(), &row.u_avatar).map(str::to_owned))
}

/// Render one member row through the plain/admin serializer kernels.
/// `fields` is the ignored `("id", "member", "role")` argument (ported
/// bug B1); datetimes render in the request timezone.
fn render_member(
    row: &MemberFullRow,
    admin: bool,
    timezone: &chrono_tz::Tz,
) -> Result<Value, Denial> {
    use crate::serializer::render_datetime_in;
    let id = row.id.to_string();
    let created_at = render_datetime_in(&row.created_at, timezone);
    let updated_at = render_datetime_in(&row.updated_at, timezone);
    let deleted_at = row
        .deleted_at
        .as_ref()
        .map(|dt| render_datetime_in(dt, timezone));
    let created_by = row.created_by_id.map(|id| id.to_string());
    let updated_by = row.updated_by_id.map(|id| id.to_string());
    let workspace = row.workspace_id.to_string();
    let uid = row.u_id.to_string();
    let email = row.u_email.clone();
    let avatar = avatar_for_row(row)?;
    let core = ser_workspace::WorkspaceMemberCore {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        role: i64::from(row.role),
        company_role: row.company_role.as_deref(),
        view_props: &row.view_props,
        default_props: &row.default_props,
        issue_props: &row.issue_props,
        is_active: row.is_active,
        getting_started_checklist: &row.getting_started_checklist,
        tips: &row.tips,
        explored_features: &row.explored_features,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    if admin {
        let lite_row = ser_user::UserAdminLiteRow {
            id: &uid,
            first_name: &row.u_first_name,
            last_name: &row.u_last_name,
            avatar: &row.u_avatar,
            avatar_url: avatar.as_deref(),
            is_bot: row.u_is_bot,
            display_name: &row.u_display_name,
            email: email.as_deref(),
            last_login_medium: &row.u_last_login_medium,
        };
        let member = ser_user::user_admin_lite_to_representation(&lite_row);
        let full_row = ser_workspace::WorkSpaceMemberAdminRow {
            core,
            workspace: &workspace,
            member,
        };
        let view =
            ser_workspace::member_admin_fields_to_representation(&full_row, qm::MEMBER_LIST_FIELDS);
        serde_json::to_value(&view).map_err(|_| Denial::ServerError)
    } else {
        let lite_row = ser_user::UserLiteRow {
            id: &uid,
            first_name: &row.u_first_name,
            last_name: &row.u_last_name,
            avatar: &row.u_avatar,
            avatar_url: avatar.as_deref(),
            is_bot: row.u_is_bot,
            display_name: &row.u_display_name,
        };
        let member = ser_user::user_lite_to_representation(&lite_row);
        let full_row = ser_workspace::WorkSpaceMemberRow {
            core,
            workspace: &workspace,
            member,
        };
        let view =
            ser_workspace::member_fields_to_representation(&full_row, qm::MEMBER_LIST_FIELDS);
        serde_json::to_value(&view).map_err(|_| Denial::ServerError)
    }
}

/// Render one me row through `WorkspaceMemberMeSerializer`: the annotation
/// is always present (the `COALESCE` never yields null), the FKs render
/// as PK strings.
fn render_me(row: &MeRow, timezone: &chrono_tz::Tz) -> Value {
    use crate::serializer::render_datetime_in;
    let id = row.id.to_string();
    let created_at = render_datetime_in(&row.created_at, timezone);
    let updated_at = render_datetime_in(&row.updated_at, timezone);
    let deleted_at = row
        .deleted_at
        .as_ref()
        .map(|dt| render_datetime_in(dt, timezone));
    let created_by = row.created_by_id.map(|id| id.to_string());
    let updated_by = row.updated_by_id.map(|id| id.to_string());
    let workspace = row.workspace_id.to_string();
    let member = row.member_id.to_string();
    let me_row = ser_workspace::WorkspaceMemberMeRow {
        core: ser_workspace::WorkspaceMemberCore {
            id: &id,
            created_at: &created_at,
            updated_at: &updated_at,
            deleted_at: deleted_at.as_deref(),
            role: i64::from(row.role),
            company_role: row.company_role.as_deref(),
            view_props: &row.view_props,
            default_props: &row.default_props,
            issue_props: &row.issue_props,
            is_active: row.is_active,
            getting_started_checklist: &row.getting_started_checklist,
            tips: &row.tips,
            explored_features: &row.explored_features,
            created_by: created_by.as_deref(),
            updated_by: updated_by.as_deref(),
        },
        draft_issue_count: Some(Some(row.draft_issue_count)),
        workspace: &workspace,
        member: &member,
    };
    let view = ser_workspace::member_me_to_representation(&me_row);
    serde_json::to_value(&view).expect("me row serializes")
}

/// The me-get null row: `WorkspaceMemberMeSerializer(None)` renders 14
/// keys (read-only `id`/`draft_issue_count`/`created_at`/`updated_at`
/// skip; the rest null except the `CharField`/`BooleanField` initials) —
/// verified against live DRF, in field order.
fn null_me_row() -> Value {
    let mut row = Map::with_capacity(14);
    row.insert("deleted_at".to_owned(), Value::Null);
    row.insert("role".to_owned(), Value::Null);
    row.insert("company_role".to_owned(), Value::String(String::new()));
    row.insert("view_props".to_owned(), Value::Null);
    row.insert("default_props".to_owned(), Value::Null);
    row.insert("issue_props".to_owned(), Value::Null);
    row.insert("is_active".to_owned(), Value::Bool(false));
    row.insert("getting_started_checklist".to_owned(), Value::Null);
    row.insert("tips".to_owned(), Value::Null);
    row.insert("explored_features".to_owned(), Value::Null);
    row.insert("created_by".to_owned(), Value::Null);
    row.insert("updated_by".to_owned(), Value::Null);
    row.insert("workspace".to_owned(), Value::Null);
    row.insert("member".to_owned(), Value::Null);
    Value::Object(row)
}

/// Render one project-members row through D-25's
/// `ProjectMemberRoleSerializer`: PK strings, `original_role` re-reading
/// `role`, the timestamp in the request timezone.
fn render_project_member(row: &ProjectMemberRow, timezone: &chrono_tz::Tz) -> Map<String, Value> {
    use crate::serializer::render_datetime_in;
    let id = row.id.to_string();
    let member = row.member_id.map(|id| id.to_string());
    let project = row.project_id.to_string();
    let created_at = render_datetime_in(&row.created_at, timezone);
    let role_row = d25::ProjectMemberRoleRow {
        id: &id,
        role: i64::from(row.role),
        member: member.as_deref(),
        project: &project,
        created_at: &created_at,
    };
    let view = d25::member_role_to_representation(&role_row);
    match serde_json::to_value(&view).expect("role row serializes") {
        Value::Object(map) => map,
        _ => unreachable!("role view is an object"),
    }
}

/// Group rendered role rows into the project dict (`member.py:257-264`):
/// the `"project"` key pops out of each row and `str(project_id)` keys
/// the groups, first-seen order.
fn group_project_members(rows: Vec<Map<String, Value>>) -> Map<String, Value> {
    let mut grouped: Map<String, Value> = Map::new();
    for mut row in rows {
        let project = row.remove("project").unwrap_or(Value::Null);
        let key = match &project {
            Value::String(id) => id.clone(),
            _ => project.to_string(),
        };
        grouped
            .entry(key)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("project groups are arrays")
            .push(Value::Object(row));
    }
    grouped
}

// ---------------------------------------------------------------------------
// Cache (leave runs its invalidations before the gate)
// ---------------------------------------------------------------------------

/// Minimal command surface the leave closure needs: `DEL` and
/// `KEYS`-then-`DEL` (the cache-invalidate shape). The shared foundation
/// handle exposes only `SET .. EX` / `GET` / `SUBSCRIBE`, so this module
/// holds its own short-lived multiplexed client off the same `REDIS_URL`
/// — `None` when the URL is unset, in which case no cache exists and the
/// deletes are vacuous.
#[derive(Debug, Clone)]
struct Cache {
    client: redis::Client,
}

impl Cache {
    fn from_state(state: &AppState) -> Option<Self> {
        let url = state
            .settings()
            .redis
            .url
            .as_deref()
            .filter(|u| !u.is_empty())?;
        redis::Client::open(url).ok().map(|client| Self { client })
    }

    async fn del(&self, key: &str) -> Result<(), redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        redis::AsyncCommands::del::<_, ()>(&mut conn, key).await
    }

    /// `cache.keys(pattern)` + `delete_many`: Django's `cache.keys` is the
    /// Redis `KEYS` command (django-redis), so the pattern match is one
    /// `KEYS` here too — never a scan-then-filter subset.
    async fn del_pattern(&self, pattern: &str) -> Result<(), redis::RedisError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let keys: Vec<String> = redis::AsyncCommands::keys(&mut conn, pattern).await?;
        if !keys.is_empty() {
            redis::AsyncCommands::del::<_, ()>(&mut conn, keys).await?;
        }
        Ok(())
    }
}

/// Stored-key form (`default_key_func`, prefix `''` + version `1`): glob
/// patterns match regardless of it, single deletes need the exact key.
fn django_cache_key(key: &str) -> String {
    format!(":1:{key}")
}

/// Run the three leave invalidations (`member.py:152-160`). Redis errors
/// propagate to the 500 (Python lets cache failures escape into
/// `handle_exception`); a missing client is a silent pass (no cache, no
/// keys).
async fn run_leave_invalidations(
    state: &AppState,
    slug: &str,
    actor: &crate::license::Actor,
) -> Result<(), Denial> {
    let Some(cache) = Cache::from_state(state) else {
        return Ok(());
    };
    let user_id = actor.id.to_string();
    for inv in invalidations_for(InvalidateAction::Leave) {
        let (key, multiple) = invalidation_key(inv, slug, Some(&user_id));
        if multiple {
            cache
                .del_pattern(&format!("*{key}*"))
                .await
                .map_err(|_| Denial::ServerError)?;
        } else {
            cache
                .del(&django_cache_key(&key))
                .await
                .map_err(|_| Denial::ServerError)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Writes (cascades, guards, deactivations)
// ---------------------------------------------------------------------------

/// Render one gate denial (shared by [`gated_actor`] and the leave path,
/// which invalidates between auth and the gate check).
fn deny_response(outcome: GateOutcome) -> Response {
    let body = outcome_body(outcome).expect("deny renders");
    let status = match outcome {
        GateOutcome::Unauthenticated => StatusCode::UNAUTHORIZED,
        GateOutcome::MissingSlug => StatusCode::BAD_REQUEST,
        _ => StatusCode::FORBIDDEN,
    };
    json_response(status, body.to_owned())
}

/// The R4 guest-demote cascade (`member.py:89`): `role = 5` over the
/// target's project rows in this workspace. Ported bug B6: no `is_active`
/// filter.
async fn guest_demote_cascade(
    pool: &sqlx::PgPool,
    slug: &str,
    target_user_id: &Uuid,
) -> Result<(), Denial> {
    sqlx::query(
        "UPDATE project_members SET role = 5 WHERE project_members.workspace_id = \
         (SELECT id FROM workspaces WHERE slug = $1) \
         AND project_members.member_id = $2 AND project_members.deleted_at IS NULL",
    )
    .bind(slug)
    .bind(target_user_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// The R5/R6 sole-project-admin probe (`member.py:122-135`, `:176-189`):
/// a project in this workspace whose only member row is an admin for the
/// compared id. The caller picks the id: the WorkspaceMember PK for
/// destroy (ported bug B3 — never fires) and the user id for leave.
async fn sole_project_admin_exists(
    pool: &sqlx::PgPool,
    slug: &str,
    compared_id: &Uuid,
) -> Result<bool, Denial> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM projects p LEFT JOIN project_members pm \
         ON pm.project_id = p.id WHERE p.workspace_id = \
         (SELECT id FROM workspaces WHERE slug = $1) AND p.deleted_at IS NULL \
         GROUP BY p.id HAVING COUNT(pm.id) = 1 \
         AND COUNT(CASE WHEN pm.member_id = $2 AND pm.role = 20 THEN 1 END) = 1)",
    )
    .bind(slug)
    .bind(compared_id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(exists)
}

/// Active admin count for the R6 sole-workspace-admin guard.
async fn workspace_admin_count(pool: &sqlx::PgPool, slug: &str) -> Result<i64, Denial> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workspace_members WHERE workspace_members.workspace_id = \
         (SELECT id FROM workspaces WHERE slug = $1) \
         AND workspace_members.role = 20 AND workspace_members.is_active = TRUE \
         AND workspace_members.deleted_at IS NULL",
    )
    .bind(slug)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(count)
}

/// The R5/R6 project deactivation (`.update(is_active=False)` — no
/// `updated_by` stamp, unlike the member `.save()` below).
async fn deactivate_project_rows(
    pool: &sqlx::PgPool,
    slug: &str,
    target_user_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), Denial> {
    sqlx::query(
        "UPDATE project_members SET is_active = FALSE, updated_at = $3 \
         WHERE project_members.workspace_id = (SELECT id FROM workspaces WHERE slug = $1) \
         AND project_members.member_id = $2 AND project_members.is_active = TRUE \
         AND project_members.deleted_at IS NULL",
    )
    .bind(slug)
    .bind(target_user_id)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// The R5/R6 member deactivation (`.save()` stamps `updated_by` from the
/// request user, `db/models/base.py:23-44`).
async fn deactivate_member(
    pool: &sqlx::PgPool,
    target_id: &Uuid,
    actor_id: &Uuid,
    now: &DateTime<Utc>,
) -> Result<(), Denial> {
    sqlx::query(
        "UPDATE workspace_members SET is_active = FALSE, updated_at = $2, updated_by_id = $3 \
         WHERE id = $1",
    )
    .bind(target_id)
    .bind(now)
    .bind(actor_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(())
}

/// Map a write error: integrity violations (class `23`) are the 400
/// payload arm, anything else the 500.
fn map_write_error(error: sqlx::Error) -> Denial {
    if let Some(db) = error.as_database_error() {
        if db.code().is_some_and(|code| code.starts_with("23")) {
            return Denial::BadPayload;
        }
    }
    Denial::ServerError
}

/// FK existence for PATCH `workspace` (the default manager scope:
/// soft-deleted workspaces do not exist).
async fn workspace_exists(pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workspaces WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// FK existence for PATCH `created_by`/`updated_by` (plain manager).
async fn user_exists(pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)
}

/// Validate one PATCH body in wire order: every field is collected, then
/// the caller saves. There are no object validators — DRF skips both
/// uniqueness validators (`member` is read-only), so a workspace
/// collision surfaces as `IntegrityError` → `BadPayload` at save.
/// Unknown and read-only keys (`id`, `member`, `created_at`,
/// `updated_at`) are silently ignored, like DRF.
#[allow(clippy::result_large_err)]
async fn validate_patch(
    pool: &sqlx::PgPool,
    fields: &Map<String, Value>,
    timezone: &chrono_tz::Tz,
) -> Result<PatchSets, Response> {
    let mut errors = FieldErrors::default();
    let mut sets = PatchSets::default();
    // deleted_at.
    if let Some(value) = fields.get("deleted_at") {
        match validate_deleted_at(value, timezone, &mut errors) {
            Ok(Some(assigned)) => sets.deleted_at = Some(assigned),
            Ok(None) => {}
            Err(denial) => return Err(denial.into_response()),
        }
    }
    // role.
    if let Some(value) = fields.get("role") {
        if let Some(role) = validate_role_choice(value, &mut errors) {
            sets.role = Some(role);
        }
    }
    // company_role.
    if let Some(value) = fields.get("company_role") {
        if let Some(assigned) = validate_company_role(value, &mut errors) {
            sets.company_role = Some(assigned);
        }
    }
    // JSON props.
    if let Some(value) = fields.get("view_props") {
        if let Some(prop) = validate_json_prop(value, &mut errors.view_props) {
            sets.view_props = Some(prop);
        }
    }
    if let Some(value) = fields.get("default_props") {
        if let Some(prop) = validate_json_prop(value, &mut errors.default_props) {
            sets.default_props = Some(prop);
        }
    }
    if let Some(value) = fields.get("issue_props") {
        if let Some(prop) = validate_json_prop(value, &mut errors.issue_props) {
            sets.issue_props = Some(prop);
        }
    }
    // is_active.
    if let Some(value) = fields.get("is_active") {
        if let Some(flag) = validate_bool(value, &mut errors) {
            sets.is_active = Some(flag);
        }
    }
    if let Some(value) = fields.get("getting_started_checklist") {
        if let Some(prop) = validate_json_prop(value, &mut errors.getting_started_checklist) {
            sets.getting_started_checklist = Some(prop);
        }
    }
    if let Some(value) = fields.get("tips") {
        if let Some(prop) = validate_json_prop(value, &mut errors.tips) {
            sets.tips = Some(prop);
        }
    }
    if let Some(value) = fields.get("explored_features") {
        if let Some(prop) = validate_json_prop(value, &mut errors.explored_features) {
            sets.explored_features = Some(prop);
        }
    }
    // FKs (existence is field-level; garbage is the fancy-quote field
    // error, coexisting with every other field error in wire order).
    if let Some(value) = fields.get("created_by") {
        match validate_fk_uuid(value, true, &mut errors.created_by) {
            FkValue::Id(id) => {
                let display = python_str(value);
                match user_exists(pool, &id).await {
                    Ok(true) => sets.created_by = Some(Some(id)),
                    Ok(false) => errors
                        .created_by
                        .push(format!("Invalid pk \"{display}\" - object does not exist.")),
                    Err(denial) => return Err(denial.into_response()),
                }
            }
            FkValue::Null => sets.created_by = Some(None),
            FkValue::Invalid => {}
        }
    }
    if let Some(value) = fields.get("updated_by") {
        match validate_fk_uuid(value, true, &mut errors.updated_by) {
            FkValue::Id(id) => {
                let display = python_str(value);
                match user_exists(pool, &id).await {
                    Ok(true) => sets.updated_by = Some(Some(id)),
                    Ok(false) => errors
                        .updated_by
                        .push(format!("Invalid pk \"{display}\" - object does not exist.")),
                    Err(denial) => return Err(denial.into_response()),
                }
            }
            FkValue::Null => sets.updated_by = Some(None),
            FkValue::Invalid => {}
        }
    }
    if let Some(value) = fields.get("workspace") {
        match validate_fk_uuid(value, false, &mut errors.workspace) {
            FkValue::Id(id) => {
                let display = python_str(value);
                match workspace_exists(pool, &id).await {
                    Ok(true) => sets.workspace = Some(id),
                    Ok(false) => errors
                        .workspace
                        .push(format!("Invalid pk \"{display}\" - object does not exist.")),
                    Err(denial) => return Err(denial.into_response()),
                }
            }
            FkValue::Null => {}
            FkValue::Invalid => {}
        }
    }
    if !errors.is_empty() {
        return Err(Denial::BadFields(errors.into_value()).into_response());
    }
    Ok(sets)
}

/// Apply validated PATCH assignments: the dynamic `SET` plus the
/// `updated_at`/`updated_by` stamps (`BaseModel.save` overwrites an
/// explicit `updated_by` input with the request user, so the stamp wins
/// unconditionally).
async fn apply_patch(
    pool: &sqlx::PgPool,
    target_id: &Uuid,
    actor_id: &Uuid,
    sets: &PatchSets,
) -> Result<(), Denial> {
    let now = Utc::now();
    let mut qb = sqlx::QueryBuilder::new("UPDATE workspace_members SET ");
    {
        let mut sep = qb.separated(", ");
        if let Some(role) = sets.role {
            sep.push("role = ");
            sep.push_bind(i16::try_from(role).expect("role fits"));
        }
        if let Some(company_role) = &sets.company_role {
            sep.push("company_role = ");
            sep.push_bind(company_role.clone());
        }
        if let Some(view_props) = &sets.view_props {
            sep.push("view_props = ");
            sep.push_bind(view_props.clone());
        }
        if let Some(default_props) = &sets.default_props {
            sep.push("default_props = ");
            sep.push_bind(default_props.clone());
        }
        if let Some(issue_props) = &sets.issue_props {
            sep.push("issue_props = ");
            sep.push_bind(issue_props.clone());
        }
        if let Some(is_active) = sets.is_active {
            sep.push("is_active = ");
            sep.push_bind(is_active);
        }
        if let Some(checklist) = &sets.getting_started_checklist {
            sep.push("getting_started_checklist = ");
            sep.push_bind(checklist.clone());
        }
        if let Some(tips) = &sets.tips {
            sep.push("tips = ");
            sep.push_bind(tips.clone());
        }
        if let Some(features) = &sets.explored_features {
            sep.push("explored_features = ");
            sep.push_bind(features.clone());
        }
        if let Some(created_by) = &sets.created_by {
            sep.push("created_by_id = ");
            sep.push_bind(*created_by);
        }
        if let Some(updated_by) = &sets.updated_by {
            // Validated (so garbage still 400s above as a field error)
            // but overwritten by the stamp below, like `BaseModel.save`.
            let _ = updated_by;
        }
        if let Some(workspace) = &sets.workspace {
            sep.push("workspace_id = ");
            sep.push_bind(*workspace);
        }
        if let Some(deleted_at) = &sets.deleted_at {
            sep.push("deleted_at = ");
            sep.push_bind(*deleted_at);
        }
        sep.push("updated_at = ");
        sep.push_bind(now);
        sep.push("updated_by_id = ");
        sep.push_bind(*actor_id);
    }
    qb.push(" WHERE id = ");
    qb.push_bind(*target_id);
    qb.build().execute(pool).await.map_err(map_write_error)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET workspaces/<slug>/members/`: the gate, the requester branch
/// (`role > 5`, literal spelling), then every scoped row in the admin or
/// plain shape.
async fn list_members(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    axum::extract::Query(query): axum::extract::Query<QueryMap>,
) -> Response {
    let actor = match gated_actor(&state, extension, "GET", MEMBERS_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let requester = match requester_lookup(pool, &actor.id, &slug).await {
        Ok(requester) => requester,
        Err(denial) => return denial.into_response(),
    };
    let Some(requester) = requester else {
        return Denial::ObjectNotFound.into_response();
    };
    let terms = search_terms(&query);
    let rows = match fetch_member_rows(pool, &slug, &terms, None, false).await {
        Ok(rows) => rows,
        Err(denial) => return denial.into_response(),
    };
    let admin = qm::is_admin_shape(requester.role);
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        match render_member(row, admin, &actor.timezone) {
            Ok(item) => items.push(item),
            Err(denial) => return denial.into_response(),
        }
    }
    json_response(
        StatusCode::OK,
        drf_escape(&serde_json::to_string(&Value::Array(items)).expect("member list")),
    )
}

/// `GET workspaces/<slug>/members/<pk>/`: the gate, the requester, the
/// target (404 `Workspace member not found`), then the admin-or-plain
/// branch (`role > ROLE.GUEST.value`, enum spelling — same threshold).
async fn retrieve_member(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<QueryMap>,
) -> Response {
    // The `<uuid:pk>` converter runs at URL resolution, before auth.
    let id = match parse_pk(&pk) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let actor = match gated_actor(&state, extension, "GET", MEMBER_DETAIL_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let requester = match requester_lookup(pool, &actor.id, &slug).await {
        Ok(requester) => requester,
        Err(denial) => return denial.into_response(),
    };
    let Some(requester) = requester else {
        return Denial::ObjectNotFound.into_response();
    };
    // `get_queryset()` (search included) plus `.get(pk)`.
    let terms = search_terms(&query);
    let rows = match fetch_member_rows(pool, &slug, &terms, Some(&id), true).await {
        Ok(rows) => rows,
        Err(denial) => return denial.into_response(),
    };
    let Some(row) = rows.into_iter().next() else {
        return Denial::MemberNotFound.into_response();
    };
    let admin = qm::is_admin_shape(requester.role);
    match render_member(&row, admin, &actor.timezone) {
        Ok(item) => json_response(
            StatusCode::OK,
            drf_escape(&serde_json::to_string(&item).expect("member detail")),
        ),
        Err(denial) => denial.into_response(),
    }
}

/// `PATCH workspaces/<slug>/members/<pk>/`: the gate, the target, the
/// self guard, the guest-demote cascade (before validation), the
/// serializer validation, then the save + plain-shape 200.
async fn partial_update_member(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    // The `<uuid:pk>` converter runs at URL resolution, before auth.
    let id = match parse_pk(&pk) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let actor = match gated_actor(&state, extension, "PATCH", MEMBER_DETAIL_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let target = match fetch_write_target(pool, &slug, &id).await {
        Ok(target) => target,
        Err(denial) => return denial.into_response(),
    };
    let Some(target) = target else {
        return Denial::ObjectNotFound.into_response();
    };
    if actor.id == target.member_id {
        return Denial::BadGuard(SELF_ROLE_BODY).into_response();
    }
    let raw_value = match parse_body_json(&body) {
        Ok(value) => value,
        Err(denial) => return denial.into_response(),
    };
    // The cascade pre-check reads the raw body before any dict check:
    // scalar bodies, and `str`/`list` bodies containing `role`, 500.
    let cascade_role: Option<Value> = match cascade_role_lookup(&raw_value) {
        Ok(role) => role,
        Err(denial) => return denial.into_response(),
    };
    let fields = match parse_body_value(raw_value) {
        Ok(fields) => fields,
        Err(denial) => return denial.into_response(),
    };
    // The cascade (`int(data["role"]) == 5`, literal): garbage escapes
    // as a 500, before validation ever runs.
    if let Some(role_value) = cascade_role.as_ref() {
        match python_int(role_value) {
            Some(role) if role == i64::from(qm::GUEST_DEMOTE_ROLE) => {
                if let Err(denial) = guest_demote_cascade(pool, &slug, &target.member_id).await {
                    return denial.into_response();
                }
            }
            Some(_) => {}
            None => return Denial::ServerError.into_response(),
        }
    }
    let sets = match validate_patch(pool, &fields, &actor.timezone).await {
        Ok(sets) => sets,
        Err(invalid) => return invalid,
    };
    if let Err(denial) = apply_patch(pool, &target.id, &actor.id, &sets).await {
        return denial.into_response();
    }
    let row = match fetch_member_row_by_id(pool, &target.id).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let Some(row) = row else {
        return Denial::ObjectNotFound.into_response();
    };
    // Partial update always renders the plain shape (no admin branch).
    match render_member(&row, false, &actor.timezone) {
        Ok(item) => json_response(
            StatusCode::OK,
            drf_escape(&serde_json::to_string(&item).expect("member patch")),
        ),
        Err(denial) => denial.into_response(),
    }
}

/// `DELETE workspaces/<slug>/members/<pk>/`: the gate, the target, the
/// requester, the self/higher-role guards, the (never-firing)
/// sole-project-admin guard, then the deactivations + 204.
async fn destroy_member(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path((slug, pk)): Path<(String, String)>,
) -> Response {
    // The `<uuid:pk>` converter runs at URL resolution, before auth.
    let id = match parse_pk(&pk) {
        Ok(id) => id,
        Err(denial) => return denial.into_response(),
    };
    let actor = match gated_actor(&state, extension, "DELETE", MEMBER_DETAIL_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let target = match fetch_write_target(pool, &slug, &id).await {
        Ok(target) => target,
        Err(denial) => return denial.into_response(),
    };
    let Some(target) = target else {
        return Denial::ObjectNotFound.into_response();
    };
    let requester = match requester_lookup(pool, &actor.id, &slug).await {
        Ok(requester) => requester,
        Err(denial) => return denial.into_response(),
    };
    let Some(requester) = requester else {
        return Denial::ObjectNotFound.into_response();
    };
    if target.id == requester.id {
        return Denial::BadGuard(SELF_REMOVE_BODY).into_response();
    }
    if !qm::can_remove(requester.role, i32::from(target.role)) {
        return Denial::BadGuard(HIGHER_ROLE_BODY).into_response();
    }
    // Ported bug B3: the WorkspaceMember PK goes where a user id belongs,
    // so this guard never fires — kept as-is, never "fixed".
    let guarded = match sole_project_admin_exists(pool, &slug, &target.id).await {
        Ok(guarded) => guarded,
        Err(denial) => return denial.into_response(),
    };
    if guarded {
        return Denial::BadGuard(SOLE_PROJECT_ADMIN_BODY).into_response();
    }
    let now = Utc::now();
    if let Err(denial) = deactivate_project_rows(pool, &slug, &target.member_id, &now).await {
        return denial.into_response();
    }
    if let Err(denial) = deactivate_member(pool, &target.id, &actor.id, &now).await {
        return denial.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `POST workspaces/<slug>/members/leave/`: auth, the invalidations
/// (before the gate — a 403 still deletes them), the gate, the
/// sole-admin guards, then the deactivations + 204.
async fn leave_workspace(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
) -> Response {
    let actor = match actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    if let Err(denial) = run_leave_invalidations(&state, &slug, &actor).await {
        return denial.into_response();
    }
    let gate = match gate_for("POST", MEMBERS_LEAVE_PATH) {
        Some(gate) => gate,
        None => return Denial::ServerError.into_response(),
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let requester = match requester_lookup(pool, &actor.id, &slug).await {
        Ok(requester) => requester,
        Err(denial) => return denial.into_response(),
    };
    let role = requester.map(|requester| requester.role);
    let scope = tenant_context(&slug);
    let outcome = match &gate.gate {
        super::gates::Gate::Workspace { roles } => {
            decide_gate(&gate.gate, &scope, &workspace_facts(&slug, role, roles))
        }
        other => decide_gate(other, &scope, &workspace_facts(&slug, role, &[])),
    };
    if outcome != GateOutcome::Allow {
        return deny_response(outcome);
    }
    let Some(requester) = requester else {
        return Denial::ObjectNotFound.into_response();
    };
    let admin_count = match workspace_admin_count(pool, &slug).await {
        Ok(count) => count,
        Err(denial) => return denial.into_response(),
    };
    // The negated spelling (`not count > 1`, i.e. `count <= 1`) is kept
    // verbatim in the kernel.
    if qm::sole_workspace_admin_blocks(requester.role, admin_count) {
        return Denial::BadGuard(SOLE_WORKSPACE_ADMIN_BODY).into_response();
    }
    // The working twin of the destroy guard: the user id, not the row PK.
    let guarded = match sole_project_admin_exists(pool, &slug, &actor.id).await {
        Ok(guarded) => guarded,
        Err(denial) => return denial.into_response(),
    };
    if guarded {
        return Denial::BadGuard(SOLE_PROJECT_ADMIN_LEAVE_BODY).into_response();
    }
    let now = Utc::now();
    if let Err(denial) = deactivate_project_rows(pool, &slug, &requester.member_id, &now).await {
        return denial.into_response();
    }
    if let Err(denial) = deactivate_member(pool, &requester.id, &actor.id, &now).await {
        return denial.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `POST workspaces/<slug>/workspace-views/`: auth, the member lookup,
/// then `view_props` saved as-is + 204. No serializer: a valid non-dict
/// body 500s on `.get`, and explicit null 400s on the `NOT NULL` column.
async fn member_views(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
    body: Bytes,
) -> Response {
    let actor = match gated_actor(&state, extension, "POST", WORKSPACE_VIEWS_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let requester = match requester_lookup(pool, &actor.id, &slug).await {
        Ok(requester) => requester,
        Err(denial) => return denial.into_response(),
    };
    let Some(requester) = requester else {
        return Denial::ObjectNotFound.into_response();
    };
    let value: Value =
        match serde_json::from_slice(&body).map_err(|error| json_parse_denial(&body, &error)) {
            Ok(value) => value,
            Err(denial) => return denial.into_response(),
        };
    let Some(fields) = value.as_object() else {
        return Denial::ServerError.into_response();
    };
    let props = fields
        .get("view_props")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    if props == Value::Null {
        return Denial::BadPayload.into_response();
    }
    let now = Utc::now();
    let saved = sqlx::query(
        "UPDATE workspace_members SET view_props = $2, updated_at = $3, updated_by_id = $4 \
         WHERE id = $1",
    )
    .bind(requester.id)
    .bind(&props)
    .bind(now)
    .bind(actor.id)
    .execute(pool)
    .await;
    if let Err(error) = saved {
        return map_write_error(error).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `GET workspaces/<slug>/workspace-members/me/`: auth, then the
/// annotated row — or the 14-key null row for non-members.
async fn member_me(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
) -> Response {
    let actor = match gated_actor(&state, extension, "GET", MEMBERS_ME_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_me_row(pool, &actor.id, &slug).await {
        Ok(row) => row,
        Err(denial) => return denial.into_response(),
    };
    let item = match row.as_ref() {
        Some(row) => render_me(row, &actor.timezone),
        None => null_me_row(),
    };
    json_response(
        StatusCode::OK,
        drf_escape(&serde_json::to_string(&item).expect("member me")),
    )
}

/// `GET workspaces/<slug>/project-members/`: the entity gate, the
/// involved-project ids (unscoped — ported bug B7), the scoped members,
/// grouped by project with the key popped.
async fn project_members(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
    Path(slug): Path<String>,
) -> Response {
    let actor = match gated_actor(&state, extension, "GET", PROJECT_MEMBERS_PATH, &slug).await {
        Ok(actor) => actor,
        Err(denied) => return denied,
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let project_ids: Vec<Uuid> = match sqlx::query_scalar(
        "SELECT DISTINCT pm.project_id FROM project_members pm WHERE pm.member_id = $1 \
         AND pm.is_active = TRUE AND pm.deleted_at IS NULL",
    )
    .bind(actor.id)
    .fetch_all(pool)
    .await
    {
        Ok(ids) => ids,
        Err(_) => return Denial::ServerError.into_response(),
    };
    if project_ids.is_empty() {
        // `project_id__in=[]` is Django's `EmptyResultSet`: no second
        // query, an empty dict.
        return json_response(StatusCode::OK, "{}".to_owned());
    }
    let rows: Vec<ProjectMemberRow> = match sqlx::query_as(
        "SELECT pm.id AS id, pm.role AS role, pm.member_id AS member_id, \
         pm.project_id AS project_id, pm.created_at AS created_at FROM project_members pm \
         WHERE pm.workspace_id = (SELECT id FROM workspaces WHERE slug = $1) \
         AND pm.project_id = ANY($2) AND pm.is_active = TRUE AND pm.deleted_at IS NULL \
         ORDER BY pm.created_at DESC",
    )
    .bind(&slug)
    .bind(&project_ids)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let rendered: Vec<Map<String, Value>> = rows
        .iter()
        .map(|row| render_project_member(row, &actor.timezone))
        .collect();
    let grouped = group_project_members(rendered);
    json_response(
        StatusCode::OK,
        drf_escape(&serde_json::to_string(&Value::Object(grouped)).expect("project dict")),
    )
}

/// `GET users/last-visited-workspace/`: auth, then the pinned 500.
/// `user.last_workspace_id` lives on `Profile`, so every call raises
/// `AttributeError` (ported bug B2); the data branches are unreachable.
async fn last_visited(
    State(state): State<AppState>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    match actor(&state, extension).await {
        Ok(_) => Denial::ServerError.into_response(),
        Err(denial) => denial.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F-W24-15 golden
    /// (`rust-api/fixtures/app_workspace/handlers/routes.golden.json`),
    /// the Done-when oracle for these routes.
    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_workspace/handlers/routes.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture reads"))
            .expect("fixture parses")
    }

    fn error_bodies() -> Vec<Value> {
        fixture()["errors"]
            .as_array()
            .expect("errors array")
            .iter()
            .map(|entry| entry["body"].clone())
            .collect()
    }

    #[test]
    fn routes_cover_w12_through_w18() {
        for path in [
            MEMBERS_PATH,
            PROJECT_MEMBERS_PATH,
            MEMBER_DETAIL_PATH,
            MEMBERS_LEAVE_PATH,
            LAST_VISITED_PATH,
            MEMBERS_ME_PATH,
            WORKSPACE_VIEWS_PATH,
        ] {
            assert!(
                gate_for("GET", path).is_some()
                    || gate_for("POST", path).is_some()
                    || gate_for("PATCH", path).is_some()
                    || gate_for("DELETE", path).is_some(),
                "gate table covers {path}"
            );
        }
        // Method rows this family owns.
        assert!(gate_for("GET", MEMBERS_PATH).is_some());
        assert!(gate_for("GET", PROJECT_MEMBERS_PATH).is_some());
        assert!(gate_for("GET", MEMBER_DETAIL_PATH).is_some());
        assert!(gate_for("PATCH", MEMBER_DETAIL_PATH).is_some());
        assert!(gate_for("DELETE", MEMBER_DETAIL_PATH).is_some());
        assert!(gate_for("POST", MEMBERS_LEAVE_PATH).is_some());
        assert!(gate_for("GET", LAST_VISITED_PATH).is_some());
        assert!(gate_for("GET", MEMBERS_ME_PATH).is_some());
        assert!(gate_for("POST", WORKSPACE_VIEWS_PATH).is_some());
    }

    #[test]
    fn member_error_bodies_match_the_fixture() {
        let bodies = error_bodies();
        for (name, body) in [
            ("retrieve 404", MEMBER_NOT_FOUND_BODY),
            ("self role", SELF_ROLE_BODY),
            ("self remove", SELF_REMOVE_BODY),
            ("higher role", HIGHER_ROLE_BODY),
            ("last-visited 500", SERVER_ERROR_BODY),
        ] {
            let parsed: Value = serde_json::from_str(body).expect("const parses");
            assert!(
                bodies.contains(&parsed),
                "{name} pinned by F-W24-15: {body}"
            );
        }
        // The sole-admin variants live in `member.py`, not the errors
        // list: pin their exact bytes against the Python source lines.
        assert_eq!(
            SOLE_PROJECT_ADMIN_BODY,
            "{\"error\":\"User is a part of some projects where they are the only admin, they should either leave that project or promote another user to admin.\"}"
        );
        assert_eq!(
            SOLE_WORKSPACE_ADMIN_BODY,
            "{\"error\":\"You cannot leave the workspace as you are the only admin of the workspace you will have to either delete the workspace or promote another user to admin.\"}"
        );
        assert_eq!(
            SOLE_PROJECT_ADMIN_LEAVE_BODY,
            "{\"error\":\"You are a part of some projects where you are the only admin, you should either leave the project or promote another user to admin.\"}"
        );
    }

    #[test]
    fn exception_branches_match_drf() {
        assert_eq!(
            OBJECT_NOT_FOUND_BODY,
            "{\"error\":\"The required object does not exist.\"}"
        );
        assert_eq!(
            INVALID_PAYLOAD_BODY,
            "{\"error\":\"The payload is not valid\"}"
        );
        assert_eq!(
            UNAUTHENTICATED_BODY,
            "{\"detail\":\"Authentication credentials were not provided.\"}"
        );
        assert_eq!(
            PERMISSION_DENIED_BODY,
            "{\"error\":\"You don't have the required permissions.\"}"
        );
        assert_eq!(
            ENTITY_DENIED_BODY,
            "{\"detail\":\"You do not have permission to perform this action.\"}"
        );
    }

    #[test]
    fn denial_statuses_match_django() {
        let status_of = |denial: &Denial| denial.status_and_body().0;
        assert_eq!(status_of(&Denial::Unauthorized), StatusCode::UNAUTHORIZED);
        assert_eq!(
            status_of(&Denial::Forbidden(FORBIDDEN_BODY)),
            StatusCode::FORBIDDEN
        );
        assert_eq!(status_of(&Denial::MemberNotFound), StatusCode::NOT_FOUND);
        assert_eq!(status_of(&Denial::ObjectNotFound), StatusCode::NOT_FOUND);
        assert_eq!(status_of(&Denial::PageNotFound), StatusCode::NOT_FOUND);
        assert_eq!(status_of(&Denial::BadPayload), StatusCode::BAD_REQUEST);
        assert_eq!(
            status_of(&Denial::BadJson("x".to_owned())),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_of(&Denial::BadGuard(SELF_ROLE_BODY)),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_of(&Denial::BadFields(Value::Null)),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_of(&Denial::ServerError),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        // The parse-error envelope carries capital `Detail`, like DRF's
        // `exception_handler`.
        let (_, body) = Denial::BadJson("JSON parse error - e".to_owned()).status_and_body();
        assert_eq!(body, "{\"detail\":\"JSON parse error - e\"}");
        // The resolver 404 carries `JsonResponse` bytes (space after the
        // colon), unlike every compact DRF body above.
        assert_eq!(PAGE_NOT_FOUND_BODY, "{\"error\": \"Page not found.\"}");
        let (_, body) = Denial::PageNotFound.status_and_body();
        assert_eq!(body, PAGE_NOT_FOUND_BODY);
    }

    #[test]
    fn detail_pks_mirror_the_uuid_converter() {
        let good = "12345678-1234-5678-1234-567812345678";
        assert_eq!(parse_pk(good).expect("converter match").to_string(), good);
        for bad in [
            "not-a-uuid",
            "",
            "12345678-1234-5678-1234-567812345678-extra",
            "12345678123456781234567812345678",
            "12345678-1234-5678-1234-56781234567G",
            "12345678-1234-5678-1234-56781234567A",
            "12345678-1234-5678-1234-567812345678 ",
            "XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX",
        ] {
            let denial = parse_pk(bad).unwrap_err();
            let (status, body) = denial.status_and_body();
            assert_eq!(status, StatusCode::NOT_FOUND, "{bad}");
            assert_eq!(body, "{\"error\": \"Page not found.\"}", "{bad}");
        }
    }

    #[test]
    fn blank_bodies_parse_error_byte_exact() {
        for (raw, column, char) in [("".as_bytes(), 1, 0), ("  ".as_bytes(), 3, 2)] {
            let denial = json_parse_denial(raw, &serde_json::from_slice::<Value>(raw).unwrap_err());
            let Denial::BadJson(detail) = denial else {
                panic!("blank input is a parse error");
            };
            assert_eq!(
                detail,
                format!("JSON parse error - Expecting value: line 1 column {column} (char {char})")
            );
        }
    }

    #[test]
    fn non_dict_bodies_name_the_python_type() {
        for (raw, kind) in [
            ("[1]", "list"),
            ("\"x\"", "str"),
            ("5", "int"),
            ("5.5", "float"),
            ("true", "bool"),
            ("null", "NoneType"),
        ] {
            let value = parse_body_json(raw.as_bytes()).expect("json parses");
            let denial = parse_body_value(value).unwrap_err();
            let Denial::BadFields(errors) = denial else {
                panic!("{raw} is a field error");
            };
            assert_eq!(
                errors,
                serde_json::json!({
                    "non_field_errors": [format!("Invalid data. Expected a dictionary, but got {kind}.")]
                }),
                "{raw}"
            );
        }
    }

    #[test]
    fn cascade_precheck_reads_the_raw_body() {
        let case = |raw: &str| cascade_role_lookup(&serde_json::from_str(raw).expect("json"));
        // Dict bodies yield the `role` member, or no cascade.
        assert_eq!(
            case("{\"role\": 5}").ok().flatten(),
            Some(serde_json::json!(5))
        );
        assert!(matches!(case("{\"a\": 1}"), Ok(None)));
        // Scalar bodies 500 on `"role" in data` (`TypeError`).
        for raw in ["5", "5.5", "true", "null"] {
            assert!(matches!(case(raw), Err(Denial::ServerError)), "{raw}");
        }
        // `str` bodies 500 on a substring hit, else fall through.
        assert!(matches!(
            case("\"has role in it\""),
            Err(Denial::ServerError)
        ));
        assert!(matches!(case("\"clean\""), Ok(None)));
        // `list` bodies 500 on element equality, else fall through.
        assert!(matches!(case("[\"role\"]"), Err(Denial::ServerError)));
        assert!(matches!(
            case("[\"a\", \"role\"]"),
            Err(Denial::ServerError)
        ));
        assert!(matches!(case("[\"a\", 5]"), Ok(None)));
        assert!(matches!(case("[5]"), Ok(None)));
        assert!(matches!(case("[]"), Ok(None)));
    }

    #[test]
    fn search_terms_follow_drf() {
        let query =
            |raw: &str| HashMap::from([("search".to_owned(), OneOrMany::One(raw.to_owned()))]);
        assert!(search_terms(&HashMap::new()).is_empty());
        // Probe battery vs the real `search_smart_split` (Django 4.2.30 +
        // DRF 3.15.2-identical): quoted phrases keep together, unclosed
        // quotes fall back to bare runs, lone quotes unescape to the
        // empty term, commas trim per token then split the rest.
        for (raw, expected) in [
            ("", vec![]),
            ("  ", vec![]),
            ("ada", vec!["ada"]),
            ("ada,lovelace x", vec!["ada", "lovelace", "x"]),
            ("\"john doe\"", vec!["john doe"]),
            ("'john doe'", vec!["john doe"]),
            ("\"a b\" c", vec!["a b", "c"]),
            ("a\"b c\"d", vec!["a\"b c\"d"]),
            ("\"unclosed", vec!["\"unclosed"]),
            ("unclosed\"", vec!["unclosed\""]),
            ("\"", vec![""]),
            ("'", vec![""]),
            ("\"\"", vec![""]),
            ("''", vec![""]),
            ("a,,b", vec!["a", "b"]),
            (",a,", vec!["a"]),
            (",", vec![]),
            (",,,", vec![]),
            ("\"a,b\",c", vec!["\"a", "b\"", "c"]),
            ("\"a,b\",", vec!["a,b"]),
            (",\"a,b\"", vec!["a,b"]),
            ("\"a b\",", vec!["a b"]),
            ("  a  b  ", vec!["a", "b"]),
            ("a'b", vec!["a'b"]),
            ("a\"b", vec!["a\"b"]),
            ("ab\"cd\"ef\"gh", vec!["ab\"cd\"ef", "\"gh"]),
            ("a\"b\"c\"d", vec!["a\"b\"c", "\"d"]),
            ("\"a\"b\"c\"", vec!["a\"b\"c"]),
            ("\"a\" \"b\"", vec!["a", "b"]),
            ("\"lead\"mid", vec!["\"lead\"mid"]),
            ("mid\"trail\"", vec!["mid\"trail\""]),
            ("\"a\"\"b\"", vec!["a\"\"b"]),
            ("\"   \"", vec!["   "]),
            ("'   '", vec!["   "]),
            ("\",\"", vec![","]),
            ("\", \"", vec![", "]),
            ("\"a b\" \"c d\" e", vec!["a b", "c d", "e"]),
            ("'\"'", vec!["\""]),
            ("\"'\"", vec!["'"]),
            ("a\tb", vec!["a", "b"]),
            ("\"caf\u{e9}\"", vec!["caf\u{e9}"]),
            ("caf\u{e9} x", vec!["caf\u{e9}", "x"]),
        ] {
            assert_eq!(search_terms(&query(raw)), expected, "{raw:?}");
        }
        // Backslashes: `\\.` escapes inside quotes, a backslash-newline
        // breaks the quoted match, embedded newlines stay in quotes.
        assert_eq!(search_terms(&query("a\\b")), vec!["a\\b"]);
        assert_eq!(search_terms(&query("\"a\\b\"")), vec!["a\\b"]);
        assert_eq!(search_terms(&query("\"a\\\"b\"")), vec!["a\"b"]);
        assert_eq!(search_terms(&query("'a\\'b'")), vec!["a'b"]);
        assert_eq!(search_terms(&query("\"a\\\n\"")), vec!["\"a\\", ""]);
        assert_eq!(search_terms(&query("\"a\nb\"")), vec!["a\nb"]);
        // Python whitespace beyond ASCII: NUL is not space, while
        // \x1c and \x85 split like any other whitespace.
        assert_eq!(search_terms(&query("a\0b")), vec!["a\0b"]);
        assert_eq!(search_terms(&query("\"\0\"")), vec!["\0"]);
        assert_eq!(search_terms(&query("\x1ca\x1f")), vec!["a"]);
        assert_eq!(search_terms(&query("a\u{85}b")), vec!["a", "b"]);
        // Repeats read last, like `QueryDict.get`.
        let repeated = HashMap::from([(
            "search".to_owned(),
            OneOrMany::Many(vec!["old".to_owned(), "new".to_owned()]),
        )]);
        assert_eq!(search_terms(&repeated), vec!["new"]);
        // LIKE metacharacters escape.
        assert_eq!(like_param("a%b_c\\d"), "%a\\%b\\_c\\\\d%");
    }

    #[test]
    fn python_int_matches_builtin() {
        let case = |json: &str| python_int(&serde_json::from_str(json).expect("json"));
        assert_eq!(case("5"), Some(5));
        assert_eq!(case("5.0"), Some(5));
        assert_eq!(case("5.9"), Some(5));
        assert_eq!(case("-5.9"), Some(-5));
        assert_eq!(case("\"5\""), Some(5));
        assert_eq!(case("\"  +5  \""), Some(5));
        assert_eq!(case("\"1_0\""), Some(10));
        assert_eq!(case("true"), Some(1));
        assert_eq!(case("false"), Some(0));
        assert_eq!(case("\"5.0\""), None);
        assert_eq!(case("\"\""), None);
        assert_eq!(case("\"1__0\""), None);
        assert_eq!(case("\"_1\""), None);
        assert_eq!(case("\"1_\""), None);
        assert_eq!(case("\"abc\""), None);
        assert_eq!(case("null"), None);
        assert_eq!(case("[5]"), None);
        assert_eq!(case("{\"a\":1}"), None);
        // Out-of-range ints never equal the trigger but never 500.
        assert_eq!(case("99999999999999999999999"), Some(i64::MAX));
        assert_eq!(case("\"99999999999999999999999\""), Some(i64::MAX));
        assert_eq!(case("\"-99999999999999999999999\""), Some(i64::MIN));
        assert_eq!(case("18446744073709551615"), Some(i64::MAX));
        assert_eq!(case("1e3"), Some(1000));
        // `int()` strips every `str.strip` char except `\x1c`-`\x1f`
        // (probed exhaustively): the four controls raise.
        assert_eq!(case("\"\\u001c5\""), None);
        assert_eq!(case("\"5\\u001f\""), None);
        assert_eq!(case("\" \\u001c5\\u001f \""), None);
        assert_eq!(case("\"\u{85}5\""), Some(5));
        assert_eq!(case("\"\u{a0}5\""), Some(5));
        assert_eq!(case("\"\u{2003}5\""), Some(5));
        // `int()` accepts the full Unicode decimal set (general
        // category Nd), with the same underscore and sign rules.
        assert_eq!(case("\"５\""), Some(5));
        assert_eq!(case("\"５５５\""), Some(555));
        assert_eq!(case("\"5５5\""), Some(555));
        assert_eq!(case("\"+٥\""), Some(5));
        assert_eq!(case("\"-٥\""), Some(-5));
        assert_eq!(case("\"٥_٦\""), Some(56));
        assert_eq!(case("\"５_５\""), Some(55));
        assert_eq!(case("\"_５\""), None);
        assert_eq!(case("\"５_\""), None);
        assert_eq!(case("\"＋5\""), None);
        assert_eq!(case("\"²\""), None);
        assert_eq!(case("\"\u{1d7ce}\""), Some(0));
        assert_eq!(case("\"\u{1d7d8}\""), Some(0));
    }

    #[test]
    fn python_str_matches_builtin() {
        let case = |json: &str| python_str(&serde_json::from_str(json).expect("json"));
        assert_eq!(case("null"), "None");
        assert_eq!(case("true"), "True");
        assert_eq!(case("false"), "False");
        assert_eq!(case("5"), "5");
        assert_eq!(case("-42"), "-42");
        assert_eq!(case("5.0"), "5.0");
        assert_eq!(case("\"x\""), "x");
        assert_eq!(case("[1, \"a\"]"), "[1, 'a']");
        assert_eq!(case("{\"a\": 1}"), "{'a': 1}");
        assert_eq!(case("99999999999999999999999"), "99999999999999999999999");
        assert_eq!(case("1e3"), "1000.0");
        assert_eq!(case("100.50"), "100.5");
        // Container items render as `repr()` (battery verified against
        // CPython).
        assert_eq!(case("[\"a'b\"]"), "[\"a'b\"]");
        assert_eq!(case("[\"a\\nb\"]"), "['a\\nb']");
        assert_eq!(case("[\"a\u{2028}b\"]"), "['a\\u2028b']");
    }

    #[test]
    #[allow(clippy::approx_constant, clippy::excessive_precision)]
    fn float_repr_matches_python() {
        // Battery verified against CPython `repr()`; the PI digits and the
        // over-precise literal are intentional vectors (nearest-f64 `repr`).
        for (float, expected) in [
            (5.0, "5.0"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (-0.0, "-0.0"),
            (123.456, "123.456"),
            (1e21, "1e+21"),
            (2.5, "2.5"),
            (100.0, "100.0"),
            (0.5, "0.5"),
            (1.2345678901234567e30, "1.2345678901234567e+30"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (1.1, "1.1"),
            (3.141592653589793, "3.141592653589793"),
            (2e-4, "0.0002"),
            (99999.0, "99999.0"),
            (9999999999999998.0, "9999999999999998.0"),
            (1.0000000000000002, "1.0000000000000002"),
            (1e-7, "1e-07"),
            (123456789.123456789, "123456789.12345679"),
        ] {
            assert_eq!(py_float_repr(float), expected, "{float}");
        }
    }

    #[test]
    fn role_choice_matches_drf() {
        let case = |json: &str| {
            let mut errors = FieldErrors::default();
            let role =
                validate_role_choice(&serde_json::from_str(json).expect("json"), &mut errors);
            (role, errors.role)
        };
        assert_eq!(case("20"), (Some(20), vec![]));
        assert_eq!(case("15"), (Some(15), vec![]));
        assert_eq!(case("5"), (Some(5), vec![]));
        assert_eq!(case("\"15\""), (Some(15), vec![]));
        // `str(5.0)` is `"5.0"`: no coercion through the choice map.
        assert_eq!(
            case("5.0"),
            (None, vec!["\"5.0\" is not a valid choice.".to_owned()])
        );
        assert_eq!(
            case("10"),
            (None, vec!["\"10\" is not a valid choice.".to_owned()])
        );
        assert_eq!(
            case("\"x\""),
            (None, vec!["\"x\" is not a valid choice.".to_owned()])
        );
        assert_eq!(
            case("true"),
            (None, vec!["\"True\" is not a valid choice.".to_owned()])
        );
        assert_eq!(
            case("[5]"),
            (None, vec!["\"[5]\" is not a valid choice.".to_owned()])
        );
        assert_eq!(
            case("null"),
            (None, vec!["This field may not be null.".to_owned()])
        );
    }

    #[test]
    fn company_role_matches_charfield() {
        let case = |json: &str| {
            let mut errors = FieldErrors::default();
            let value =
                validate_company_role(&serde_json::from_str(json).expect("json"), &mut errors);
            (value, errors.company_role)
        };
        assert_eq!(case("null"), (Some(None), vec![]));
        assert_eq!(case("\"eng\""), (Some(Some("eng".to_owned())), vec![]));
        assert_eq!(case("\"  eng  \""), (Some(Some("eng".to_owned())), vec![]));
        assert_eq!(case("\"\""), (Some(Some(String::new())), vec![]));
        assert_eq!(case("\"   \""), (Some(Some(String::new())), vec![]));
        assert_eq!(case("5"), (Some(Some("5".to_owned())), vec![]));
        assert_eq!(case("5.5"), (Some(Some("5.5".to_owned())), vec![]));
        assert_eq!(case("true"), (None, vec!["Not a valid string.".to_owned()]));
        assert_eq!(
            case("[\"x\"]"),
            (None, vec!["Not a valid string.".to_owned()])
        );
        assert_eq!(
            case("\"a\\u0000b\""),
            (None, vec!["Null characters are not allowed.".to_owned()])
        );
    }

    #[test]
    fn bool_matches_booleanfield() {
        let case = |json: &str| {
            let mut errors = FieldErrors::default();
            let value = validate_bool(&serde_json::from_str(json).expect("json"), &mut errors);
            (value, errors.is_active)
        };
        assert_eq!(case("true"), (Some(true), vec![]));
        assert_eq!(case("false"), (Some(false), vec![]));
        assert_eq!(case("1"), (Some(true), vec![]));
        assert_eq!(case("0"), (Some(false), vec![]));
        assert_eq!(case("1.0"), (Some(true), vec![]));
        assert_eq!(case("0.0"), (Some(false), vec![]));
        assert_eq!(case("\"True\""), (Some(true), vec![]));
        assert_eq!(case("\"off\""), (Some(false), vec![]));
        assert_eq!(case("\"1\""), (Some(true), vec![]));
        assert_eq!(case("\"0\""), (Some(false), vec![]));
        assert_eq!(
            case("2"),
            (None, vec!["Must be a valid boolean.".to_owned()])
        );
        assert_eq!(
            case("1.5"),
            (None, vec!["Must be a valid boolean.".to_owned()])
        );
        assert_eq!(
            case("\"\""),
            (None, vec!["Must be a valid boolean.".to_owned()])
        );
        assert_eq!(
            case("\"null\""),
            (None, vec!["Must be a valid boolean.".to_owned()])
        );
        assert_eq!(
            case("null"),
            (None, vec!["This field may not be null.".to_owned()])
        );
    }

    #[test]
    fn json_props_reject_only_null() {
        let mut errors = FieldErrors::default();
        assert!(validate_json_prop(&serde_json::json!({"a": 1}), &mut errors.view_props).is_some());
        assert!(validate_json_prop(&serde_json::json!([1]), &mut errors.view_props).is_some());
        assert!(validate_json_prop(&serde_json::json!("x"), &mut errors.view_props).is_some());
        assert!(validate_json_prop(&serde_json::json!(0), &mut errors.view_props).is_some());
        assert!(errors.view_props.is_empty());
        assert!(validate_json_prop(&Value::Null, &mut errors.view_props).is_none());
        assert_eq!(errors.view_props, vec!["This field may not be null."]);
    }

    #[test]
    fn fk_values_match_pkrelatedfield() {
        let case = |json: &str, allow_null: bool| {
            let mut errors = FieldErrors::default();
            let outcome = validate_fk_uuid(
                &serde_json::from_str(json).expect("json"),
                allow_null,
                &mut errors.workspace,
            );
            (outcome, errors.workspace)
        };
        let id = Uuid::parse_str("12345678-1234-5678-1234-567812345678").expect("uuid");
        assert!(
            matches!(case("\"12345678-1234-5678-1234-567812345678\"", false).0, FkValue::Id(got) if got == id)
        );
        assert!(matches!(case("5", false).0, FkValue::Id(_)));
        assert!(matches!(case("null", true).0, FkValue::Null));
        assert!(matches!(case("null", false).0, FkValue::Invalid));
        assert!(matches!(case("true", false).0, FkValue::Invalid));
        // Garbage is the fancy-quote field error (Django's
        // `ValidationError`, caught per-field by
        // `Serializer.to_internal_value`), never an escaping branch —
        // proven on the real serializer class, all seven displays.
        for (json, display) in [
            ("\"abc\"", "abc"),
            ("\"5\"", "5"),
            ("-5", "-5"),
            ("5.5", "5.5"),
            ("[1]", "[1]"),
            ("{\"a\":1}", "{'a': 1}"),
            // Past-`u64` int text keeps its full digits
            // (`arbitrary_precision`): 2**128 shows all 39, like Django.
            (
                "340282366920938463463374607431768211456",
                "340282366920938463463374607431768211456",
            ),
        ] {
            let (outcome, errors) = case(json, false);
            assert!(matches!(outcome, FkValue::Invalid), "{json}");
            assert_eq!(
                errors,
                vec![format!("\u{201c}{display}\u{201d} is not a valid UUID.")],
                "{json}"
            );
        }
        // `""` converts to `None` before validation: a valid NULL where
        // the field allows it, the null message where it does not.
        assert!(matches!(case("\"\"", true).0, FkValue::Null));
        assert!(case("\"\"", true).1.is_empty());
        let (outcome, errors) = case("\"\"", false);
        assert!(matches!(outcome, FkValue::Invalid));
        assert_eq!(errors, vec!["This field may not be null."]);
        // The garbage error coexists with other field errors in wire
        // order (no short-circuit): role sorts before workspace.
        let mut errors = FieldErrors::default();
        errors.role.push("\"xx\" is not a valid choice.".to_owned());
        let outcome = validate_fk_uuid(
            &serde_json::from_str("\"abc\"").expect("json"),
            false,
            &mut errors.workspace,
        );
        assert!(matches!(outcome, FkValue::Invalid));
        let rendered = errors.into_value();
        let object = rendered.as_object().expect("object");
        let keys: Vec<&str> = object.keys().map(String::as_str).collect();
        assert_eq!(keys, ["role", "workspace"]);
        assert_eq!(
            object["workspace"],
            Value::Array(vec![Value::String(
                "\u{201c}abc\u{201d} is not a valid UUID.".to_owned(),
            )]),
        );
        let (_, errors) = case("true", false);
        assert_eq!(
            errors,
            vec!["Incorrect type. Expected pk value, received bool."]
        );
        let (_, errors) = case("null", false);
        assert_eq!(errors, vec!["This field may not be null."]);
    }

    #[test]
    fn field_errors_serialize_in_wire_order() {
        // Full writable order, proven against the real serializer on DRF
        // 3.15.2 (relations sort after concrete fields): deleted_at, role,
        // company_role, view_props, default_props, issue_props, is_active,
        // getting_started_checklist, tips, explored_features, created_by,
        // updated_by, workspace, then non_field_errors.
        let mut errors = FieldErrors::default();
        errors.workspace.push("w".to_owned());
        errors.role.push("r".to_owned());
        errors.non_field_errors.push("n".to_owned());
        errors.deleted_at.push("d".to_owned());
        errors.created_by.push("c".to_owned());
        errors.updated_by.push("u".to_owned());
        errors.company_role.push("cr".to_owned());
        errors.view_props.push("v".to_owned());
        errors.default_props.push("df".to_owned());
        errors.issue_props.push("i".to_owned());
        errors.is_active.push("a".to_owned());
        errors.getting_started_checklist.push("g".to_owned());
        errors.tips.push("t".to_owned());
        errors.explored_features.push("e".to_owned());
        let text = serde_json::to_string(&errors.into_value()).expect("errors");
        assert_eq!(
            text,
            "{\"deleted_at\":[\"d\"],\"role\":[\"r\"],\"company_role\":[\"cr\"],\"view_props\":[\"v\"],\"default_props\":[\"df\"],\"issue_props\":[\"i\"],\"is_active\":[\"a\"],\"getting_started_checklist\":[\"g\"],\"tips\":[\"t\"],\"explored_features\":[\"e\"],\"created_by\":[\"c\"],\"updated_by\":[\"u\"],\"workspace\":[\"w\"],\"non_field_errors\":[\"n\"]}"
        );
    }

    #[test]
    fn datetime_parser_matches_django() {
        use chrono::{NaiveDate, NaiveDateTime};
        let naive = |y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32, us: u32| {
            ParsedDt::Naive(
                NaiveDate::from_ymd_opt(y, m, d)
                    .expect("date")
                    .and_hms_micro_opt(h, mi, s, us)
                    .expect("time"),
            )
        };
        let aware = |dt: NaiveDateTime, offset_secs: i64| {
            ParsedDt::Aware(dt.and_utc() - chrono::Duration::seconds(offset_secs))
        };
        let aware_us = |dt: NaiveDateTime, offset_micros: i64| {
            ParsedDt::Aware(dt.and_utc() - chrono::Duration::microseconds(offset_micros))
        };
        let day = |y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32, us: u32| {
            NaiveDate::from_ymd_opt(y, m, d)
                .expect("date")
                .and_hms_micro_opt(h, mi, s, us)
                .expect("time")
        };
        // Every case verified against CPython 3.12 `parse_datetime`.
        for (input, expected) in [
            (
                "2024-01-15T10:30:00Z",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 0)),
            ),
            (
                "2024-01-15 10:30:00",
                Some(naive(2024, 1, 15, 10, 30, 0, 0)),
            ),
            ("2024-01-15T10:30", Some(naive(2024, 1, 15, 10, 30, 0, 0))),
            ("2024-01-15T10", Some(naive(2024, 1, 15, 10, 0, 0, 0))),
            ("2024-01-15", Some(naive(2024, 1, 15, 0, 0, 0, 0))),
            ("20240115", Some(naive(2024, 1, 15, 0, 0, 0, 0))),
            ("20240115T103000", Some(naive(2024, 1, 15, 10, 30, 0, 0))),
            ("20240115T1030", Some(naive(2024, 1, 15, 10, 30, 0, 0))),
            ("2024-01-15T1030", Some(naive(2024, 1, 15, 10, 30, 0, 0))),
            (
                "2024-01-15T10:30:00.123",
                Some(naive(2024, 1, 15, 10, 30, 0, 123_000)),
            ),
            (
                "2024-01-15T10:30:00.1234567",
                Some(naive(2024, 1, 15, 10, 30, 0, 123_456)),
            ),
            (
                "2024-01-15T10:30:00,5",
                Some(naive(2024, 1, 15, 10, 30, 0, 500_000)),
            ),
            (
                "2024-01-15T10.5",
                Some(naive(2024, 1, 15, 10, 0, 0, 500_000)),
            ),
            (
                "2024-01-15T10:30.5",
                Some(naive(2024, 1, 15, 10, 30, 0, 500_000)),
            ),
            (
                "2024-01-15X10:30:00",
                Some(naive(2024, 1, 15, 10, 30, 0, 0)),
            ),
            (
                "2024-01-15t10:30:00",
                Some(naive(2024, 1, 15, 10, 30, 0, 0)),
            ),
            ("2024-01-15+10:30", Some(naive(2024, 1, 15, 10, 30, 0, 0))),
            ("2024-01-15-05:00", Some(naive(2024, 1, 15, 5, 0, 0, 0))),
            ("20240115-05:00", Some(naive(2024, 1, 15, 5, 0, 0, 0))),
            ("2024-01-15T1:2:3", Some(naive(2024, 1, 15, 1, 2, 3, 0))),
            ("2024-W03-1", Some(naive(2024, 1, 15, 0, 0, 0, 0))),
            ("2024-W03", Some(naive(2024, 1, 15, 0, 0, 0, 0))),
            ("2024W031", Some(naive(2024, 1, 15, 0, 0, 0, 0))),
            ("2024W03", Some(naive(2024, 1, 15, 0, 0, 0, 0))),
            ("2024W03T10:30", Some(naive(2024, 1, 15, 10, 30, 0, 0))),
            ("2024-W03T10:30", Some(naive(2024, 1, 15, 10, 30, 0, 0))),
            ("2024W031T10:30", Some(naive(2024, 1, 15, 10, 30, 0, 0))),
            (
                "2024-W03-1T10:30:00+05:30",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 19_800)),
            ),
            (
                "2024-01-15T10:30:00+0530",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 19_800)),
            ),
            (
                "2024-01-15T10:30:00+05",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 18_000)),
            ),
            (
                "2024-01-15T10:30:00+0000",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 0)),
            ),
            (
                "2024-01-15T10:30:00-0500",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), -18_000)),
            ),
            (
                "2024-01-15T10:30:00+053000",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 19_800)),
            ),
            (
                "2024-01-15T10:30:00+05:30:15",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 19_815)),
            ),
            (
                "2024-01-15T10:30:00+00:61",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 3_660)),
            ),
            (
                "2024-01-15T10:30:00.5+05:30",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 500_000), 19_800)),
            ),
            (
                "2024-01-15T10:30:00+23:59:59",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 86_399)),
            ),
            ("0001-01-01", Some(naive(1, 1, 1, 0, 0, 0, 0))),
            (
                "9999-12-31T23:59:59Z",
                Some(aware(day(9999, 12, 31, 23, 59, 59, 0), 0)),
            ),
            // Regex-fallback arm (`datetime_re`): unpadded date/time.
            ("2026-1-2T03:04:05", Some(naive(2026, 1, 2, 3, 4, 5, 0))),
            ("2026-1-2T3:4", Some(naive(2026, 1, 2, 3, 4, 0, 0))),
            ("2026-1-2T03:04", Some(naive(2026, 1, 2, 3, 4, 0, 0))),
            ("2026-1-2T03:04:5", Some(naive(2026, 1, 2, 3, 4, 5, 0))),
            ("2026-1-2 03:04:05", Some(naive(2026, 1, 2, 3, 4, 5, 0))),
            // Regex arm fractions: `.`/`,`, right-padded, first 6 of ≤12.
            (
                "2026-1-2T03:04:05.5",
                Some(naive(2026, 1, 2, 3, 4, 5, 500_000)),
            ),
            (
                "2026-1-2T03:04:05,5",
                Some(naive(2026, 1, 2, 3, 4, 5, 500_000)),
            ),
            (
                "2026-1-2T03:04:05.123456789012",
                Some(naive(2026, 1, 2, 3, 4, 5, 123_456)),
            ),
            // Regex arm `\s*`: trailing whitespace without a tz.
            ("2026-01-02T03:04:05 ", Some(naive(2026, 1, 2, 3, 4, 5, 0))),
            ("2026-01-02T03:04:05\t", Some(naive(2026, 1, 2, 3, 4, 5, 0))),
            ("2026-01-02T03:04:05\n", Some(naive(2026, 1, 2, 3, 4, 5, 0))),
            ("2026-1-2T03:04:05\x0b", Some(naive(2026, 1, 2, 3, 4, 5, 0))),
            (
                "2026-1-2T03:04:05\u{85}",
                Some(naive(2026, 1, 2, 3, 4, 5, 0)),
            ),
            ("2026-1-2 03:04:05 ", Some(naive(2026, 1, 2, 3, 4, 5, 0))),
            (
                "2026-01-02T03:04:05.5 ",
                Some(naive(2026, 1, 2, 3, 4, 5, 500_000)),
            ),
            // Regex arm `\s*`: whitespace before the tz.
            (
                "2026-01-02T03:04:05  +05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-1-2T03:04:05 +05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-1-2T03:04:05\t+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-01-02T03:04:05\n+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-1-2T03:04:05 Z",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 0)),
            ),
            // Regex arm zones: `Z`/`±HH`/`±HHMM`/`±HH:MM`, unchecked
            // parts, total under 24h.
            (
                "2026-1-2T03:04:05Z",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 0)),
            ),
            (
                "2026-1-2T03:04:05+05",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-1-2T03:04:05+0530",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 19_800)),
            ),
            (
                "2026-1-2T03:04:05+05:30",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 19_800)),
            ),
            (
                "2026-1-2T03:04:05+00:61",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 3_660)),
            ),
            (
                "2026-1-2T03:04:05.5+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 500_000), 18_000)),
            ),
            (
                "2026-1-2T03:04:05.123456789012+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 123_456), 18_000)),
            ),
            // Regex arm `$`: one trailing newline after the tz matches.
            (
                "2026-01-02T03:04:05+05:00\n",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-01-02T03:04:05Z\n",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 0)),
            ),
            (
                "2026-1-2T03:04:05+05:00\n",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-1-2T03:04:05 +05:00\n",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            // Fromisoformat arm: fractional offset seconds (seconds
            // fraction after any component count, `.`/`,`, truncated
            // to 6, total strictly under 24h in microseconds).
            (
                "2026-01-02T03:04:05+05:00:00.5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 18_000_500_000)),
            ),
            (
                "2026-01-02T03:04:05+05:00:00,5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 18_000_500_000)),
            ),
            (
                "2026-01-02T03:04:05+05:00:00.1",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 18_000_100_000)),
            ),
            (
                "2026-01-02T03:04:05+05:00:00.123456789",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 18_000_123_456)),
            ),
            (
                "2026-01-02T03:04:05+050000.5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 18_000_500_000)),
            ),
            (
                "2026-01-02T03:04:05+05.5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 18_000_500_000)),
            ),
            (
                "2026-01-02T03:04:05+05:30.5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 19_800_500_000)),
            ),
            (
                "2026-01-02T03:04:05+0530.5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 19_800_500_000)),
            ),
            (
                "2026-01-02T03:04:05+23:59:59.999999",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 86_399_999_999)),
            ),
            (
                "2026-01-02T03:04:05-05:00:00.5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), -18_000_500_000)),
            ),
            (
                "2026-01-02T03:04:05+00:61:00.5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 3_660_500_000)),
            ),
            (
                "2026-01-02T03:04:05+00:00:01.5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 1_500_000)),
            ),
            (
                "2026-01-02T03:04:05+05:00:00.000000",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            // Fromisoformat arm: a fraction-only offset (zero
            // hours/minutes/seconds) collapses to UTC, sign included.
            (
                "2026-01-02T03:04:05+00:00:00.5",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 0)),
            ),
            (
                "2026-01-02T03:04:05+00:00:00.000001",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 0)),
            ),
            (
                "2026-01-02T03:04:05-00:00:00.5",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 0)),
            ),
            (
                "2026-01-02T03:04:05+000000.5",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 0)),
            ),
            (
                "2026-01-02T03:04:05+00.5",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 0)),
            ),
            // 1-digit times reroute through the regex arm with identical
            // values (arm 1 reads strict 2-char windows, like the C
            // parser).
            ("2024-01-15T01:2:3", Some(naive(2024, 1, 15, 1, 2, 3, 0))),
            (
                "2024-01-15T1:2:3.5",
                Some(naive(2024, 1, 15, 1, 2, 3, 500_000)),
            ),
            ("2024-01-15T10:3", Some(naive(2024, 1, 15, 10, 3, 0, 0))),
            (
                "2024-01-15T1:2:3+05:00",
                Some(aware(day(2024, 1, 15, 1, 2, 3, 0), 18_000)),
            ),
            (
                "2024-01-15T1:2:3+0530",
                Some(aware(day(2024, 1, 15, 1, 2, 3, 0), 19_800)),
            ),
            (
                "2024-01-15T1:2:3Z",
                Some(aware(day(2024, 1, 15, 1, 2, 3, 0), 0)),
            ),
            // Fromisoformat arm: one gap byte before the tz (week and
            // basic dates, `X` separator, 13-digit fractions — shapes
            // the regex arm cannot spell).
            (
                "2024-W03T10:30:00 +05:00",
                Some(aware(day(2024, 1, 15, 10, 30, 0, 0), 18_000)),
            ),
            (
                "20260102T030405 +05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "20260102T030405\t+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-01-02X03:04:05\n+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-01-02X03:04:05\r+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-01-02X03:04:05\x1c+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 18_000)),
            ),
            (
                "2026-01-02X03:04:05\tZ",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 0), 0)),
            ),
            (
                "2026-01-02T03:04:05.1234567890123 +05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 123_456), 18_000)),
            ),
            // Gap byte past 6+ fraction digits (a short fraction must
            // abut the tz instead).
            (
                "2026-01-02X03:04:05.123456\t+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 123_456), 18_000)),
            ),
            (
                "2026-01-02X03:04:05.1234567\t+05:00",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 123_456), 18_000)),
            ),
            (
                "2026-01-02X03:04:05.123456\tZ",
                Some(aware(day(2026, 1, 2, 3, 4, 5, 123_456), 0)),
            ),
            (
                "2026-01-02X03:04:05\t+05:00:00.5",
                Some(aware_us(day(2026, 1, 2, 3, 4, 5, 0), 18_000_500_000)),
            ),
            (
                "2026-01-02X03:04:05.1234567",
                Some(naive(2026, 1, 2, 3, 4, 5, 123_456)),
            ),
            (
                "2026-01-02T03:04:05.1234567 ",
                Some(naive(2026, 1, 2, 3, 4, 5, 123_456)),
            ),
        ] {
            assert_eq!(parse_django_datetime(input), expected, "{input}");
        }
        // Rejects and raisers (both land in the invalid arm).
        for input in [
            "",
            "2024-015",
            "2024-01-15t10:30:00z",
            "2024-13-01",
            "2024-01-32",
            "2024-00-10",
            "2024-02-30",
            "24-01-15",
            "2024-1-5",
            "10:30",
            "T10:30:00",
            "+05:30",
            "2024-01-15T10:30:00.",
            "2024-01-15T10:30:00,",
            "  2024-01-15T10:30:00  ",
            "2024-01-15T10:30:00+00:00 ",
            "2024-01-15  10:30:00",
            "2024-01-15T25:00:00",
            "2024-01-15T24:00",
            "2024-01-15T10:60",
            "2024-01-15T10:30:60",
            "2024-W03-8",
            "2024W03-1",
            "2024-W031",
            "2024w03",
            "2024-01-15T930",
            "2024-01-15T10300",
            "2024-01-15T10:30:00+",
            "2024-01-15T10:30:00+5",
            "2024-01-15T10:30:00+24:00",
            "2024-01-15T10:30:00-24:00",
            "2024-01-15T",
            "2024-0é-15",
            "2024-01-1é",
            // Regex arm edges: 13+ fractional digits, empty fraction,
            // hour-only, non-`T`/space separators, leading whitespace.
            "2026-1-2T03:04:05.1234567890123",
            "2026-1-2T03:04:05.",
            "2026-1-2T3",
            "2026-1-2t03:04:05",
            "2026-1-2X03:04:05",
            " 2026-1-2T03:04:05",
            // Regex arm zones: 24h totals, offset seconds (no arm
            // accepts unpadded dates with tz seconds), trailing
            // space after the tz, garbage, lowercase `z`.
            "2026-1-2T03:04:05+24:00",
            "2026-1-2T03:04:05+05:00:00",
            "2026-1-2T03:04:05+05:00:00.5",
            "2026-1-2T03:04:05+05 ",
            "2026-1-2T03:04:05+05:00garbage",
            "2026-1-2T03:04:05z",
            "2026-1-2T03:04:05+053",
            "2026-1-2T03:04:05+05:3",
            "2026-1-2T03:04:05.1234567890123+05:00",
            // Regex arm `$`: only one bare trailing newline matches.
            "2026-01-02T03:04:05+05:00\n\n",
            "2026-01-02T03:04:05+05:00 \n",
            "2026-01-02T03:04:05+05:00\r\n",
            "2026-01-02T03:04:05+05:00:00.5\n",
            // Regex arm shapes: invalid unpadded dates/times, doubled
            // separator, 3-digit month, trailing junk, date-only with
            // trailing space (the whitespace tolerance needs the time).
            "2026-13-2T03:04:05",
            "2026-1-2T25:04:05",
            "2026-1-2  03:04:05",
            "2026-111-2T03:04:05",
            "2026-1-2T03:045",
            "2026-1-2T03:04:05x",
            "2026-1-2 ",
            "2026-01-02 ",
            // Fromisoformat arm: malformed tz fractions (empty,
            // non-digit, on `Z`, after a bare colon, on 5-digit
            // basic) and 24h totals with a fraction.
            "2026-01-02T03:04:05+05:00:00.",
            "2026-01-02T03:04:05+050000.",
            "2026-01-02T03:04:05+05:00:00.5x",
            "2026-01-02T03:04:05+24:00:00.0",
            "2026-01-02T03:04:05Z.5",
            "2026-01-02T03:04:05+05:.5",
            "2026-01-02T03:04:05+05300.5",
            // 1-digit times outside regex reach (tz seconds, week and
            // basic dates, `X` separator, hour-only, fraction without
            // seconds).
            "2024-01-15T1",
            "2024-01-15T3+05:00",
            "2024-01-15T1.5",
            "2024-01-15T03:4.5",
            "2024-01-15T1:2:3+05:00:00",
            "2024-W03T1:2:3",
            "20260102T1:2:3",
            "2026-01-02X1:2:3+05:00",
            // One gap byte at most, and single-byte only.
            "2026-01-02X03:04:05  +05:00",
            "20260102T030405  +05:00",
            "2026-01-02X03:04:05\u{85}+05:00",
            "2026-01-02X03:04:05\u{2003}+05:00",
            // A short fraction must abut the tz (no gap byte), and a
            // gap byte needs a tz after it.
            "2026-01-02X03:04:05.5\t+05:00",
            "2026-01-02X03:04:05.5 +05:00",
            "2026-01-02X03:04:05.5 Z",
            "2026-01-02X03:04:05.55\t+05:00",
            "2026-01-02X03:04:05,55\t+05:00",
            "2026-01-02X03:04:05.12345\t+05:00",
            "2026-01-02X03:04:05.1234567 ",
        ] {
            assert_eq!(parse_django_datetime(input), None, "{input}");
        }
    }

    #[test]
    fn deleted_at_validation_matches_datetimefield() {
        use chrono_tz::Tz;
        let utc = Tz::UTC;
        let case_in = |json: &str, timezone: &Tz| {
            let mut errors = FieldErrors::default();
            let value = validate_deleted_at(
                &serde_json::from_str(json).expect("json"),
                timezone,
                &mut errors,
            );
            (value, errors.deleted_at)
        };
        let case = |json: &str| case_in(json, &utc);
        let (value, errors) = case("null");
        assert!(errors.is_empty());
        assert!(matches!(value, Ok(Some(None))));
        let (parsed, errors) = case("\"2024-01-15T10:30:00Z\"");
        assert!(errors.is_empty());
        assert_eq!(
            parsed.expect("parses").expect("some").expect("some"),
            chrono::DateTime::parse_from_rfc3339("2024-01-15T10:30:00Z")
                .expect("rfc3339")
                .with_timezone(&chrono::Utc)
        );
        let (_, errors) = case("\"yesterday\"");
        assert_eq!(
            errors,
            vec!["Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].".to_owned()]
        );
        let (_, errors) = case("5");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("Datetime has wrong format."));
        // Aware instants past Python's year range are the `overflow`
        // field error (400), not the 500: `enforce_timezone` catches it.
        for raw in [
            "\"0001-01-01T00:00:00+05:00\"",
            "\"9999-12-31T23:00:00-05:00\"",
            "\"9999-12-31T23:00:00-14:00\"",
        ] {
            let (value, errors) = case(raw);
            assert_eq!(
                errors,
                vec!["Datetime value out of range.".to_owned()],
                "{raw}"
            );
            assert!(matches!(value, Ok(None)), "{raw}");
        }
        // The range covers both representations: an in-UTC instant whose
        // request-tz wall leaves the range overflows, and a UTC-out
        // instant overflows in every request timezone (CPython raises on
        // the UTC-side subtraction first — probed, both directions).
        let york = Tz::America__New_York;
        let kiriti = Tz::Pacific__Kiritimati;
        let (value, errors) = case_in("\"0001-01-01T00:30:00Z\"", &utc);
        assert!(errors.is_empty());
        assert!(matches!(value, Ok(Some(Some(_)))));
        let (value, errors) = case_in("\"0001-01-01T00:30:00Z\"", &york);
        assert_eq!(errors, vec!["Datetime value out of range.".to_owned()]);
        assert!(matches!(value, Ok(None)));
        let (value, errors) = case_in("\"9999-12-31T23:30:00Z\"", &utc);
        assert!(errors.is_empty());
        assert!(matches!(value, Ok(Some(Some(_)))));
        let (value, errors) = case_in("\"9999-12-31T23:30:00Z\"", &kiriti);
        assert_eq!(errors, vec!["Datetime value out of range.".to_owned()]);
        assert!(matches!(value, Ok(None)));
        for timezone in [&utc, &york] {
            let (value, errors) = case_in("\"9999-12-31T19:30:00-05:00\"", timezone);
            assert_eq!(
                errors,
                vec!["Datetime value out of range.".to_owned()],
                "{timezone:?}"
            );
            assert!(matches!(value, Ok(None)), "{timezone:?}");
        }
        // In-range extremes still validate.
        assert!(case("\"0001-01-01T00:00:00Z\"").0.is_ok());
        assert!(case("\"9999-12-31T23:59:59Z\"").0.is_ok());
    }

    #[test]
    fn project_dict_groups_and_pops() {
        let row = |project: &str, id: &str| {
            Map::from_iter([
                ("id".to_owned(), Value::String(id.to_owned())),
                ("role".to_owned(), Value::Number(15.into())),
                ("member".to_owned(), Value::String("m".to_owned())),
                ("project".to_owned(), Value::String(project.to_owned())),
                ("original_role".to_owned(), Value::Number(15.into())),
                ("created_at".to_owned(), Value::String("t".to_owned())),
            ])
        };
        let grouped = group_project_members(vec![row("p1", "a"), row("p2", "b"), row("p1", "c")]);
        assert_eq!(grouped.len(), 2);
        // First-seen key order, queryset row order within groups.
        let keys: Vec<&str> = grouped.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["p1", "p2"]);
        assert_eq!(grouped["p1"].as_array().expect("array").len(), 2);
        assert_eq!(grouped["p1"][0]["id"], Value::String("a".to_owned()));
        // The project key popped out of every row.
        for group in grouped.values() {
            for item in group.as_array().expect("array") {
                assert!(item.get("project").is_none());
                assert_eq!(item.as_object().expect("object").len(), 5);
            }
        }
        assert!(group_project_members(vec![]).is_empty());
    }

    #[test]
    fn null_me_row_matches_live_drf() {
        // Verified against `WorkspaceMemberMeSerializer(None).data` (field
        // order, read-only skips, `CharField`/`BooleanField` initials).
        let text = serde_json::to_string(&null_me_row()).expect("null row");
        assert_eq!(
            text,
            "{\"deleted_at\":null,\"role\":null,\"company_role\":\"\",\"view_props\":null,\"default_props\":null,\"issue_props\":null,\"is_active\":false,\"getting_started_checklist\":null,\"tips\":null,\"explored_features\":null,\"created_by\":null,\"updated_by\":null,\"workspace\":null,\"member\":null}"
        );
    }

    #[test]
    fn member_select_carries_scope_without_active_filter() {
        // R1 shape via the built statement: the slug subselect bound as
        // `$1`, the soft-delete guard, the `-created_at` ordering — and
        // no `is_active` filter (B5).
        let sql = member_rows_builder("acme", &[], None, false)
            .sql()
            .to_owned();
        assert!(
            sql.contains("(SELECT id FROM workspaces WHERE slug = $1)"),
            "{sql}"
        );
        assert!(sql.contains("wm.deleted_at IS NULL"));
        assert!(sql.contains("JOIN users u ON u.id = wm.member_id"));
        // The avatar joins are bare `LEFT` joins: `select_related` and
        // the FK descriptors ignore the related managers, so soft-deleted
        // assets and asset workspaces still attach.
        assert!(sql.contains("LEFT JOIN file_assets fa ON fa.id = u.avatar_asset_id "));
        assert!(sql.contains("LEFT JOIN workspaces faw ON faw.id = fa.workspace_id "));
        assert!(!sql.contains("fa.deleted_at"));
        assert!(!sql.contains("faw.deleted_at"));
        // B5 pins the *filter*, not the column: `is_active` is selected
        // (the serializer renders it) but never filtered on.
        let where_clause = sql
            .split_once("WHERE")
            .expect("member select filters in where")
            .1;
        assert!(!where_clause.contains("is_active"));
        // The slug binds exactly once: a literal `$1` in the head plus
        // `push_bind` would emit it twice and 500 every list/retrieve.
        assert_eq!(sql.matches("$1").count(), 1, "{sql}");
    }

    #[test]
    fn member_statement_numbers_placeholders() {
        // No terms, no target: the slug binds `$1`, then the guard and
        // the ordering follow with no gap or duplicated bind.
        let plain = member_rows_builder("acme", &[], None, false)
            .sql()
            .to_owned();
        assert_eq!(
            plain,
            format!("{MEMBER_SELECT}$1) AND wm.deleted_at IS NULL ORDER BY wm.created_at DESC")
        );
        // Terms AND after the guard, each binding two LIKEs.
        // `icontains` is the `UPPER` form, not `ILIKE`.
        let terms = member_rows_builder("acme", &["ada".to_owned()], None, false)
            .sql()
            .to_owned();
        assert_eq!(
            terms,
            format!(
                "{MEMBER_SELECT}$1) AND wm.deleted_at IS NULL AND (UPPER(u.display_name::text) LIKE UPPER($2) OR UPPER(u.first_name::text) LIKE UPPER($3)) ORDER BY wm.created_at DESC"
            )
        );
        // Target + LIMIT 1 (retrieve): the id binds after the term LIKEs.
        let id = Uuid::parse_str("12345678-1234-5678-1234-567812345678").expect("uuid");
        let detail = member_rows_builder("acme", &["ada".to_owned()], Some(&id), true)
            .sql()
            .to_owned();
        assert_eq!(
            detail,
            format!(
                "{MEMBER_SELECT}$1) AND wm.deleted_at IS NULL AND (UPPER(u.display_name::text) LIKE UPPER($2) OR UPPER(u.first_name::text) LIKE UPPER($3)) AND wm.id = $4 ORDER BY wm.created_at DESC LIMIT 1"
            )
        );
    }

    #[test]
    fn admin_branch_matches_both_spellings() {
        // `role > 5` (list, literal) and `role > ROLE.GUEST.value`
        // (retrieve, enum) are the same threshold.
        assert_eq!(qm::admin_branch_literal_sql(), "role > 5");
        assert_eq!(qm::admin_branch_enum_sql(), "role > ROLE.GUEST.value");
        assert!(!qm::is_admin_shape(5));
        assert!(qm::is_admin_shape(6));
        assert!(qm::is_admin_shape(15));
        assert!(qm::is_admin_shape(20));
        // Equal roles can remove (strict `<`, ported bug B8).
        assert!(qm::can_remove(20, 20));
        assert!(!qm::can_remove(15, 20));
        // The negated sole-admin spelling (`not count > 1`).
        assert!(qm::sole_workspace_admin_blocks(20, 1));
        assert!(!qm::sole_workspace_admin_blocks(20, 2));
        assert!(!qm::sole_workspace_admin_blocks(15, 1));
    }

    #[test]
    fn leave_keys_match_the_decorators() {
        use super::super::gates::{resolve_invalidation_path, InvalidationOrder};
        let user = "11111111-1111-1111-1111-111111111111";
        let points = invalidations_for(InvalidateAction::Leave);
        assert_eq!(points.len(), 3);
        assert!(points
            .iter()
            .all(|inv| inv.order == InvalidationOrder::BeforeGate));
        let keys: Vec<(String, bool)> = points
            .iter()
            .map(|inv| invalidation_key(inv, "acme", Some(user)))
            .collect();
        assert_eq!(
            keys[0],
            (
                resolve_invalidation_path("/api/workspaces/:slug/members/", "acme"),
                true
            )
        );
        assert_eq!(keys[1], (format!("/api/users/me/settings/:{user}"), false));
        // The slash-less key (ported bug B4): harmless inside the glob.
        assert_eq!(keys[2], ("api/users/me/workspaces/".to_owned(), true));
        // Single deletes need Django's stored-key prefix.
        assert_eq!(
            django_cache_key(&keys[1].0),
            format!(":1:/api/users/me/settings/:{user}")
        );
    }

    #[test]
    fn sample_row_renders_admin_and_plain_shapes() {
        use chrono_tz::Tz;
        let row = sample_row();
        let admin = render_member(&row, true, &Tz::UTC).expect("admin renders");
        let plain = render_member(&row, false, &Tz::UTC).expect("plain renders");
        for item in [&admin, &plain] {
            let keys: Vec<&str> = item
                .as_object()
                .expect("object")
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ser_workspace::WORKSPACE_MEMBER_WIRE_FIELDS);
            assert_eq!(item["role"], serde_json::json!(15));
        }
        // Only the nested user differs (admin-lite carries email).
        assert_eq!(
            admin["member"].as_object().expect("user").keys().len(),
            ser_user::USER_ADMIN_LITE_WIRE_FIELDS.len()
        );
        assert_eq!(
            plain["member"].as_object().expect("user").keys().len(),
            ser_user::USER_LITE_WIRE_FIELDS.len()
        );
        assert_eq!(admin["member"]["email"], serde_json::json!("ada@x.io"));
        assert!(plain["member"].get("email").is_none());
        // Datetimes render DRF-style in the request zone.
        assert_eq!(
            admin["created_at"],
            serde_json::json!("2025-01-01T00:00:00Z")
        );
        // No avatar asset and empty text: null.
        assert_eq!(admin["member"]["avatar_url"], Value::Null);
    }

    #[test]
    fn avatar_url_branches_match_the_property() {
        let mut row = sample_row();
        row.fa_id = Some(Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").expect("uuid"));
        // Attached static asset wins as-is.
        row.fa_entity_type = Some("USER_AVATAR".to_owned());
        let url = avatar_for_row(&row).expect("avatar").expect("some");
        assert_eq!(
            url,
            format!("/api/assets/v2/static/{}/", row.fa_id.expect("asset"))
        );
        // Attached asset with an unlisted entity returns None — no
        // fall-through to the text (Python checks only the FK).
        row.fa_entity_type = Some("BANNER".to_owned());
        row.u_avatar = "https://x/y.png".to_owned();
        assert_eq!(avatar_for_row(&row).expect("avatar"), None);
        // Unattached falls back to the text, then null.
        row.fa_id = None;
        assert_eq!(
            avatar_for_row(&row).expect("avatar"),
            Some("https://x/y.png".to_owned())
        );
        row.u_avatar = String::new();
        assert_eq!(avatar_for_row(&row).expect("avatar"), None);
    }

    #[test]
    fn attachment_asset_urls_need_workspace_context() {
        let mut row = sample_row();
        row.fa_id = Some(Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").expect("uuid"));
        row.fa_entity_type = Some("ISSUE_ATTACHMENT".to_owned());
        row.fa_workspace_slug = Some("acme".to_owned());
        let url = asset_url_for_row(&row).expect("url").expect("some");
        assert!(
            url.starts_with("/api/assets/v2/workspaces/acme/projects/"),
            "{url}"
        );
        assert!(url.contains("/attachments/"), "{url}");
        // NULL workspace FK: Python raises `AttributeError` (500).
        row.fa_workspace_id = None;
        row.fa_workspace_slug = None;
        assert!(matches!(asset_url_for_row(&row), Err(Denial::ServerError)));
        // Dangling FK (no row at all — a soft-deleted workspace still
        // joins): the FK descriptor 404s.
        row.fa_workspace_id = Some(Uuid::new_v4());
        assert!(matches!(
            asset_url_for_row(&row),
            Err(Denial::ObjectNotFound)
        ));
    }

    #[test]
    fn me_row_renders_with_count() {
        use chrono_tz::Tz;
        let row = MeRow {
            id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("uuid"),
            created_at: chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
                .expect("dt")
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339("2025-01-02T00:00:00Z")
                .expect("dt")
                .with_timezone(&chrono::Utc),
            deleted_at: None,
            role: 20,
            company_role: None,
            view_props: serde_json::json!({}),
            default_props: serde_json::json!({}),
            issue_props: serde_json::json!({}),
            is_active: true,
            getting_started_checklist: serde_json::json!({}),
            tips: serde_json::json!({}),
            explored_features: serde_json::json!({}),
            created_by_id: None,
            updated_by_id: None,
            workspace_id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").expect("uuid"),
            member_id: Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").expect("uuid"),
            draft_issue_count: 3,
        };
        let item = render_me(&row, &Tz::UTC);
        let keys: Vec<&str> = item
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_workspace::WORKSPACE_MEMBER_ME_WIRE_FIELDS);
        assert_eq!(item["draft_issue_count"], serde_json::json!(3));
        assert_eq!(item["role"], serde_json::json!(20));
        assert_eq!(
            item["member"],
            serde_json::json!("cccccccc-cccc-cccc-cccc-cccccccccccc")
        );
    }

    #[test]
    fn drf_escape_matches_the_renderer() {
        assert_eq!(drf_escape("a\u{2028}b\u{2029}c"), "a\\u2028b\\u2029c");
        assert_eq!(drf_escape("héllo"), "héllo");
    }

    fn sample_row() -> MemberFullRow {
        MemberFullRow {
            id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").expect("uuid"),
            created_at: chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
                .expect("dt")
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339("2025-01-02T00:00:00Z")
                .expect("dt")
                .with_timezone(&chrono::Utc),
            deleted_at: None,
            role: 15,
            company_role: None,
            view_props: serde_json::json!({}),
            default_props: serde_json::json!({}),
            issue_props: serde_json::json!({}),
            is_active: true,
            getting_started_checklist: serde_json::json!({}),
            tips: serde_json::json!({}),
            explored_features: serde_json::json!({}),
            created_by_id: None,
            updated_by_id: None,
            workspace_id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").expect("uuid"),
            u_id: Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").expect("uuid"),
            u_first_name: "Ada".to_owned(),
            u_last_name: "L".to_owned(),
            u_avatar: String::new(),
            u_is_bot: false,
            u_display_name: "Ada".to_owned(),
            u_email: Some("ada@x.io".to_owned()),
            u_last_login_medium: "email".to_owned(),
            fa_id: None,
            fa_entity_type: None,
            fa_workspace_id: None,
            fa_project_id: None,
            fa_issue_id: None,
            fa_workspace_slug: None,
        }
    }
}
