#![forbid(unsafe_code)]

//! Issue version + description-version + move handlers (D-26, stage 5, PIDASHCONV-657).
//!
//! Ports `app/views/issue/version.py` (2 units, 144 lines) and
//! `app/views/issue/move.py` (1 unit, 46 lines):
//!
//! - `GET issues/<issue_id>/versions/` (`IssueVersionEndpoint.get`,
//!   `version.py:36-74`): the 9-key `paginate` envelope over 10-key rows
//!   (`required_fields`, model `-created_at` order). No parent checks at
//!   all — a missing issue is an empty page, not a 404.
//! - `GET issues/<issue_id>/versions/<pk>/` (`:38-44`): the 30-key
//!   `IssueVersionDetailSerializer` row (the dup-`name` collapses to one,
//!   first position — the 640 finding).
//! - `GET work-items/<work_item_id>/description-versions/`
//!   (`WorkItemDescriptionVersionEndpoint.get`, `:86-144`):
//!   `Project.get` + `Issue.get` (404s), the guest-view 403 gate, then
//!   the same 9-key envelope over the explicit
//!   `.order_by("-created_at")` list.
//! - `GET work-items/<work_item_id>/description-versions/<pk>/`
//!   (`:107-116`): the 14-key `IssueDescriptionVersionDetailSerializer`
//!   row (`description_binary` renders base64, null stays null).
//! - `POST work-items/<pk>/move/` (`IssueMoveEndpoint.post`,
//!   `move.py:28-46`): the ADMIN/MEMBER gate, the non-dict-body `{}` coercion,
//!   `move_work_item_to_project` via the 650 port, `IssueMoveError` mapped
//!   to `{"error"}` + status, the explicit 594 signal pair around the
//!   state-changing save, then the 22-key APP `IssueSerializer` read shape
//!   (a bare instance: the 7 annotation keys are `SkipField`-omitted, the
//!   639 rule, verified live).
//!
//! Every other method on those paths proxies to Django (route table in
//! `super::routes` via [`routes`]); `HEAD` rides axum's `get` handling
//! on the GET-owned paths like Django's GET-backed `HEAD`.
//!
//! Reuse (call, don't copy): the 649 version SQL builders
//! (`super::queries_engage`), pilot-2's `v2_page` cursor math
//! (`super::render`), the 640 version detail rendering
//! (`serializers_assoc`), the 639 base-shape rendering
//! (`serializers_detail`), the 650 move driver (`issue_move`), the 594
//! signal entries (`orchestration::entries`), the D-14 drain recipe
//! (`runner_sessions::drain`) + pubsub verbs, the shared DRF body
//! negotiator (`v1_cycles_modules::body`), the D-12 creation seam
//! (`crate::orchestration`), and the jobs Celery enqueue.
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * The versions list never 404s: a missing issue (or project, past the
//!   gate) is an empty 200 page (`filter`, never `get`).
//! * `IssueDescriptionVersion` declares no `Meta.ordering`, so the detail
//!   `.get()` carries no `ORDER BY` (unlike its version sibling) — the
//!   649 SQL preserves the asymmetry.
//! * `PaginateCursor`'s offset part is parsed but never read (`start_index`
//!   derives from `current_page` alone) — inherited from pilot-2's
//!   `v2_page`, reused, not re-ported.
//! * The move's `now` is drawn once per request (Python draws it inside
//!   the transaction, after the locks); the 650 driver takes it frozen.
//! * A non-finite `sort_order` answers 500 (DRF renders with
//!   `allow_nan=False` under the default `STRICT_JSON=True`).
//!
//! # Known gaps (documented, not silent)
//!
//! * Past a ticking-state transition the 594 fire needs the same runtime
//!   (ticker reconcile, entry-run dispatch); the seam answers store
//!   errors there, which the fire swallows by design (counter + the
//!   verbatim log line) while the move stands. Non-trigger transitions
//!   (every gate seed, the backlog norm) run no seam I/O past the two
//!   state lookups and are fully faithful.
//! * `v2_page` parses cursor ints strictly (Rust `parse`), where Python's
//!   `int()` accepts whitespace/signs/underscores — pilot-2 shared code,
//!   reused, not re-ported.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use tokio::sync::Mutex;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::membership::ProjectRoleFacts;
use pidash_db::app_project::models::project::ProjectLookup;
use pidash_db::config::RunnerSettings;
use pidash_db::dispatch::{AgentRunStatus, AgentRunTrigger};
use pidash_db::orchestration::workpad::AgentUserCollisionError;
use pidash_db::runner_sessions::machine_outbox::{self, MachineOutboxError};
use pidash_db::runner_sessions::models::runner_session;
use pidash_db::runner_sessions::outbox as runner_outbox;
use pidash_db::runner_sessions::outbox::OutboxError;
use pidash_db::runner_sessions::RunnerSession;
use pidash_db::tasks_ticker::models::issue_agent_ticker::IssueAgentTicker;
use pidash_db::tx::Transaction as DbTransaction;
use pidash_services::app_issues::issue_move::{
    CREATE_ADVISORY_LOCK_SQL, CREATE_SAVE_DEFAULT_STATE_SQL, CREATE_SAVE_FALLBACK_STATE_SQL,
    CREATE_SEQ_SCAN_SQL, DEFAULT_FOR_PROJECT_ID_SQL,
};
use pidash_services::app_issues::serializers_assoc::{
    issue_description_version_detail_to_representation, issue_version_detail_to_representation,
    IssueDescriptionVersionDetailRow, IssueVersionDetailRow,
};
use pidash_services::app_issues::{
    issue_detail_base_to_representation, issue_is_actively_synced, move_work_item_to_project,
    repoint_sql, send_project_move_cancel, IssueDetailBaseRow, MoveError, MovePostCommit,
    MoveResult, MoveStore, MovedIssueFields, MovedIssueRow, PodRow, ProjectRow, SourceIssueRow,
    StateRow, ASSIGNEE_IDS_SQL, ASSIGNEE_PRUNE_SQL, ASSIGNEE_REPOINT_SQL, CHILDREN_DETACH_SQL,
    COMMENT_IDS_SQL, CYCLE_DELETE_SQL, DESCRIPTION_IDS_SQL, GITHUB_ISSUE_SYNC_PROBE_SQL,
    GIT_ISSUE_SYNC_PROBE_SQL, HANDOFF_PARENT_UPDATE_SQL, HANDOFF_RUNS_LOCK_SQL,
    INERT_RUN_UPDATE_SQL, ISSUE_MOVE_UPDATE_SQL, LABEL_DELETE_SQL, LABEL_IDS_SQL,
    MEMBER_EXISTS_SQL, MODULE_DELETE_SQL, MOVED_ISSUE_REFETCH_SQL, PROJECT_IDENTIFIER_SQL,
    PROJECT_RESOLVE_BY_IDENTIFIER_SQL, PROJECT_RESOLVE_BY_PK_SQL, RELATION_DELETE_SQL,
    SEQUENCE_CREATE_SQL, SEQUENCE_DETACH_SQL, SOURCE_ISSUE_LOCK_SQL, SOURCE_ISSUE_SQL,
    WORKSPACE_SLUG_SQL,
};
use pidash_services::app_issues::{HandoffRunRow, RepointTarget};
use pidash_services::dispatch::policy::UserFlags;
use pidash_services::orchestration::blockers::{has_open_blockers_sql, summary_sql, BlockerRow};
use pidash_services::orchestration::clock::{ClockWrite, ProjectClockPolicy};
use pidash_services::orchestration::creation::{
    create_project_move_handoff_run, AdmissionError, CreationError, CreationSeam, ExecutionError,
    ExecutionFields, ExecutionRequest, FinalizeAgentRunSeam, HandoffCreateRequest, IssueView,
    LockedIssue, NewAgentRun, PodView, ProjectView, RenderBundle, RenderedTurn, RunView,
    RunnerView, StateView, STATE_SELECT_SQL,
};
use pidash_services::orchestration::entries::{
    capture_prior_state, fire_state_transition, BindingView, EntriesSeam, FireOutcome, FireRequest,
    NewSchedulerRun, PreflightSeam, SchedulerView, WorkspaceView, PRIOR_STATE_SELECT_SQL,
};
use pidash_services::prompting::composer::{OverrideIndex, OverrideRow};
use pidash_services::runner_sessions::drain::{
    drain_pod_log, next_for_runner_sql, plan_assignment, AssignmentFacts, DrainEffect,
    ASSIGN_RUN_UPDATE_SQL, DRAIN_POD_BY_ID_LOOKUP_SQL, DRAIN_POD_IDLE_RUNNERS_SQL,
};
use pidash_services::runner_sessions::guards::alive_threshold;
use pidash_services::runner_sessions::pubsub::{
    send_to_runner, PubsubStore, CLOSE_ACTIVE_SESSIONS_SQL,
};
use pidash_types::dispatch::AgentExecutorKind;

use super::queries_engage::{
    description_count_sql, description_detail_sql, description_list_sql, version_count_sql,
    version_detail_sql, version_list_sql, VERSION_PAGE_KEYS,
};
use super::render::{v2_page, V2Page};
use super::{query_last, Binder, QueryMap};
use crate::orchestration::{
    drain_creation_outboxes, handoff_store, split_outboxes, CreationDrainError, CreationOutboxes,
    SpanCreationDeps,
};
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json_cpython::{
    parse_json_text, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `GET .../issues/<issue_id>/versions/` (`urls/issue.py:279-283`).
pub const VERSIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/";
/// `GET .../issues/<issue_id>/versions/<pk>/` (`urls/issue.py:284-288`).
pub const VERSION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/";
/// `GET .../work-items/<work_item_id>/description-versions/` (`:289-293`).
pub const DESC_VERSIONS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/";
/// `GET .../work-items/<work_item_id>/description-versions/<pk>/` (`:294-298`).
pub const DESC_VERSION_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/";
/// `POST .../work-items/<pk>/move/` (`:309-313`,
/// `http_method_names=["post"]`).
pub const MOVE_PATH: &str = "/api/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/";

/// Register the five versions/move routes. Nothing else: sibling paths
/// stay unmatched and proxy to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            VERSIONS_PATH,
            owned(axum::routing::get(version_list), &["GET"]),
        )
        .route(
            VERSION_PATH,
            owned(axum::routing::get(version_detail), &["GET"]),
        )
        .route(
            DESC_VERSIONS_PATH,
            owned(axum::routing::get(desc_version_list), &["GET"]),
        )
        .route(
            DESC_VERSION_PATH,
            owned(axum::routing::get(desc_version_detail), &["GET"]),
        )
        .route(MOVE_PATH, owned(axum::routing::post(move_issue), &["POST"]))
}

/// A versions/move path: the owned methods serve from Rust, everything
/// else falls through to Django (the engage cutover shape — DRF
/// authenticates before it checks the method, so proxying reproduces
/// the 401-anon and Django's own 405s with no per-method logic).
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
    /// 403, `@allow_permission` body.
    Forbidden,
    /// 403, the description-versions guest-view gate
    /// (`version.py:102-105`).
    GuestView,
    /// 404, `handle_exception`'s `ObjectDoesNotExist` branch.
    NotFound,
    /// 404, `{"detail":"Project not found"}` (project-kwarg rewrite miss
    /// and the move's `Project.resolve` miss — DRF propagates `Http404`
    /// args via `NotFound(*exc.args)`).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (JSON `ParseError`).
    BadDetail(String),
    /// 400/403/409, `{"error": ...}` (the move's `IssueMoveError` mapping).
    MoveRejection { status: u16, message: String },
    /// 415, `UnsupportedMediaType` (content negotiation).
    UnsupportedMediaType(String),
    /// 413, `RequestBodySizeLimitMiddleware` past 5 MiB.
    RequestTooLarge,
    /// 500, generic branch.
    ServerError,
}

