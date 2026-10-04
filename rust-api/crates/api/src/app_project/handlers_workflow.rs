//! State + estimate handlers (D-25, stage 5, PIDASHCONV-574).
//!
//! Ports `apps/api/pi_dash/app/views/state/base.py:27-157`
//! (`StateViewSet`, `IntakeStateEndpoint`) and
//! `apps/api/pi_dash/app/views/estimate/base.py:29-247`
//! (`generate_random_name`, `ProjectEstimatePointEndpoint`,
//! `BulkEstimatePointEndpoint`, `EstimatePointEndpoint`) — 5 units over
//! the 9 routes in `app/urls/state.py:11-32` + `app/urls/estimate.py:15-41`.
//!
//! Fixture id: FX-APROJ-10 (`rust-api/fixtures/app_project/`).
//!
//! Layering: read shapes + validators from
//! `pidash_services::app_project::ser_workflow` (L3), column consts +
//! `slugify_name` / `sequence_on_add` / `classify_lookup` from
//! `pidash_db::app_project::models` (L5), gate rows + key math from
//! `super::gates` (L7), `point_destroy_plan` / `estimate_point_dumps` /
//! error-body consts from `pidash_services::app_project::tasks` (L8).
//! Auth is the shared `crate::license::resolve_actor` (Django session +
//! user timezone, the sibling-handler precedent). Like the sibling
//! handler files this module is self-contained (private `Denial`,
//! private validators); merges keep both sides.
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * Bulk create persists the `Estimate` row *before* points validation
//!   (`estimate/base.py:69` vs `:78-80`), so a 400 on bad points still
//!   creates a point-less estimate (fixture `bulk_create_long_value_400`
//!   + the `Bad` row in `bulk_list`).
//! * Bulk `partial_update` looks the estimate up unscoped
//!   (`Estimate.objects.get(pk=estimate_id)`, `:116`) — a UUID from any
//!   project renames it — while the points filter stays scoped.
//! * `EstimatePointEndpoint.create` requires *truthy* key and value
//!   (`:157`): `key=0` is rejected with 400 even though the model
//!   default is 0 (fixture `point_create_zero_key_400`).
//! * Point destroy runs `issues.update(estimate_point_id=new)` *inside*
//!   the per-issue loop (`:198-210`), re-evaluated each iteration, and
//!   enqueues the first `issue_activity` before the update that would
//!   reject a garbage id.
//! * Point destroy on a missing point raises `AttributeError`
//!   (`old_estimate_point.key` on `None`, `:236`) → 500, not 404.
//! * `mark_as_default` checks nothing: a missing `pk` still clears the
//!   old default and answers 204.
//! * State create with a valid `order` key raises `TypeError` (the
//!   declared field is passed to `State.objects.create`) → 500; on
//!   `partial_update` the same key is `setattr`'d (unwritten) and echoed
//!   as the response's last field.
//! * Point `partial_update` with `{}` fails `validate()` (`if not data`,
//!   `estimate.py:22`) → 400 `{"non_field_errors": ["Estimate points
//!   are required"]}`.
//! * The state-list member join carries no soft-delete guard (L6 ported
//!   bug 9), so deleted memberships still scope rows (with `DISTINCT`).
//! * `ProjectEstimatePointEndpoint.get` returns `[]` without a query
//!   when `project.estimate_id` is `None` (`:46`).
//!
//! # Unpinned edges (documented, not ported)
//!
//! * Audit keys (`created_by`, `updated_by`, `deleted_at`, …) inside bulk
//!   point payloads are validated by DRF but ignored by the ORM
//!   construct; this port ignores them outright — no fixture or contract
//!   pins invalid audit keys there.
//! * Cache invalidation (`invalidate_cache`, single-key `DEL`) has no
//!   primitive in the read-only foundation `RedisHandle` (only the
//!   `multiple=True` `KEYS`+`DEL`), and no D-25 `GET` reads the cache,
//!   so the key + order (`super::gates::invalidation_for`) is cited at
//!   each write site but not executed — the license-handler precedent.
//! * Garbage UUIDs on `<uuid:…>` path params answer the 400
//!   invalid-detail body (the merged favorites precedent); Django's URL
//!   resolver would 404 instead. Unpinned either way.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};

use pidash_auth::permissions::allow::AllowFacts;
use pidash_auth::permissions::project::ProjectFacts;
use pidash_auth::permissions::project::StateMutationFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_db::app_project::models::project::{classify_lookup, ProjectLookup};
use pidash_db::app_project::models::state as state_model;
use pidash_services::app_project::ser_workflow as ser;
use pidash_services::app_project::tasks as t;
use pidash_types::{ProjectId, WorkspaceId};
use rand::Rng;

use crate::license::{resolve_actor, Actor};
use crate::middleware::SessionHandle;
use crate::serializer::render_datetime_in;
use crate::state::AppState;

use super::gates;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `StateViewSet` collection (`app/urls/state.py:13-18`).
pub const STATES_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/states/";
/// `StateViewSet` detail (`app/urls/state.py:19-24`).
pub const STATE_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/states/{pk}/";
/// `IntakeStateEndpoint` (`app/urls/state.py:25-29`).
pub const INTAKE_STATE_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/intake-state/";
/// `StateViewSet.mark_as_default` (`app/urls/state.py:30-34` — the route
/// name is `project-state`, same as the detail route).
pub const MARK_DEFAULT_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/states/{pk}/mark-default/";
/// `ProjectEstimatePointEndpoint` (`app/urls/estimate.py:15-20`).
pub const PROJECT_ESTIMATES_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/project-estimates/";
/// `BulkEstimatePointEndpoint` collection (`app/urls/estimate.py:21-26`).
pub const ESTIMATES_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/estimates/";
/// `BulkEstimatePointEndpoint` detail (`app/urls/estimate.py:27-32`).
pub const ESTIMATE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/";
/// `EstimatePointEndpoint` collection (`app/urls/estimate.py:33-37`).
pub const POINTS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/";
/// `EstimatePointEndpoint` detail (`app/urls/estimate.py:38-43`; the id
/// segment is an unconverted `<estimate_point_id>`).
pub const POINT_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/{estimate_point_id}/";

/// The nine state/estimate route groups: owned methods serve, every other
/// method proxies to Django (its 405-after-auth lives there — the pilot
/// cutover row). Cutover into the serving router stays with the domain
/// gate (PIDASHCONV-575), so this is additive only.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            STATES_PATH,
            owned(
                axum::routing::get(state_list).post(state_create),
                &["GET", "POST"],
            ),
        )
        .route(
            STATE_PATH,
            owned(
                axum::routing::get(state_retrieve)
                    .patch(state_partial_update)
                    .delete(state_destroy),
                &["GET", "PATCH", "DELETE"],
            ),
        )
        .route(
            INTAKE_STATE_PATH,
            owned(axum::routing::get(intake_state_get), &["GET"]),
        )
        .route(
            MARK_DEFAULT_PATH,
            owned(axum::routing::post(state_mark_default), &["POST"]),
        )
        .route(
            PROJECT_ESTIMATES_PATH,
            owned(axum::routing::get(project_estimates_get), &["GET"]),
        )
        .route(
            ESTIMATES_PATH,
            owned(
                axum::routing::get(bulk_list).post(bulk_create),
                &["GET", "POST"],
            ),
        )
        .route(
            ESTIMATE_PATH,
            owned(
                axum::routing::get(bulk_retrieve)
                    .patch(bulk_partial_update)
                    .delete(bulk_destroy),
                &["GET", "PATCH", "DELETE"],
            ),
        )
        .route(
            POINTS_PATH,
            owned(axum::routing::post(point_create), &["POST"]),
        )
        .route(
            POINT_PATH,
            owned(
                axum::routing::patch(point_partial_update).delete(point_destroy),
                &["PATCH", "DELETE"],
            ),
        )
}

/// An owned path: listed methods serve from Rust, everything else falls
/// through to Django (the sibling-handler `owned` shape).
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

/// `Project.resolve` miss (`db/models/project.py:214-218`): Django `Http404`
/// propagates through DRF's `exception_handler` as `NotFound(*args)`,
/// rendering compact lowercase `{"detail": …}` (DRF 3.15.2 `views.py:96`,
/// verified live — FX-APROJ-08's capital-`Detail` pin is a fixture typo;
/// the gate compares against live Django).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// Dup state name (`state/base.py:60-64`): any `23505` at state insert —
/// the `DETAIL` line (`… already exists.`) fires for the name constraint.
/// (A PK collision would match too, but fresh `gen_random_uuid()`s never
/// collide.)
pub const STATE_DUP_NAME_BODY: &str = r#"{"name":"The state name is already taken"}"#;
/// Default-state destroy veto (`state/base.py:129-132`).
pub const DEFAULT_DELETE_BODY: &str = r#"{"error":"Default state cannot be deleted"}"#;
/// Non-empty-state destroy veto (`state/base.py:137-141`).
pub const NONEMPTY_DELETE_BODY: &str =
    r#"{"error":"The state is not empty, only empty states can be deleted"}"#;
/// Missing triage state (`state/base.py:152-155`).
pub const TRIAGE_MISSING_BODY: &str = r#"{"error":"Triage state not found"}"#;
/// Bulk `partial_update` without points (`estimate/base.py:110-114`).
pub const POINTS_REQUIRED_BODY: &str = r#"{"error":"Estimate points are required"}"#;
/// Point create without truthy key+value (`estimate/base.py:157-161`).
pub const KEY_VALUE_REQUIRED_BODY: &str = r#"{"error":"Key and value are required"}"#;

/// Handler failure with its exact status + body (the sibling-handler
/// file-local `Denial` shape).
#[derive(Debug)]
enum Denial {
    /// 401, DRF `NotAuthenticated`.
    Unauthorized,
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 403, permission-class body (`ProjectEntityPermission` on the bulk
    /// routes: DRF's default `PermissionDenied` detail).
    ClassDenied,
    /// 403, inline `can_mutate_states` body on the four state writes.
    MembersBlocked,
    /// 404, `{"Detail":"Project not found"}` (identifier-rewrite miss).
    ResolveNotFound,
    /// 404, `ObjectDoesNotExist` branch (bare `.get()` miss).
    ObjectNotFound,
    /// 400, Django `ValidationError` branch (bad UUIDs in filters/paths).
    BadValidation,
    /// 400, `IntegrityError` branch (write constraint failures).
    BadPayload,
    /// 500, generic branch.
    ServerError,
    /// A pre-rendered exact body with its status (serializer `errors`
    /// dicts, whose key order DRF fixes).
    Raw(StatusCode, String),
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, gates::ANON_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, gates::FORBIDDEN_BODY.to_owned()),
            Denial::ClassDenied => (StatusCode::FORBIDDEN, gates::CLASS_DENIED_BODY.to_owned()),
            Denial::MembersBlocked => (
                StatusCode::FORBIDDEN,
                gates::MEMBERS_BLOCKED_BODY.to_owned(),
            ),
            Denial::ResolveNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::ObjectNotFound => (StatusCode::NOT_FOUND, t::OBJECT_NOT_FOUND_BODY.to_owned()),
            Denial::BadValidation => (StatusCode::BAD_REQUEST, t::VALIDATION_ERROR_BODY.to_owned()),
            Denial::BadPayload => (StatusCode::BAD_REQUEST, t::INTEGRITY_ERROR_BODY.to_owned()),
            Denial::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                t::SERVER_ERROR_BODY.to_owned(),
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
            .expect("denial response")
    }
}

type HandlerResult = Result<Response, Denial>;

/// `sqlx` failure mapped through `handle_exception`: constraint-class
/// (`23…`) → the `IntegrityError` 400, everything else → the generic 500
/// (the sibling `db_denial` shape).
fn db_denial(error: sqlx::Error) -> Denial {
    if let sqlx::Error::Database(db_error) = &error {
        if db_error.code().is_some_and(|code| code.starts_with("23")) {
            return Denial::BadPayload;
        }
    }
    Denial::ServerError
}

fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("view response")
}

fn json_ok(body: String) -> Response {
    json_response(StatusCode::OK, body)
}

fn no_content() -> Response {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(Vec::new()))
        .expect("empty 204")
}

// ---------------------------------------------------------------------------
// Query map (Django QueryDict: repeats legal, .get returns the last)
// ---------------------------------------------------------------------------

/// One query value, repeated or not (the pilot `OneOrMany` shape).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map the list handler extracts.
type QueryMap = HashMap<String, OneOrMany>;

/// Django `QueryDict.get`: the last value, or `None` when absent.
fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => one.clone(),
        OneOrMany::Many(many) => many.last().cloned().unwrap_or_default(),
    })
}

// ---------------------------------------------------------------------------
// Request context: auth + rewrite + tenant + membership
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// Session auth (`BaseSessionAuthentication` + `IsAuthenticated`):
/// anonymous answers the DRF `NotAuthenticated` body before anything else
/// runs (the sibling `actor` shape over the shared resolver).
async fn actor(
    state: &AppState,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Actor, Denial> {
    let pool = pool_of(state)?;
    resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::Unauthorized)
}

/// `_rewrite_project_kwarg` (`app/views/base.py:49-81`): UUID-looking input
/// passes through unverified (the L5 `classify_lookup` rule, same
/// spellings as `uuid.UUID()`); anything else matches
/// `UPPER(identifier)` in the workspace; a miss answers the resolve 404.
async fn resolve_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, Denial> {
    match classify_lookup(raw) {
        ProjectLookup::Pk(id) => Ok(id),
        ProjectLookup::Identifier(name) => {
            let row: Option<(uuid::Uuid,)> = sqlx::query_as(
                r#"SELECT p.id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
                   WHERE w.slug = $1 AND p.identifier = $2 AND p.deleted_at IS NULL"#,
            )
            .bind(slug)
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(db_denial)?;
            row.map(|row| row.0).ok_or(Denial::ResolveNotFound)
        }
    }
}

/// Badly-formed UUIDs in filters/paths render the `ValidationError` branch
/// (`app/views/base.py:126-130`): 400 `{"error": "Please provide valid
/// detail"}`.
fn parse_uuid_or_invalid(raw: &str) -> Result<uuid::Uuid, Denial> {
    raw.parse::<uuid::Uuid>().map_err(|_| Denial::BadValidation)
}

/// `UUIDField.to_python` for a raw JSON id (`fields/__init__.py`):
/// in-range ints/bools → `UUID(int=…)`; floats, composites,
/// out-of-range ints, and garbage strings raise `ValidationError`.
/// (Callers filter `None`/`null` first — it passes `to_python` through.
/// Integer spellings past `u64::MAX` parse as `f64`, so they 400 here;
/// Django would `UUID(int=…)` the few below 2^128 — no test sends
/// 20-digit ids.)
fn raw_uuid_prep(raw: &Value) -> Result<uuid::Uuid, ()> {
    match raw {
        Value::Null => Err(()),
        Value::Bool(flag) => Ok(uuid::Uuid::from_u128(u128::from(*flag))),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                u128::try_from(int)
                    .map(uuid::Uuid::from_u128)
                    .map_err(|_| ())
            } else if let Some(uint) = number.as_u64() {
                Ok(uuid::Uuid::from_u128(u128::from(uint)))
            } else {
                Err(())
            }
        }
        Value::String(text) => text.parse::<uuid::Uuid>().map_err(|_| ()),
        Value::Array(_) | Value::Object(_) => Err(()),
    }
}

/// Membership roles for the gates: active, non-deleted rows scoped to the
/// workspace slug (and project id for the project row) — the sibling
/// `membership` shape. The joined `workspaces` rows carry no soft-delete
/// guard (forward-FK traversal, L6 ported bug 9).
struct Membership {
    workspace_role: Option<i16>,
    project_role: Option<i16>,
}