/// The description-versions guest-view refusal
/// (`version.py:102-105`).
const GUEST_VIEW_BODY: &str = r#"{"error":"You are not allowed to view this issue"}"#;

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                super::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                crate::permissions::PERMISSION_DENIED_BODY.to_owned(),
            ),
            Denial::GuestView => (StatusCode::FORBIDDEN, GUEST_VIEW_BODY.to_owned()),
            Denial::NotFound => (StatusCode::NOT_FOUND, super::NOT_FOUND_BODY.to_owned()),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                super::PROJECT_NOT_FOUND_BODY.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::MoveRejection { status, message } => (
                StatusCode::from_u16(*status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::RequestTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                r#"{"error":"REQUEST_BODY_TOO_LARGE","detail":"The size of the request body exceeds the maximum allowed size."}"#.to_owned(),
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
            tracing::warn!("app_issues versions/move handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(escape_json_text(&body)))
            .expect("versions/move error response")
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// DRF `JSONRenderer` post-pass (`renderers.py`): U+2028/U+2029 escape to
/// `\\u2028`/`\\u2029` in every response so the output stays a strict
/// JavaScript subset.
fn escape_json_text(body: &str) -> String {
    body.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Render `body` (already exact JSON bytes) with an explicit status.
fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(escape_json_text(&body)))
        .expect("versions/move json response")
}

type HandlerResult = Result<Response, Denial>;

// ---------------------------------------------------------------------------
// Request plumbing: auth + project rewrite + permission gates
// ---------------------------------------------------------------------------

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, a non-UUID id, or a session pointing at no user row means
/// anonymous → 401. (Django PKs are UUIDs; a session id that is not a
/// UUID cannot be a user.)
async fn actor_user_id(
    pool: &PgPool,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Uuid, Denial> {
    let handle = extension.ok_or(Denial::Unauthorized)?.0;
    let mut session = handle.snapshot();
    let raw = session
        .get("_auth_user_id")
        .and_then(|value| value.as_str().to_owned())
        .ok_or(Denial::Unauthorized)?;
    let id = raw.parse::<Uuid>().map_err(|_| Denial::Unauthorized)?;
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
async fn resolve_project_id(pool: &PgPool, slug: &str, raw: &str) -> Result<Uuid, Denial> {
    if let Ok(id) = raw.parse::<Uuid>() {
        return Ok(id);
    }
    let upper = raw.trim().to_uppercase();
    let row: Option<(Uuid,)> = sqlx::query_as(
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
/// Uppercase / simple / braced / `urn:` spellings never route in Django
/// (resolver 404), so they proxy here instead of serving.
fn path_uuid(raw: &str) -> Result<Uuid, ()> {
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
    raw.parse::<Uuid>().map_err(|_| ())
}

/// Membership facts for one `(user, slug, project)` over the same rows
/// the decorator reads: active project and workspace memberships scoped
/// by slug/project, soft-deleted rows excluded (`SoftDeletionManager`).
struct Membership {
    project_role: Option<i16>,
    workspace_role: Option<i16>,
}

async fn fetch_membership(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<Membership, Denial> {
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
    Ok(Membership {
        project_role: project_role.map(|row| row.0),
        workspace_role: workspace_role.map(|row| row.0),
    })
}

/// `allow_permission(allowed_roles)` at `PROJECT` level
/// (`app/permissions/base.py:19-86`): an allowed project role, or any
/// active project membership plus an active workspace ADMIN membership.
/// Denies with the decorator `error` body.
fn check_allow(membership: &Membership, allowed: &[i16]) -> Result<(), Denial> {
    if membership
        .project_role
        .is_some_and(|role| allowed.contains(&role))
    {
        return Ok(());
    }
    if membership.project_role.is_some() && membership.workspace_role == Some(20) {
        return Ok(());
    }
    Err(Denial::Forbidden)
}

/// View-body tenant facts: the actor's render timezone
/// (`TimezoneMixin` activation; a bad zone or a missing user row is a
/// 500). Project existence is per-path (only the reads whose Python
/// calls `Project.objects.get` 404 on a missing row).
struct Tenant {
    timezone: Tz,
}

async fn fetch_tenant(pool: &PgPool, user_id: &Uuid) -> Result<Tenant, Denial> {
    let timezone_name: Option<(String,)> =
        sqlx::query_as("SELECT user_timezone FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let timezone: Tz = timezone_name
        .ok_or(Denial::ServerError)?
        .0
        .parse()
        .map_err(|_| Denial::ServerError)?;
    Ok(Tenant { timezone })
}

/// The pool behind the request. `None` until the binary connects, so a
/// missing pool is a 500, never a panic.
fn pool_of(state: &AppState) -> Result<PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Datetime rendering
// ---------------------------------------------------------------------------

/// Render a timestamptz exactly like DRF `DateTimeField` with the default
/// `iso-8601` format: ISO-8601 in the request's zone with `+00:00`
/// rewritten to `Z` (serializer kernel rule — DRF converts aware values
/// to the current timezone, `fields.py:1168-1231`, which `TimezoneMixin`
/// set to the actor's).
fn render_dt(dt: &DateTime<Utc>, tz: &Tz) -> String {
    crate::serializer::render_datetime_in(&dt.fixed_offset(), tz)
}

fn render_dt_opt(dt: Option<DateTime<Utc>>, tz: &Tz) -> Option<String> {
    dt.map(|dt| render_dt(&dt, tz))
}

/// Render a timestamptz exactly like DRF's `JSONEncoder` (the `.values()`
/// list rows, which bypass `DateTimeField`): the stored instant as-is,
/// `+00:00` rewritten to `Z` (`encoders.py:28-32` — no timezone
/// conversion, unlike the field path above).
fn render_dt_utc(dt: &DateTime<Utc>) -> String {
    crate::serializer::render_datetime(dt)
}

/// DRF `DateField` wire format.
fn render_date(value: &chrono::NaiveDate) -> String {
    value.format("%Y-%m-%d").to_string()
}

/// A finite `sort_order` for the wire; a non-finite one answers 500
/// (DRF renders with `allow_nan=False` under the default
/// `STRICT_JSON=True`, so `ValueError` escapes into the generic 500).
fn render_sort_order(value: f64) -> Result<f64, Denial> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Denial::ServerError)
    }
}

fn uuid_string(id: &Uuid) -> String {
    id.to_string()
}

fn uuid_string_opt(id: Option<Uuid>) -> Value {
    match id {
        Some(id) => Value::String(id.to_string()),
        None => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Issue versions (`version.py:27-74`)
// ---------------------------------------------------------------------------

/// `GET .../issues/<issue_id>/versions/`: the gate, the count, the
/// `paginate` page over the model's `-created_at` order, the 10-key rows
/// with `created_at`/`updated_at` shifted to the actor's zone. No parent
/// checks — a missing issue is an empty 200 page.
async fn version_list(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(issue_id) = path_uuid(&issue_raw) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15, 5])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    let total = version_count(&pool, &slug, &project_id, &issue_id).await?;
    let cursor = query_last(&query, "cursor");
    let page = v2_page(cursor.as_deref(), total).map_err(|_| Denial::ServerError)?;
    let mut binder = Binder::default();
    let sql = version_list_sql(&mut binder, &slug, project_id, issue_id, &page);
    let rows = sqlx::query(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(version_page_row(row, &tenant.timezone)?);
    }
    Ok(json_response(
        StatusCode::OK,
        version_envelope(&page, total, results),
    ))
}

/// `GET .../issues/<issue_id>/versions/<pk>/`: the gate, then the full
/// row through the 640 detail rendering. A miss (no row, or a row
/// outside the slug/project/issue scope) is the 404.
async fn version_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, issue_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let (Ok(issue_id), Ok(version_id)) = (path_uuid(&issue_raw), path_uuid(&pk_raw)) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15, 5])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    let mut binder = Binder::default();
    let sql = version_detail_sql(&mut binder, &slug, project_id, issue_id, version_id);
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .bind(version_id)
        .bind(project_id)
        .bind(&slug)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let body = render_version_detail(&row, &tenant.timezone)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `paginate`'s `base_queryset.count()`.
async fn version_count(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<i64, Denial> {
    let mut binder = Binder::default();
    let sql = version_count_sql(&mut binder, slug, *project_id, *issue_id);
    let row: Option<(i64,)> = sqlx::query_as(&sql)
        .bind(issue_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// One `.values(*required_fields)` row in [`VERSION_PAGE_KEYS`] order.
/// Every value renders DRF-nullable (a NULL reads `null`, never 500s);
/// `created_at`/`updated_at` shift to the actor's zone (the explicit
/// `user_timezone_converter` pass), `last_saved_at` renders as stored
/// (the JSON-encoder path converts nothing).
fn version_page_row(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<Value, Denial> {
    let id: Option<Uuid> = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let workspace_id: Option<Uuid> = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    let project_id: Option<Uuid> = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
    let issue_id: Option<Uuid> = row.try_get("issue_id").map_err(|_| Denial::ServerError)?;
    let last_saved_at: Option<DateTime<Utc>> = row
        .try_get("last_saved_at")
        .map_err(|_| Denial::ServerError)?;
    let owned_by_id: Option<Uuid> = row
        .try_get("owned_by_id")
        .map_err(|_| Denial::ServerError)?;
    let created_at: Option<DateTime<Utc>> =
        row.try_get("created_at").map_err(|_| Denial::ServerError)?;
    let updated_at: Option<DateTime<Utc>> =
        row.try_get("updated_at").map_err(|_| Denial::ServerError)?;
    let created_by_id: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|_| Denial::ServerError)?;
    let updated_by_id: Option<Uuid> = row
        .try_get("updated_by_id")
        .map_err(|_| Denial::ServerError)?;
    let last_saved_at = last_saved_at
        .as_ref()
        .map(render_dt_utc)
        .map(Value::String)
        .unwrap_or(Value::Null);
    let created_at = render_dt_opt(created_at, tz)
        .map(Value::String)
        .unwrap_or(Value::Null);
    let updated_at = render_dt_opt(updated_at, tz)
        .map(Value::String)
        .unwrap_or(Value::Null);
    // Iterate the fixture's key list so the row order is structural,
    // not a second literal that could drift from `required_fields`.
    let mut map = Map::with_capacity(VERSION_PAGE_KEYS.len());
    for &key in VERSION_PAGE_KEYS.iter() {
        let value = match key {
            "id" => uuid_string_opt(id),
            "workspace" => uuid_string_opt(workspace_id),
            "project" => uuid_string_opt(project_id),
            "issue" => uuid_string_opt(issue_id),
            "last_saved_at" => last_saved_at.clone(),
            "owned_by" => uuid_string_opt(owned_by_id),
            "created_at" => created_at.clone(),
            "updated_at" => updated_at.clone(),
            "created_by" => uuid_string_opt(created_by_id),
            "updated_by" => uuid_string_opt(updated_by_id),
            _ => unreachable!("VERSION_PAGE_KEYS"),
        };
        map.insert(key.to_owned(), value);
    }
    Ok(Value::Object(map))
}

/// The 9-key `paginate` envelope (`global_paginator.py:75-85`), in dict
/// order. `page_count` is this page's row count.
fn version_envelope(page: &V2Page, total_results: i64, results: Vec<Value>) -> String {
    let mut map = Map::with_capacity(9);
    map.insert(
        "prev_cursor".to_owned(),
        Value::String(page.prev_cursor.clone()),
    );
    map.insert("cursor".to_owned(), Value::String(page.cursor.clone()));
    map.insert(
        "next_cursor".to_owned(),
        page.next_cursor
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    map.insert(
        "prev_page_results".to_owned(),
        Value::Bool(page.prev_page_results),
    );
    map.insert(
        "next_page_results".to_owned(),
        Value::Bool(page.next_page_results),
    );
    map.insert(
        "page_count".to_owned(),
        Value::Number((results.len() as i64).into()),
    );
    map.insert(
        "total_results".to_owned(),
        Value::Number(total_results.into()),
    );
    map.insert(
        "total_pages".to_owned(),
        Value::Number(page.total_pages.into()),
    );
    map.insert("results".to_owned(), Value::Array(results));
    serde_json::to_string(&Value::Object(map)).expect("version envelope")
}

/// The full version row through the 640 detail rendering. All datetimes
/// render in the request's zone (DRF `DateTimeField`); dates render
/// bare; the UUID arrays render hyphenated strings.
fn render_version_detail(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<String, Denial> {
    let id: Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let workspace_id: Uuid = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    let project_id: Uuid = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
    let issue_id: Uuid = row.try_get("issue_id").map_err(|_| Denial::ServerError)?;
    let id = uuid_string(&id);
    let workspace = uuid_string(&workspace_id);
    let project = uuid_string(&project_id);
    let issue = uuid_string(&issue_id);
    let parent: Option<Uuid> = row.try_get("parent").map_err(|_| Denial::ServerError)?;
    let state: Option<Uuid> = row.try_get("state").map_err(|_| Denial::ServerError)?;
    let estimate_point: Option<Uuid> = row
        .try_get("estimate_point")
        .map_err(|_| Denial::ServerError)?;
    let parent = parent.map(|id| uuid_string(&id));
    let state = state.map(|id| uuid_string(&id));
    let estimate_point = estimate_point.map(|id| uuid_string(&id));
    let name: String = row.try_get("name").map_err(|_| Denial::ServerError)?;
    let priority: String = row.try_get("priority").map_err(|_| Denial::ServerError)?;
    let start_date: Option<chrono::NaiveDate> =
        row.try_get("start_date").map_err(|_| Denial::ServerError)?;
    let target_date: Option<chrono::NaiveDate> = row
        .try_get("target_date")
        .map_err(|_| Denial::ServerError)?;
    let start_date = start_date.as_ref().map(render_date);
    let target_date = target_date.as_ref().map(render_date);
    let assignees: Vec<Uuid> = row.try_get("assignees").map_err(|_| Denial::ServerError)?;
    let labels: Vec<Uuid> = row.try_get("labels").map_err(|_| Denial::ServerError)?;
    let modules: Vec<Uuid> = row.try_get("modules").map_err(|_| Denial::ServerError)?;
    let assignees: Vec<String> = assignees.iter().map(uuid_string).collect();
    let labels: Vec<String> = labels.iter().map(uuid_string).collect();
    let modules: Vec<String> = modules.iter().map(uuid_string).collect();
    let assignees_ref: Vec<&str> = assignees.iter().map(String::as_str).collect();
    let labels_ref: Vec<&str> = labels.iter().map(String::as_str).collect();
    let modules_ref: Vec<&str> = modules.iter().map(String::as_str).collect();
    let sequence_id: i32 = row
        .try_get("sequence_id")
        .map_err(|_| Denial::ServerError)?;
    let sort_order: f64 = row.try_get("sort_order").map_err(|_| Denial::ServerError)?;
    let sort_order = render_sort_order(sort_order)?;
    let completed_at: Option<DateTime<Utc>> = row
        .try_get("completed_at")
        .map_err(|_| Denial::ServerError)?;
    let archived_at: Option<chrono::NaiveDate> = row
        .try_get("archived_at")
        .map_err(|_| Denial::ServerError)?;
    let completed_at = render_dt_opt(completed_at, tz);
    let archived_at = archived_at.as_ref().map(render_date);
    let is_draft: bool = row.try_get("is_draft").map_err(|_| Denial::ServerError)?;
    let external_source: Option<String> = row
        .try_get("external_source")
        .map_err(|_| Denial::ServerError)?;
    let external_id: Option<String> = row
        .try_get("external_id")
        .map_err(|_| Denial::ServerError)?;
    let r#type: Option<Uuid> = row.try_get("type").map_err(|_| Denial::ServerError)?;
    let cycle: Option<Uuid> = row.try_get("cycle").map_err(|_| Denial::ServerError)?;
    let r#type = r#type.map(|id| uuid_string(&id));
    let cycle = cycle.map(|id| uuid_string(&id));
    let meta: Value = row.try_get("meta").map_err(|_| Denial::ServerError)?;
    let last_saved_at: DateTime<Utc> = row
        .try_get("last_saved_at")
        .map_err(|_| Denial::ServerError)?;
    let last_saved_at = render_dt(&last_saved_at, tz);
    let owned_by_id: Uuid = row
        .try_get("owned_by_id")
        .map_err(|_| Denial::ServerError)?;
    let owned_by = uuid_string(&owned_by_id);
    let created_at: Option<DateTime<Utc>> =
        row.try_get("created_at").map_err(|_| Denial::ServerError)?;
    let updated_at: Option<DateTime<Utc>> =
        row.try_get("updated_at").map_err(|_| Denial::ServerError)?;
    let created_at = render_dt_opt(created_at, tz);
    let updated_at = render_dt_opt(updated_at, tz);
    let created_by_id: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|_| Denial::ServerError)?;
    let updated_by_id: Option<Uuid> = row
        .try_get("updated_by_id")
        .map_err(|_| Denial::ServerError)?;
    let created_by = created_by_id.map(|id| uuid_string(&id));
    let updated_by = updated_by_id.map(|id| uuid_string(&id));
    let row = IssueVersionDetailRow {
        id: &id,
        workspace: &workspace,
        project: &project,
        issue: &issue,
        parent: parent.as_deref(),
        state: state.as_deref(),
        estimate_point: estimate_point.as_deref(),
        name: &name,
        priority: &priority,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        assignees: &assignees_ref,
        sequence_id,
        labels: &labels_ref,
        sort_order,
        completed_at: completed_at.as_deref(),
        archived_at: archived_at.as_deref(),
        is_draft,
        external_source: external_source.as_deref(),
        external_id: external_id.as_deref(),
        r#type: r#type.as_deref(),
        cycle: cycle.as_deref(),
        modules: &modules_ref,
        meta: &meta,
        last_saved_at: &last_saved_at,
        owned_by: &owned_by,
        created_at: created_at.as_deref(),
        updated_at: updated_at.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    let view = issue_version_detail_to_representation(&row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Description versions (`version.py:77-144`)
// ---------------------------------------------------------------------------

/// `GET .../work-items/<work_item_id>/description-versions/`: the parent
/// `Project.get` + `Issue.get`, the guest-view gate, then the `paginate`
/// page over the explicit `.order_by("-created_at")` list.
async fn desc_version_list(
    State(state): State<AppState>,
    Path((slug, project_raw, work_item_raw)): Path<(String, String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(work_item_id) = path_uuid(&work_item_raw) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15, 5])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    desc_version_parents(&pool, &slug, &project_id, &work_item_id, &user_id).await?;
    let total = desc_version_count(&pool, &slug, &project_id, &work_item_id).await?;
    let cursor = query_last(&query, "cursor");
    let page = v2_page(cursor.as_deref(), total).map_err(|_| Denial::ServerError)?;
    let mut binder = Binder::default();
    let sql = description_list_sql(&mut binder, &slug, project_id, work_item_id, &page);
    let rows = sqlx::query(&sql)
        .bind(work_item_id)
        .bind(project_id)
        .bind(&slug)
        .fetch_all(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut results = Vec::with_capacity(rows.len());
    for row in &rows {
        results.push(version_page_row(row, &tenant.timezone)?);
    }
    Ok(json_response(
        StatusCode::OK,
        version_envelope(&page, total, results),
    ))
}

/// `GET .../work-items/<work_item_id>/description-versions/<pk>/`: the
/// parents + gate, then the full row through the 640 rendering.
async fn desc_version_detail(
    State(state): State<AppState>,
    Path((slug, project_raw, work_item_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let (Ok(work_item_id), Ok(version_id)) = (path_uuid(&work_item_raw), path_uuid(&pk_raw)) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15, 5])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    desc_version_parents(&pool, &slug, &project_id, &work_item_id, &user_id).await?;
    let mut binder = Binder::default();
    let sql = description_detail_sql(&mut binder, &slug, project_id, work_item_id, version_id);
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(work_item_id)
        .bind(version_id)
        .bind(project_id)
        .bind(&slug)
        .fetch_optional(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Err(Denial::NotFound);
    };
    let body = render_desc_version_detail(&row, &tenant.timezone)?;
    Ok(json_response(StatusCode::OK, body))
}

/// The description-versions view preamble (`version.py:88-105`):
/// `Project.objects.get(pk)` (no slug scope, soft-delete scope only),
/// `Issue.objects.get(workspace, project, pk)` (soft-delete scope only
/// — triage/archived/draft rows stay visible here), then the guest-view
/// gate: an active GUEST membership + `not guest_view_all_features` +
/// a foreign `created_by` refuses 403.
async fn desc_version_parents(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    work_item_id: &Uuid,
    user_id: &Uuid,
) -> Result<(), Denial> {
    let project: Option<(bool,)> = sqlx::query_as(
        "SELECT guest_view_all_features FROM projects WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((guest_view_all,)) = project else {
        return Err(Denial::NotFound);
    };
    let issue: Option<(Option<Uuid>,)> = sqlx::query_as(
        r#"SELECT i.created_by_id FROM issues i
           JOIN workspaces w ON w.id = i.workspace_id
           WHERE i.deleted_at IS NULL AND w.slug = $1 AND i.project_id = $2 AND i.id = $3"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(work_item_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((issue_created_by,)) = issue else {
        return Err(Denial::NotFound);
    };
    let is_guest: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.role = 5 AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if is_guest.is_some() && !guest_view_all && issue_created_by != Some(*user_id) {
        return Err(Denial::GuestView);
    }
    Ok(())
}

/// `paginate`'s `base_queryset.count()` for description versions.
async fn desc_version_count(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    work_item_id: &Uuid,
) -> Result<i64, Denial> {
    let mut binder = Binder::default();
    let sql = description_count_sql(&mut binder, slug, *project_id, *work_item_id);
    let row: Option<(i64,)> = sqlx::query_as(&sql)
        .bind(work_item_id)
        .bind(project_id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.0).ok_or(Denial::ServerError)
}

/// The full description-version row through the 640 rendering
/// (`description_binary` re-encodes base64 inside the port).
fn render_desc_version_detail(row: &sqlx::postgres::PgRow, tz: &Tz) -> Result<String, Denial> {
    let id: Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let workspace_id: Uuid = row
        .try_get("workspace_id")
        .map_err(|_| Denial::ServerError)?;
    let project_id: Uuid = row.try_get("project_id").map_err(|_| Denial::ServerError)?;
    let issue_id: Uuid = row.try_get("issue_id").map_err(|_| Denial::ServerError)?;
    let id = uuid_string(&id);
    let workspace = uuid_string(&workspace_id);
    let project = uuid_string(&project_id);
    let issue = uuid_string(&issue_id);
    let description_binary: Option<Vec<u8>> = row
        .try_get("description_binary")
        .map_err(|_| Denial::ServerError)?;
    let description_html: String = row
        .try_get("description_html")
        .map_err(|_| Denial::ServerError)?;
    let description_stripped: Option<String> = row
        .try_get("description_stripped")
        .map_err(|_| Denial::ServerError)?;
    let description_json: Value = row
        .try_get("description_json")
        .map_err(|_| Denial::ServerError)?;
    let last_saved_at: DateTime<Utc> = row
        .try_get("last_saved_at")
        .map_err(|_| Denial::ServerError)?;
    let last_saved_at = render_dt(&last_saved_at, tz);
    let owned_by_id: Uuid = row
        .try_get("owned_by_id")
        .map_err(|_| Denial::ServerError)?;
    let owned_by = uuid_string(&owned_by_id);
    let created_at: Option<DateTime<Utc>> =
        row.try_get("created_at").map_err(|_| Denial::ServerError)?;
    let updated_at: Option<DateTime<Utc>> =
        row.try_get("updated_at").map_err(|_| Denial::ServerError)?;
    let created_at = render_dt_opt(created_at, tz);
    let updated_at = render_dt_opt(updated_at, tz);
    let created_by_id: Option<Uuid> = row
        .try_get("created_by_id")
        .map_err(|_| Denial::ServerError)?;
    let updated_by_id: Option<Uuid> = row
        .try_get("updated_by_id")
        .map_err(|_| Denial::ServerError)?;
    let created_by = created_by_id.map(|id| uuid_string(&id));
    let updated_by = updated_by_id.map(|id| uuid_string(&id));
    let row = IssueDescriptionVersionDetailRow {
        id: &id,
        workspace: &workspace,
        project: &project,
        issue: &issue,
        description_binary: description_binary.as_deref(),
        description_html: &description_html,
        description_stripped: description_stripped.as_deref(),
        description_json: &description_json,
        last_saved_at: &last_saved_at,
        owned_by: &owned_by,
        created_at: created_at.as_deref(),
        updated_at: updated_at.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
    };
    let view = issue_description_version_detail_to_representation(&row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Move (`move.py:17-46`)
// ---------------------------------------------------------------------------

/// Django's `DATA_UPLOAD_MAX_MEMORY_SIZE` (5 MiB): past it the
/// `RequestBodySizeLimitMiddleware` answers 413 before the view runs.
const MAX_BODY: usize = 5_242_880;

/// Move body shape for the shared negotiator: a single scalar,
/// last-value-wins; uploads are dropped (the view reads `request.data`
/// only, and no contract body is multipart).
const MOVE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// `POST .../work-items/<pk>/move/`: the ADMIN/MEMBER gate, the body
/// read, the pre-save snapshot, the 650 move driver over the pool
/// store, the creation-outbox drain, the post-commit cancel/drain,
/// the explicit 594 fire, the two enqueues, then the refreshed issue
/// in the 22-key APP read shape.
async fn move_issue(
    State(state): State<AppState>,
    Path((slug, project_raw, pk_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    req: axum::extract::Request,
) -> HandlerResult {
    let Ok(pk) = path_uuid(&pk_raw) else {
        return Ok(crate::edge::proxy(State(state), req).await);
    };
    let pool = pool_of(&state)?;
    let user_id = actor_user_id(&pool, extension).await?;
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;
    let membership = fetch_membership(&pool, &slug, &project_id, &user_id).await?;
    check_allow(&membership, &[20, 15])?;
    let tenant = fetch_tenant(&pool, &user_id).await?;
    let target_ref = read_move_target(req).await?;
    let origin = move_origin(&state)?;
    let web_base_url = move_web_base_url(&state);
    let now = Utc::now();
    // The pre-save snapshot, before the move's own write.
    let mut signal = MoveSignalSeam { pool: &pool };
    let prev = capture_prior_state(&mut signal, Some(pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let store = PoolMoveStore::new(&pool, SpanCreationDeps::from_state(&state));
    let outcome = match move_work_item_to_project(
        &store,
        &slug,
        project_id,
        pk,
        &target_ref,
        user_id,
        &origin,
        web_base_url.as_deref(),
        &now,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            store.abort().await;
            return Err(map_move_error(error));
        }
    };
    match outcome {
        MoveResult::AlreadyThere(source) => {
            // Same project: no writes at all, no signals, no enqueues —
            // just the issue in the read shape.
            let body = render_moved_issue(&pool, &source, &tenant.timezone).await?;
            Ok(json_response(StatusCode::OK, body))
        }
        MoveResult::Moved(moved) => {
            // The creation registered its `on_commit` entries first
            // (the dispatch precedes the cancel send and pod drains),
            // so its outboxes drain before the move's own post-commit
            // work. A drain failure is a 500 with the move standing.
            if let Some(outboxes) = store.take_creation_outboxes().await {
                let redis = redis_client(&state);
                let pubsub = MovePubsub {
                    pool: &pool,
                    redis: redis.as_ref(),
                    runner: &state.settings().runner,
                };
                drain_creation_outboxes(&pool, &state, outboxes, |pod_id| {
                    drain_pod_by_id(&pool, &pubsub, pod_id)
                })
                .await
                .map_err(|error| {
                    match error {
                        CreationDrainError::Dispatch(db) => {
                            tracing::warn!("move creation dispatch failed: {db}")
                        }
                        CreationDrainError::PodDrain(denial) => {
                            tracing::warn!("move creation pod drain failed: {denial:?}")
                        }
                    }
                    Denial::ServerError
                })?;
            }
            run_post_commit(&state, &pool, &moved.post_commit).await?;
            fire_after_move(
                &pool,
                pk,
                prev,
                moved.issue.row.state_id,
                moved.dispatch_immediate,
                Some(user_id),
                now,
            )
            .await?;
            for enqueue in &moved.enqueues {
                let job = pidash_jobs::queue::NewJob::new(
                    enqueue.task,
                    Value::Array(enqueue.args.clone()),
                    Value::Object(enqueue.kwargs.clone()),
                );
                // `.delay` past the commit: a broker failure is a 500
                // with the move standing, as in Django.
                pidash_jobs::queue::enqueue(&pool, &job)
                    .await
                    .map_err(|_| Denial::ServerError)?;
            }
            let body = render_moved_issue(&pool, &moved.issue.row, &tenant.timezone).await?;
            Ok(json_response(StatusCode::OK, body))
        }
    }
}

/// The move's `target_ref` (`move.py:33-39`): `request.data` coerced to
/// `{}` when it is not a dict, then `.get("project")` (missing reads
/// JSON null, which the driver blanks to the required-400).
async fn read_move_target(req: axum::extract::Request) -> Result<Value, Denial> {
    let (parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| Denial::RequestTooLarge)?;
    match shared_body::negotiate_body(&parts.headers, &bytes, &MOVE_BODY_SPEC) {
        Ok(shared_body::NegotiatedBody::Empty) => Ok(Value::Null),
        Ok(shared_body::NegotiatedBody::JsonText { text, .. }) => {
            match parse_json_text(&text) {
                Ok(parsed) => match parsed.into_object() {
                    Some(map) => Ok(map
                        .get("project")
                        .map(to_serde_publish)
                        .unwrap_or(Value::Null)),
                    None => Ok(Value::Null),
                },
                Err(JsonFail::Message(detail)) => {
                    Err(Denial::BadDetail(format!("{JSON_PARSE_PREFIX}{detail}")))
                }
                // Deep nesting: Django's `RecursionError` escapes DRF's
                // `ValueError` catch into the generic 500.
                Err(JsonFail::Recursion) => Err(Denial::ServerError),
            }
        }
        Ok(shared_body::NegotiatedBody::Form { map, .. }) => {
            Ok(map.get("project").cloned().unwrap_or(Value::Null))
        }
        Err(shared_body::BodyError::UnsupportedMediaType(message)) => {
            Err(Denial::UnsupportedMediaType(message))
        }
        Err(shared_body::BodyError::ParseDetail(message)) => Err(Denial::BadDetail(message)),
        Err(shared_body::BodyError::ServerError) => Err(Denial::ServerError),
    }
}

/// `base_host(request, is_app=True)`: `APP_BASE_URL` when set, else the
/// `WEB_URL or APP_BASE_URL` origin — unset everywhere is a 500
/// (`ImproperlyConfigured`).
fn move_origin(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    urls.app_base_url
        .clone()
        .or_else(|| urls.web_url.clone())
        .ok_or(Denial::ServerError)
}

/// `web_base_url()`: `WEB_URL`, falling back to `APP_BASE_URL`, trailing
/// slash stripped — `None` when neither is configured, so the
/// `current_instance` render omits the url.
fn move_web_base_url(state: &AppState) -> Option<String> {
    let urls = &state.settings().urls;
    urls.web_url
        .clone()
        .or_else(|| urls.app_base_url.clone())
        .map(|base| base.trim_end_matches('/').to_owned())
}

/// Map the 650 driver's failures onto the view's responses: the
/// recoverable `{"error"}` + status, the source-miss 404, the
/// resolve-miss 404 detail, anything else the 500.
fn map_move_error(error: MoveError) -> Denial {
    match error {
        MoveError::Issue(issue) => Denial::MoveRejection {
            status: issue.status_code,
            message: issue.message,
        },
        MoveError::IssueNotFound => Denial::NotFound,
        MoveError::ProjectNotFound => Denial::ProjectNotFound,
        MoveError::Store(_) => Denial::ServerError,
    }
}

/// Run the driver's post-commit actions in order (the cancel first,
/// then one drain per source pod): the cancel send is best-effort with
/// logged warnings; a drain failure propagates (rows already
/// committed — the delete-cmds precedent, mirroring `on_commit`
/// raising past the commit).
async fn run_post_commit(
    state: &AppState,
    pool: &PgPool,
    actions: &[MovePostCommit],
) -> Result<(), Denial> {
    if actions.is_empty() {
        return Ok(());
    }
    let redis = redis_client(state);
    let pubsub = MovePubsub {
        pool,
        redis: redis.as_ref(),
        runner: &state.settings().runner,
    };
    for action in actions {
        match action {
            MovePostCommit::SendCancel { runner_id, run_id } => {
                let outcome = send_project_move_cancel(&pubsub, *runner_id, *run_id).await;
                for warning in outcome.warnings {
                    tracing::warn!("{warning}");
                }
            }
            MovePostCommit::DrainPod { pod_id } => {
                drain_pod_by_id(pool, &pubsub, *pod_id).await?;
            }
        }
    }
    Ok(())
}

/// Api-crate-owned Redis client (the delete-cmds precedent): `None`
/// when `REDIS_URL` is unset, empty, or unparsable, mirroring
/// `redis_instance()` returning `None`.
fn redis_client(state: &AppState) -> Option<redis::Client> {
    state
        .settings()
        .redis
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .and_then(|url| redis::Client::open(url).ok())
}

/// Fire the state-transition handler after the move's save: the state
/// always changes here, `dispatch_immediate` is false exactly when
/// handoff runs exist, `moved_by_run` stays `None`. A handler failure
/// answers `Failed` with the verbatim log line — the move stands.
async fn fire_after_move(
    pool: &PgPool,
    issue_id: Uuid,
    prev_state_id: Option<Uuid>,
    current_state_id: Option<Uuid>,
    dispatch_immediate: bool,
    created_by: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    let mut seam = MoveSignalSeam { pool };
    let mut preflight = MovePreflight;
    let outcome = fire_state_transition(
        &mut seam,
        &mut preflight,
        &FireRequest {
            issue_id,
            prev_state_id,
            current_state_id,
            dispatch_immediate,
            moved_by_run: None,
            now,
            jitter_secs: 0.0,
            created_by,
        },
    )
    .await
    .map_err(|_| Denial::ServerError)?;
    if let FireOutcome::Failed { log_line, .. } = outcome {
        tracing::error!("{log_line}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Pool `MoveStore`
// ---------------------------------------------------------------------------

/// The 650 [`MoveStore`] over the pool: guard reads run on pool
/// checkouts, the `:173-373` span runs on one held transaction
/// (`BEGIN` at [`MoveStore::lock_source_issue`], `COMMIT` at
/// [`MoveStore::moved_issue`], `ROLLBACK` via [`PoolMoveStore::abort`]
/// on any driver error), and every statement is the 650 SQL text
/// executed verbatim.
///
/// The span is a [`DbTransaction`], borrowed from the pool, so
/// [`MoveStore::create_handoff_run`] can lend it to the D-12 creation
/// seam in-span; the creation outboxes wait on the store for the
/// handler's post-commit drain.
///
/// The mutex only bridges `&self` to the held transaction — the store
/// is per-request, so it is never contended. It is a tokio mutex so
/// the guard stays `Send` across awaits (a std guard would poison the
/// handler future's `Send` bound).
struct PoolMoveStore<'p> {
    pool: &'p PgPool,
    txn: Mutex<Option<DbTransaction<'p>>>,
    creation_deps: SpanCreationDeps,
    creation: Mutex<Option<CreationOutboxes>>,
}

impl<'p> PoolMoveStore<'p> {
    fn new(pool: &'p PgPool, creation_deps: SpanCreationDeps) -> Self {
        Self {
            pool,
            txn: Mutex::new(None),
            creation_deps,
            creation: Mutex::new(None),
        }
    }

    /// Take the creation outboxes for the post-commit drain (`None`
    /// when the move created no handoff run — every pre-743 path).
    async fn take_creation_outboxes(&self) -> Option<CreationOutboxes> {
        self.creation.lock().await.take()
    }

    /// Roll back the span when the driver failed (Django's
    /// `transaction.atomic` exit on raise). A no-op when no span is
    /// open (pre-span failures).
    async fn abort(&self) {
        let tx = self.txn.lock().await.take();
        if let Some(tx) = tx {
            if let Err(error) = tx.rollback().await {
                tracing::warn!(%error, "move store: rollback failed");
            }
        }
    }

    /// Open the span: hold one transaction on the pool.
    async fn begin_span(&self) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let tx = DbTransaction::begin(self.pool)
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        self.txn.lock().await.replace(tx);
        Ok(())
    }

    /// Close the span: `COMMIT` and release the connection.
    async fn commit_span(&self) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let tx = self
            .txn
            .lock()
            .await
            .take()
            .ok_or_else(|| StoreError("move store: commit without a span".to_owned()))?;
        tx.commit()
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        Ok(())
    }
}

fn store_error(
    error: impl std::fmt::Display,
) -> pidash_services::app_issues::issue_move::StoreError {
    pidash_services::app_issues::issue_move::StoreError(error.to_string())
}

/// Map one `SOURCE_ISSUE_SQL` / `SOURCE_ISSUE_LOCK_SQL` /
/// `MOVED_ISSUE_REFETCH_SQL` row. The int4 columns (`point`,
/// `complexity_score`, `sequence_id`) widen to the driver's i64s;
/// `type_id` stringifies (the 650 row carries the hyphenated form);
/// the `archived_at` date widens to midnight UTC (the 650 row types it
/// a datetime; the move's SQL filters it NULL in every one of these
/// reads, so the widening only ever sees `None`).
fn map_source_row(
    row: &sqlx::postgres::PgRow,
) -> Result<SourceIssueRow, pidash_services::app_issues::issue_move::StoreError> {
    use pidash_services::app_issues::issue_move::StoreError;
    let fail = |error: sqlx::Error| StoreError(error.to_string());
    let point: Option<i32> = row.try_get("point").map_err(fail)?;
    let complexity_score: i32 = row.try_get("complexity_score").map_err(fail)?;
    let sequence_id: i32 = row.try_get("sequence_id").map_err(fail)?;
    let type_id: Option<Uuid> = row.try_get("type_id").map_err(fail)?;
    let archived_at: Option<chrono::NaiveDate> = row.try_get("archived_at").map_err(fail)?;
    let archived_at = archived_at
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|naive| naive.and_utc());
    Ok(SourceIssueRow {
        id: row.try_get("id").map_err(fail)?,
        project_id: row.try_get("project_id").map_err(fail)?,
        workspace_id: row.try_get("workspace_id").map_err(fail)?,
        state_id: row.try_get("state_id").map_err(fail)?,
        parent_id: row.try_get("parent_id").map_err(fail)?,
        estimate_point_id: row.try_get("estimate_point_id").map_err(fail)?,
        assigned_pod_id: row.try_get("assigned_pod_id").map_err(fail)?,
        type_id: type_id.map(|id| id.to_string()),
        created_at: row.try_get("created_at").map_err(fail)?,
        updated_at: row.try_get("updated_at").map_err(fail)?,
        deleted_at: row.try_get("deleted_at").map_err(fail)?,
        point: point.map(i64::from),
        name: row.try_get("name").map_err(fail)?,
        description_html: row.try_get("description_html").map_err(fail)?,
        description_binary: row.try_get("description_binary").map_err(fail)?,
        priority: row.try_get("priority").map_err(fail)?,
        complexity_score: i64::from(complexity_score),
        start_date: row.try_get("start_date").map_err(fail)?,
        target_date: row.try_get("target_date").map_err(fail)?,
        sequence_id: i64::from(sequence_id),
        sort_order: row.try_get("sort_order").map_err(fail)?,
        completed_at: row.try_get("completed_at").map_err(fail)?,
        archived_at,
        is_draft: row.try_get("is_draft").map_err(fail)?,
        external_source: row.try_get("external_source").map_err(fail)?,
        external_id: row.try_get("external_id").map_err(fail)?,
        git_work_branch: row.try_get("git_work_branch").map_err(fail)?,
        created_via: row.try_get("created_via").map_err(fail)?,
        agent_executor: row.try_get("agent_executor").map_err(fail)?,
        created_by_id: row.try_get("created_by_id").map_err(fail)?,
        updated_by_id: row.try_get("updated_by_id").map_err(fail)?,
    })
}

/// Map one `HANDOFF_RUNS_LOCK_SQL` row. Unknown stored `status` /
/// `trigger` values are store failures (loud 500s, never silent
/// coercions — the 731 tolerance lives jobs-side).
fn map_handoff_row(
    row: &sqlx::postgres::PgRow,
) -> Result<HandoffRunRow, pidash_services::app_issues::issue_move::StoreError> {
    use pidash_services::app_issues::issue_move::StoreError;
    let fail = |error: sqlx::Error| StoreError(error.to_string());
    let status: String = row.try_get("status").map_err(fail)?;
    let status = AgentRunStatus::from_value(&status)
        .ok_or_else(|| StoreError(format!("move store: unknown agent_run.status {status:?}")))?;
    let trigger: String = row.try_get("trigger").map_err(fail)?;
    let trigger = AgentRunTrigger::from_value(&trigger)
        .ok_or_else(|| StoreError(format!("move store: unknown agent_run.trigger {trigger:?}")))?;
    let run_config: Option<Value> = row.try_get("run_config").map_err(fail)?;
    Ok(HandoffRunRow {
        id: row.try_get("id").map_err(fail)?,
        status,
        runner_id: row.try_get("runner_id").map_err(fail)?,
        pod_id: row.try_get("pod_id").map_err(fail)?,
        created_by_id: row.try_get("created_by_id").map_err(fail)?,
        run_config: run_config.unwrap_or(Value::Null),
        trigger,
    })
}

/// Map one `blockers::summary_sql` row.
fn map_blocker_row(
    row: &sqlx::postgres::PgRow,
) -> Result<BlockerRow, pidash_services::app_issues::issue_move::StoreError> {
    use pidash_services::app_issues::issue_move::StoreError;
    let fail = |error: sqlx::Error| StoreError(error.to_string());
    Ok(BlockerRow {
        issue_id: row.try_get("id").map_err(fail)?,
        sequence_id: row.try_get("sequence_id").map_err(fail)?,
        project_identifier: row.try_get("identifier").map_err(fail)?,
        state_name: row.try_get("name").map_err(fail)?,
        state_group: row.try_get("group").map_err(fail)?,
    })
}

/// Replace the `:issue_id` anchor placeholder the blocker builders emit
/// with the caller's `$1` bind.
fn issue_param(sql: String) -> String {
    sql.replace(":issue_id", "$1")
}

#[allow(async_fn_in_trait)]
impl MoveStore for PoolMoveStore<'_> {
    async fn source_issue(
        &self,
        slug: &str,
        project_id: Uuid,
        pk: Uuid,
    ) -> Result<Option<SourceIssueRow>, pidash_services::app_issues::issue_move::StoreError> {
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(SOURCE_ISSUE_SQL)
            .bind(slug)
            .bind(project_id)
            .bind(pk)
            .fetch_optional(self.pool)
            .await
            .map_err(store_error)?;
        row.map(|row| map_source_row(&row)).transpose()
    }

    async fn resolve_project(
        &self,
        slug: &str,
        lookup: &ProjectLookup,
    ) -> Result<Option<ProjectRow>, pidash_services::app_issues::issue_move::StoreError> {
        let row: Option<(Uuid, Uuid, String)> = match lookup {
            ProjectLookup::Pk(id) => sqlx::query_as(PROJECT_RESOLVE_BY_PK_SQL)
                .bind(id)
                .bind(slug)
                .fetch_optional(self.pool)
                .await
                .map_err(store_error)?,
            ProjectLookup::Identifier(identifier) => {
                sqlx::query_as(PROJECT_RESOLVE_BY_IDENTIFIER_SQL)
                    .bind(identifier)
                    .bind(slug)
                    .fetch_optional(self.pool)
                    .await
                    .map_err(store_error)?
            }
        };
        Ok(row.map(|(id, workspace_id, identifier)| ProjectRow {
            id,
            workspace_id,
            identifier,
        }))
    }

    async fn can_move_into_target(
        &self,
        slug: &str,
        target_project_id: Uuid,
        actor_id: Uuid,
    ) -> Result<bool, pidash_services::app_issues::issue_move::StoreError> {
        let hit: Option<(i32,)> = sqlx::query_as(MEMBER_EXISTS_SQL)
            .bind(slug)
            .bind(target_project_id)
            .bind(actor_id)
            .fetch_optional(self.pool)
            .await
            .map_err(store_error)?;
        Ok(hit.is_some())
    }

    async fn representation_workspace_slug(
        &self,
        workspace_id: Uuid,
    ) -> Result<Option<String>, pidash_services::app_issues::issue_move::StoreError> {
        let row: Option<(String,)> = sqlx::query_as(WORKSPACE_SLUG_SQL)
            .bind(workspace_id)
            .fetch_optional(self.pool)
            .await
            .map_err(store_error)?;
        Ok(row.map(|row| row.0))
    }

    async fn representation_project_identifier(
        &self,
        project_id: Uuid,
    ) -> Result<Option<String>, pidash_services::app_issues::issue_move::StoreError> {
        let row: Option<(String,)> = sqlx::query_as(PROJECT_IDENTIFIER_SQL)
            .bind(project_id)
            .fetch_optional(self.pool)
            .await
            .map_err(store_error)?;
        Ok(row.map(|row| row.0))
    }

    async fn representation_assignee_ids(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<Uuid>, pidash_services::app_issues::issue_move::StoreError> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(ASSIGNEE_IDS_SQL)
            .bind(issue_id)
            .fetch_all(self.pool)
            .await
            .map_err(store_error)?;
        Ok(rows.into_iter().map(|row| row.0).collect())
    }

    async fn representation_label_ids(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<Uuid>, pidash_services::app_issues::issue_move::StoreError> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(LABEL_IDS_SQL)
            .bind(issue_id)
            .fetch_all(self.pool)
            .await
            .map_err(store_error)?;
        Ok(rows.into_iter().map(|row| row.0).collect())
    }

    async fn representation_blocked_by(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<BlockerRow>, pidash_services::app_issues::issue_move::StoreError> {
        let rows = sqlx::query(&issue_param(summary_sql(false)))
            .bind(issue_id)
            .fetch_all(self.pool)
            .await
            .map_err(store_error)?;
        rows.iter().map(map_blocker_row).collect()
    }

    async fn representation_blocking(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<BlockerRow>, pidash_services::app_issues::issue_move::StoreError> {
        let rows = sqlx::query(&issue_param(summary_sql(true)))
            .bind(issue_id)
            .fetch_all(self.pool)
            .await
            .map_err(store_error)?;
        rows.iter().map(map_blocker_row).collect()
    }

    async fn representation_has_open_blockers(
        &self,
        issue_id: Uuid,
    ) -> Result<bool, pidash_services::app_issues::issue_move::StoreError> {
        let hit: Option<(i32,)> = sqlx::query_as(&issue_param(has_open_blockers_sql()))
            .bind(issue_id)
            .fetch_optional(self.pool)
            .await
            .map_err(store_error)?;
        Ok(hit.is_some())
    }

    async fn target_default_state(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<StateRow>, pidash_services::app_issues::issue_move::StoreError> {
        let row: Option<(Uuid,)> = sqlx::query_as(CREATE_SAVE_DEFAULT_STATE_SQL)
            .bind(target_project_id)
            .fetch_optional(self.pool)
            .await
            .map_err(store_error)?;
        Ok(row.map(|row| StateRow { id: row.0 }))
    }

    async fn target_fallback_state(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<StateRow>, pidash_services::app_issues::issue_move::StoreError> {
        let row: Option<(Uuid,)> = sqlx::query_as(CREATE_SAVE_FALLBACK_STATE_SQL)
            .bind(target_project_id)
            .fetch_optional(self.pool)
            .await
            .map_err(store_error)?;
        Ok(row.map(|row| StateRow { id: row.0 }))
    }

    async fn lock_source_issue(
        &self,
        slug: &str,
        project_id: Uuid,
        pk: Uuid,
    ) -> Result<Option<SourceIssueRow>, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        self.begin_span().await?;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: span lost".to_owned()))?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(SOURCE_ISSUE_LOCK_SQL)
            .bind(slug)
            .bind(project_id)
            .bind(pk)
            .fetch_optional(&mut **conn.inner())
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        row.map(|row| map_source_row(&row)).transpose()
    }

    async fn advisory_lock_project(
        &self,
        lock_key: i64,
    ) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: advisory lock outside a span".to_owned()))?;
        sqlx::query(CREATE_ADVISORY_LOCK_SQL)
            .bind(lock_key)
            .execute(&mut **conn.inner())
            .await
            .map(|_| ())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn lock_handoff_runs(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<HandoffRunRow>, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: handoff lock outside a span".to_owned()))?;
        let rows = sqlx::query(HANDOFF_RUNS_LOCK_SQL)
            .bind(issue_id)
            .fetch_all(&mut **conn.inner())
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        rows.iter().map(map_handoff_row).collect()
    }

    async fn max_target_sequence(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<i64>, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: sequence scan outside a span".to_owned()))?;
        let row: Option<(Option<i64>,)> = sqlx::query_as(CREATE_SEQ_SCAN_SQL)
            .bind(target_project_id)
            .fetch_optional(&mut **conn.inner())
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        Ok(row.and_then(|row| row.0))
    }

    async fn target_default_pod(
        &self,
        target_project_id: Uuid,
    ) -> Result<Option<PodRow>, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: pod lookup outside a span".to_owned()))?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(DEFAULT_FOR_PROJECT_ID_SQL)
            .bind(target_project_id)
            .fetch_optional(&mut **conn.inner())
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        row.map(|row| {
            row.try_get("id")
                .map(|id| PodRow { id })
                .map_err(|error| StoreError(error.to_string()))
        })
        .transpose()
    }

    async fn save_moved_issue(
        &self,
        issue_id: Uuid,
        fields: &MovedIssueFields,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let sequence = i32::try_from(fields.sequence_id)
            .map_err(|_| StoreError("move store: sequence_id overflows int4".to_owned()))?;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: save outside a span".to_owned()))?;
        sqlx::query(ISSUE_MOVE_UPDATE_SQL)
            .bind(fields.project_id)
            .bind(fields.workspace_id)
            .bind(sequence)
            .bind(fields.state_id)
            .bind(fields.assigned_pod_id)
            .bind(None::<Uuid>)
            .bind(None::<Uuid>)
            .bind(None::<Uuid>)
            .bind(now)
            .bind(issue_id)
            .execute(&mut **conn.inner())
            .await
            .map(|_| ())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn cancel_inert_run(
        &self,
        run_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: inert cancel outside a span".to_owned()))?;
        sqlx::query(INERT_RUN_UPDATE_SQL)
            .bind(run_id)
            .bind(now)
            .execute(&mut **conn.inner())
            .await
            .map(|_| ())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn mark_handoff_parent(
        &self,
        run_id: Uuid,
        run_config: &Value,
    ) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: handoff mark outside a span".to_owned()))?;
        sqlx::query(HANDOFF_PARENT_UPDATE_SQL)
            .bind(run_id)
            .bind(run_config)
            .execute(&mut **conn.inner())
            .await
            .map(|_| ())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn detach_old_sequences(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
    ) -> Result<u64, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: sequence detach outside a span".to_owned()))?;
        sqlx::query(SEQUENCE_DETACH_SQL)
            .bind(issue_id)
            .bind(target_project_id)
            .execute(&mut **conn.inner())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_target_sequence(
        &self,
        sequence_id: Uuid,
        issue_id: Uuid,
        sequence: i64,
        target_project_id: Uuid,
        target_workspace_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
        actor_id: Uuid,
    ) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: sequence create outside a span".to_owned()))?;
        sqlx::query(SEQUENCE_CREATE_SQL)
            .bind(sequence_id)
            .bind(now)
            .bind(now)
            .bind(actor_id)
            .bind(issue_id)
            .bind(sequence)
            .bind(target_project_id)
            .bind(target_workspace_id)
            .execute(&mut **conn.inner())
            .await
            .map(|_| ())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn prune_assignees(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: assignee prune outside a span".to_owned()))?;
        sqlx::query(ASSIGNEE_PRUNE_SQL)
            .bind(now)
            .bind(issue_id)
            .bind(target_project_id)
            .execute(&mut **conn.inner())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn repoint_assignees(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
        target_workspace_id: Uuid,
    ) -> Result<u64, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: assignee repoint outside a span".to_owned()))?;
        sqlx::query(ASSIGNEE_REPOINT_SQL)
            .bind(target_project_id)
            .bind(target_workspace_id)
            .bind(issue_id)
            .execute(&mut **conn.inner())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn soft_delete_issue_labels(
        &self,
        issue_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: label wipe outside a span".to_owned()))?;
        sqlx::query(LABEL_DELETE_SQL)
            .bind(now)
            .bind(issue_id)
            .execute(&mut **conn.inner())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn soft_delete_cycle_issues(
        &self,
        issue_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: cycle wipe outside a span".to_owned()))?;
        sqlx::query(CYCLE_DELETE_SQL)
            .bind(now)
            .bind(issue_id)
            .execute(&mut **conn.inner())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn soft_delete_module_issues(
        &self,
        issue_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: module wipe outside a span".to_owned()))?;
        sqlx::query(MODULE_DELETE_SQL)
            .bind(now)
            .bind(issue_id)
            .execute(&mut **conn.inner())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn soft_delete_issue_relations(
        &self,
        issue_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: relation wipe outside a span".to_owned()))?;
        sqlx::query(RELATION_DELETE_SQL)
            .bind(now)
            .bind(issue_id)
            .execute(&mut **conn.inner())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn detach_children(
        &self,
        issue_id: Uuid,
        target_project_id: Uuid,
    ) -> Result<u64, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: children detach outside a span".to_owned()))?;
        sqlx::query(CHILDREN_DETACH_SQL)
            .bind(issue_id)
            .bind(target_project_id)
            .execute(&mut **conn.inner())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn moved_comment_ids(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<Uuid>, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: comment ids outside a span".to_owned()))?;
        let rows: Vec<(Uuid,)> = sqlx::query_as(COMMENT_IDS_SQL)
            .bind(issue_id)
            .fetch_all(&mut **conn.inner())
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        Ok(rows.into_iter().map(|row| row.0).collect())
    }

    async fn moved_description_ids(
        &self,
        issue_id: Uuid,
    ) -> Result<Vec<Uuid>, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: description ids outside a span".to_owned()))?;
        let rows: Vec<(Uuid,)> = sqlx::query_as(DESCRIPTION_IDS_SQL)
            .bind(issue_id)
            .fetch_all(&mut **conn.inner())
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        Ok(rows.into_iter().map(|row| row.0).collect())
    }

    async fn repoint_related(
        &self,
        target: RepointTarget,
        issue_id: Uuid,
        comment_ids: &[Uuid],
        description_ids: &[Uuid],
        target_project_id: Uuid,
        target_workspace_id: Uuid,
    ) -> Result<u64, pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut guard = self.txn.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| StoreError("move store: repoint outside a span".to_owned()))?;
        let sql = repoint_sql(target);
        let mut query = sqlx::query(sql)
            .bind(target_project_id)
            .bind(target_workspace_id)
            .bind(issue_id);
        query = match target {
            RepointTarget::IssueActivity
            | RepointTarget::CommentReaction
            | RepointTarget::FileAsset => query.bind(comment_ids.to_vec()),
            RepointTarget::Description => query.bind(description_ids.to_vec()),
            _ => query,
        };
        query
            .execute(&mut **conn.inner())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn create_handoff_run(
        &self,
        issue_id: Uuid,
        parent_run: &HandoffRunRow,
        pod_id: Uuid,
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        // Lend the span to the D-12 creation seam: the create must see
        // the move's uncommitted writes (the moved issue, the cancelled
        // parent, the repoints), exactly like Python's in-atomic call
        // (`issue_move.py:353-362`).
        let tx =
            self.txn.lock().await.take().ok_or_else(|| {
                StoreError("move store: handoff create outside a span".to_owned())
            })?;
        let mut creation = handoff_store(tx, self.pool, &self.creation_deps);
        // The locked row is the parent's pre-cancel snapshot, and the
        // `RunView` projection needs the full row: re-read it in-span,
        // under the held lock. The cancel touched only
        // status/ended_at/queue_position, so this is the row Python's
        // mutated in-memory instance describes.
        let parent = creation
            .run(parent_run.id)
            .await
            .map_err(|error| StoreError(error.to_string()))?
            .ok_or_else(|| {
                StoreError(format!(
                    "move store: handoff parent {} gone under its lock",
                    parent_run.id
                ))
            })?;
        let outcome = create_project_move_handoff_run(
            &mut creation,
            &HandoffCreateRequest {
                issue_id,
                parent,
                pod_id,
                now: *now,
            },
        )
        .await;
        // The span goes back on the store before any error return, so
        // `abort` still rolls it back.
        let (tx, outboxes) = split_outboxes(creation);
        self.txn.lock().await.replace(tx);
        let _created = outcome.map_err(|error| StoreError(error.to_string()))?;
        self.creation.lock().await.replace(outboxes);
        Ok(())
    }

    async fn moved_issue(
        &self,
        pk: Uuid,
    ) -> Result<Option<MovedIssueRow>, pidash_services::app_issues::issue_move::StoreError> {
        self.commit_span().await?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(MOVED_ISSUE_REFETCH_SQL)
            .bind(pk)
            .fetch_optional(self.pool)
            .await
            .map_err(store_error)?;
        row.map(|row| {
            let workspace_slug: String = row.try_get("slug").map_err(|error| {
                pidash_services::app_issues::issue_move::StoreError(error.to_string())
            })?;
            let project_identifier: String = row.try_get("identifier").map_err(|error| {
                pidash_services::app_issues::issue_move::StoreError(error.to_string())
            })?;
            Ok(MovedIssueRow {
                row: map_source_row(&row)?,
                workspace_slug,
                project_identifier,
            })
        })
        .transpose()
    }
}

// ---------------------------------------------------------------------------
// Move signal seam (594)
// ---------------------------------------------------------------------------

/// The 594 [`EntriesSeam`] for the move's save: `state` + `prior_state_id`
/// are live (they cover the capture and the driver's non-trigger early
/// return — every gate seed, the backlog norm); the ticker/creation
/// surface answers store errors, which the fire swallows by design
/// (counter + the verbatim log line) while the move stands. The archive
/// handler's precedent, with the live lookups this path needs.
struct MoveSignalSeam<'a> {
    pool: &'a PgPool,
}

/// A creation-runtime method the move's fire reached past a ticking
/// transition without the D-11/D-12 runtime. The fire maps it to
/// `Failed` (logged); the move stands.
fn move_seam_gap<T>(method: &str) -> Result<T, CreationError> {
    Err(CreationError::Db(format!(
        "move signal seam: {method} needs the D-12 creation runtime"
    )))
}

impl CreationSeam for MoveSignalSeam<'_> {
    async fn issue(&mut self, _issue_id: Uuid) -> Result<IssueView, CreationError> {
        move_seam_gap("issue")
    }

    async fn project(&mut self, _project_id: Uuid) -> Result<ProjectView, CreationError> {
        move_seam_gap("project")
    }

    async fn state(&mut self, state_id: Option<Uuid>) -> Result<Option<StateView>, CreationError> {
        let Some(state_id) = state_id else {
            return Ok(None);
        };
        let row: Option<(Uuid, String, String)> = sqlx::query_as(STATE_SELECT_SQL)
            .bind(state_id)
            .fetch_optional(self.pool)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?;
        row.map(|(id, name, group)| StateView { id, name, group })
            .ok_or_else(|| CreationError::MissingRow("state".to_owned()))
            .map(Some)
    }

    async fn latest_prior_run(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        move_seam_gap("latest_prior_run")
    }

    async fn active_run_for(&mut self, _issue_id: Uuid) -> Result<Option<RunView>, CreationError> {
        move_seam_gap("active_run_for")
    }

    async fn run(&mut self, _run_id: Uuid) -> Result<Option<RunView>, CreationError> {
        move_seam_gap("run")
    }

    async fn runner(&mut self, _runner_id: Uuid) -> Result<Option<RunnerView>, CreationError> {
        move_seam_gap("runner")
    }

    async fn assigned_pod(&mut self, _pod_id: Uuid) -> Result<Option<PodView>, CreationError> {
        move_seam_gap("assigned_pod")
    }

    async fn default_pod_for_project(
        &mut self,
        _project_id: Uuid,
    ) -> Result<Option<PodView>, CreationError> {
        move_seam_gap("default_pod_for_project")
    }

    async fn resume_parent_run_id(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<Uuid>, CreationError> {
        move_seam_gap("resume_parent_run_id")
    }

    async fn work_item_id_for_run(&mut self, _run_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        move_seam_gap("work_item_id_for_run")
    }

    async fn lock_issue_for_handoff(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<LockedIssue>, CreationError> {
        move_seam_gap("lock_issue_for_handoff")
    }

    async fn lock_run_for_handoff(
        &mut self,
        _run_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        move_seam_gap("lock_run_for_handoff")
    }

    async fn user_flags(&mut self, _user_id: Uuid) -> Result<UserFlags, CreationError> {
        move_seam_gap("user_flags")
    }

    async fn insert_run(&mut self, _row: &NewAgentRun) -> Result<RunView, CreationError> {
        move_seam_gap("insert_run")
    }

    async fn save_prompt(
        &mut self,
        _run_id: Uuid,
        _prompt: &str,
        _manifest: &Value,
    ) -> Result<(), CreationError> {
        move_seam_gap("save_prompt")
    }

    async fn save_run_config(
        &mut self,
        _run_id: Uuid,
        _config: &Value,
    ) -> Result<(), CreationError> {
        move_seam_gap("save_run_config")
    }

    async fn execution_fields(
        &mut self,
        _req: &ExecutionRequest,
    ) -> Result<ExecutionFields, ExecutionError> {
        Err(ExecutionError::Store(CreationError::Db(
            "move signal seam: execution_fields needs the D-12 creation runtime".to_owned(),
        )))
    }

    async fn lock_cloud_creation_capacity(
        &mut self,
        _workspace_id: Uuid,
        _executor_kind: AgentExecutorKind,
        _automatic: bool,
    ) -> Result<Option<AdmissionError>, CreationError> {
        move_seam_gap("lock_cloud_creation_capacity")
    }

    fn dispatch_after_commit(&mut self, _run_id: Uuid) {}

    async fn render_bundle(
        &mut self,
        _issue_id: Uuid,
        _run_id: Uuid,
        _parent_run_id: Option<Uuid>,
        _trigger: &str,
        _created_by_id: Uuid,
    ) -> Result<RenderBundle, CreationError> {
        move_seam_gap("render_bundle")
    }

    fn extra_toolsets_schema_tool(&self) -> String {
        String::new()
    }
}

impl FinalizeAgentRunSeam for MoveSignalSeam<'_> {
    async fn finalize_failed_run(
        &mut self,
        _run_id: Uuid,
        _error_code: &str,
        _error: &str,
        _now: DateTime<Utc>,
    ) -> Result<RunView, CreationError> {
        move_seam_gap("finalize_failed_run")
    }
}

impl EntriesSeam for MoveSignalSeam<'_> {
    async fn prior_state_id(&mut self, issue_id: Uuid) -> Result<Option<Uuid>, CreationError> {
        let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(PRIOR_STATE_SELECT_SQL)
            .bind(issue_id)
            .fetch_optional(self.pool)
            .await
            .map_err(|error| CreationError::Db(error.to_string()))?;
        Ok(row.and_then(|(_, state_id)| state_id))
    }

    async fn queued_follow_up(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<RunView>, CreationError> {
        move_seam_gap("queued_follow_up")
    }

    async fn lock_ticker(
        &mut self,
        _issue_id: Uuid,
    ) -> Result<Option<IssueAgentTicker>, CreationError> {
        move_seam_gap("lock_ticker")
    }

    async fn save_ticker(
        &mut self,
        _row: &IssueAgentTicker,
        _write: ClockWrite,
    ) -> Result<(), CreationError> {
        move_seam_gap("save_ticker")
    }

    fn set_rollback(&mut self) {}

    async fn clock_policy(
        &mut self,
        _project_id: Uuid,
    ) -> Result<ProjectClockPolicy, CreationError> {
        move_seam_gap("clock_policy")
    }

    async fn binding(&mut self, _binding_id: Uuid) -> Result<BindingView, CreationError> {
        move_seam_gap("binding")
    }

    async fn scheduler_override_pod(
        &mut self,
        _pod_id: Uuid,
        _project_id: Option<Uuid>,
    ) -> Result<Option<PodView>, CreationError> {
        move_seam_gap("scheduler_override_pod")
    }

    async fn workspace(&mut self, _workspace_id: Uuid) -> Result<WorkspaceView, CreationError> {
        move_seam_gap("workspace")
    }

    async fn scheduler_row(&mut self, _scheduler_id: Uuid) -> Result<SchedulerView, CreationError> {
        move_seam_gap("scheduler_row")
    }

    async fn scheduler_override_rows(
        &mut self,
        _workspace_id: Uuid,
    ) -> Result<Vec<OverrideRow>, CreationError> {
        move_seam_gap("scheduler_override_rows")
    }

    async fn project_role_facts(
        &mut self,
        _user_id: Uuid,
        _workspace_slug: &str,
        _project_id: Uuid,
    ) -> Result<ProjectRoleFacts, CreationError> {
        move_seam_gap("project_role_facts")
    }

    async fn has_usable_llm_config(&mut self, _user_id: Uuid) -> Result<bool, CreationError> {
        move_seam_gap("has_usable_llm_config")
    }

    async fn agent_system_user(
        &mut self,
    ) -> Result<Result<Uuid, AgentUserCollisionError>, CreationError> {
        move_seam_gap("agent_system_user")
    }

    async fn insert_scheduler_run(
        &mut self,
        _row: &NewSchedulerRun,
    ) -> Result<RunView, CreationError> {
        move_seam_gap("insert_scheduler_run")
    }

    fn compose_scheduler_turn(
        &mut self,
        _context: &Value,
        _index: &OverrideIndex,
        _workspace_id: Option<&str>,
        _executor_kind: Option<&str>,
        _tool_catalog_version: i64,
    ) -> Result<RenderedTurn, String> {
        Err("move signal seam: compose_scheduler_turn needs the D-12 creation runtime".to_owned())
    }
}

/// Preflight for the move's fire: the L8 eligibility check is unmerged,
/// so any dispatch decision fails closed (the entry aborts loudly, the
/// move stands).
struct MovePreflight;

impl PreflightSeam for MovePreflight {
    async fn preflight_eligibility_or_bounce(
        &mut self,
        _issue_id: Uuid,
        _creator_id: Uuid,
        _pod_id: Uuid,
        _triggered_by: &str,
    ) -> Result<bool, CreationError> {
        move_seam_gap("preflight_eligibility_or_bounce")
    }
}

// ---------------------------------------------------------------------------
// Post-commit: cancel send + pod drain
// ---------------------------------------------------------------------------

/// The 552 [`PubsubStore`] over the pool + Redis (the delete-cmds
/// shape): same verbs, same outbox helpers, no runner-enroll
/// dependency.
struct MovePubsub<'p> {
    pool: &'p PgPool,
    redis: Option<&'p redis::Client>,
    runner: &'p RunnerSettings,
}

impl PubsubStore for MovePubsub<'_> {
    async fn enqueue_for_runner(
        &self,
        runner_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, OutboxError> {
        runner_outbox::enqueue_for_runner(self.redis, self.pool, self.runner, runner_id, message)
            .await
    }

    async fn enqueue_for_machine(
        &self,
        dev_machine_id: Uuid,
        message: &Map<String, Value>,
    ) -> Result<Option<String>, MachineOutboxError> {
        machine_outbox::enqueue_for_machine(
            self.redis,
            self.pool,
            self.runner,
            dev_machine_id,
            message,
        )
        .await
    }

    async fn active_runner_sessions(
        &self,
        runner_id: Uuid,
    ) -> Result<Vec<RunnerSession>, OutboxError> {
        let rows = sqlx::query(CLOSE_ACTIVE_SESSIONS_SQL)
            .bind(runner_id)
            .fetch_all(self.pool)
            .await?;
        rows.iter()
            .map(runner_session::runner_session_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(OutboxError::from)
    }

    async fn revoke_runner_session(
        &self,
        session_id: Uuid,
        reason: &str,
    ) -> Result<(), OutboxError> {
        sqlx::query(runner_session::REVOKE_SQL)
            .bind(Utc::now())
            .bind(reason)
            .bind(session_id)
            .execute(self.pool)
            .await
            .map(|_| ())
            .map_err(OutboxError::from)
    }

    async fn clear_session_marker(&self, session_id: Uuid) -> Result<(), OutboxError> {
        runner_outbox::clear_session_marker(self.redis, &session_id.to_string()).await
    }

    async fn publish_session_eviction(
        &self,
        runner_id: Uuid,
        old_session_id: Uuid,
        new_session_id: &str,
    ) -> Result<(), OutboxError> {
        runner_outbox::publish_session_eviction(
            self.redis,
            &runner_id.to_string(),
            Some(&old_session_id.to_string()),
            new_session_id,
        )
        .await
    }
}

/// The `drain_pod_by_id` executor (`runner/services/matcher.py`,
/// through the 552 recipe): miss → 0 with no transaction; else one
/// transaction over the idle list, one `next_for_runner` claim per
/// runner, the `ASSIGN_RUN_UPDATE_SQL` write per claim — then, past
/// the commit, one `assign` send per claim. Effects dispatch in
/// idle-list order; the log line fires only when claims happened.
///
/// A dispatch failure propagates (the drain contract: the offline
/// error must not be swallowed) — rows already committed, mirroring
/// `on_commit` raising past the commit.
async fn drain_pod_by_id(
    pool: &PgPool,
    pubsub: &MovePubsub<'_>,
    pod_id: Uuid,
) -> Result<(), Denial> {
    let pod: Option<(Uuid,)> = sqlx::query_as(DRAIN_POD_BY_ID_LOOKUP_SQL)
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if pod.is_none() {
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    let threshold = alive_threshold(Utc::now());
    let runners = sqlx::query(DRAIN_POD_IDLE_RUNNERS_SQL)
        .bind(threshold)
        .bind(pod_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut effects = Vec::new();
    for runner in &runners {
        let runner_id: Uuid = runner.try_get("id").map_err(|_| Denial::ServerError)?;
        let owner_id: Uuid = runner
            .try_get("owner_id")
            .map_err(|_| Denial::ServerError)?;
        let provisioning: String = runner
            .try_get("provisioning")
            .map_err(|_| Denial::ServerError)?;
        let visibility: i32 = runner
            .try_get("visibility")
            .map_err(|_| Denial::ServerError)?;
        let Some(next_sql) = next_for_runner_sql(&provisioning, visibility) else {
            continue;
        };
        let run = sqlx::query(&next_sql)
            .bind(pod_id)
            .bind(runner_id)
            .bind(owner_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| Denial::ServerError)?;
        let Some(run) = run else {
            continue;
        };
        let run_id: Uuid = run.try_get("id").map_err(|_| Denial::ServerError)?;
        let work_item_id: Option<Uuid> = run
            .try_get("work_item_id")
            .map_err(|_| Denial::ServerError)?;
        let prompt: String = run.try_get("prompt").map_err(|_| Denial::ServerError)?;
        let run_config: Option<Value> =
            run.try_get("run_config").map_err(|_| Denial::ServerError)?;
        let run_config = run_config
            .and_then(|config| config.as_object().cloned())
            .ok_or(Denial::ServerError)?;
        let plan = plan_assignment(&AssignmentFacts {
            run_id,
            runner_id,
            owner_id,
            work_item_id,
            prompt,
            run_config,
        });
        sqlx::query(ASSIGN_RUN_UPDATE_SQL)
            .bind(owner_id)
            .bind(runner_id)
            .bind(Utc::now())
            .bind(run_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| Denial::ServerError)?;
        effects.push(plan.after_commit);
    }
    tx.commit().await.map_err(|_| Denial::ServerError)?;
    for effect in &effects {
        let DrainEffect::SendAssign { runner_id, message } = effect;
        send_to_runner(pubsub, *runner_id, message)
            .await
            .map_err(|_| Denial::ServerError)?;
    }
    if !effects.is_empty() {
        tracing::info!("{}", drain_pod_log(&pod_id, effects.len()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Move response
// ---------------------------------------------------------------------------

/// The move's 200 body: `IssueSerializer(issue).data` over the
/// refreshed (or same-project) issue — a bare instance, so the 7
/// queryset-annotation keys omit via `SkipField` and the 639 base shape
/// renders 22 keys. Datetimes render in the request's zone (DRF
/// `DateTimeField`); `archived_at` renders a bare date; `is_synced`
/// answers the merged predicate against the live sync tables.
async fn render_moved_issue(
    pool: &PgPool,
    issue: &SourceIssueRow,
    tz: &Tz,
) -> Result<String, Denial> {
    let sort_order = render_sort_order(issue.sort_order)?;
    let completed_at = render_dt_opt(issue.completed_at, tz);
    let created_at = render_dt(&issue.created_at, tz);
    let updated_at = render_dt(&issue.updated_at, tz);
    let archived_at = issue
        .archived_at
        .as_ref()
        .map(|at| render_date(&at.date_naive()));
    let is_synced = moved_is_synced(pool, issue).await?;
    let id = uuid_string(&issue.id);
    let state_id = issue.state_id.map(|id| uuid_string(&id));
    let parent_id = issue.parent_id.map(|id| uuid_string(&id));
    let assigned_pod_id = issue.assigned_pod_id.map(|id| uuid_string(&id));
    let project_id = uuid_string(&issue.project_id);
    let estimate_point = issue.estimate_point_id.map(|id| uuid_string(&id));
    let created_by = issue.created_by_id.map(|id| uuid_string(&id));
    let updated_by = issue.updated_by_id.map(|id| uuid_string(&id));
    let sequence_id = i32::try_from(issue.sequence_id).map_err(|_| Denial::ServerError)?;
    let complexity_score =
        i32::try_from(issue.complexity_score).map_err(|_| Denial::ServerError)?;
    let start_date = issue.start_date.as_ref().map(render_date);
    let target_date = issue.target_date.as_ref().map(render_date);
    let row = IssueDetailBaseRow {
        id: &id,
        name: &issue.name,
        state_id: state_id.as_deref(),
        sort_order,
        completed_at: completed_at.as_deref(),
        estimate_point: estimate_point.as_deref(),
        priority: &issue.priority,
        complexity_score,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        sequence_id,
        project_id: &project_id,
        parent_id: parent_id.as_deref(),
        cycle_id: None,
        assigned_pod_id: assigned_pod_id.as_deref(),
        agent_executor: issue.agent_executor.as_deref(),
        module_ids: None,
        label_ids: None,
        assignee_ids: None,
        sub_issues_count: None,
        created_at: &created_at,
        updated_at: &updated_at,
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        attachment_count: None,
        link_count: None,
        is_draft: issue.is_draft,
        archived_at: archived_at.as_deref(),
        is_synced,
    };
    let view = issue_detail_base_to_representation(&row);
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

/// The move's `is_synced` (`serializers/issue.py:297-323`): the bare
/// instance carries no annotation, so the merged predicate runs — no
/// probe when `external_source` is blank, else git then github.
async fn moved_is_synced(pool: &PgPool, issue: &SourceIssueRow) -> Result<bool, Denial> {
    let blank = issue
        .external_source
        .as_deref()
        .is_none_or(|source| source.trim().is_empty());
    if blank {
        return Ok(issue_is_actively_synced(
            issue.external_source.as_deref(),
            None,
            || false,
            || false,
        ));
    }
    let git_hit: Option<(i32,)> = sqlx::query_as(GIT_ISSUE_SYNC_PROBE_SQL)
        .bind(issue.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if git_hit.is_some() {
        return Ok(issue_is_actively_synced(
            issue.external_source.as_deref(),
            None,
            || true,
            || false,
        ));
    }
    let github_hit: Option<(i32,)> = sqlx::query_as(GITHUB_ISSUE_SYNC_PROBE_SQL)
        .bind(issue.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(issue_is_actively_synced(
        issue.external_source.as_deref(),
        None,
        || false,
        || github_hit.is_some(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_django_routes() {
        assert_eq!(
            VERSIONS_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/"
        );
        assert_eq!(
            VERSION_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/"
        );
        assert_eq!(
            DESC_VERSIONS_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/"
        );
        assert_eq!(
            DESC_VERSION_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/"
        );
        assert_eq!(
            MOVE_PATH,
            "/api/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/"
        );
    }

    #[test]
    fn envelope_key_order_and_empty_page() {
        let page = v2_page(None, 0).expect("empty page");
        let body = version_envelope(&page, 0, vec![]);
        let value: Value = serde_json::from_str(&body).expect("json");
        let keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "prev_cursor",
                "cursor",
                "next_cursor",
                "prev_page_results",
                "next_page_results",
                "page_count",
                "total_results",
                "total_pages",
                "results",
            ]
        );
        assert_eq!(
            body,
            // `prev_cursor` is `"1000:-1:0"` even on the empty first page
            // (Python formats `current_page - 1` unconditionally).
            r#"{"prev_cursor":"1000:-1:0","cursor":"1000:0:0","next_cursor":null,"prev_page_results":false,"next_page_results":false,"page_count":0,"total_results":0,"total_pages":0,"results":[]}"#
        );
    }

    #[test]
    fn envelope_mid_page_cursors() {
        let page = v2_page(Some("1000:2:0"), 2500).expect("page");
        let body = version_envelope(&page, 2500, vec![Value::Bool(true)]);
        assert_eq!(
            body,
            r#"{"prev_cursor":"1000:1:0","cursor":"1000:2:0","next_cursor":null,"prev_page_results":true,"next_page_results":false,"page_count":1,"total_results":2500,"total_pages":3,"results":[true]}"#
        );
    }

    #[test]
    fn envelope_first_page_next_cursor() {
        let page = v2_page(None, 1500).expect("page");
        let body = version_envelope(&page, 1500, vec![]);
        let value: Value = serde_json::from_str(&body).expect("json");
        assert_eq!(value["next_cursor"], Value::String("1000:1:0".to_owned()));
        assert_eq!(value["next_page_results"], Value::Bool(true));
        assert_eq!(value["total_pages"], Value::Number(2.into()));
    }

    #[test]
    fn bad_cursors_answer_server_error() {
        assert!(v2_page(Some("nope"), 10).is_err());
        assert!(v2_page(Some("0:0:0"), 10).is_err());
        assert!(v2_page(Some("1000:0"), 10).is_err());
    }

    #[test]
    fn denial_bodies() {
        let (status, body) = Denial::Unauthorized.status_and_body();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, super::super::UNAUTHENTICATED_BODY);
        let (status, body) = Denial::Forbidden.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, crate::permissions::PERMISSION_DENIED_BODY);
        let (status, body) = Denial::GuestView.status_and_body();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            r#"{"error":"You are not allowed to view this issue"}"#
        );
        let (status, body) = Denial::NotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, super::super::NOT_FOUND_BODY);
        let (status, body) = Denial::ProjectNotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, super::super::PROJECT_NOT_FOUND_BODY);
        let (status, body) = Denial::BadDetail("JSON parse error - x".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, r#"{"detail":"JSON parse error - x"}"#);
        let (status, body) = Denial::MoveRejection {
            status: 409,
            message: "Target project has no default pod".to_owned(),
        }
        .status_and_body();
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body, r#"{"error":"Target project has no default pod"}"#);
        let (status, body) = Denial::RequestTooLarge.status_and_body();
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(body.contains("REQUEST_BODY_TOO_LARGE"));
        let (status, body) = Denial::UnsupportedMediaType("m".to_owned()).status_and_body();
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(body, r#"{"detail":"m"}"#);
        let (status, body) = Denial::ServerError.status_and_body();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, super::super::SERVER_ERROR_BODY);
    }

    #[test]
    fn move_rejection_statuses() {
        for (status, code) in [
            (400, StatusCode::BAD_REQUEST),
            (403, StatusCode::FORBIDDEN),
            (409, StatusCode::CONFLICT),
        ] {
            let (got, _) = Denial::MoveRejection {
                status,
                message: "x".to_owned(),
            }
            .status_and_body();
            assert_eq!(got, code);
        }
    }

    #[test]
    fn json_text_escapes_line_separators() {
        assert_eq!(
            escape_json_text("a b c"),
            "a\u{2028}b\u{2029}c"
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029")
        );
        assert_eq!(escape_json_text("a\u{2028}b\u{2029}c"), "a\\u2028b\\u2029c");
        assert_eq!(escape_json_text(r#"{"a":1}"#), r#"{"a":1}"#);
    }

    #[test]
    fn sort_order_finite_only() {
        assert_eq!(render_sort_order(65535.0).expect("finite"), 65535.0);
        assert!(render_sort_order(f64::INFINITY).is_err());
        assert!(render_sort_order(f64::NAN).is_err());
    }

    #[test]
    fn path_uuid_strictness() {
        let id = "12345678-1234-1234-1234-1234567890ab";
        assert_eq!(path_uuid(id).expect("valid").to_string(), id);
        assert!(path_uuid("12345678-1234-1234-1234-1234567890AB").is_err());
        assert!(path_uuid("123456781234123412341234567890ab").is_err());
        assert!(path_uuid("{12345678-1234-1234-1234-1234567890ab}").is_err());
        assert!(path_uuid("urn:uuid:12345678-1234-1234-1234-1234567890ab").is_err());
        assert!(path_uuid("").is_err());
        assert!(path_uuid("not-a-uuid-at-all----------------").is_err());
    }

    #[test]
    fn allow_matrix() {
        let member = |project_role, workspace_role| Membership {
            project_role,
            workspace_role,
        };
        assert!(check_allow(&member(Some(20), None), &[20, 15]).is_ok());
        assert!(check_allow(&member(Some(15), None), &[20, 15]).is_ok());
        assert!(check_allow(&member(Some(5), None), &[20, 15, 5]).is_ok());
        assert!(check_allow(&member(Some(5), None), &[20, 15]).is_err());
        assert!(check_allow(&member(Some(5), Some(20)), &[20, 15]).is_ok());
        assert!(check_allow(&member(Some(10), Some(20)), &[20, 15, 5]).is_ok());
        assert!(check_allow(&member(Some(10), Some(15)), &[20, 15, 5]).is_err());
        assert!(check_allow(&member(None, Some(20)), &[20, 15, 5]).is_err());
        assert!(check_allow(&member(None, None), &[20, 15, 5]).is_err());
    }

    #[test]
    fn move_error_mapping() {
        let denial = map_move_error(MoveError::IssueNotFound);
        assert!(matches!(denial, Denial::NotFound));
        let denial = map_move_error(MoveError::ProjectNotFound);
        assert!(matches!(denial, Denial::ProjectNotFound));
        let denial = map_move_error(MoveError::Store(
            pidash_services::app_issues::issue_move::StoreError("x".to_owned()),
        ));
        assert!(matches!(denial, Denial::ServerError));
        let denial = map_move_error(MoveError::Issue(
            pidash_services::app_issues::IssueMoveError {
                message: "m".to_owned(),
                status_code: 403,
            },
        ));
        match denial {
            Denial::MoveRejection { status, message } => {
                assert_eq!(status, 403);
                assert_eq!(message, "m");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn blocker_anchor_rewrites() {
        assert_eq!(
            issue_param("a :issue_id b :issue_id".to_owned()),
            "a $1 b $1"
        );
        let blocked = issue_param(summary_sql(false));
        let blocking = issue_param(summary_sql(true));
        let open = issue_param(has_open_blockers_sql());
        for sql in [&blocked, &blocking, &open] {
            assert!(!sql.contains(":issue_id"), "{sql}");
            assert!(sql.contains("$1"), "{sql}");
        }
    }

    #[test]
    fn page_keys_cover_both_lists() {
        assert_eq!(
            VERSION_PAGE_KEYS,
            [
                "id",
                "workspace",
                "project",
                "issue",
                "last_saved_at",
                "owned_by",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
            ]
        );
    }

    #[test]
    fn datetime_renders() {
        let utc: Tz = "UTC".parse().expect("utc");
        let dt = chrono::DateTime::parse_from_rfc3339("2026-10-04T03:04:05.123456789Z")
            .expect("dt")
            .with_timezone(&Utc);
        assert_eq!(render_dt(&dt, &utc), "2026-10-04T03:04:05.123456789Z");
        assert_eq!(render_dt_utc(&dt), "2026-10-04T03:04:05.123456789Z");
        let kolkata: Tz = "Asia/Kolkata".parse().expect("tz");
        assert_eq!(
            render_dt(&dt, &kolkata),
            "2026-10-04T08:34:05.123456789+05:30"
        );
        assert_eq!(render_dt_utc(&dt), "2026-10-04T03:04:05.123456789Z");
        assert_eq!(render_dt_opt(None, &utc), None);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 4).expect("date");
        assert_eq!(render_date(&date), "2026-10-04");
    }
}