async fn membership(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Membership, Denial> {
    let workspace_role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    let row: Option<(i16,)> = sqlx::query_as(
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
    .map_err(db_denial)?;
    Ok(Membership {
        workspace_role: workspace_role.map(|row| row.0),
        project_role: row.map(|row| row.0),
    })
}

/// `can_mutate_states` inputs (`app/permissions/project.py:146-184`): the
/// live active project row plus `project.members_can_edit_states` (the
/// `.values(…)` traversal applies no liveness filter to `projects`), and
/// the workspace-admin flag.
async fn state_mutation_facts(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<StateMutationFacts, Denial> {
    let row: Option<(i16, bool)> = sqlx::query_as(
        r#"SELECT pm.role, p.members_can_edit_states FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           JOIN projects p ON p.id = pm.project_id
           WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3
             AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    let admin: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE w.slug = $1 AND wm.member_id = $2 AND wm.role = $3
             AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .bind(ROLE_ADMIN)
    .fetch_optional(pool)
    .await
    .map_err(db_denial)?;
    // A missing membership row denies before the admin override is
    // reached (the kernel rule); the flag is unread then.
    let (project_role, members_can_edit_states) = match row {
        Some((role, flag)) => (Some(i32::from(role)), flag),
        None => (None, false),
    };
    Ok(StateMutationFacts {
        authenticated: true,
        project_role,
        members_can_edit_states,
        is_workspace_admin: admin.is_some(),
    })
}

/// Build `AllowFacts` for one decorator gate row.
fn allow_facts(
    slug: &str,
    workspace_role: Option<i16>,
    project_role: Option<i16>,
    allowed: &[i32],
) -> AllowFacts {
    let ws = workspace_role.map(i32::from);
    let proj = project_role.map(i32::from);
    AllowFacts {
        workspace: WorkspaceId::from(slug),
        authenticated: true,
        is_workspace_member: ws.is_some(),
        has_allowed_workspace_role: ws.is_some_and(|role| allowed.contains(&role)),
        is_creator: false,
        has_allowed_project_role: proj.is_some_and(|role| allowed.contains(&role)),
        is_project_member: proj.is_some(),
        is_workspace_admin: ws == Some(ROLE_ADMIN),
    }
}

/// Enforce the gate-table row for one decorator-guarded method+path:
/// anonymous never reaches here ([`actor`] denied first); a deny answers
/// the decorator 403.
fn check_gate(
    method: &str,
    path: &str,
    slug: &str,
    workspace_role: Option<i16>,
    project_role: Option<i16>,
) -> Result<(), Denial> {
    let row = gates::gate_for(method, path).ok_or(Denial::ServerError)?;
    let scope = gates::tenant_context(slug);
    let roles: &[i32] = match &row.gate {
        gates::Gate::Workspace { roles } | gates::Gate::Project { roles } => roles,
        _ => &[],
    };
    match gates::decide_gate(
        &row.gate,
        &scope,
        &allow_facts(slug, workspace_role, project_role, roles),
    ) {
        gates::GateOutcome::Allow => Ok(()),
        gates::GateOutcome::Deny => Err(Denial::Forbidden),
        gates::GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

/// Build `ProjectFacts` for the bulk-estimate class gate
/// (`ProjectEntityPermission`, `project.py:85-116`): safe methods need
/// project membership, writes need project Admin/Member. No D-25 view
/// sets `project_identifier`, so that branch stays `false` (ported per
/// the fixture, dead in this domain).
fn project_facts(
    slug: &str,
    project_id: &uuid::Uuid,
    workspace_role: Option<i16>,
    project_role: Option<i16>,
) -> ProjectFacts {
    let ws = workspace_role.map(i32::from);
    let proj = project_role.map(i32::from);
    let senior = |role: Option<i32>| role == Some(ROLE_ADMIN) || role == Some(ROLE_MEMBER);
    ProjectFacts {
        workspace: WorkspaceId::from(slug),
        project_id: ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: ws.is_some(),
        has_workspace_admin_or_member: senior(ws),
        is_workspace_admin: ws == Some(ROLE_ADMIN),
        is_project_member: proj.is_some(),
        is_project_admin: proj == Some(ROLE_ADMIN),
        has_project_admin_or_member: senior(proj),
        has_identifier_membership: false,
        has_project_identifier: false,
    }
}

/// Enforce the bulk-estimate class gate: anonymous 401s (already denied
/// by [`actor`]); authenticated denials render the class 403 body.
/// Runs in DRF `initial()`, *before* the `@invalidate_cache` wrapper —
/// a 403 here does not invalidate (`InvalidationOrder::AfterPermissionCheck`).
fn check_project_entity_gate(
    method: &str,
    slug: &str,
    project_id: &uuid::Uuid,
    membership: &Membership,
) -> Result<(), Denial> {
    let scope = gates::tenant_context(slug);
    let facts = project_facts(
        slug,
        project_id,
        membership.workspace_role,
        membership.project_role,
    );
    match gates::decide_project_entity_gate(method, &scope, &facts) {
        gates::GateOutcome::Allow => Ok(()),
        gates::GateOutcome::Deny => Err(Denial::ClassDenied),
        gates::GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

/// Enforce the inline `can_mutate_states` check on the four state writes:
/// runs *after* the decorator gate; a deny answers the members-blocked
/// 403. (The `@invalidate_cache` wrapper sits *outside* the decorator,
/// so a 403 still invalidates — `InvalidationOrder::BeforeGate`; the key
/// math lives in `super::gates` and is cited, not executed — see the
/// module docs.)
fn check_state_mutation(facts: &StateMutationFacts) -> Result<(), Denial> {
    match gates::decide_state_mutation(facts) {
        gates::GateOutcome::Allow => Ok(()),
        gates::GateOutcome::Deny => Err(Denial::MembersBlocked),
        gates::GateOutcome::Unauthenticated => Err(Denial::Unauthorized),
    }
}

// ---------------------------------------------------------------------------
// Time + rendering
// ---------------------------------------------------------------------------

/// `timezone.now()` truncated to microseconds: Postgres `timestamptz`
/// stores micros, and Python datetimes carry micros at most.
fn utc_now_micros() -> DateTime<Utc> {
    let now = Utc::now();
    now.with_nanosecond(now.nanosecond() / 1000 * 1000)
        .unwrap_or(now)
}

/// Render an aware datetime in the request user's zone (the
/// `TimezoneMixin` rule) through the DRF `iso-8601` kernel.
fn render_dt(value: &DateTime<Utc>, timezone: Tz) -> String {
    render_datetime_in(value, &timezone)
}

fn render_dt_opt(value: Option<DateTime<Utc>>, timezone: Tz) -> Option<String> {
    value.map(|dt| render_datetime_in(&dt, &timezone))
}

// ---------------------------------------------------------------------------
// DRF field validation (verified against the pinned DRF 3.15.2 source)
// ---------------------------------------------------------------------------

/// CPython `str()` for one JSON value (nested position: strings raw).
/// Used where the ORM coerces raw input (`CharField.get_prep_value`).
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", python_repr_string(key), python_repr(item)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// CPython `repr` for one JSON value (nested position: strings quoted).
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(text) => python_repr_string(text),
        Value::Array(_) | Value::Object(_) => python_str(value),
        _ => python_str(value),
    }
}

/// CPython `repr` for one string: single quotes unless the text contains
/// one (but no double quote), with `\`/`\n`/`\r`/`\t` escaped.
fn python_repr_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// Python `type(data).__name__` for JSON inputs (serializer `invalid` /
/// `not_a_list` messages).
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Python truthiness for JSON inputs (`request.data.get(…)` guards).
fn python_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(float) = number.as_f64() {
                float != 0.0
            } else {
                // `u64`-only magnitudes are never zero here.
                true
            }
        }
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

/// DRF `CharField`: null → the null error; blank (empty, or whitespace
/// when trimming) → blank/`""`; numbers coerce via `str()`; bools and
/// composites → invalid. `max_length` counts code points over the
/// *stripped* value (`fields.py`).
fn validate_char(
    value: &Value,
    max_length: Option<usize>,
    allow_blank: bool,
) -> Result<String, String> {
    if value.is_null() {
        return Err("This field may not be null.".to_owned());
    }
    match value {
        Value::String(text) => {
            if text.is_empty() || text.trim().is_empty() {
                if allow_blank {
                    return Ok(String::new());
                }
                return Err("This field may not be blank.".to_owned());
            }
            // `trim_whitespace=True` (the default): strip, then validate.
            let stripped = text.trim();
            if let Some(max) = max_length {
                if stripped.chars().count() > max {
                    return Err(format!(
                        "Ensure this field has no more than {max} characters."
                    ));
                }
            }
            Ok(stripped.to_owned())
        }
        Value::Number(number) => {
            let text = number.to_string();
            if let Some(max) = max_length {
                if text.chars().count() > max {
                    return Err(format!(
                        "Ensure this field has no more than {max} characters."
                    ));
                }
            }
            Ok(text)
        }
        _ => Err("Not a valid string.".to_owned()),
    }
}

/// DRF `BooleanField`: set membership, so `1.0` is `true` (`1.0 == 1`,
/// same hash) exactly like `1`, and `0.0` is `false` like `0`; null →
/// the null error; unhashables → invalid (`fields.py`).
fn validate_bool(value: &Value) -> Result<bool, String> {
    const INVALID: &str = "Must be a valid boolean.";
    match value {
        Value::Null => Err("This field may not be null.".to_owned()),
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if number.as_i64() == Some(1) || number.as_f64() == Some(1.0) {
                Ok(true)
            } else if number.as_i64() == Some(0) || number.as_f64() == Some(0.0) {
                Ok(false)
            } else {
                Err(INVALID.to_owned())
            }
        }
        Value::String(text) => match text.to_ascii_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
            _ => Err(INVALID.to_owned()),
        },
        _ => Err(INVALID.to_owned()),
    }
}

/// DRF `re_decimal = re.compile(r'\.0*\s*$')` substitution, then
/// `int(…)`: strings over 1000 chars → the size error; bools, floats
/// with a fraction, and non-numerics → invalid. Values outside `i64`
/// (Python ints are unbounded) surface as [`IntValue::TooBig`], which
/// dies at the column like Python's `DataError` 500.
#[derive(Debug)]
enum IntValue {
    Value(i64),
    TooBig,
}

/// DRF `IntegerField` (`fields.py`).
fn validate_int(value: &Value) -> Result<IntValue, String> {
    const INVALID: &str = "A valid integer is required.";
    if value.is_null() {
        return Err("This field may not be null.".to_owned());
    }
    if let Value::String(text) = value {
        if text.len() > 1000 {
            return Err("String value too large.".to_owned());
        }
    }
    // `str(data)` for JSON inputs, then DRF's `re_decimal`.
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => {
            return Err(INVALID.to_owned());
        }
        Value::Null => unreachable!("checked above"),
    };
    let stripped = strip_decimal_zeros(text.trim_end());
    let trimmed = stripped.trim();
    match trimmed.parse::<i64>() {
        Ok(int) => Ok(IntValue::Value(int)),
        Err(_) => {
            // `i128` still parses what `i64` cannot: Python would accept
            // it and die at the column (`DataError` 500).
            if trimmed.parse::<i128>().is_ok() {
                Ok(IntValue::TooBig)
            } else {
                Err(INVALID.to_owned())
            }
        }
    }
}

/// DRF `re_decimal` (`\.0*\s*$`, applied to the `trim_end`-ed text):
/// strip a trailing zero run only when a literal dot precedes it.
fn strip_decimal_zeros(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'0' {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] == b'.' {
        return text[..end - 1].to_owned();
    }
    text.to_owned()
}

/// DRF `FloatField`: `float(data)` — bools coerce (`True` → `1.0`);
/// strings over 1000 chars → the size error; non-numerics → invalid.
/// Non-finite results answer the overflow message (DRF accepts
/// float-syntax `inf`, an unpinned edge — see module docs).
fn validate_float(value: &Value) -> Result<f64, String> {
    const INVALID: &str = "A valid number is required.";
    if value.is_null() {
        return Err("This field may not be null.".to_owned());
    }
    if let Value::String(text) = value {
        if text.len() > 1000 {
            return Err("String value too large.".to_owned());
        }
    }
    let parsed: Option<f64> = match value {
        Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        Value::Array(_) | Value::Object(_) | Value::Null => None,
    };
    match parsed {
        Some(float) if float.is_finite() => Ok(float),
        Some(_) => Err("Integer value too large to convert to float".to_owned()),
        None => Err(INVALID.to_owned()),
    }
}

/// DRF `ChoiceField` over string choices: the input is `str()`-coerced
/// before lookup; a miss renders the coerced input in the message
/// (`fields.py`).
fn validate_str_choice(value: &Value, choices: &[&str]) -> Result<String, String> {
    if value.is_null() {
        return Err("This field may not be null.".to_owned());
    }
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Array(_) | Value::Object(_) => python_str(value),
        Value::Null => unreachable!("checked above"),
    };
    if choices.contains(&text.as_str()) {
        Ok(text)
    } else {
        Err(format!("\"{text}\" is not a valid choice."))
    }
}

/// One field error entry, in field order: `{"<field>": ["<msg>", …]}`.
fn push_field_error(out: &mut Map<String, Value>, field: &str, message: String) {
    match out.get_mut(field) {
        Some(Value::Array(messages)) => {
            messages.push(Value::String(message));
        }
        _ => {
            out.insert(field.to_owned(), Value::Array(vec![Value::String(message)]));
        }
    }
}

/// Root `{"non_field_errors": […]}` 400 for a non-mapping body:
/// `Invalid data. Expected a dictionary, but got {type}.`
/// (`serializers.py`).
fn invalid_data_body(value: &Value) -> String {
    let body = ser::non_field_errors(&format!(
        "Invalid data. Expected a dictionary, but got {}.",
        python_type_name(value)
    ));
    body.to_string()
}

/// `MinValueValidator(0)` on the point key (`estimate.py:45`,
/// application-level): DRF renders the `min_value` message.
fn key_min_error() -> String {
    "Ensure this value is greater than or equal to 0.".to_owned()
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

/// One state row for the read shape (the 9 `STATE_READ_KEYS` columns).
#[derive(Debug, Clone)]
struct StateRow {
    id: uuid::Uuid,
    project_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    name: String,
    color: String,
    group: String,
    default: bool,
    description: String,
    sequence: f64,
}

/// Render one state through the L3 read shape.
fn render_state(row: &StateRow) -> Value {
    let id = row.id.to_string();
    let project_id = row.project_id.to_string();
    let workspace_id = row.workspace_id.to_string();
    let row_view = ser::StateRow {
        id: &id,
        project_id: &project_id,
        workspace_id: &workspace_id,
        name: &row.name,
        color: &row.color,
        group: &row.group,
        default: row.default,
        description: &row.description,
        sequence: row.sequence,
    };
    let view = ser::state_to_representation(&row_view);
    serde_json::to_value(&view).expect("state view serializes")
}

type StateRowTuple = (
    uuid::Uuid,
    uuid::Uuid,
    uuid::Uuid,
    String,
    String,
    String,
    bool,
    String,
    f64,
);

fn state_row_from(tuple: StateRowTuple) -> StateRow {
    StateRow {
        id: tuple.0,
        project_id: tuple.1,
        workspace_id: tuple.2,
        name: tuple.3,
        color: tuple.4,
        group: tuple.5,
        default: tuple.6,
        description: tuple.7,
        sequence: tuple.8,
    }
}

/// `StateViewSet.list` (`state/base.py:84-109`): the scoped queryset
/// (live, non-triage, this slug+project, caller an active project
/// member, project unarchived, `is_triage=False`, `DISTINCT`, sequence
/// order) with the per-group `order = index/count` injection; `grouped`
/// answers the group-keyed dict only for the exact value `"true"`.
async fn state_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "GET",
        "workspaces/<slug>/projects/<project_id>/states/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    // The member join carries no soft-delete guard (L6 ported bug 9);
    // `DISTINCT` collapses the fan-out (`state/base.py:31-46`).
    let rows: Vec<StateRowTuple> = sqlx::query_as(
        r#"SELECT DISTINCT s.id, s.project_id, s.workspace_id, s.name, s.color,
                  s."group", s."default", s.description, s.sequence
           FROM states s
           JOIN projects p ON p.id = s.project_id
           JOIN workspaces w ON w.id = s.workspace_id
           JOIN project_members pm ON pm.project_id = p.id
           WHERE s.deleted_at IS NULL AND NOT (s."group" = 'triage')
             AND w.slug = $1 AND s.project_id = $2
             AND pm.member_id = $3 AND pm.is_active
             AND p.archived_at IS NULL AND NOT s.is_triage
           ORDER BY s.sequence ASC"#,
    )
    .bind(&slug)
    .bind(project_id)
    .bind(actor.id)
    .fetch_all(&pool)
    .await
    .map_err(db_denial)?;
    let mut states: Vec<Value> = rows
        .iter()
        .map(|tuple| render_state(&state_row_from(tuple.clone())))
        .collect();
    inject_state_order(&mut states);
    if query_last(&query, "grouped").as_deref() == Some("true") {
        return Ok(json_ok(render_grouped_states(&states)));
    }
    Ok(json_ok(Value::Array(states).to_string()))
}

/// The `order = index/count` injection (`state/base.py:92-96`): per
/// group, in queryset (sequence) order, `index` from 1.
fn inject_state_order(states: &mut [Value]) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for state in states.iter() {
        if let Some(group) = state.get("group").and_then(Value::as_str) {
            *counts.entry(group.to_owned()).or_insert(0) += 1;
        }
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    for state in states.iter_mut() {
        let group = state
            .get("group")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let count = counts.get(&group).copied().unwrap_or(1) as f64;
        let index = seen.entry(group).or_insert(0);
        *index += 1;
        let order = (*index as f64) / count;
        if let Some(map) = state.as_object_mut() {
            map.insert("order".to_owned(), serde_json::json!(order));
        }
    }
}

/// The `grouped=true` dict (`state/base.py:100-107`): stable sort by
/// group (Python code-point order = byte order), consecutive runs keyed
/// by `str(group)` in first-seen order.
fn render_grouped_states(states: &[Value]) -> String {
    let mut sorted: Vec<&Value> = states.iter().collect();
    sorted.sort_by(|a, b| {
        let group_a = a.get("group").and_then(Value::as_str).unwrap_or_default();
        let group_b = b.get("group").and_then(Value::as_str).unwrap_or_default();
        group_a.cmp(group_b)
    });
    let mut out = Map::with_capacity(sorted.len());
    for state in sorted {
        let group = state
            .get("group")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        match out.get_mut(&group) {
            Some(Value::Array(items)) => items.push(state.clone()),
            _ => {
                out.insert(group, Value::Array(vec![state.clone()]));
            }
        }
    }
    Value::Object(out).to_string()
}

/// `StateViewSet.retrieve` (DRF default over `get_queryset`): any
/// authenticated caller passes the gate; the scoped lookup 404s misses
/// (including triage rows, other projects, and non-members) through
/// `get_object_or_404`, whose `Http404` message DRF renders as
/// `{"detail": "No State matches the given query."}` — verified live,
/// not the bare-`.get()` error body.
async fn state_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "GET",
        "workspaces/<slug>/projects/<project_id>/states/<pk>/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let row: Option<StateRowTuple> = sqlx::query_as(
        r#"SELECT DISTINCT s.id, s.project_id, s.workspace_id, s.name, s.color,
                  s."group", s."default", s.description, s.sequence
           FROM states s
           JOIN projects p ON p.id = s.project_id
           JOIN workspaces w ON w.id = s.workspace_id
           JOIN project_members pm ON pm.project_id = p.id
           WHERE s.deleted_at IS NULL AND NOT (s."group" = 'triage')
             AND w.slug = $1 AND s.project_id = $2 AND s.id = $3
             AND pm.member_id = $4 AND pm.is_active
             AND p.archived_at IS NULL AND NOT s.is_triage"#,
    )
    .bind(&slug)
    .bind(project_id)
    .bind(pk)
    .bind(actor.id)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let row = row.ok_or_else(|| {
        Denial::Raw(
            StatusCode::NOT_FOUND,
            r#"{"detail":"No State matches the given query."}"#.to_owned(),
        )
    })?;
    Ok(json_ok(render_state(&state_row_from(row)).to_string()))
}

/// Validated state write fields (create + partial update share the
/// shape; `None` = absent on patch).
#[derive(Debug, Default)]
struct StateInput {
    name: Option<String>,
    color: Option<String>,
    group: Option<String>,
    default: Option<bool>,
    description: Option<String>,
    sequence: Option<f64>,
    /// Validated `order`: present on create → the `TypeError` 500; on
    /// patch `update()` `setattr`s it onto the in-memory instance so
    /// `.data` echoes it (last field) though `save()` never persists it.
    order: Option<f64>,
}

/// Validate one state payload through the `StateSerializer` field rules
/// (`serializers/state.py:12-30` + DRF model mapping): `name`/`color`
/// required non-blank ≤255; `group` a choice (default `backlog`);
/// `default` a bool (default `false`); `description` blank-ok (default
/// `""`); `sequence` a float (default `65535`); `order` a float.
/// `id`/`project_id`/`workspace_id` are read-only (property fallback)
/// and unknown keys are silently dropped. Returns the input plus a
/// validity flag; field errors accumulate in field order.
fn validate_state_input(
    data: &Map<String, Value>,
    partial: bool,
) -> (StateInput, Map<String, Value>) {
    let mut errors = Map::new();
    let mut input = StateInput::default();
    // `name`: required, non-blank, ≤255.
    match data.get("name") {
        None if partial => {}
        None => push_field_error(&mut errors, "name", "This field is required.".to_owned()),
        Some(value) => match validate_char(value, Some(255), false) {
            Ok(name) => input.name = Some(name),
            Err(message) => push_field_error(&mut errors, "name", message),
        },
    }
    // `color`: required, non-blank, ≤255.
    match data.get("color") {
        None if partial => {}
        None => push_field_error(&mut errors, "color", "This field is required.".to_owned()),
        Some(value) => match validate_char(value, Some(255), false) {
            Ok(color) => input.color = Some(color),
            Err(message) => push_field_error(&mut errors, "color", message),
        },
    }
    // `group`: choice, default `backlog`.
    match data.get("group") {
        None if partial => {}
        None => input.group = Some(state_model::DEFAULT_GROUP.to_owned()),
        Some(value) => match validate_str_choice(value, state_model::ALL_GROUPS) {
            Ok(group) => input.group = Some(group),
            Err(message) => push_field_error(&mut errors, "group", message),
        },
    }
    // `default`: bool, default `false`.
    match data.get("default") {
        None if partial => {}
        None => input.default = Some(false),
        Some(value) => match validate_bool(value) {
            Ok(default) => input.default = Some(default),
            Err(message) => push_field_error(&mut errors, "default", message),
        },
    }
    // `description`: blank-ok, default `""`.
    match data.get("description") {
        None if partial => {}
        None => input.description = Some(String::new()),
        Some(value) => match validate_char(value, None, true) {
            Ok(description) => input.description = Some(description),
            Err(message) => push_field_error(&mut errors, "description", message),
        },
    }
    // `sequence`: float, default `65535`.
    match data.get("sequence") {
        None if partial => {}
        None => input.sequence = Some(state_model::DEFAULT_SEQUENCE),
        Some(value) => match validate_float(value) {
            Ok(sequence) => input.sequence = Some(sequence),
            Err(message) => push_field_error(&mut errors, "sequence", message),
        },
    }
    // `order`: declared float; the validated value is retained for the
    // patch-response echo (create 500s on any present value instead).
    if let Some(value) = data.get("order") {
        match validate_float(value) {
            Ok(order) => input.order = Some(order),
            Err(message) => push_field_error(&mut errors, "order", message),
        }
    }
    (input, errors)
}

/// `StateViewSet.create` (`state/base.py:48-64`): invalidate-before-gate
/// (cited, not executed — see module docs), the decorator gate, the
/// `can_mutate_states` inline gate, then validate → triage veto →
/// insert, answering explicit 200 (never 201). A dup name answers the
/// `{"name": …}` 400; any other `IntegrityError` falls through to the
/// generic 500 (the bare `except` fallthrough returns `None`).
async fn state_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    // `invalidate_cache("workspaces/:slug/states/")` runs here, before
    // the gate (`InvalidationOrder::BeforeGate`) — cited, not executed.
    check_gate(
        "POST",
        "workspaces/<slug>/projects/<project_id>/states/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    let mutation = state_mutation_facts(&pool, &slug, &project_id, &actor.id).await?;
    check_state_mutation(&mutation)?;
    let data = body
        .0
        .as_object()
        .ok_or_else(|| Denial::Raw(StatusCode::BAD_REQUEST, invalid_data_body(&body.0)))?;
    let (input, errors) = validate_state_input(data, false);
    if !errors.is_empty() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            Value::Object(errors).to_string(),
        ));
    }
    // `StateSerializer.validate`: the triage veto (`state.py:31-34`).
    if let Some(errors) = ser::state_validate(input.group.as_deref()) {
        return Ok(json_response(StatusCode::BAD_REQUEST, errors.to_string()));
    }
    if input.order.is_some() {
        // The declared `order` reaches `State.objects.create(**validated)`
        // → `TypeError` → the generic 500.
        return Err(Denial::ServerError);
    }
    // `ProjectBaseModel.save` derives the workspace from the project
    // (plain FK traversal, no liveness filter); a missing project row
    // raises `DoesNotExist` → 404 (unreachable past the gates).
    let workspace: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT p.workspace_id FROM projects p WHERE p.id = $1"#)
            .bind(project_id)
            .fetch_optional(&pool)
            .await
            .map_err(db_denial)?;
    let (workspace_id,) = workspace.ok_or(Denial::ObjectNotFound)?;
    // `State.save`: `sequence = max(sibling) + 15000` over the default
    // (triage-excluding) manager; an explicit sequence is overwritten
    // when siblings exist.
    let largest: Option<(Option<f64>,)> = sqlx::query_as(
        r#"SELECT MAX(s.sequence) FROM states s
           WHERE s.project_id = $1 AND s.deleted_at IS NULL AND NOT (s."group" = 'triage')"#,
    )
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let max_sibling = largest.and_then(|(max,)| max);
    let sequence = state_model::sequence_on_add(max_sibling)
        .or(input.sequence)
        .unwrap_or(state_model::DEFAULT_SEQUENCE);
    let name = input.name.expect("validated name");
    let id = uuid::Uuid::new_v4();
    let now = utc_now_micros();
    let insert = sqlx::query(
        r#"INSERT INTO states
           (id, project_id, workspace_id, name, description, color, slug, sequence,
            "group", is_triage, "default", external_source, external_id,
            created_at, updated_at, created_by_id, updated_by_id, deleted_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, FALSE, $10, NULL, NULL,
                   $11, $11, $12, NULL, NULL)"#,
    )
    .bind(id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(&name)
    .bind(input.description.expect("validated description"))
    .bind(input.color.expect("validated color"))
    .bind(state_model::slugify_name(&name))
    .bind(sequence)
    .bind(input.group.expect("validated group"))
    .bind(input.default.expect("validated default"))
    .bind(now)
    .bind(actor.id)
    .execute(&pool)
    .await;
    if let Err(error) = insert {
        // `except IntegrityError: if "already exists"` — the `DETAIL`
        // line fires exactly for `23505`.
        if let sqlx::Error::Database(db_error) = &error {
            if db_error.code().as_deref() == Some("23505") {
                return Ok(json_response(
                    StatusCode::BAD_REQUEST,
                    STATE_DUP_NAME_BODY.to_owned(),
                ));
            }
        }
        return Err(Denial::ServerError);
    }
    // `serializer.data` renders the saved instance; re-read unscoped by
    // pk (the post-write re-read precedent).
    let row: Option<StateRowTuple> = sqlx::query_as(
        r#"SELECT s.id, s.project_id, s.workspace_id, s.name, s.color,
                  s."group", s."default", s.description, s.sequence
           FROM states s WHERE s.id = $1"#,
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let row = row.ok_or(Denial::ServerError)?;
    Ok(json_ok(render_state(&state_row_from(row)).to_string()))
}

/// `StateViewSet.partial_update` (`state/base.py:66-82`): the decorator
/// gate plus the inline `can_mutate_states` check (no `invalidate_cache`
/// on this action — ported as-is), the scoped lookup (default manager:
/// live + non-triage — no member/archived filter here), then partial
/// validate → triage veto → save, answering explicit 200.
async fn state_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "PATCH",
        "workspaces/<slug>/projects/<project_id>/states/<pk>/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    let mutation = state_mutation_facts(&pool, &slug, &project_id, &actor.id).await?;
    check_state_mutation(&mutation)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let current: Option<StateRowTuple> = sqlx::query_as(
        r#"SELECT s.id, s.project_id, s.workspace_id, s.name, s.color,
                  s."group", s."default", s.description, s.sequence
           FROM states s JOIN workspaces w ON w.id = s.workspace_id
           WHERE s.deleted_at IS NULL AND NOT (s."group" = 'triage')
             AND s.id = $1 AND s.project_id = $2 AND w.slug = $3"#,
    )
    .bind(pk)
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let current = state_row_from(current.ok_or(Denial::ObjectNotFound)?);
    let data = body
        .0
        .as_object()
        .ok_or_else(|| Denial::Raw(StatusCode::BAD_REQUEST, invalid_data_body(&body.0)))?;
    let (input, errors) = validate_state_input(data, true);
    if !errors.is_empty() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            Value::Object(errors).to_string(),
        ));
    }
    // `validate()` sees the partial attrs only: an absent group passes.
    if let Some(errors) = ser::state_validate(input.group.as_deref()) {
        return Ok(json_response(StatusCode::BAD_REQUEST, errors.to_string()));
    }
    // A valid `order` is `setattr`'d onto the in-memory instance and
    // dropped by `save()` (unwritten) — but `.data` still renders it.
    let name = input.name.unwrap_or(current.name);
    let color = input.color.unwrap_or(current.color);
    let group = input.group.unwrap_or(current.group);
    let default = input.default.unwrap_or(current.default);
    let description = input.description.unwrap_or(current.description);
    let sequence = input.sequence.unwrap_or(current.sequence);
    let now = utc_now_micros();
    let update = sqlx::query(
        r#"UPDATE states SET name = $1, color = $2, "group" = $3, "default" = $4,
                  description = $5, sequence = $6, slug = $7,
                  updated_at = $8, updated_by_id = $9 WHERE id = $10"#,
    )
    .bind(&name)
    .bind(&color)
    .bind(&group)
    .bind(default)
    .bind(&description)
    .bind(sequence)
    .bind(state_model::slugify_name(&name))
    .bind(now)
    .bind(actor.id)
    .bind(pk)
    .execute(&pool)
    .await;
    if let Err(error) = update {
        if let sqlx::Error::Database(db_error) = &error {
            if db_error.code().as_deref() == Some("23505") {
                return Ok(json_response(
                    StatusCode::BAD_REQUEST,
                    STATE_DUP_NAME_BODY.to_owned(),
                ));
            }
        }
        return Err(Denial::ServerError);
    }
    let row: Option<StateRowTuple> = sqlx::query_as(
        r#"SELECT s.id, s.project_id, s.workspace_id, s.name, s.color,
                  s."group", s."default", s.description, s.sequence
           FROM states s WHERE s.id = $1"#,
    )
    .bind(pk)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let row = row.ok_or(Denial::ServerError)?;
    Ok(json_ok(
        render_patched_state(&state_row_from(row), input.order).to_string(),
    ))
}

/// The patch response: the read shape plus the `setattr`'d `order` echo
/// as its last field when the payload carried a valid one (`.data`
/// renders the in-memory instance — `serializers.py`
/// `ModelSerializer.update` — and `order` is last in `Meta.fields`).
fn render_patched_state(row: &StateRow, order: Option<f64>) -> Value {
    let mut rendered = render_state(row);
    if let Some(order) = order {
        if let Some(map) = rendered.as_object_mut() {
            map.insert("order".to_owned(), serde_json::json!(order));
        }
    }
    rendered
}

/// `StateViewSet.mark_as_default` (`state/base.py:111-119`): clear every
/// default in the project, set this one — both `UPDATE`s manager-scoped
/// (live, non-triage), neither checking the `pk` exists — answering 204.
async fn state_mark_default(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    // `invalidate_cache("workspaces/:slug/states/")` runs here, before
    // the gate (`InvalidationOrder::BeforeGate`) — cited, not executed.
    check_gate(
        "POST",
        "workspaces/<slug>/projects/<project_id>/states/<pk>/mark-default/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    let mutation = state_mutation_facts(&pool, &slug, &project_id, &actor.id).await?;
    check_state_mutation(&mutation)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    // Plain `.update()`s: no `updated_at`/`updated_by` touch (queryset
    // updates bypass `save()`).
    sqlx::query(
        r#"UPDATE states s SET "default" = FALSE FROM workspaces w
           WHERE s.workspace_id = w.id AND w.slug = $1 AND s.project_id = $2
             AND s."default" AND s.deleted_at IS NULL AND NOT (s."group" = 'triage')"#,
    )
    .bind(&slug)
    .bind(project_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    sqlx::query(
        r#"UPDATE states s SET "default" = TRUE FROM workspaces w
           WHERE s.workspace_id = w.id AND w.slug = $1 AND s.project_id = $2 AND s.id = $3
             AND s.deleted_at IS NULL AND NOT (s."group" = 'triage')"#,
    )
    .bind(&slug)
    .bind(project_id)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    Ok(no_content())
}

/// `StateViewSet.destroy` (`state/base.py:121-144`): the scoped lookup
/// (live, non-triage, plus the explicit `is_triage=False`), the
/// default/non-empty vetoes, then a soft delete answering 204.
async fn state_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    // `invalidate_cache("workspaces/:slug/states/")` runs here, before
    // the gate (`InvalidationOrder::BeforeGate`) — cited, not executed.
    check_gate(
        "DELETE",
        "workspaces/<slug>/projects/<project_id>/states/<pk>/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    let mutation = state_mutation_facts(&pool, &slug, &project_id, &actor.id).await?;
    check_state_mutation(&mutation)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let row: Option<(bool,)> = sqlx::query_as(
        r#"SELECT s."default" FROM states s JOIN workspaces w ON w.id = s.workspace_id
           WHERE s.deleted_at IS NULL AND NOT (s."group" = 'triage') AND NOT s.is_triage
             AND s.id = $1 AND s.project_id = $2 AND w.slug = $3"#,
    )
    .bind(pk)
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let (is_default,) = row.ok_or(Denial::ObjectNotFound)?;
    if is_default {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            DEFAULT_DELETE_BODY.to_owned(),
        ));
    }
    // `Issue.objects` (live-only) with this state, unscoped by project
    // (state PKs are globally unique) — `:135`.
    let blocked: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM issues i WHERE i.state_id = $1 AND i.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(pk)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    if blocked.is_some() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            NONEMPTY_DELETE_BODY.to_owned(),
        ));
    }
    // `SoftDeleteModel.delete(soft=True)`: tombstone + `save()` (which
    // stamps `updated_at`/`updated_by`) + the `soft_delete_related_objects`
    // publish.
    let now = utc_now_micros();
    sqlx::query(
        r#"UPDATE states SET deleted_at = $1, updated_at = $1, updated_by_id = $2
           WHERE id = $3"#,
    )
    .bind(now)
    .bind(actor.id)
    .bind(pk)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    enqueue_soft_delete(&pool, "state", &pk).await;
    Ok(no_content())
}

/// `IntakeStateEndpoint.get` (`state/base.py:147-157`): the first live
/// triage state in sequence order, or the triage 404. No membership or
/// archived check in the body (the decorator gate owns auth).
async fn intake_state_get(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "GET",
        "workspaces/<slug>/projects/<project_id>/intake-state/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    // `State.triage_objects` (live + `group = triage`), `.first()` in
    // `Meta.ordering` (sequence).
    let row: Option<StateRowTuple> = sqlx::query_as(
        r#"SELECT s.id, s.project_id, s.workspace_id, s.name, s.color,
                  s."group", s."default", s.description, s.sequence
           FROM states s JOIN workspaces w ON w.id = s.workspace_id
           WHERE s.deleted_at IS NULL AND s."group" = 'triage'
             AND w.slug = $1 AND s.project_id = $2
           ORDER BY s.sequence ASC LIMIT 1"#,
    )
    .bind(&slug)
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let row =
        row.ok_or_else(|| Denial::Raw(StatusCode::NOT_FOUND, TRIAGE_MISSING_BODY.to_owned()))?;
    Ok(json_ok(render_state(&state_row_from(row)).to_string()))
}

// ---------------------------------------------------------------------------
// Estimates
// ---------------------------------------------------------------------------

/// `generate_random_name` (`estimate/base.py:29-31`): 10 chars from
/// `string.ascii_lowercase`.
fn generate_random_name() -> String {
    let mut rng = rand::rng();
    (0..10)
        .map(|_| (b'a' + rng.random_range(0..26)) as char)
        .collect()
}

/// One estimate row for the read shape.
#[derive(Debug, Clone)]
struct EstimateRow {
    id: uuid::Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    name: String,
    description: String,
    estimate_type: String,
    last_used: bool,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project: uuid::Uuid,
    workspace: uuid::Uuid,
}

/// One estimate-point row for the read shape.
#[derive(Debug, Clone)]
struct EstimatePointRow {
    id: uuid::Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    key: i32,
    description: String,
    value: String,
    created_by: Option<uuid::Uuid>,
    updated_by: Option<uuid::Uuid>,
    project: uuid::Uuid,
    workspace: uuid::Uuid,
    estimate: uuid::Uuid,
}

type EstimateRowTuple = (
    uuid::Uuid,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    String,
    String,
    String,
    bool,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    uuid::Uuid,
    uuid::Uuid,
);

type EstimatePointRowTuple = (
    uuid::Uuid,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    i32,
    String,
    String,
    Option<uuid::Uuid>,
    Option<uuid::Uuid>,
    uuid::Uuid,
    uuid::Uuid,
    uuid::Uuid,
);

fn estimate_row_from(tuple: EstimateRowTuple) -> EstimateRow {
    EstimateRow {
        id: tuple.0,
        created_at: tuple.1,
        updated_at: tuple.2,
        deleted_at: tuple.3,
        name: tuple.4,
        description: tuple.5,
        estimate_type: tuple.6,
        last_used: tuple.7,
        created_by: tuple.8,
        updated_by: tuple.9,
        project: tuple.10,
        workspace: tuple.11,
    }
}

fn estimate_point_row_from(tuple: EstimatePointRowTuple) -> EstimatePointRow {
    EstimatePointRow {
        id: tuple.0,
        created_at: tuple.1,
        updated_at: tuple.2,
        deleted_at: tuple.3,
        key: tuple.4,
        description: tuple.5,
        value: tuple.6,
        created_by: tuple.7,
        updated_by: tuple.8,
        project: tuple.9,
        workspace: tuple.10,
        estimate: tuple.11,
    }
}

/// Render one point through the L3 read shape, datetimes in the
/// request user's zone.
fn render_point(row: &EstimatePointRow, timezone: Tz) -> Value {
    let id = row.id.to_string();
    let created_at = render_dt(&row.created_at, timezone);
    let updated_at = render_dt(&row.updated_at, timezone);
    let deleted_at = render_dt_opt(row.deleted_at, timezone);
    let created_by = row.created_by.map(|id| id.to_string());
    let updated_by = row.updated_by.map(|id| id.to_string());
    let project = row.project.to_string();
    let workspace = row.workspace.to_string();
    let estimate = row.estimate.to_string();
    let row_view = ser::EstimatePointRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        key: row.key,
        description: &row.description,
        value: &row.value,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        project: &project,
        workspace: &workspace,
        estimate: &estimate,
    };
    let view = ser::estimate_point_to_representation(&row_view);
    serde_json::to_value(&view).expect("point view serializes")
}

/// Render one estimate with its nested points through the L3
/// `EstimateReadSerializer` shape.
fn render_estimate_read(row: &EstimateRow, points: &[EstimatePointRow], timezone: Tz) -> Value {
    let id = row.id.to_string();
    let created_at = render_dt(&row.created_at, timezone);
    let updated_at = render_dt(&row.updated_at, timezone);
    let deleted_at = render_dt_opt(row.deleted_at, timezone);
    let created_by = row.created_by.map(|id| id.to_string());
    let updated_by = row.updated_by.map(|id| id.to_string());
    let project = row.project.to_string();
    let workspace = row.workspace.to_string();
    // The L3 builder borrows `&str`s; stage owned strings first so the
    // borrows live through the call.
    struct StagedPoint {
        id: String,
        created_at: String,
        updated_at: String,
        deleted_at: Option<String>,
        description: String,
        value: String,
        created_by: Option<String>,
        updated_by: Option<String>,
        project: String,
        workspace: String,
        estimate: String,
    }
    let staged: Vec<StagedPoint> = points
        .iter()
        .map(|point| StagedPoint {
            id: point.id.to_string(),
            created_at: render_dt(&point.created_at, timezone),
            updated_at: render_dt(&point.updated_at, timezone),
            deleted_at: render_dt_opt(point.deleted_at, timezone),
            description: point.description.clone(),
            value: point.value.clone(),
            created_by: point.created_by.map(|id| id.to_string()),
            updated_by: point.updated_by.map(|id| id.to_string()),
            project: point.project.to_string(),
            workspace: point.workspace.to_string(),
            estimate: point.estimate.to_string(),
        })
        .collect();
    let point_rows: Vec<ser::EstimatePointRow<'_>> = points
        .iter()
        .zip(staged.iter())
        .map(|(point, staged)| ser::EstimatePointRow {
            id: &staged.id,
            created_at: &staged.created_at,
            updated_at: &staged.updated_at,
            deleted_at: staged.deleted_at.as_deref(),
            key: point.key,
            description: &staged.description,
            value: &staged.value,
            created_by: staged.created_by.as_deref(),
            updated_by: staged.updated_by.as_deref(),
            project: &staged.project,
            workspace: &staged.workspace,
            estimate: &staged.estimate,
        })
        .collect();
    let row_view = ser::EstimateRow {
        id: &id,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        name: &row.name,
        description: &row.description,
        estimate_type: &row.estimate_type,
        last_used: row.last_used,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        project: &project,
        workspace: &workspace,
    };
    let view = ser::estimate_read_to_representation(&row_view, &point_rows);
    serde_json::to_value(&view).expect("estimate view serializes")
}

/// `ProjectEstimatePointEndpoint.get` (`estimate/base.py:34-46`): the
/// project's estimate rows, or `[]` without a query when
/// `project.estimate_id` is `None`.
async fn project_estimates_get(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "GET",
        "workspaces/<slug>/projects/<project_id>/project-estimates/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    // `Project.objects.get` (live-only) in this slug.
    let project: Option<(Option<uuid::Uuid>,)> = sqlx::query_as(
        r#"SELECT p.estimate_id FROM projects p JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let (estimate_id,) = project.ok_or(Denial::ObjectNotFound)?;
    let Some(estimate_id) = estimate_id else {
        return Ok(json_ok("[]".to_owned()));
    };
    // Live points of the estimate, `Meta.ordering = ("value",)` — the
    // *string* column, so `"10"` sorts before `"2"`.
    let rows: Vec<EstimatePointRowTuple> = sqlx::query_as(
        r#"SELECT ep.id, ep.created_at, ep.updated_at, ep.deleted_at, ep.key,
                  ep.description, ep.value, ep.created_by_id, ep.updated_by_id,
                  ep.project_id, ep.workspace_id, ep.estimate_id
           FROM estimate_points ep JOIN workspaces w ON w.id = ep.workspace_id
           WHERE ep.deleted_at IS NULL AND ep.estimate_id = $1
             AND ep.project_id = $2 AND w.slug = $3
           ORDER BY ep.value ASC"#,
    )
    .bind(estimate_id)
    .bind(project_id)
    .bind(&slug)
    .fetch_all(&pool)
    .await
    .map_err(db_denial)?;
    let points: Vec<Value> = rows
        .iter()
        .map(|tuple| render_point(&estimate_point_row_from(tuple.clone()), actor.timezone))
        .collect();
    Ok(json_ok(Value::Array(points).to_string()))
}

/// `BulkEstimatePointEndpoint.list` (`estimate/base.py:54-61`): live
/// estimates in name order with prefetched live points in value order
/// (two queries, the `prefetch_related("points")` shape).
async fn bulk_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_project_entity_gate("GET", &slug, &project_id, &member)?;
    let rows: Vec<EstimateRowTuple> = sqlx::query_as(
        r#"SELECT e.id, e.created_at, e.updated_at, e.deleted_at, e.name, e.description,
                  e.type, e.last_used, e.created_by_id, e.updated_by_id,
                  e.project_id, e.workspace_id
           FROM estimates e JOIN workspaces w ON w.id = e.workspace_id
           WHERE e.deleted_at IS NULL AND e.project_id = $1 AND w.slug = $2
           ORDER BY e.name ASC"#,
    )
    .bind(project_id)
    .bind(&slug)
    .fetch_all(&pool)
    .await
    .map_err(db_denial)?;
    let ids: Vec<uuid::Uuid> = rows.iter().map(|row| row.0).collect();
    let point_rows: Vec<EstimatePointRowTuple> = if ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            r#"SELECT ep.id, ep.created_at, ep.updated_at, ep.deleted_at, ep.key,
                      ep.description, ep.value, ep.created_by_id, ep.updated_by_id,
                      ep.project_id, ep.workspace_id, ep.estimate_id
               FROM estimate_points ep
               WHERE ep.deleted_at IS NULL AND ep.estimate_id = ANY($1)
               ORDER BY ep.value ASC"#,
        )
        .bind(&ids)
        .fetch_all(&pool)
        .await
        .map_err(db_denial)?
    };
    let mut by_estimate: HashMap<uuid::Uuid, Vec<EstimatePointRow>> = HashMap::new();
    for tuple in &point_rows {
        let point = estimate_point_row_from(tuple.clone());
        by_estimate.entry(point.estimate).or_default().push(point);
    }
    let body: Vec<Value> = rows
        .iter()
        .map(|tuple| {
            let row = estimate_row_from(tuple.clone());
            let empty = Vec::new();
            let points = by_estimate.get(&row.id).unwrap_or(&empty);
            render_estimate_read(&row, points, actor.timezone)
        })
        .collect();
    Ok(json_ok(Value::Array(body).to_string()))
}

/// One validated bulk-create point item (raw ORM coercions, not the
/// validated values — the view constructs from the raw dicts).
#[derive(Debug)]
struct BulkPointInput {
    key: i64,
    value: String,
    description: String,
}

/// Validate one bulk-create item through `EstimatePointSerializer`
/// (`estimate.py:20-32`): `key` optional int ≥ 0 (default 0); `value`
/// required ≤255 (field level); `description` blank-ok (default `""`);
/// then `validate()` (the 20-char value cap). Audit keys are validated
/// by DRF but ignored by the construct — ignored here outright (see
/// module docs). Field errors accumulate in model order
/// (`key`, `description`, `value`), `non_field_errors` last.
fn validate_bulk_point(item: &Map<String, Value>) -> Result<BulkPointInput, Value> {
    let mut errors = Map::new();
    let mut key: Option<i64> = None;
    let mut too_big = false;
    // `key`: optional int, `MinValueValidator(0)`, default 0.
    if let Some(raw) = item.get("key") {
        match validate_int(raw) {
            Ok(IntValue::Value(int)) => {
                if int < 0 {
                    push_field_error(&mut errors, "key", key_min_error());
                } else {
                    key = Some(int);
                }
            }
            Ok(IntValue::TooBig) => too_big = true,
            Err(message) => push_field_error(&mut errors, "key", message),
        }
    }
    // `description`: blank-ok, default `""` (validated only — the
    // construct reads the raw dict).
    if let Some(raw) = item.get("description") {
        if let Err(message) = validate_char(raw, None, true) {
            push_field_error(&mut errors, "description", message);
        }
    }
    // `value`: required ≤255.
    let mut value: Option<String> = None;
    match item.get("value") {
        None => push_field_error(&mut errors, "value", "This field is required.".to_owned()),
        Some(raw) => match validate_char(raw, Some(255), false) {
            Ok(text) => value = Some(text),
            Err(message) => push_field_error(&mut errors, "value", message),
        },
    }
    if !errors.is_empty() {
        return Err(Value::Object(errors));
    }
    // `validate()`: the 20-char value cap (`estimate.py:21-27`). The
    // empty-payload branch is unreachable here (`value` is required),
    // but the helper runs faithfully.
    if let Some(errors) = ser::estimate_point_validate(false, value.as_deref()) {
        return Err(errors);
    }
    if too_big {
        // Python accepts the unbounded int and dies at the column
        // (`DataError` 500) — the same 500, without the insert.
        return Err(Value::String("too-big".to_owned()));
    }
    Ok(BulkPointInput {
        // Raw-dict construction (`estimate/base.py:86-88`): `key` via
        // `int()` (reaching raws are int-syntax — validated above),
        // `value`/`description` via `str()`. Note the validated
        // (stripped) value is NOT what is stored.
        key: key.unwrap_or(0),
        value: match item.get("value") {
            Some(Value::String(text)) => text.clone(),
            Some(raw) => python_str(raw),
            None => String::new(),
        },
        description: match item.get("description") {
            Some(Value::String(text)) => text.clone(),
            Some(raw) => python_str(raw),
            None => String::new(),
        },
    })
}

/// Bulk-create `estimate_points` gate (`estimate/base.py:76-80`): a
/// missing key and an explicit `null` both reach the `many=True`
/// serializer as `None` → `validate_empty_values` fails `null`, whose
/// single-error list `.errors` rewrites to `{"non_field_errors": ["No
/// data provided"]}` (`serializers.py`).
enum CreatePoints<'a> {
    Missing,
    Items(Vec<&'a Value>),
    NotAList(String),
}

fn bulk_create_points_shape(points: Option<&Value>) -> CreatePoints<'_> {
    match points {
        None | Some(Value::Null) => CreatePoints::Missing,
        Some(Value::Array(items)) => CreatePoints::Items(items.iter().collect()),
        Some(other) => CreatePoints::NotAList(format!(
            "Expected a list of items but got type \"{}\".",
            python_type_name(other)
        )),
    }
}

/// `BulkEstimatePointEndpoint.create` (`estimate/base.py:63-101`):
/// persist the `Estimate` row *first* (unvalidated header fields, random
/// name default), *then* validate the points — a 400 still creates a
/// point-less estimate (the pinned bug). Answers 200 with the read
/// shape. Missing/non-dict `estimate`, or a non-dict body, raises
/// `AttributeError` → 500.
async fn bulk_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_project_entity_gate("POST", &slug, &project_id, &member)?;
    // `invalidate_cache("/api/workspaces/:slug/estimates/")` runs here,
    // after the permission check (`InvalidationOrder::AfterPermissionCheck`)
    // — cited, not executed.
    let data = body.0.as_object().ok_or(Denial::ServerError)?;
    let header = data.get("estimate").ok_or(Denial::ServerError)?;
    let header = header.as_object().ok_or(Denial::ServerError)?;
    // Header fields are unvalidated ORM input (no serializer): `name`
    // defaults to a random string, `type` to `categories`, `last_used`
    // to `false`. `None` violates `NOT NULL` → the `IntegrityError` 400.
    let name = match header.get("name") {
        None => generate_random_name(),
        Some(Value::Null) => return Err(Denial::BadPayload),
        Some(Value::String(text)) => text.clone(),
        Some(raw) => python_str(raw),
    };
    let estimate_type = match header.get("type") {
        None => "categories".to_owned(),
        Some(Value::Null) => return Err(Denial::BadPayload),
        Some(Value::String(text)) => text.clone(),
        Some(raw) => python_str(raw),
    };
    // `BooleanField.get_prep_value` → `to_python` (Django 4.2
    // `fields/__init__.py:1102-1122`): `None` passes through (→ `NOT
    // NULL` → the `IntegrityError` 400); `1`/`0` equal `True`/`False`;
    // only `"t"`/`"True"`/`"1"` and `"f"`/`"False"`/`"0"` coerce —
    // everything else raises `ValidationError` → the invalid-detail
    // 400 (note: NOT the Postgres bool parser — `"yes"` 400s).
    let last_used = match header.get("last_used") {
        None => false,
        Some(Value::Null) => return Err(Denial::BadPayload),
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => {
            if number.as_i64() == Some(1) || number.as_f64() == Some(1.0) {
                true
            } else if number.as_i64() == Some(0) || number.as_f64() == Some(0.0) {
                false
            } else {
                return Err(Denial::BadValidation);
            }
        }
        Some(Value::String(text)) => match text.as_str() {
            "t" | "True" | "1" => true,
            "f" | "False" | "0" => false,
            _ => return Err(Denial::BadValidation),
        },
        Some(Value::Array(_) | Value::Object(_)) => return Err(Denial::BadValidation),
    };
    let workspace: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT p.workspace_id FROM projects p WHERE p.id = $1"#)
            .bind(project_id)
            .fetch_optional(&pool)
            .await
            .map_err(db_denial)?;
    let (workspace_id,) = workspace.ok_or(Denial::ObjectNotFound)?;
    let estimate_id = uuid::Uuid::new_v4();
    let now = utc_now_micros();
    // The row persists BEFORE points validation (pinned bug).
    sqlx::query(
        r#"INSERT INTO estimates
           (id, project_id, workspace_id, name, description, type, last_used,
            created_at, updated_at, created_by_id, updated_by_id, deleted_at)
           VALUES ($1, $2, $3, $4, '', $5, $6, $7, $7, $8, NULL, NULL)"#,
    )
    .bind(estimate_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(&name)
    .bind(&estimate_type)
    .bind(last_used)
    .bind(now)
    .bind(actor.id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    // Points validation (`many=True`): `:78` passes
    // `request.data.get("estimate_points")` (no default) — the estimate
    // row above already persists whatever the gate below answers.
    let items: Vec<&Value> = match bulk_create_points_shape(data.get("estimate_points")) {
        CreatePoints::Missing => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                ser::non_field_errors("No data provided").to_string(),
            ));
        }
        CreatePoints::Items(items) => items,
        CreatePoints::NotAList(message) => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                ser::non_field_errors(&message).to_string(),
            ));
        }
    };
    let mut validated: Vec<BulkPointInput> = Vec::with_capacity(items.len());
    let mut item_errors: Vec<Value> = Vec::with_capacity(items.len());
    let mut failed = false;
    for item in items {
        let Some(map) = item.as_object() else {
            item_errors.push(ser::non_field_errors(&format!(
                "Invalid data. Expected a dictionary, but got {}.",
                python_type_name(item)
            )));
            failed = true;
            continue;
        };
        match validate_bulk_point(map) {
            Ok(input) => {
                item_errors.push(Value::Object(Map::new()));
                validated.push(input);
            }
            Err(Value::String(marker)) if marker == "too-big" => {
                // Unbounded-int key: Python dies at the column (500) —
                // the estimate row above already persists, as in Python.
                return Err(Denial::ServerError);
            }
            Err(errors) => {
                item_errors.push(errors);
                failed = true;
            }
        }
    }
    if failed {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            Value::Array(item_errors).to_string(),
        ));
    }
    // `bulk_create(ignore_conflicts=True)` — no unique constraints exist
    // on the table, so this is a plain multi-row insert (audit columns
    // explicit, since `bulk_create` bypasses `save()`).
    for chunk in validated.chunks(10) {
        let mut ids = Vec::with_capacity(chunk.len());
        let mut keys = Vec::with_capacity(chunk.len());
        let mut values = Vec::with_capacity(chunk.len());
        let mut descriptions = Vec::with_capacity(chunk.len());
        for input in chunk {
            ids.push(uuid::Uuid::new_v4());
            keys.push(input.key);
            values.push(input.value.clone());
            descriptions.push(input.description.clone());
        }
        sqlx::query(
            r#"INSERT INTO estimate_points
               (id, estimate_id, project_id, workspace_id, "key", value, description,
                created_at, updated_at, created_by_id, updated_by_id, deleted_at)
               SELECT id, $2, $3, $4, "key", value, description,
                      $5, $5, $6, $6, NULL
               FROM UNNEST($1::uuid[], $7::bigint[], $8::text[], $9::text[])
                    AS t(id, "key", value, description)"#,
        )
        .bind(&ids)
        .bind(estimate_id)
        .bind(project_id)
        .bind(workspace_id)
        .bind(now)
        .bind(actor.id)
        .bind(&keys)
        .bind(&values)
        .bind(&descriptions)
        .execute(&pool)
        .await
        .map_err(db_denial)?;
    }
    Ok(json_ok(
        render_created_estimate(&pool, estimate_id, actor.timezone).await?,
    ))
}

/// Re-read one estimate with its live points in value order and render
/// the read shape (bulk create / bulk patch responses).
async fn render_created_estimate(
    pool: &sqlx::PgPool,
    estimate_id: uuid::Uuid,
    timezone: Tz,
) -> Result<String, Denial> {
    let row: Option<EstimateRowTuple> = sqlx::query_as(
        r#"SELECT e.id, e.created_at, e.updated_at, e.deleted_at, e.name, e.description,
                  e.type, e.last_used, e.created_by_id, e.updated_by_id,
                  e.project_id, e.workspace_id
           FROM estimates e WHERE e.id = $1"#,
    )
    .bind(estimate_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let row = estimate_row_from(row.ok_or(Denial::ServerError)?);
    let point_rows: Vec<EstimatePointRowTuple> = sqlx::query_as(
        r#"SELECT ep.id, ep.created_at, ep.updated_at, ep.deleted_at, ep.key,
                  ep.description, ep.value, ep.created_by_id, ep.updated_by_id,
                  ep.project_id, ep.workspace_id, ep.estimate_id
           FROM estimate_points ep
           WHERE ep.deleted_at IS NULL AND ep.estimate_id = $1
           ORDER BY ep.value ASC"#,
    )
    .bind(estimate_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let points: Vec<EstimatePointRow> = point_rows
        .iter()
        .map(|tuple| estimate_point_row_from(tuple.clone()))
        .collect();
    Ok(render_estimate_read(&row, &points, timezone).to_string())
}

/// `BulkEstimatePointEndpoint.retrieve` (`estimate/base.py:103-106`):
/// the scoped live lookup or the 404, rendered with nested points.
async fn bulk_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, estimate_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_project_entity_gate("GET", &slug, &project_id, &member)?;
    let estimate_id = parse_uuid_or_invalid(&estimate_raw)?;
    let row: Option<EstimateRowTuple> = sqlx::query_as(
        r#"SELECT e.id, e.created_at, e.updated_at, e.deleted_at, e.name, e.description,
                  e.type, e.last_used, e.created_by_id, e.updated_by_id,
                  e.project_id, e.workspace_id
           FROM estimates e JOIN workspaces w ON w.id = e.workspace_id
           WHERE e.deleted_at IS NULL AND e.id = $1 AND w.slug = $2 AND e.project_id = $3"#,
    )
    .bind(estimate_id)
    .bind(&slug)
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let row = estimate_row_from(row.ok_or(Denial::ObjectNotFound)?);
    let point_rows: Vec<EstimatePointRowTuple> = sqlx::query_as(
        r#"SELECT ep.id, ep.created_at, ep.updated_at, ep.deleted_at, ep.key,
                  ep.description, ep.value, ep.created_by_id, ep.updated_by_id,
                  ep.project_id, ep.workspace_id, ep.estimate_id
           FROM estimate_points ep
           WHERE ep.deleted_at IS NULL AND ep.estimate_id = $1
           ORDER BY ep.value ASC"#,
    )
    .bind(estimate_id)
    .fetch_all(&pool)
    .await
    .map_err(db_denial)?;
    let points: Vec<EstimatePointRow> = point_rows
        .iter()
        .map(|tuple| estimate_point_row_from(tuple.clone()))
        .collect();
    Ok(json_ok(
        render_estimate_read(&row, &points, actor.timezone).to_string(),
    ))
}

/// Bulk-PATCH `estimate_points` `len()` gate (`estimate/base.py:110`):
/// only missing/`[]`/`{}`/`""` take the required 400; `len()` on a
/// scalar/`null` raises `TypeError` → pre-lookup 500. Non-empty
/// containers pass the gate — iterating them yields non-dicts whose
/// `.get("id")` raises `AttributeError` only after the header write
/// (`:118-130`).
enum PatchPoints<'a> {
    Missing,
    Items(&'a Vec<Value>),
    IteratesNonDicts,
    Unsized,
}

fn bulk_patch_points_gate(points: Option<&Value>) -> PatchPoints<'_> {
    match points {
        None => PatchPoints::Missing,
        Some(Value::Array(items)) if items.is_empty() => PatchPoints::Missing,
        Some(Value::Array(items)) => PatchPoints::Items(items),
        Some(Value::Object(map)) if map.is_empty() => PatchPoints::Missing,
        Some(Value::Object(_)) => PatchPoints::IteratesNonDicts,
        Some(Value::String(text)) if text.is_empty() => PatchPoints::Missing,
        Some(Value::String(_)) => PatchPoints::IteratesNonDicts,
        Some(_) => PatchPoints::Unsized,
    }
}

/// `BulkEstimatePointEndpoint.partial_update`
/// (`estimate/base.py:108-144`): empty/missing points → the required
/// 400; the estimate lookup is *unscoped* (live-only); a truthy
/// `estimate` dict renames/ retypes (no choices validation, `None`
/// violates `NOT NULL` → the `IntegrityError` 400); matched points get
/// `key`/`value` from the raw payload; answers the read shape.
async fn bulk_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, estimate_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_project_entity_gate("PATCH", &slug, &project_id, &member)?;
    // `invalidate_cache("/api/workspaces/:slug/estimates/")` runs here,
    // after the permission check (`InvalidationOrder::AfterPermissionCheck`)
    // — cited, not executed.
    let data = body.0.as_object().ok_or(Denial::ServerError)?;
    // `if not len(request.data.get("estimate_points", []))` (`:110`).
    // Item-shape failures wait for the points-filter comprehension
    // (`:125-130`) — after the header write below.
    let raw_items: Option<&Vec<Value>> = match bulk_patch_points_gate(data.get("estimate_points")) {
        PatchPoints::Missing => {
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                POINTS_REQUIRED_BODY.to_owned(),
            ));
        }
        PatchPoints::Unsized => return Err(Denial::ServerError),
        PatchPoints::Items(items) => Some(items),
        PatchPoints::IteratesNonDicts => None,
    };
    let estimate_id = parse_uuid_or_invalid(&estimate_raw)?;
    // Unscoped lookup (pinned bug): live-only, any project.
    let row: Option<EstimateRowTuple> = sqlx::query_as(
        r#"SELECT e.id, e.created_at, e.updated_at, e.deleted_at, e.name, e.description,
                  e.type, e.last_used, e.created_by_id, e.updated_by_id,
                  e.project_id, e.workspace_id
           FROM estimates e WHERE e.deleted_at IS NULL AND e.id = $1"#,
    )
    .bind(estimate_id)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    row.ok_or(Denial::ObjectNotFound)?;
    // `if request.data.get("estimate"):` — falsy values skip; a truthy
    // non-dict raises `AttributeError` → 500.
    if python_truthy(data.get("estimate")) {
        let header = data
            .get("estimate")
            .and_then(Value::as_object)
            .ok_or(Denial::ServerError)?;
        let name = match header.get("name") {
            None => None,
            Some(Value::Null) => return Err(Denial::BadPayload),
            Some(Value::String(text)) => Some(text.clone()),
            Some(raw) => Some(python_str(raw)),
        };
        let estimate_type = match header.get("type") {
            None => None,
            Some(Value::Null) => return Err(Denial::BadPayload),
            Some(Value::String(text)) => Some(text.clone()),
            Some(raw) => Some(python_str(raw)),
        };
        // `estimate.save()`: stamps `updated_at`/`updated_by` even when
        // the values are unchanged.
        let now = utc_now_micros();
        sqlx::query(
            r#"UPDATE estimates SET name = COALESCE($1, name), type = COALESCE($2, type),
                      updated_at = $3, updated_by_id = $4 WHERE id = $5"#,
        )
        .bind(name.as_deref())
        .bind(estimate_type.as_deref())
        .bind(now)
        .bind(actor.id)
        .bind(estimate_id)
        .execute(&pool)
        .await
        .map_err(db_denial)?;
    }
    // The points-filter comprehension (`:125-130`): the header `save()`
    // above already landed. Iterating a non-empty dict/string yields
    // non-dicts, and any non-dict item, raises `AttributeError` → 500.
    let items: Vec<&Map<String, Value>> = match raw_items {
        None => return Err(Denial::ServerError),
        Some(raw) => {
            let mut maps = Vec::with_capacity(raw.len());
            for item in raw {
                maps.push(item.as_object().ok_or(Denial::ServerError)?);
            }
            maps
        }
    };
    // `pk__in=[…]` (`UUIDField.to_python`): a missing id is `NULL`
    // (matches nothing); in-range ints/bools coerce via `UUID(int=…)`
    // (a valid lookup the `str()` match then skips); floats, composites,
    // out-of-range ints, and garbage strings raise `ValidationError` →
    // the invalid-detail 400.
    let mut ids: Vec<uuid::Uuid> = Vec::with_capacity(items.len());
    for item in &items {
        match item.get("id") {
            None | Some(Value::Null) => {}
            Some(Value::String(raw)) => {
                ids.push(parse_uuid_or_invalid(raw)?);
            }
            Some(other) => {
                raw_uuid_prep(other).map_err(|_| Denial::BadValidation)?;
            }
        }
    }
    let matched: Vec<EstimatePointRowTuple> = if ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            r#"SELECT ep.id, ep.created_at, ep.updated_at, ep.deleted_at, ep.key,
                      ep.description, ep.value, ep.created_by_id, ep.updated_by_id,
                      ep.project_id, ep.workspace_id, ep.estimate_id
               FROM estimate_points ep JOIN workspaces w ON w.id = ep.workspace_id
               WHERE ep.deleted_at IS NULL AND ep.id = ANY($1)
                 AND w.slug = $2 AND ep.project_id = $3 AND ep.estimate_id = $4"#,
        )
        .bind(&ids)
        .bind(&slug)
        .bind(project_id)
        .bind(estimate_id)
        .fetch_all(&pool)
        .await
        .map_err(db_denial)?
    };
    // Match payload entries by `str(point.id)` (first wins); update
    // `value`/`key` from the raw payload (`bulk_update(["key",
    // "value"])` — no `updated_at` touch).
    for tuple in &matched {
        let point = estimate_point_row_from(tuple.clone());
        let key = point.id.to_string();
        let Some(entry) = items
            .iter()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some(key.as_str()))
        else {
            continue;
        };
        let value = match entry.get("value") {
            None => None,
            Some(Value::Null) => return Err(Denial::BadPayload),
            Some(Value::String(text)) => Some(text.clone()),
            Some(raw) => Some(python_str(raw)),
        };
        let point_key: Option<i64> = match entry.get("key") {
            None => None,
            Some(Value::Null) => return Err(Denial::BadPayload),
            Some(raw) => Some(orm_int(raw).map_err(|_| Denial::ServerError)?),
        };
        if value.is_none() && point_key.is_none() {
            continue;
        }
        sqlx::query(
            r#"UPDATE estimate_points SET value = COALESCE($1, value),
                      "key" = COALESCE($2, "key") WHERE id = $3"#,
        )
        .bind(value.as_deref())
        .bind(point_key)
        .bind(point.id)
        .execute(&pool)
        .await
        .map_err(db_denial)?;
    }
    Ok(json_ok(
        render_created_estimate(&pool, estimate_id, actor.timezone).await?,
    ))
}

/// Django `IntegerField.get_prep_value` (`int(value)`) for raw ORM
/// writes: ints pass, floats truncate, strings parse (`ValueError` →
/// 500), bools are `0`/`1`, anything else is a `TypeError` → 500.
fn orm_int(value: &Value) -> Result<i64, ()> {
    match value {
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Ok(int)
            } else if let Some(uint) = number.as_u64() {
                i64::try_from(uint).map_err(|_| ())
            } else if let Some(float) = number.as_f64() {
                // Python `int()` truncates toward zero; out-of-range →
                // `OverflowError` → 500.
                const MAX: f64 = i64::MAX as f64;
                const MIN: f64 = i64::MIN as f64;
                if float.is_finite() && float < MAX && float > MIN {
                    Ok(float.trunc() as i64)
                } else {
                    Err(())
                }
            } else {
                Err(())
            }
        }
        Value::String(text) => {
            // Python `int()` strips whitespace and takes `+`/`-`.
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Err(());
            }
            trimmed.parse::<i64>().map_err(|_| ())
        }
        Value::Bool(true) => Ok(1),
        Value::Bool(false) => Ok(0),
        Value::Null | Value::Array(_) | Value::Object(_) => Err(()),
    }
}

/// `BulkEstimatePointEndpoint.destroy` (`estimate/base.py:146-150`): the
/// scoped live lookup or the 404, then a soft delete answering 204. The
/// points are untouched (no cascade on soft delete).
async fn bulk_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, estimate_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_project_entity_gate("DELETE", &slug, &project_id, &member)?;
    // `invalidate_cache("/api/workspaces/:slug/estimates/")` runs here,
    // after the permission check (`InvalidationOrder::AfterPermissionCheck`)
    // — cited, not executed.
    let estimate_id = parse_uuid_or_invalid(&estimate_raw)?;
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT e.id FROM estimates e JOIN workspaces w ON w.id = e.workspace_id
           WHERE e.deleted_at IS NULL AND e.id = $1 AND w.slug = $2 AND e.project_id = $3"#,
    )
    .bind(estimate_id)
    .bind(&slug)
    .bind(project_id)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    row.ok_or(Denial::ObjectNotFound)?;
    let now = utc_now_micros();
    sqlx::query(
        r#"UPDATE estimates SET deleted_at = $1, updated_at = $1, updated_by_id = $2
           WHERE id = $3"#,
    )
    .bind(now)
    .bind(actor.id)
    .bind(estimate_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    enqueue_soft_delete(&pool, "estimate", &estimate_id).await;
    Ok(no_content())
}

/// `EstimatePointEndpoint.create` (`estimate/base.py:153-168`): falsy
/// key or value → the required 400 (note `key=0` fails); otherwise an
/// unvalidated ORM insert answering 200 with the point shape. No
/// estimate-existence check (the FK decides → the `IntegrityError` 400).
async fn point_create(
    State(state): State<AppState>,
    Path((slug, project_raw, estimate_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: Option<axum::Json<Value>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "POST",
        "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    let estimate_id = parse_uuid_or_invalid(&estimate_raw)?;
    // A missing body is `{}` (falsy key → the required 400); a present
    // non-dict body raises `AttributeError` → 500.
    let data = body.map(|body| body.0).unwrap_or(Value::Object(Map::new()));
    let data = data.as_object().ok_or(Denial::ServerError)?;
    if !python_truthy(data.get("key")) || !python_truthy(data.get("value")) {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            KEY_VALUE_REQUIRED_BODY.to_owned(),
        ));
    }
    // Raw ORM coercions (no serializer): `key` via `int()`, `value` via
    // `str()`. Both are truthy here, so `None`/empty are excluded.
    let key = orm_int(data.get("key").unwrap_or(&Value::Null)).map_err(|_| Denial::ServerError)?;
    let value = match data.get("value") {
        Some(Value::String(text)) => text.clone(),
        Some(raw) => python_str(raw),
        None => String::new(),
    };
    let workspace: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT p.workspace_id FROM projects p WHERE p.id = $1"#)
            .bind(project_id)
            .fetch_optional(&pool)
            .await
            .map_err(db_denial)?;
    let (workspace_id,) = workspace.ok_or(Denial::ObjectNotFound)?;
    let id = uuid::Uuid::new_v4();
    let now = utc_now_micros();
    sqlx::query(
        r#"INSERT INTO estimate_points
           (id, estimate_id, project_id, workspace_id, "key", value, description,
            created_at, updated_at, created_by_id, updated_by_id, deleted_at)
           VALUES ($1, $2, $3, $4, $5, $6, '', $7, $7, $8, NULL, NULL)"#,
    )
    .bind(id)
    .bind(estimate_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(key)
    .bind(&value)
    .bind(now)
    .bind(actor.id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    let row: Option<EstimatePointRowTuple> = sqlx::query_as(
        r#"SELECT ep.id, ep.created_at, ep.updated_at, ep.deleted_at, ep.key,
                  ep.description, ep.value, ep.created_by_id, ep.updated_by_id,
                  ep.project_id, ep.workspace_id, ep.estimate_id
           FROM estimate_points ep WHERE ep.id = $1"#,
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let row = estimate_point_row_from(row.ok_or(Denial::ServerError)?);
    Ok(json_ok(render_point(&row, actor.timezone).to_string()))
}

/// One validated point-patch payload (partial: `None` = absent).
#[derive(Debug, Default)]
struct PointPatch {
    key: Option<i64>,
    value: Option<String>,
    description: Option<String>,
}

/// Validate one point `partial_update` payload through
/// `EstimatePointSerializer` (`estimate.py:20-32`): partial, so only
/// present keys validate — `key` int ≥ 0, `value` ≤255 non-blank,
/// `description` blank-ok — in model order (`key`, `description`,
/// `value`). Audit keys ignored (see module docs).
fn validate_point_patch(data: &Map<String, Value>) -> (PointPatch, Map<String, Value>, bool) {
    let mut errors = Map::new();
    let mut patch = PointPatch::default();
    let mut too_big = false;
    if let Some(raw) = data.get("key") {
        match validate_int(raw) {
            Ok(IntValue::Value(int)) => {
                if int < 0 {
                    push_field_error(&mut errors, "key", key_min_error());
                } else {
                    patch.key = Some(int);
                }
            }
            Ok(IntValue::TooBig) => too_big = true,
            Err(message) => push_field_error(&mut errors, "key", message),
        }
    }
    if let Some(raw) = data.get("description") {
        match validate_char(raw, None, true) {
            Ok(text) => patch.description = Some(text),
            Err(message) => push_field_error(&mut errors, "description", message),
        }
    }
    if let Some(raw) = data.get("value") {
        match validate_char(raw, Some(255), false) {
            Ok(text) => patch.value = Some(text),
            Err(message) => push_field_error(&mut errors, "value", message),
        }
    }
    (patch, errors, too_big)
}

/// `EstimatePointEndpoint.partial_update` (`estimate/base.py:170-183`):
/// the scoped live lookup or the 404, then partial validate →
/// `validate()` (an empty patch fails `if not data` → the required
/// 400) → save, answering 200.
async fn point_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, estimate_raw, point_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "PATCH",
        "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/<estimate_point_id>/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    let estimate_id = parse_uuid_or_invalid(&estimate_raw)?;
    // The `<estimate_point_id>` segment is unconverted: Django's
    // `.get(pk=…)` raises `ValidationError` on garbage → the
    // invalid-detail 400 (exact parity, not the `<uuid:…>` precedent).
    let point_id = parse_uuid_or_invalid(&point_raw)?;
    let row: Option<EstimatePointRowTuple> = sqlx::query_as(
        r#"SELECT ep.id, ep.created_at, ep.updated_at, ep.deleted_at, ep.key,
                  ep.description, ep.value, ep.created_by_id, ep.updated_by_id,
                  ep.project_id, ep.workspace_id, ep.estimate_id
           FROM estimate_points ep JOIN workspaces w ON w.id = ep.workspace_id
           WHERE ep.deleted_at IS NULL AND ep.id = $1 AND ep.estimate_id = $2
             AND ep.project_id = $3 AND w.slug = $4"#,
    )
    .bind(point_id)
    .bind(estimate_id)
    .bind(project_id)
    .bind(&slug)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let current = estimate_point_row_from(row.ok_or(Denial::ObjectNotFound)?);
    let data = body
        .0
        .as_object()
        .ok_or_else(|| Denial::Raw(StatusCode::BAD_REQUEST, invalid_data_body(&body.0)))?;
    let (patch, errors, too_big) = validate_point_patch(data);
    if !errors.is_empty() {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            Value::Object(errors).to_string(),
        ));
    }
    // `validate()` over the partial attrs (`estimate.py:21-27`): `{}` →
    // the required-points 400; a truthy value over 20 chars → the
    // too-long 400.
    let attrs_empty = patch.key.is_none() && patch.value.is_none() && patch.description.is_none();
    if let Some(errors) = ser::estimate_point_validate(attrs_empty, patch.value.as_deref()) {
        return Ok(json_response(StatusCode::BAD_REQUEST, errors.to_string()));
    }
    if too_big {
        // Unbounded-int key: Python dies at the column (`DataError` 500).
        return Err(Denial::ServerError);
    }
    let now = utc_now_micros();
    sqlx::query(
        r#"UPDATE estimate_points SET "key" = COALESCE($1, "key"),
                  value = COALESCE($2, value), description = COALESCE($3, description),
                  updated_at = $4, updated_by_id = $5 WHERE id = $6"#,
    )
    .bind(patch.key)
    .bind(patch.value.as_deref())
    .bind(patch.description.as_deref())
    .bind(now)
    .bind(actor.id)
    .bind(point_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    let row: Option<EstimatePointRowTuple> = sqlx::query_as(
        r#"SELECT ep.id, ep.created_at, ep.updated_at, ep.deleted_at, ep.key,
                  ep.description, ep.value, ep.created_by_id, ep.updated_by_id,
                  ep.project_id, ep.workspace_id, ep.estimate_id
           FROM estimate_points ep WHERE ep.id = $1"#,
    )
    .bind(point_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let row = estimate_point_row_from(row.ok_or(Denial::ServerError)?);
    // Sanity: the patch wrote what validation promised.
    debug_assert_eq!(row.id, current.id);
    Ok(json_ok(render_point(&row, actor.timezone).to_string()))
}

/// `EstimatePointEndpoint.destroy` (`estimate/base.py:185-247`): the
/// per-issue activity loop (each issue emits, then — on the
/// `new_estimate_id` branch only — the whole queryset updates, inside
/// the loop), then the key-decrement over higher keys, the soft delete,
/// and the 200 decremented-points array. A missing point is the 500
/// (`AttributeError` on `None.key`), not a 404.
async fn point_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, estimate_raw, point_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
    body: Option<axum::Json<Value>>,
) -> HandlerResult {
    let pool = pool_of(&state)?.clone();
    let actor = actor(&state, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let member = membership(&pool, &slug, &project_id, &actor.id).await?;
    check_gate(
        "DELETE",
        "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/<estimate_point_id>/",
        &slug,
        member.workspace_role,
        member.project_role,
    )?;
    let estimate_id = parse_uuid_or_invalid(&estimate_raw)?;
    let point_id = parse_uuid_or_invalid(&point_raw)?;
    // A missing body is `{}`; a present non-dict body raises
    // `AttributeError` → 500.
    let data = body.map(|body| body.0).unwrap_or(Value::Object(Map::new()));
    let data = data.as_object().ok_or(Denial::ServerError)?;
    // `new_estimate_id` is truthy-gated (`if new_estimate_id:`): falsy
    // values (`""`, `0`, missing) take the else branch. The `str()`-ed
    // value feeds the dumps; the update feeds the RAW value through
    // `UUIDField` prep (`to_python`): in-range ints/bools → `UUID(int=…)`
    // (a dangling id fails the FK → the `IntegrityError` 400); floats,
    // composites, out-of-range ints, and garbage strings raise
    // `ValidationError` → the invalid-detail 400.
    let new_raw = data.get("new_estimate_id");
    let (new_id_text, new_uuid): (Option<String>, Option<Result<uuid::Uuid, ()>>) =
        if python_truthy(new_raw) {
            let raw = new_raw.unwrap_or(&Value::Null);
            (Some(python_str(raw)), Some(raw_uuid_prep(raw)))
        } else {
            (None, None)
        };
    // `Issue.objects` (live-only) on this point, `Meta.ordering`
    // (`-created_at`) — the loop order the emits follow.
    let issues: Vec<(uuid::Uuid, Option<uuid::Uuid>)> = sqlx::query_as(
        r#"SELECT i.id, i.estimate_point_id FROM issues i
           JOIN workspaces w ON w.id = i.workspace_id
           WHERE i.deleted_at IS NULL AND i.project_id = $1 AND w.slug = $2
             AND i.estimate_point_id = $3
           ORDER BY i.created_at DESC"#,
    )
    .bind(project_id)
    .bind(&slug)
    .bind(point_id)
    .fetch_all(&pool)
    .await
    .map_err(db_denial)?;
    // The activity loop, literally (`:192-228`): per issue, enqueue the
    // emit, then — new-id branch only — run the queryset update. The
    // first emit precedes the update that would reject a garbage id.
    for (issue_id, current_point) in &issues {
        let requested = t::estimate_point_dumps(new_id_text.as_deref());
        let current_text = current_point.map(|id| id.to_string());
        let current = t::estimate_point_dumps(current_text.as_deref());
        let emit = t::IssueActivityEmit {
            requested_data: requested,
            actor_id: actor.id.to_string(),
            issue_id: issue_id.to_string(),
            project_id: project_id.to_string(),
            current_instance: current,
            epoch: Utc::now().timestamp(),
        };
        enqueue_issue_activity(&pool, &emit).await;
        if new_id_text.is_some() {
            // The re-evaluated queryset: after the first iteration
            // nothing matches (all rows moved) — except the message
            // still sends per issue. Django's UUID prep (raw value
            // through `to_python`) rejects garbage → the invalid-detail
            // 400 *after* the emit above; a well-formed but dangling id
            // fails the FK → the `IntegrityError` 400.
            let new_uuid = match &new_uuid {
                Some(Ok(id)) => *id,
                _ => return Err(Denial::BadValidation),
            };
            sqlx::query(
                r#"UPDATE issues SET estimate_point_id = $1 WHERE id IN (
                     SELECT i.id FROM issues i JOIN workspaces w ON w.id = i.workspace_id
                     WHERE i.deleted_at IS NULL AND i.project_id = $2 AND w.slug = $3
                       AND i.estimate_point_id = $4
                   )"#,
            )
            .bind(new_uuid)
            .bind(project_id)
            .bind(&slug)
            .bind(point_id)
            .execute(&pool)
            .await
            .map_err(db_denial)?;
        }
    }
    // `old_estimate_point` (live-only, unscoped): missing → the 500.
    let old: Option<(i32,)> = sqlx::query_as(
        r#"SELECT ep.key FROM estimate_points ep
           WHERE ep.deleted_at IS NULL AND ep.id = $1"#,
    )
    .bind(point_id)
    .fetch_optional(&pool)
    .await
    .map_err(db_denial)?;
    let (old_key,) = old.ok_or(Denial::ServerError)?;
    // The decremented set (`:234-240`, `bulk_update(["key"])` — no
    // `updated_at` touch): exactly the rows with `key > old_key`,
    // captured by id BEFORE the decrement — a non-deleted row sharing
    // the old key (no unique constraint; bulk PATCH sets raw keys) is
    // excluded from the response. The deleted row itself (key == old
    // key) is never in the set.
    let moved_rows: Vec<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT ep.id FROM estimate_points ep JOIN workspaces w ON w.id = ep.workspace_id
           WHERE ep.deleted_at IS NULL AND ep.estimate_id = $1
             AND ep.project_id = $2 AND w.slug = $3 AND ep."key" > $4"#,
    )
    .bind(estimate_id)
    .bind(project_id)
    .bind(&slug)
    .bind(old_key)
    .fetch_all(&pool)
    .await
    .map_err(db_denial)?;
    let moved_ids: Vec<uuid::Uuid> = moved_rows.into_iter().map(|row| row.0).collect();
    sqlx::query(
        r#"UPDATE estimate_points ep SET "key" = ep."key" - 1
           FROM workspaces w
           WHERE ep.workspace_id = w.id AND ep.deleted_at IS NULL
             AND ep.estimate_id = $1 AND ep.project_id = $2 AND w.slug = $3
             AND ep."key" > $4"#,
    )
    .bind(estimate_id)
    .bind(project_id)
    .bind(&slug)
    .bind(old_key)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    let decremented: Vec<EstimatePointRowTuple> = if moved_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            r#"SELECT ep.id, ep.created_at, ep.updated_at, ep.deleted_at, ep.key,
                      ep.description, ep.value, ep.created_by_id, ep.updated_by_id,
                      ep.project_id, ep.workspace_id, ep.estimate_id
               FROM estimate_points ep
               WHERE ep.id = ANY($1)
               ORDER BY ep.value ASC"#,
        )
        .bind(&moved_ids)
        .fetch_all(&pool)
        .await
        .map_err(db_denial)?
    };
    let now = utc_now_micros();
    sqlx::query(
        r#"UPDATE estimate_points SET deleted_at = $1, updated_at = $1, updated_by_id = $2
           WHERE id = $3"#,
    )
    .bind(now)
    .bind(actor.id)
    .bind(point_id)
    .execute(&pool)
    .await
    .map_err(db_denial)?;
    enqueue_soft_delete(&pool, "estimatepoint", &point_id).await;
    // The captured set, in `Meta.ordering` (value) order — the deleted
    // row was never a member (its key equals, not exceeds, the old key).
    let points: Vec<Value> = decremented
        .iter()
        .map(|tuple| estimate_point_row_from(tuple.clone()))
        .map(|point| render_point(&point, actor.timezone))
        .collect();
    Ok(json_ok(Value::Array(points).to_string()))
}

// ---------------------------------------------------------------------------
// Task publishing (best-effort: without the queue the response stands)
// ---------------------------------------------------------------------------

/// `.delay("db", model, pk, using=None)` for the soft-delete sweep (the
/// sticky-handler precedent): three positionals, `using` as a null
/// kwarg — matching the FX-APROJ-08 capture.
fn soft_delete_message(model: &str, id: &uuid::Uuid) -> pidash_jobs::celery::CeleryTaskMessage {
    let mut kwargs = Map::new();
    kwargs.insert("using".to_owned(), Value::Null);
    pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            Value::String("db".to_owned()),
            Value::String(model.to_owned()),
            Value::String(id.to_string()),
        ],
        kwargs,
    )
}

/// Enqueue the related sweep after the tombstone lands (without it the
/// 204 still stands).
async fn enqueue_soft_delete(pool: &sqlx::PgPool, model: &str, id: &uuid::Uuid) {
    let message = soft_delete_message(model, id);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

/// Enqueue one `issue_activity.delay(…)` emit from point destroy (same
/// best-effort shape: the emit never changes the response).
async fn enqueue_issue_activity(pool: &sqlx::PgPool, emit: &t::IssueActivityEmit) {
    let message =
        pidash_jobs::celery::CeleryTaskMessage::new(emit.task_name(), vec![], emit.kwargs());
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn gates_cover_every_owned_action() {
        // (method, gate-table path, expectation): decorator rows deny
        // guests where the table says so; the retrieve row is
        // auth-only; the bulk rows are the class gate.
        let decorator = [
            ("GET", "workspaces/<slug>/projects/<project_id>/states/"),
            ("POST", "workspaces/<slug>/projects/<project_id>/states/"),
            (
                "PATCH",
                "workspaces/<slug>/projects/<project_id>/states/<pk>/",
            ),
            (
                "DELETE",
                "workspaces/<slug>/projects/<project_id>/states/<pk>/",
            ),
            (
                "GET",
                "workspaces/<slug>/projects/<project_id>/intake-state/",
            ),
            (
                "POST",
                "workspaces/<slug>/projects/<project_id>/states/<pk>/mark-default/",
            ),
            (
                "GET",
                "workspaces/<slug>/projects/<project_id>/project-estimates/",
            ),
            (
                "POST",
                "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/",
            ),
            (
                "PATCH",
                "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/<estimate_point_id>/",
            ),
            (
                "DELETE",
                "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/<estimate_point_id>/",
            ),
        ];
        for (method, path) in decorator {
            let row = gates::gate_for(method, path).expect("decorator gate");
            assert!(
                matches!(row.gate, gates::Gate::Project { .. }),
                "{method} {path}"
            );
        }
        let row = gates::gate_for(
            "GET",
            "workspaces/<slug>/projects/<project_id>/states/<pk>/",
        )
        .expect("retrieve gate");
        assert!(matches!(row.gate, gates::Gate::Authenticated));
        for (method, path) in [
            ("GET", "workspaces/<slug>/projects/<project_id>/estimates/"),
            ("POST", "workspaces/<slug>/projects/<project_id>/estimates/"),
            (
                "GET",
                "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/",
            ),
            (
                "PATCH",
                "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/",
            ),
            (
                "DELETE",
                "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/",
            ),
        ] {
            let row = gates::gate_for(method, path).expect("class gate");
            assert!(
                matches!(row.gate, gates::Gate::ProjectEntity),
                "{method} {path}"
            );
        }
    }

    #[test]
    fn denial_bodies_match_the_fixtures() {
        // FX-APROJ-10 error pins (+ the FX-APROJ-08 exception table).
        assert_eq!(
            Denial::ObjectNotFound.status_and_body().1,
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(
            Denial::BadValidation.status_and_body().1,
            r#"{"error":"Please provide valid detail"}"#
        );
        assert_eq!(
            Denial::BadPayload.status_and_body().1,
            r#"{"error":"The payload is not valid"}"#
        );
        assert_eq!(
            Denial::ServerError.status_and_body().1,
            r#"{"error":"Something went wrong please try again later"}"#
        );
        assert_eq!(
            Denial::ResolveNotFound.status_and_body().1,
            r#"{"detail":"Project not found"}"#
        );
        assert_eq!(
            Denial::MembersBlocked.status_and_body().1,
            r#"{"error":"Members are not permitted to edit workflow states for this project."}"#
        );
        assert_eq!(
            STATE_DUP_NAME_BODY,
            r#"{"name":"The state name is already taken"}"#
        );
        assert_eq!(
            DEFAULT_DELETE_BODY,
            r#"{"error":"Default state cannot be deleted"}"#
        );
        assert_eq!(
            NONEMPTY_DELETE_BODY,
            r#"{"error":"The state is not empty, only empty states can be deleted"}"#
        );
        assert_eq!(TRIAGE_MISSING_BODY, r#"{"error":"Triage state not found"}"#);
        assert_eq!(
            POINTS_REQUIRED_BODY,
            r#"{"error":"Estimate points are required"}"#
        );
        assert_eq!(
            KEY_VALUE_REQUIRED_BODY,
            r#"{"error":"Key and value are required"}"#
        );
        assert_eq!(
            Denial::ClassDenied.status_and_body().1,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
    }

    #[test]
    fn state_list_golden_byte_identical() {
        // FX-APROJ-10 `state_list[0]` + the injected `order` (single
        // state in its group → `1/1`).
        let row = StateRow {
            id: "08bfa3c6-3213-4559-af45-b8aa934012b8".parse().unwrap(),
            project_id: "3e0008a4-62ec-448c-abe2-5fca6ee60c5a".parse().unwrap(),
            workspace_id: "94f09a58-8b7e-46c3-a43b-afe7c649a108".parse().unwrap(),
            name: "Backlog".to_owned(),
            color: "#60646C".to_owned(),
            group: "backlog".to_owned(),
            default: true,
            description: String::new(),
            sequence: 15000.0,
        };
        let mut states = vec![render_state(&row)];
        inject_state_order(&mut states);
        assert_eq!(
            states[0].to_string(),
            r##"{"id":"08bfa3c6-3213-4559-af45-b8aa934012b8","project_id":"3e0008a4-62ec-448c-abe2-5fca6ee60c5a","workspace_id":"94f09a58-8b7e-46c3-a43b-afe7c649a108","name":"Backlog","color":"#60646C","group":"backlog","default":true,"description":"","sequence":15000.0,"order":1.0}"##
        );
    }

    #[test]
    fn state_order_injects_index_over_count() {
        // Three states in one group → `1/3`, `2/3`, `3/3` in sequence
        // order (DRF float rendering: shortest round-trip).
        let mut states: Vec<Value> = ["A", "B", "C"]
            .iter()
            .map(|name| json!({"group": "started", "name": name}))
            .collect();
        inject_state_order(&mut states);
        let orders: Vec<String> = states
            .iter()
            .map(|state| state["order"].to_string())
            .collect();
        assert_eq!(
            orders,
            vec![
                "0.3333333333333333".to_owned(),
                "0.6666666666666666".to_owned(),
                "1.0".to_owned()
            ]
        );
    }

    #[test]
    fn grouped_states_sort_by_group_stably() {
        // `sorted(states, key=group)` is stable: within-group sequence
        // order survives, groups key in sorted order.
        let mut states = vec![
            json!({"group": "unstarted", "name": "Todo", "order": 1.0}),
            json!({"group": "backlog", "name": "Backlog", "order": 1.0}),
        ];
        inject_state_order(&mut states);
        let grouped = render_grouped_states(&states);
        let parsed: Value = serde_json::from_str(&grouped).expect("json");
        let keys: Vec<&str> = parsed
            .as_object()
            .expect("dict")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["backlog", "unstarted"]);
        assert_eq!(parsed["backlog"][0]["name"], json!("Backlog"));
    }

    #[test]
    fn state_create_golden_byte_identical() {
        // FX-APROJ-10 `state_create.body` (no `order` key: create never
        // injects it).
        let row = StateRow {
            id: "51c37f86-28ce-4312-bc88-652b24de6290".parse().unwrap(),
            project_id: "3e0008a4-62ec-448c-abe2-5fca6ee60c5a".parse().unwrap(),
            workspace_id: "94f09a58-8b7e-46c3-a43b-afe7c649a108".parse().unwrap(),
            name: "Staging".to_owned(),
            color: "#00FF00".to_owned(),
            group: "started".to_owned(),
            default: false,
            description: String::new(),
            sequence: 70000.0,
        };
        assert_eq!(
            render_state(&row).to_string(),
            r##"{"id":"51c37f86-28ce-4312-bc88-652b24de6290","project_id":"3e0008a4-62ec-448c-abe2-5fca6ee60c5a","workspace_id":"94f09a58-8b7e-46c3-a43b-afe7c649a108","name":"Staging","color":"#00FF00","group":"started","default":false,"description":"","sequence":70000.0}"##
        );
    }

    #[test]
    fn state_patch_echoes_validated_order_last() {
        // D1: a valid `order` is echoed as the last field (the validated
        // float — int input renders `1.0`); absent stays absent.
        let (input, errors) = validate_state_input(&object(&[("order", json!(1))]), true);
        assert!(errors.is_empty());
        assert_eq!(input.order, Some(1.0));
        let row = StateRow {
            id: "51c37f86-28ce-4312-bc88-652b24de6290".parse().unwrap(),
            project_id: "3e0008a4-62ec-448c-abe2-5fca6ee60c5a".parse().unwrap(),
            workspace_id: "94f09a58-8b7e-46c3-a43b-afe7c649a108".parse().unwrap(),
            name: "Staging".to_owned(),
            color: "#00FF00".to_owned(),
            group: "started".to_owned(),
            default: false,
            description: String::new(),
            sequence: 70000.0,
        };
        assert!(render_patched_state(&row, input.order)
            .to_string()
            .ends_with(r#""sequence":70000.0,"order":1.0}"#));
        assert!(!render_patched_state(&row, None)
            .to_string()
            .contains("order"));
    }

    #[test]
    fn state_validation_matches_drf() {
        // Required pair missing → both field errors, field order.
        let (input, errors) = validate_state_input(&object(&[]), false);
        assert_eq!(
            Value::Object(errors).to_string(),
            r#"{"name":["This field is required."],"color":["This field is required."]}"#
        );
        assert_eq!(input.group.as_deref(), Some("backlog"));
        assert_eq!(input.default, Some(false));
        assert_eq!(input.description.as_deref(), Some(""));
        assert_eq!(input.sequence, Some(65535.0));
        // Blank name, bad group, bad bool.
        let (input, errors) = validate_state_input(
            &object(&[
                ("name", json!("   ")),
                ("color", json!("#000")),
                ("group", json!("nope")),
                ("default", json!("maybe")),
            ]),
            false,
        );
        assert_eq!(
            Value::Object(errors).to_string(),
            r#"{"name":["This field may not be blank."],"group":["\"nope\" is not a valid choice."],"default":["Must be a valid boolean."]}"#
        );
        assert!(input.name.is_none());
        // Triage veto message says "create" even on update (ported as-is).
        let veto = ser::state_validate(Some("triage")).expect("veto");
        assert_eq!(
            veto.to_string(),
            r#"{"non_field_errors":["Cannot create triage state"]}"#
        );
        assert!(ser::state_validate(Some("started")).is_none());
        assert!(ser::state_validate(None).is_none());
        // `order` validates as a float; present-and-valid is the
        // create-path 500 marker.
        let (input, errors) = validate_state_input(
            &object(&[
                ("name", json!("S")),
                ("color", json!("#000")),
                ("order", json!(2.5)),
            ]),
            false,
        );
        assert!(errors.is_empty());
        assert_eq!(input.order, Some(2.5));
        let (_, errors) = validate_state_input(&object(&[("order", json!("x"))]), true);
        assert_eq!(
            Value::Object(errors).to_string(),
            r#"{"order":["A valid number is required."]}"#
        );
        // Read-only + unknown keys are silently dropped.
        let (input, errors) = validate_state_input(
            &object(&[
                ("name", json!("S")),
                ("color", json!("#000")),
                ("project_id", json!("11111111-1111-1111-1111-111111111111")),
                ("workspace_id", json!("nope")),
                ("id", json!("nope")),
                ("bogus", json!(1)),
            ]),
            false,
        );
        assert!(errors.is_empty());
        assert_eq!(input.name.as_deref(), Some("S"));
    }

    #[test]
    fn bulk_point_validation_matches_drf() {
        // `{}` → the field-level value error (the `if not data` branch
        // is unreachable via `is_valid()`).
        let failed = validate_bulk_point(&object(&[])).expect_err("value required");
        assert_eq!(
            failed.to_string(),
            r#"{"value":["This field is required."]}"#
        );
        // Over-long value → the `validate()` non-field error.
        let failed = validate_bulk_point(&object(&[
            ("key", json!(1)),
            ("value", json!("x".repeat(21))),
        ]))
        .expect_err("too long");
        assert_eq!(
            failed.to_string(),
            r#"{"non_field_errors":["Value can't be more than 20 characters"]}"#
        );
        // But the field-level 255 cap fires first.
        let failed = validate_bulk_point(&object(&[
            ("key", json!(1)),
            ("value", json!("x".repeat(256))),
        ]))
        .expect_err("max length");
        assert_eq!(
            failed.to_string(),
            r#"{"value":["Ensure this field has no more than 255 characters."]}"#
        );
        // Negative key → the min-value message.
        let failed = validate_bulk_point(&object(&[("key", json!(-1)), ("value", json!("v"))]))
            .expect_err("min value");
        assert_eq!(
            failed.to_string(),
            r#"{"key":["Ensure this value is greater than or equal to 0."]}"#
        );
        // Valid item: raw (unstripped) value stored, defaults applied.
        let input = validate_bulk_point(&object(&[("value", json!("  v  "))])).expect("valid");
        assert_eq!(input.key, 0);
        assert_eq!(input.value, "  v  ");
        assert_eq!(input.description, "");
    }

    #[test]
    fn bulk_create_points_gate_matches_list_serializer() {
        // D2: missing/`null` → the `No data provided` rewrite (probe-run
        // against DRF 3.15.2); other non-lists → `not_a_list`; lists pass.
        assert!(matches!(
            bulk_create_points_shape(None),
            CreatePoints::Missing
        ));
        assert!(matches!(
            bulk_create_points_shape(Some(&Value::Null)),
            CreatePoints::Missing
        ));
        assert_eq!(
            ser::non_field_errors("No data provided").to_string(),
            r#"{"non_field_errors":["No data provided"]}"#
        );
        match bulk_create_points_shape(Some(&json!("x"))) {
            CreatePoints::NotAList(message) => {
                assert_eq!(message, "Expected a list of items but got type \"str\".")
            }
            _ => panic!("not_a_list"),
        }
        match bulk_create_points_shape(Some(&json!([1]))) {
            CreatePoints::Items(items) => assert_eq!(items.len(), 1),
            _ => panic!("items"),
        }
    }

    #[test]
    fn bulk_patch_points_gate_matches_len_check() {
        // D3: only missing/`[]`/`{}`/`""` take the required 400;
        // scalars/`null` are pre-lookup 500s; non-empty containers pass
        // the gate (their item-shape failure lands post-header).
        assert!(matches!(bulk_patch_points_gate(None), PatchPoints::Missing));
        assert!(matches!(
            bulk_patch_points_gate(Some(&json!([]))),
            PatchPoints::Missing
        ));
        assert!(matches!(
            bulk_patch_points_gate(Some(&json!({}))),
            PatchPoints::Missing
        ));
        assert!(matches!(
            bulk_patch_points_gate(Some(&json!(""))),
            PatchPoints::Missing
        ));
        assert!(matches!(
            bulk_patch_points_gate(Some(&Value::Null)),
            PatchPoints::Unsized
        ));
        assert!(matches!(
            bulk_patch_points_gate(Some(&json!(7))),
            PatchPoints::Unsized
        ));
        assert!(matches!(
            bulk_patch_points_gate(Some(&json!(true))),
            PatchPoints::Unsized
        ));
        assert!(matches!(
            bulk_patch_points_gate(Some(&json!({"a": 1}))),
            PatchPoints::IteratesNonDicts
        ));
        assert!(matches!(
            bulk_patch_points_gate(Some(&json!("x"))),
            PatchPoints::IteratesNonDicts
        ));
        match bulk_patch_points_gate(Some(&json!([7]))) {
            PatchPoints::Items(items) => assert_eq!(items.len(), 1),
            _ => panic!("items"),
        }
    }

    #[test]
    fn raw_uuid_prep_matches_to_python() {
        // D4/D5: in-range ints/bools → `UUID(int=…)` (probe-run against
        // Django 4.2.30); floats, composites, out-of-range ints, and
        // garbage strings fail.
        assert_eq!(
            raw_uuid_prep(&json!(5)).expect("int"),
            uuid::Uuid::from_u128(5)
        );
        assert_eq!(
            raw_uuid_prep(&json!(0)).expect("zero"),
            uuid::Uuid::from_u128(0)
        );
        assert_eq!(
            raw_uuid_prep(&json!(true)).expect("bool"),
            uuid::Uuid::from_u128(1)
        );
        assert_eq!(
            raw_uuid_prep(&json!("08bfa3c6-3213-4559-af45-b8aa934012b8")).expect("uuid"),
            "08bfa3c6-3213-4559-af45-b8aa934012b8"
                .parse::<uuid::Uuid>()
                .unwrap()
        );
        assert!(raw_uuid_prep(&json!(5.0)).is_err());
        assert!(raw_uuid_prep(&json!(-1)).is_err());
        assert!(raw_uuid_prep(&json!([1])).is_err());
        assert!(raw_uuid_prep(&json!({"a": 1})).is_err());
        assert!(raw_uuid_prep(&json!("x")).is_err());
        assert!(raw_uuid_prep(&json!("")).is_err());
    }

    #[test]
    fn point_patch_empty_dict_demands_points() {
        // Partial `{}` → `validate({})` → the required-points 400.
        let (patch, errors, _) = validate_point_patch(&object(&[]));
        assert!(errors.is_empty());
        let empty = patch.key.is_none() && patch.value.is_none() && patch.description.is_none();
        let failed = ser::estimate_point_validate(empty, None).expect("required");
        assert_eq!(
            failed.to_string(),
            r#"{"non_field_errors":["Estimate points are required"]}"#
        );
    }

    #[test]
    fn int_float_bool_coercions_match_drf() {
        // `IntegerField`: `1.0` strips, bools/fractions fail, huge passes.
        assert!(matches!(
            validate_int(&json!("1.0")).expect("strips"),
            IntValue::Value(1)
        ));
        assert!(matches!(
            validate_int(&json!(7)).expect("int"),
            IntValue::Value(7)
        ));
        assert_eq!(
            validate_int(&json!(true)).expect_err("bool"),
            "A valid integer is required."
        );
        assert_eq!(
            validate_int(&json!(1.5)).expect_err("fraction"),
            "A valid integer is required."
        );
        assert!(matches!(
            validate_int(&json!("99999999999999999999999")).expect("big"),
            IntValue::TooBig
        ));
        assert_eq!(strip_decimal_zeros("10"), "10");
        assert_eq!(strip_decimal_zeros("5.00"), "5");
        // `FloatField`: bools coerce, junk fails.
        assert_eq!(validate_float(&json!(true)).expect("bool"), 1.0);
        assert_eq!(
            validate_float(&json!("x")).expect_err("junk"),
            "A valid number is required."
        );
        // `BooleanField`: the case-insensitive sets, `1`/`0`/`0.0` — and
        // `1.0`, which set-membership accepts (`1.0 == 1`, same hash).
        assert!(validate_bool(&json!("YES")).expect("yes"));
        assert!(!validate_bool(&json!(0)).expect("zero"));
        assert!(!validate_bool(&json!(0.0)).expect("zero float"));
        assert!(validate_bool(&json!(1.0)).expect("one float"));
        assert_eq!(
            validate_bool(&json!(1.5)).expect_err("float"),
            "Must be a valid boolean."
        );
        assert_eq!(
            validate_bool(&Value::Null).expect_err("null"),
            "This field may not be null."
        );
        // `ChoiceField`: `str()` coercion before lookup.
        assert_eq!(
            validate_str_choice(&json!(true), &["a"]).expect_err("bool"),
            "\"True\" is not a valid choice."
        );
    }

    #[test]
    fn python_helpers_match_cpython() {
        assert_eq!(python_str(&json!(true)), "True");
        assert_eq!(python_str(&Value::Null), "None");
        assert_eq!(python_str(&json!({"a": 1})), "{'a': 1}");
        assert_eq!(python_repr_string("it's"), "\"it's\"");
        assert!(!python_truthy(None));
        assert!(!python_truthy(Some(&json!(0))));
        assert!(!python_truthy(Some(&json!(""))));
        assert!(python_truthy(Some(&json!("0"))));
        assert!(python_truthy(Some(&json!({"a": 1}))));
        assert_eq!(python_type_name(&Value::Null), "NoneType");
        assert_eq!(python_type_name(&json!([1])), "list");
        // ORM `int()`: floats truncate, `+`/space parse, junk fails.
        assert_eq!(orm_int(&json!(5.9)).expect("trunc"), 5);
        assert_eq!(orm_int(&json!(" +7 ")).expect("sign"), 7);
        assert_eq!(orm_int(&json!(true)).expect("bool"), 1);
        assert!(orm_int(&json!("5.0")).is_err());
        assert!(orm_int(&json!("x")).is_err());
        // Root non-dict body message.
        assert_eq!(
            invalid_data_body(&json!([1])),
            r#"{"non_field_errors":["Invalid data. Expected a dictionary, but got list."]}"#
        );
    }

    #[test]
    fn random_names_are_ten_lowercase_letters() {
        for _ in 0..25 {
            let name = generate_random_name();
            assert_eq!(name.len(), 10);
            assert!(name.chars().all(|ch| ch.is_ascii_lowercase()));
        }
    }

    #[test]
    fn point_render_golden_byte_identical() {
        // FX-APROJ-10 `point_create.body`, datetimes staged verbatim.
        let row = EstimatePointRow {
            id: "3e0db108-e218-4041-b6a6-0a76bc13a179".parse().unwrap(),
            created_at: "2026-10-02T22:24:00.179158Z".parse().unwrap(),
            updated_at: "2026-10-02T22:24:00.179174Z".parse().unwrap(),
            deleted_at: None,
            key: 3,
            description: String::new(),
            value: "3".to_owned(),
            created_by: Some("acb103e9-7710-4be7-8f51-aa6a006fc2df".parse().unwrap()),
            updated_by: None,
            project: "3e0008a4-62ec-448c-abe2-5fca6ee60c5a".parse().unwrap(),
            workspace: "94f09a58-8b7e-46c3-a43b-afe7c649a108".parse().unwrap(),
            estimate: "dd543f35-910b-4876-9d4a-80d0c2d2635a".parse().unwrap(),
        };
        assert_eq!(
            render_point(&row, chrono_tz::UTC).to_string(),
            r#"{"id":"3e0db108-e218-4041-b6a6-0a76bc13a179","created_at":"2026-10-02T22:24:00.179158Z","updated_at":"2026-10-02T22:24:00.179174Z","deleted_at":null,"key":3,"description":"","value":"3","created_by":"acb103e9-7710-4be7-8f51-aa6a006fc2df","updated_by":null,"project":"3e0008a4-62ec-448c-abe2-5fca6ee60c5a","workspace":"94f09a58-8b7e-46c3-a43b-afe7c649a108","estimate":"dd543f35-910b-4876-9d4a-80d0c2d2635a"}"#
        );
    }

    #[test]
    fn estimate_read_nests_points_second() {
        // FX-APROJ-10 `bulk_create.body` shape: `id`, then `points`.
        let row = EstimateRow {
            id: "6716edbb-7472-41e1-8775-950c7af9f921".parse().unwrap(),
            created_at: "2026-10-02T22:23:58.970585Z".parse().unwrap(),
            updated_at: "2026-10-02T22:23:58.970617Z".parse().unwrap(),
            deleted_at: None,
            name: "Est10".to_owned(),
            description: String::new(),
            estimate_type: "points".to_owned(),
            last_used: false,
            created_by: Some("acb103e9-7710-4be7-8f51-aa6a006fc2df".parse().unwrap()),
            updated_by: None,
            project: "3e0008a4-62ec-448c-abe2-5fca6ee60c5a".parse().unwrap(),
            workspace: "94f09a58-8b7e-46c3-a43b-afe7c649a108".parse().unwrap(),
        };
        let rendered = render_estimate_read(&row, &[], chrono_tz::UTC).to_string();
        assert!(rendered.starts_with(
            r#"{"id":"6716edbb-7472-41e1-8775-950c7af9f921","points":[],"created_at":"2026-10-02T22:23:58.970585Z""#
        ));
        assert!(rendered.contains(r#""type":"points""#));
        assert!(rendered.ends_with(
            r#""project":"3e0008a4-62ec-448c-abe2-5fca6ee60c5a","workspace":"94f09a58-8b7e-46c3-a43b-afe7c649a108"}"#
        ));
    }

    #[test]
    fn soft_delete_message_matches_the_celery_call() {
        // `.delay("db", model, pk, using=None)`: three positionals,
        // `using` as a null kwarg (the FX-APROJ-08 capture shape).
        let pk: uuid::Uuid = "11111111-1111-1111-1111-111111111111".parse().unwrap();
        for model in ["state", "estimate", "estimatepoint"] {
            let message = soft_delete_message(model, &pk);
            assert_eq!(
                message.task,
                "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"
            );
            assert_eq!(
                message.args,
                vec![
                    Value::String("db".to_owned()),
                    Value::String(model.to_owned()),
                    Value::String(pk.to_string()),
                ]
            );
            assert_eq!(message.kwargs.get("using"), Some(&Value::Null));
            assert_eq!(message.kwargs.len(), 1);
        }
    }

    #[test]
    fn issue_activity_emit_matches_the_fixture() {
        // FX-APROJ-08 `issue_activity_on_point_destroy.with_new_estimate_id`:
        // one emit per issue, CPython-separator dumps, kwarg order pinned.
        let emit = t::IssueActivityEmit {
            requested_data: t::estimate_point_dumps(Some("c4e32397-132a-487c-80fa-725c7a766ea9")),
            actor_id: "db5932ed-aebf-48e2-8757-499eb04dc52b".to_owned(),
            issue_id: "31af8870-2aaf-4fa7-8b3b-e20f0d61b6af".to_owned(),
            project_id: "063e5ab4-4fe7-4d95-894f-5b7a4443bab9".to_owned(),
            current_instance: t::estimate_point_dumps(Some("88bd172f-e7ce-4500-aec6-341ce3f51970")),
            epoch: 1790979535,
        };
        assert_eq!(
            emit.task_name(),
            "pi_dash.bgtasks.issue_activities_task.issue_activity"
        );
        let kwargs = emit.kwargs();
        let order: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            order,
            vec![
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch"
            ]
        );
        assert_eq!(
            kwargs.get("requested_data"),
            Some(&Value::String(
                r#"{"estimate_point": "c4e32397-132a-487c-80fa-725c7a766ea9"}"#.to_owned()
            ))
        );
    }

    #[test]
    fn route_paths_match_the_url_conf() {
        // `app/urls/state.py` + `app/urls/estimate.py`, axum form.
        assert_eq!(
            STATES_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/states/"
        );
        assert_eq!(
            STATE_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/states/{pk}/"
        );
        assert_eq!(
            MARK_DEFAULT_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/states/{pk}/mark-default/"
        );
        assert_eq!(
            POINT_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/{estimate_point_id}/"
        );
    }
}
