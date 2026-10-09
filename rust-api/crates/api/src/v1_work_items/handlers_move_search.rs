//! Move + search handlers (D-18 handlers E, PIDASHCONV-677).
//!
//! Ports `apps/api/pi_dash/api/views/issue.py` (3 units):
//!
//! * `IssueMoveAPIEndpoint.post` (`:920-937`) — move a work item to
//!   another project, with `utils/issue_move.py:122-393` as its closure:
//!   the non-dict-body `{}` coercion, the 650
//!   [`move_work_item_to_project`](pidash_services::app_issues::issue_move::move_work_item_to_project)
//!   driver behind a pool-backed [`MoveStore`], the post-commit
//!   cancel/drain, the explicit 594 state-transition fire, the two
//!   enqueues, then the S1 API read shape.
//! * `IssueSearchEndpoint.get` (`:2682-2720`) — legacy thin search at
//!   `work-items/search/` plus its deprecated `issues/` twin: raw
//!   `values()` rows (NOT the S6 `IssueSearchSerializer` — the fixture
//!   pins a numeric `sequence_id` while the serializer stringifies it),
//!   `{"issues": [...]}` envelope.
//! * `IssueAdvancedSearchEndpoint.get` (`:2779-2915`) — CLI/agent search
//!   at `work-items/search/advanced/`: manual result dicts (NOT the S6
//!   `render_advanced_result` — the view builds the dicts inline),
//!   `{"query", "count", "results"}` envelope.
//!
//! Registered by [`super::routes`] at the three
//! `apps/api/pi_dash/api/urls/work_item.py:39-42,103-111` search paths
//! (GET only) and the `urls/work_item.py:128-132` move path (POST
//! only); every other method proxies to Django through the edge
//! fallback.
//!
//! Layering (all foundation use is read-only): the move reuses the 650
//! driver (`pidash_services::app_issues::issue_move` — the ported util,
//! shared with the D-26 app surface, whose pool driver in
//! `app_issues::handlers_versions_move` this module's store mirrors)
//! plus the 594 signal entries and the D-14 drain recipe; the search SQL
//! text and param parsers live in
//! `pidash_services::v1_work_items::queries_search` (Q3, PIDASHCONV-670)
//! over `app_views_search::fts`; the advanced row assembly is
//! `queries_search::{advanced_result, advanced_envelope}`; the success
//! datetime rendering is `crate::serializer::render_datetime_in`;
//! auth/preamble/rewrite/query helpers are reused from
//! [`super::handlers_social`]. This module owns the HTTP shells: the
//! move's `ProjectEntityPermission` POST gate, the search AuthOnly gate
//! (both search views inherit `IsAuthenticated` from `BaseAPIView` and
//! declare nothing else), the `:name` → `$N` bind numbering, row
//! decoding, and the envelopes.
//!
//! Request order (preserved, not redesigned): API-key authentication
//! (anonymous 401s before any pool or database access — the shared
//! [`preamble`](super::handlers_social::preamble)); on the move path,
//! the slug→UUID rewrite (identifier misses 404 before the gate), the
//! entity gate (unknown slugs 403), timezone activation (unknown zones
//! 400 — `ZoneInfoNotFoundError` subclasses `KeyError`), then the body
//! parse (malformed JSON 400s after the gate, as `request.data` parses
//! lazily in the view) and the driver; on the search paths, the AuthOnly
//! gate (passes for every authenticated caller, so an unknown slug 200s
//! with empty rows exactly like the slug-filtered queryset missing in
//! Python), timezone activation, then the handler body. There is no
//! agent-run header check on any of these routes (no `get()`/`post()`
//! here reads `X-Pi-Dash-Run-Id`).
//!
//! Ported bugs (also listed in the PR; the Q3 numbers are the
//! `queries_search` doc list, re-stated where the handler maps them):
//!
//! * BUG-1 (Q3-1, `:2718`): legacy `?limit=` is an unguarded `int()` —
//!   garbage and negatives 500 (mapped here to 500
//!   [`SERVER_ERROR_BODY`]).
//! * BUG-2 (Q3-2, `:2709`): legacy `?project_id=` with a non-UUID 500s.
//!   The `UUIDField` `ValidationError` raises at response-render time
//!   (the `values()` queryset stays lazy through `Response(...)`), so
//!   Django answers its technical-500 page — outside `handle_exception`
//!   and with no JSON contract. Mapped here to 500
//!   [`SERVER_ERROR_BODY`] (deliberate bytes edge, unpinned).
//! * BUG-3 (Q3-5, `:2837`): advanced `?since=` well-formed-but-invalid
//!   500s (`parse_datetime`'s `ValueError` propagates past the view).
//! * BUG-4 (Q3-3): `?status=` is unvalidated — anything but
//!   `closed`/`open` behaves as `all`, silently.
//! * BUG-5 (Q3-9): legacy `?search=` is not stripped (whitespace-only
//!   searches) while advanced `?q=` is stripped.
//! * BUG-6 (Q3-10): `workspace_search` matches the exact string
//!   `"false"` — `?workspace_search=` (empty) disables the project
//!   filter.
//! * BUG-7 (Q3-7): the comment-text arm ignores `IssueComment.access`,
//!   so INTERNAL comment text can surface an issue to a member who
//!   cannot read that comment.
//!
//! Retired: BUG-8 (the 650 driver's bare `NOT (group = 'triage')`
//! conjunct dropping stateless rows) is fixed upstream by
//! PIDASHCONV-792 — the driver consts this module imports now spell
//! the NULL-tolerant form, so stateless moves behave as in Django.
//!
//! Deliberate edges (all unpinned — no fixture or contract case sends
//! them):
//!
//! * Reads run on the primary pool: no merged read handler opts into
//!   replica routing (`Pools::pool_for` has no per-handler precedent),
//!   and the contract environments configure no replica.
//! * `ts_rank` (`real`) renders through Rust `f32` shortest-roundtrip
//!   (`Display` → parse as `f64`), which matches Postgres 12+
//!   `float4out` shortest output up to the algorithm's last-digit
//!   choices on adversarial values.
//! * Huge legacy limits reach Postgres as full-precision literals and
//!   500 on int8 overflow; Python raises the same overflow at render
//!   time (HTML page) while Rust 500s JSON — same status, no shared
//!   body contract.
//! * The move's `now` is drawn once per request (Python draws it inside
//!   the transaction, after the locks); the 650 driver takes it frozen
//!   (the D-26 twin documents the same edge).
//! * The immediate-handoff insert (a move whose handoff runs are all
//!   QUEUED/PAUSED) answers an explicit 500 naming the D-12 creation
//!   seam gap; unreachable in every automated gate (seeds carry no
//!   agent runs) and loud when hit. The D-26 twin has since adopted
//!   the API-side seam (`create_project_move_handoff_run`,
//!   PIDASHCONV-743) — adopting it here is a follow-up, not this
//!   port (no gate covers the path).
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/` —
//! `search`, `search_advanced`, the deprecated `search` twin, and the
//! `move` SQL + DB record).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use tokio::sync::Mutex;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::pool::PoolConnection;
use sqlx::{PgPool, Postgres, Row};
use uuid::Uuid;

use pidash_auth::permissions::membership::ProjectRoleFacts;
use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
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
use pidash_services::app_issues::issue_move::{
    CREATE_ADVISORY_LOCK_SQL, CREATE_SAVE_DEFAULT_STATE_SQL, CREATE_SAVE_FALLBACK_STATE_SQL,
    CREATE_SEQ_SCAN_SQL, DEFAULT_FOR_PROJECT_ID_SQL,
};
use pidash_services::app_issues::{
    move_work_item_to_project, repoint_sql, send_project_move_cancel, MoveError, MovePostCommit,
    MoveResult, MoveStore, MovedIssueFields, MovedIssueRow, PodRow, ProjectRow, SourceIssueRow,
    StateRow, ASSIGNEE_IDS_SQL, ASSIGNEE_PRUNE_SQL, ASSIGNEE_REPOINT_SQL, CHILDREN_DETACH_SQL,
    COMMENT_IDS_SQL, CYCLE_DELETE_SQL, DESCRIPTION_IDS_SQL, HANDOFF_PARENT_UPDATE_SQL,
    HANDOFF_RUNS_LOCK_SQL, INERT_RUN_UPDATE_SQL, ISSUE_MOVE_UPDATE_SQL, LABEL_DELETE_SQL,
    LABEL_IDS_SQL, MEMBER_EXISTS_SQL, MODULE_DELETE_SQL, MOVED_ISSUE_REFETCH_SQL,
    PROJECT_IDENTIFIER_SQL, PROJECT_RESOLVE_BY_IDENTIFIER_SQL, PROJECT_RESOLVE_BY_PK_SQL,
    RELATION_DELETE_SQL, SEQUENCE_CREATE_SQL, SEQUENCE_DETACH_SQL, SOURCE_ISSUE_LOCK_SQL,
    SOURCE_ISSUE_SQL, WORKSPACE_SLUG_SQL,
};
use pidash_services::app_issues::{HandoffRunRow, RepointTarget};
use pidash_services::dispatch::policy::UserFlags;
use pidash_services::orchestration::blockers::{has_open_blockers_sql, summary_sql, BlockerRow};
use pidash_services::orchestration::clock::{ClockWrite, ProjectClockPolicy};
use pidash_services::orchestration::creation::{
    AdmissionError, CreationError, CreationSeam, ExecutionError, ExecutionFields, ExecutionRequest,
    FinalizeAgentRunSeam, IssueView, LockedIssue, NewAgentRun, PodView, ProjectView, RenderBundle,
    RenderedTurn, RunView, RunnerView, StateView, STATE_SELECT_SQL,
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
use pidash_services::v1_work_items::queries_search as q;
use pidash_services::v1_work_items::shape_issue::{
    BlockerSummary, IssueRow as ApiIssueRow, RepresentationInput, SummaryItem,
};
use pidash_types::dispatch::AgentExecutorKind;

use crate::serializer::render_datetime_in;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json_cpython::{
    parse_json_text, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};

use super::handlers_social::{preamble, query_last, rewrite_project_id, QueryMap};
use super::perms::{gate_for, V1WorkItemsRoute};

/// `handle_exception`'s generic branch (`api/views/base.py:166-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// Handler failure with its exact status + body.
#[derive(Debug, PartialEq, Eq)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (no `X-Api-Key` header).
    Unauthorized,
    /// 403, invalid/expired/inactive API or machine token.
    InvalidToken,
    /// 404, `{"detail":"Project not found"}` — the advanced
    /// identifier-form `?project=` miss (`Project.resolve` raises
    /// `Http404("Project not found")`, `db/models/project.py:213-217`;
    /// DRF propagates the args through `NotFound`).
    ProjectNotFound,
    /// 400, pre-rendered `{"error": ...}` (view-inline: bad `sort`,
    /// malformed `since`).
    BadError(String),
    /// 403, the DRF-default `PermissionDenied` body (no D-18 guard class
    /// sets `message`) — the move's `ProjectEntityPermission` POST gate.
    Forbidden,
    /// 404, `handle_exception`'s `ObjectDoesNotExist` branch — the move's
    /// source-issue miss (`Issue.DoesNotExist` propagates, `:140`/`:182`).
    NotFound,
    /// 400, `{"detail": ...}` (DRF `ParseError`: malformed move JSON).
    BadDetail(String),
    /// 400/403/409, `{"error": ...}` (the move's `IssueMoveError` mapping).
    MoveRejection { status: u16, message: String },
    /// 415, `{"detail": ...}` (DRF `UnsupportedMediaType`).
    UnsupportedMediaType(String),
    /// 413, the `RequestBodySizeLimitMiddleware` JSON body past 5 MiB
    /// (a plain `JsonResponse`, so default separators — with spaces).
    RequestTooLarge,
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                super::perms::UNAUTHENTICATED_BODY.to_owned(),
            ),
            Denial::InvalidToken => (
                StatusCode::FORBIDDEN,
                r#"{"detail":"Given API token is not valid"}"#.to_owned(),
            ),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned(),
            ),
            Denial::BadError(body) => (StatusCode::BAD_REQUEST, body.clone()),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::perms::CLASS_DENIAL_BODY.to_owned(),
            ),
            Denial::NotFound => (
                StatusCode::NOT_FOUND,
                r#"{"error":"The requested resource does not exist."}"#.to_owned(),
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
                r#"{"error": "REQUEST_BODY_TOO_LARGE", "detail": "The size of the request body exceeds the maximum allowed size."}"#.to_owned(),
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
        json_response(status, body)
    }
}

impl From<super::handlers_social::Denial> for Denial {
    fn from(denial: super::handlers_social::Denial) -> Self {
        use super::handlers_social::Denial as Social;
        match denial {
            Social::Unauthorized => Denial::Unauthorized,
            Social::InvalidToken => Denial::InvalidToken,
            // `rewrite_project_id` fails the identifier miss
            // (`ProjectNotFound`, byte-identical here); `preamble` only
            // fails authentication or the workspace lookup. Every other
            // social variant is unreachable from either.
            Social::ProjectNotFound => Denial::ProjectNotFound,
            _ => Denial::ServerError,
        }
    }
}

/// Render a JSON response with exact bytes and status. DRF's `JSONRenderer`
/// post-pass escapes U+2028/U+2029 (`rest_framework/renderers.py`); the
/// `app_project` `escape_u2028` precedent, applied to every JSON body here.
fn json_response(status: StatusCode, body: String) -> Response {
    let body = body
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

/// Quote one JSON string value (DRF `detail`/`error` wrappers).
fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
}

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_work_items move_search database failure");
    Denial::ServerError
}

// ---------------------------------------------------------------------------
// Cutover wiring
// ---------------------------------------------------------------------------

/// Route registration is the cutover granularity (the pilot `owned()`
/// pattern shared with `v1_projects`): the owned methods serve from Rust,
/// every other method on the path proxies to Django so its 405-after-auth
/// and metadata responses are preserved byte for byte.
fn owned(
    router: axum::routing::MethodRouter<AppState>,
    methods: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = router;
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
        if !methods.contains(&method) {
            router = match method {
                "GET" => router.get(crate::edge::proxy),
                "POST" => router.post(crate::edge::proxy),
                "PUT" => router.put(crate::edge::proxy),
                "PATCH" => router.patch(crate::edge::proxy),
                "DELETE" => router.delete(crate::edge::proxy),
                "HEAD" => router.head(crate::edge::proxy),
                _ => router.options(crate::edge::proxy),
            };
        }
    }
    // DRF runs `initial()` (auth → permissions) before its method check, so
    // exotic methods (TRACE et al.) answer 401/403/405 JSON. The fallback
    // proxies them with the original request (the `app_issues` precedent).
    router.fallback(crate::edge::proxy)
}

/// The search paths own GET only (`urls/work_item.py:39-42,103-106`,
/// `as_view(http_method_names=["get"])`) — both the `work-items/`
/// spelling and the deprecated `issues/` twin, which share the view
/// class and therefore the handler.
pub fn owned_search(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET"])
}

/// The advanced-search path owns GET only (`urls/work_item.py:108-111`).
pub fn owned_search_advanced(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET"])
}

// ---------------------------------------------------------------------------
// Timezone
// ---------------------------------------------------------------------------

/// Activate the actor's rendering timezone (`TimezoneMixin.initial` runs
/// after `super().initial()`). A missing zone defaults to UTC; an unknown
/// zone name 400s: `zoneinfo.ZoneInfo` raises `ZoneInfoNotFoundError`,
/// which subclasses `KeyError`, so `handle_exception` answers the
/// `KeyError` branch (`api/views/base.py:160-164`), never a 500. An EMPTY
/// zone 500s: `ZoneInfo('')` raises `ValueError` (not `KeyError`), which
/// falls through to the generic 500 (`api/views/base.py:166-171`).
fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Denial> {
    match timezone {
        None => Ok(chrono_tz::UTC),
        Some("") => Err(Denial::ServerError),
        Some(zone) => zone.parse().map_err(|_| {
            Denial::BadError(r#"{"error":"The required key does not exist."}"#.to_owned())
        }),
    }
}

// ---------------------------------------------------------------------------
// Legacy search
// ---------------------------------------------------------------------------

/// A numbered legacy statement with its binds in `$N` order.
#[derive(Debug, Clone, PartialEq)]
struct LegacyStatement {
    sql: String,
    project_id: Option<Uuid>,
    fts: String,
    seq: Vec<i64>,
    like: String,
}

/// Compose the legacy statement (`views/issue.py:2696-2718`): the Q3 text
/// with `:name` placeholders numbered to `$N` (`$1` member, `$2` slug,
/// `$3` project when the filter applies, then fts/seq/like in order).
/// `LegacyProject::InvalidUuid` 500s (BUG-2: the `ValidationError` raises
/// at response-render time, outside `handle_exception`, so Django answers
/// its technical-500 page — no JSON contract — and Rust answers the
/// generic JSON 500).
fn legacy_statement(
    query: &str,
    workspace_search: Option<&str>,
    project_id: Option<&str>,
    limit: Option<&str>,
) -> Result<LegacyStatement, Denial> {
    let project = q::legacy_project_filter(workspace_search, project_id);
    if matches!(project, q::LegacyProject::InvalidUuid) {
        return Err(Denial::ServerError);
    }
    let limit_sql = q::legacy_limit_sql(limit).map_err(|_| Denial::ServerError)?;
    let sql = q::legacy_search_sql(query, project, &limit_sql);
    let sql = sql.replace(":member_id", "$1").replace(":slug", "$2");
    let (sql, numbered_project) = match project {
        q::LegacyProject::Filter(id) => (
            sql.replace(":project_id", "$3")
                .replace(":fts", "$4")
                .replace(":seq", "$5")
                .replace(":like", "$6"),
            Some(id),
        ),
        // `InvalidUuid` returned above; `None` carries no `:project_id`
        // occurrence, so only the text binds are numbered.
        _ => (
            sql.replace(":fts", "$3")
                .replace(":seq", "$4")
                .replace(":like", "$5"),
            None,
        ),
    };
    let binds = q::search_text_binds(query);
    Ok(LegacyStatement {
        sql,
        project_id: numbered_project,
        fts: binds.fts,
        seq: binds.seq,
        like: binds.like,
    })
}

/// Fetch the legacy rows. One concrete bind chain per project-filter arm
/// (each `.bind` changes the query type, so the conditional bind cannot
/// sit mid-chain).
async fn fetch_legacy_rows(
    pool: &PgPool,
    slug: &str,
    member_id: &Uuid,
    stmt: &LegacyStatement,
) -> Result<Vec<sqlx::postgres::PgRow>, Denial> {
    if let Some(project_id) = stmt.project_id {
        sqlx::query(&stmt.sql)
            .bind(member_id)
            .bind(slug)
            .bind(project_id)
            .bind(&stmt.fts)
            .bind(&stmt.seq)
            .bind(&stmt.like)
            .fetch_all(pool)
            .await
            .map_err(|error| db_error(error, "search-legacy"))
    } else {
        sqlx::query(&stmt.sql)
            .bind(member_id)
            .bind(slug)
            .bind(&stmt.fts)
            .bind(&stmt.seq)
            .bind(&stmt.like)
            .fetch_all(pool)
            .await
            .map_err(|error| db_error(error, "search-legacy"))
    }
}

/// One decoded legacy `values()` row, in
/// [`LEGACY_VALUES_KEYS`](q::LEGACY_VALUES_KEYS) order.
struct LegacyRow {
    name: String,
    id: Uuid,
    sequence_id: i32,
    project_identifier: String,
    project_id: Uuid,
    workspace_slug: String,
}

fn decode_legacy_row(row: &sqlx::postgres::PgRow) -> Result<LegacyRow, Denial> {
    Ok(LegacyRow {
        name: row.try_get("name").map_err(|_| Denial::ServerError)?,
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        sequence_id: row
            .try_get("sequence_id")
            .map_err(|_| Denial::ServerError)?,
        project_identifier: row.try_get("identifier").map_err(|_| Denial::ServerError)?,
        project_id: row.try_get("project_id").map_err(|_| Denial::ServerError)?,
        workspace_slug: row.try_get("slug").map_err(|_| Denial::ServerError)?,
    })
}

/// Render one legacy row: the RAW `values()` mapping (`:2711-2718`) in
/// call order — notably NOT the S6 `IssueSearchSerializer` (which
/// stringifies `sequence_id`; the fixture pins the numeric form).
fn render_legacy_row(row: &LegacyRow) -> Map<String, Value> {
    let mut out = Map::with_capacity(6);
    out.insert("name".to_owned(), Value::String(row.name.clone()));
    out.insert("id".to_owned(), Value::String(row.id.to_string()));
    out.insert(
        "sequence_id".to_owned(),
        Value::Number(row.sequence_id.into()),
    );
    out.insert(
        "project__identifier".to_owned(),
        Value::String(row.project_identifier.clone()),
    );
    out.insert(
        "project_id".to_owned(),
        Value::String(row.project_id.to_string()),
    );
    out.insert(
        "workspace__slug".to_owned(),
        Value::String(row.workspace_slug.clone()),
    );
    out
}

/// `GET .../search/` (`views/issue.py:2682-2720`): both the `work-items/`
/// spelling and the deprecated `issues/` twin, which share the view
/// class and therefore this handler.
pub async fn get_search(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    match search_inner(&state, &headers, &slug, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn search_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    // AuthOnly gate (`gate_for(Search) == AuthOnly`): the base
    // `IsAuthenticated` already passed, so the authenticated caller
    // reaches the handler body — an unknown slug 200s with empty rows,
    // exactly like the slug-filtered queryset missing in Python.
    // Timezone activation still runs (order parity): an unknown zone
    // 400s even though no datetime renders on this path.
    activate_timezone(pre.actor.timezone.as_deref())?;
    let raw_query = query_last(query, "search");
    if !q::legacy_query_present(raw_query.as_deref()) {
        return Ok(json_response(
            StatusCode::OK,
            q::EMPTY_SEARCH_BODY.to_owned(),
        ));
    }
    let text = raw_query.unwrap_or_default();
    let stmt = legacy_statement(
        &text,
        query_last(query, "workspace_search").as_deref(),
        query_last(query, "project_id").as_deref(),
        query_last(query, "limit").as_deref(),
    )?;
    let rows = fetch_legacy_rows(&pre.pool, slug, &pre.actor.id, &stmt).await?;
    let mut rendered = Vec::with_capacity(rows.len());
    for row in &rows {
        rendered.push(Value::Object(render_legacy_row(&decode_legacy_row(row)?)));
    }
    let mut envelope = Map::with_capacity(1);
    envelope.insert("issues".to_owned(), Value::Array(rendered));
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&envelope).map_err(|_| Denial::ServerError)?,
    ))
}

// ---------------------------------------------------------------------------
// Advanced search
// ---------------------------------------------------------------------------

/// A numbered advanced statement with its binds in `$N` order. The limit
/// is inlined as a `LIMIT` literal (a computed `1..=50` integer, exactly
/// like Django's rendered literal); `limit` is kept for the tests.
#[derive(Debug, Clone, PartialEq)]
struct AdvancedStatement {
    sql: String,
    project_id: Option<Uuid>,
    since: Option<DateTime<Utc>>,
    fts: String,
    seq: Vec<i64>,
    like: String,
    limit: i64,
}

/// Resolve the identifier-form `?project=` (`Project.resolve`,
/// `db/models/project.py:208-217`): strip + upper before the btree
/// equality lookup. A miss 404s before any statement runs; the UUID
/// form never 404s (a miss yields empty rows — Q3 quirk 4).
async fn resolve_advanced_project(pool: &PgPool, slug: &str, text: &str) -> Result<Uuid, Denial> {
    let sql = format!(
        "SELECT \"projects\".\"id\" FROM \"projects\" {} WHERE ({}) {}",
        q::resolve_identifier_joins_sql(),
        q::resolve_identifier_where_sql()
            .replace(":slug", "$1")
            .replace(":identifier", "$2"),
        q::resolve_identifier_tail_sql(),
    );
    let normalized = q::normalize_project_identifier(text);
    let id: Option<Uuid> = sqlx::query_scalar(&sql)
        .bind(slug)
        .bind(&normalized)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "search-advanced-project"))?;
    id.ok_or(Denial::ProjectNotFound)
}

/// `float(row["_rank"] or 0.0)` (`:2899`): Postgres 12+ `float4out`
/// spells the shortest roundtrip for the `real`; Rust `f32` `Display`
/// is the same shortest spelling, parsed back as `f64` so serde emits
/// the identical digits (`Value::from(f64)` maps the impossible
/// non-finite case to `null`, never panics).
fn rank_f64(rank: Option<f32>) -> f64 {
    rank.map(|value| value.to_string().parse::<f64>().unwrap_or(0.0))
        .unwrap_or(0.0)
}

/// Compose the advanced statement (`views/issue.py:2809-2876`): params in
/// source order (query → sort → limit → project → status → since), then
/// the Q3 text with `:name` placeholders numbered to `$N` (`$1` member,
/// `$2` slug, then the project/since arms in application order, then
/// fts/seq/like). The `LIMIT` is the clamped literal.
async fn advanced_statement(
    pool: &PgPool,
    slug: &str,
    query: &QueryMap,
) -> Result<(String, q::Sort, AdvancedStatement), Denial> {
    let Some(text) = q::parse_advanced_query(query_last(query, "q").as_deref()) else {
        // Unreachable: the caller early-returns `EMPTY_ADVANCED_BODY` on
        // the same predicate before composing.
        return Err(Denial::ServerError);
    };
    let sort = q::parse_sort(query_last(query, "sort").as_deref())
        .map_err(|error| Denial::BadError(error.body()))?;
    let limit = q::advanced_limit(query_last(query, "limit").as_deref());
    let project_id = match q::parse_project_param(query_last(query, "project").as_deref()) {
        q::ProjectRef::None => None,
        q::ProjectRef::Uuid(id) => Some(id),
        q::ProjectRef::Identifier(text) => Some(resolve_advanced_project(pool, slug, text).await?),
    };
    let status = q::parse_status(query_last(query, "status").as_deref());
    let since = match q::parse_since(query_last(query, "since").as_deref()) {
        q::SinceOutcome::NoFilter => None,
        q::SinceOutcome::At(at) => Some(at),
        q::SinceOutcome::BadFormat(error) => return Err(Denial::BadError(error.body())),
        q::SinceOutcome::InvalidValue(_) => return Err(Denial::ServerError),
    };
    let sql = q::advanced_search_sql(&q::AdvancedParts {
        query: &text,
        project_uuid_filter: project_id.is_some(),
        status,
        since_filter: since.is_some(),
        sort,
    });
    let sql = number_advanced_placeholders(&sql, project_id.is_some(), since.is_some(), limit);
    let binds = q::search_text_binds(&text);
    Ok((
        text,
        sort,
        AdvancedStatement {
            sql,
            project_id,
            since,
            fts: binds.fts,
            seq: binds.seq,
            like: binds.like,
            limit,
        },
    ))
}

/// Number the `:name` placeholders of an advanced statement to `$N`
/// (`$1` member, `$2` slug, then the project/since arms in application
/// order, then fts/seq/like). The `LIMIT` becomes the clamped literal.
fn number_advanced_placeholders(
    sql: &str,
    project_filter: bool,
    since_filter: bool,
    limit: i64,
) -> String {
    let mut next: u32 = 3;
    let mut sql = sql.replace(":member_id", "$1").replace(":slug", "$2");
    if project_filter {
        sql = sql.replace(":project_id", &format!("${next}"));
        next += 1;
    }
    if since_filter {
        sql = sql.replace(":since", &format!("${next}"));
        next += 1;
    }
    let (fts_n, seq_n, like_n) = (next, next + 1, next + 2);
    sql = sql
        .replace(":fts", &format!("${fts_n}"))
        .replace(":seq", &format!("${seq_n}"))
        .replace(":like", &format!("${like_n}"));
    sql.replace("LIMIT :limit", &format!("LIMIT {limit}"))
}

/// Fetch the advanced rows. One concrete bind chain per
/// project × since arm (each `.bind` changes the query type, so the
/// conditional binds cannot sit mid-chain).
async fn fetch_advanced_rows(
    pool: &PgPool,
    slug: &str,
    member_id: &Uuid,
    stmt: &AdvancedStatement,
) -> Result<Vec<sqlx::postgres::PgRow>, Denial> {
    match (stmt.project_id, stmt.since) {
        (Some(project_id), Some(since)) => sqlx::query(&stmt.sql)
            .bind(member_id)
            .bind(slug)
            .bind(project_id)
            .bind(since)
            .bind(&stmt.fts)
            .bind(&stmt.seq)
            .bind(&stmt.like)
            .fetch_all(pool)
            .await
            .map_err(|error| db_error(error, "search-advanced")),
        (Some(project_id), None) => sqlx::query(&stmt.sql)
            .bind(member_id)
            .bind(slug)
            .bind(project_id)
            .bind(&stmt.fts)
            .bind(&stmt.seq)
            .bind(&stmt.like)
            .fetch_all(pool)
            .await
            .map_err(|error| db_error(error, "search-advanced")),
        (None, Some(since)) => sqlx::query(&stmt.sql)
            .bind(member_id)
            .bind(slug)
            .bind(since)
            .bind(&stmt.fts)
            .bind(&stmt.seq)
            .bind(&stmt.like)
            .fetch_all(pool)
            .await
            .map_err(|error| db_error(error, "search-advanced")),
        (None, None) => sqlx::query(&stmt.sql)
            .bind(member_id)
            .bind(slug)
            .bind(&stmt.fts)
            .bind(&stmt.seq)
            .bind(&stmt.like)
            .fetch_all(pool)
            .await
            .map_err(|error| db_error(error, "search-advanced")),
    }
}

/// Owned advanced row strings: the `AdvancedRow` borrows these across
/// the `advanced_result` render.
struct AdvancedDecoded {
    id: String,
    name: String,
    headline: Option<String>,
    created_at: String,
    updated_at: String,
    completed_at: Option<String>,
    state_name: Option<String>,
    state_group: Option<String>,
    project_id: String,
    project_identifier: String,
    project_name: String,
    workspace_slug: String,
}

/// Decode one advanced `values()` row by POSITION (`:2860-2875` order —
/// `name` repeats across issues/states/projects, so by-name decoding
/// would collide). Datetimes render in the request timezone
/// (`TimezoneMixin`); `_rank` is `ts_rank`'s `real`.
fn decode_advanced_row(
    row: &sqlx::postgres::PgRow,
    tz: &Tz,
) -> Result<(AdvancedDecoded, i32, Option<f32>), Denial> {
    let id: Uuid = row.try_get(0).map_err(|_| Denial::ServerError)?;
    let sequence_id: i32 = row.try_get(1).map_err(|_| Denial::ServerError)?;
    let name: String = row.try_get(2).map_err(|_| Denial::ServerError)?;
    let headline: Option<String> = row.try_get(3).map_err(|_| Denial::ServerError)?;
    let rank: Option<f32> = row.try_get(4).map_err(|_| Denial::ServerError)?;
    let created_at: DateTime<Utc> = row.try_get(5).map_err(|_| Denial::ServerError)?;
    let updated_at: DateTime<Utc> = row.try_get(6).map_err(|_| Denial::ServerError)?;
    let completed_at: Option<DateTime<Utc>> = row.try_get(7).map_err(|_| Denial::ServerError)?;
    let state_name: Option<String> = row.try_get(8).map_err(|_| Denial::ServerError)?;
    let state_group: Option<String> = row.try_get(9).map_err(|_| Denial::ServerError)?;
    let project_id: Uuid = row.try_get(10).map_err(|_| Denial::ServerError)?;
    let project_identifier: String = row.try_get(11).map_err(|_| Denial::ServerError)?;
    let project_name: String = row.try_get(12).map_err(|_| Denial::ServerError)?;
    let workspace_slug: String = row.try_get(13).map_err(|_| Denial::ServerError)?;
    Ok((
        AdvancedDecoded {
            id: id.to_string(),
            name,
            headline,
            created_at: render_datetime_in(&created_at, tz),
            updated_at: render_datetime_in(&updated_at, tz),
            completed_at: completed_at.as_ref().map(|dt| render_datetime_in(dt, tz)),
            state_name,
            state_group,
            project_id: project_id.to_string(),
            project_identifier,
            project_name,
            workspace_slug,
        },
        sequence_id,
        rank,
    ))
}

/// `GET .../search/advanced/` (`views/issue.py:2779-2915`).
pub async fn get_search_advanced(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    headers: HeaderMap,
) -> Response {
    match search_advanced_inner(&state, &headers, &slug, &query).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn search_advanced_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    query: &QueryMap,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    // AuthOnly gate (see `search_inner`): the authenticated caller
    // reaches the handler body for any slug.
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    if q::parse_advanced_query(query_last(query, "q").as_deref()).is_none() {
        return Ok(json_response(
            StatusCode::OK,
            q::EMPTY_ADVANCED_BODY.to_owned(),
        ));
    }
    let (text, _sort, stmt) = advanced_statement(&pre.pool, slug, query).await?;
    let rows = fetch_advanced_rows(&pre.pool, slug, &pre.actor.id, &stmt).await?;
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let mut rendered = Vec::with_capacity(rows.len());
    for row in &rows {
        let (decoded, sequence_id, rank) = decode_advanced_row(row, &tz)?;
        let url = pidash_services::v1_work_items::shape_issue::issue_url(
            web_base.as_deref(),
            Some(&decoded.workspace_slug),
            Some(&decoded.project_identifier),
            Some(i64::from(sequence_id)),
        );
        rendered.push(q::advanced_result(
            &q::AdvancedRow {
                id: &decoded.id,
                sequence_id,
                name: &decoded.name,
                headline: decoded.headline.as_deref(),
                rank: Some(rank_f64(rank)),
                created_at: &decoded.created_at,
                updated_at: &decoded.updated_at,
                completed_at: decoded.completed_at.as_deref(),
                state_name: decoded.state_name.as_deref(),
                state_group: decoded.state_group.as_deref(),
                project_id: &decoded.project_id,
                project_identifier: &decoded.project_identifier,
                project_name: &decoded.project_name,
                workspace_slug: &decoded.workspace_slug,
            },
            url.as_deref(),
        ));
    }
    let envelope = q::advanced_envelope(&text, rendered);
    Ok(json_response(
        StatusCode::OK,
        serde_json::to_string(&envelope).map_err(|_| Denial::ServerError)?,
    ))
}

// ---------------------------------------------------------------------------
// Tests (pure replays — no database; the live contract suites in
// `rust-api/contract-tests/v1_work_items/` cover the handlers end to end)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Move
// ---------------------------------------------------------------------------

/// The move path owns POST only (`urls/work_item.py:128-132`,
/// `as_view(http_method_names=["post"])`). No deprecated twin exists for
/// this route (the `old_url_patterns` list carries no move entry).
pub fn owned_move(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["POST"])
}

/// Django's `DATA_UPLOAD_MAX_MEMORY_SIZE` (5 MiB,
/// `settings/common.py:594`): past it the size middleware answers 413
/// before the view runs.
const MAX_BODY: usize = 5_242_880;

/// Move body shape for the shared negotiator: a single scalar,
/// last-value-wins; uploads are dropped (the view reads `request.data`
/// only, and no contract body is multipart).
const MOVE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// `POST .../projects/<project_id>/work-items/<pk>/move/`
/// (`views/issue.py:920-937`): the entity gate, the body read, the
/// pre-save snapshot, the 650 move driver over the pool store, the
/// post-commit cancel/drain, the explicit 594 fire, the two enqueues,
/// then the refreshed issue in the S1 API read shape.
pub async fn post_move(
    State(state): State<AppState>,
    Path((slug, project_id, pk)): Path<(String, String, String)>,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&pk) {
        return crate::edge::proxy(State(state), req).await;
    }
    let pk = pk.parse::<Uuid>().expect("checked segment");
    let (parts, body) = req.into_parts();
    // The size middleware reads before the view, so 413 precedes auth —
    // exactly as `RequestBodySizeLimitMiddleware` 413s before DRF runs.
    let bytes = match axum::body::to_bytes(body, MAX_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => return Denial::RequestTooLarge.into_response(),
    };
    match move_inner(&state, &parts, &bytes, &headers, &slug, &project_id, &pk).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn move_inner(
    state: &AppState,
    parts: &axum::http::request::Parts,
    bytes: &[u8],
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    pk: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    // Rewrite before the gate (`base.py:107-111`): an identifier miss 404s
    // even under an unknown slug, exactly as `Project.resolve` raising
    // inside `initial()` does before `check_permissions` runs.
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::IssueMove,
    )
    .await?;
    // `TimezoneMixin.initial` runs after the gate: survivors with an
    // unknown stored zone 400 here (an empty zone 500s).
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    // `request.data` parses lazily in the view — after auth, gate, and
    // timezone — so a denying gate answers before a malformed body 400s.
    let target_ref = read_move_target(&parts.headers, bytes)?;
    let origin = move_origin(state)?;
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let now = Utc::now();
    // The pre-save snapshot, before the move's own write.
    let mut signal = MoveSignalSeam { pool: &pre.pool };
    let prev = capture_prior_state(&mut signal, Some(*pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let store = PoolMoveStore::new(pre.pool.clone());
    let outcome = match move_work_item_to_project(
        &store,
        slug,
        project_id,
        *pk,
        &target_ref,
        pre.actor.id,
        &origin,
        web_base.as_deref(),
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
            // just the issue in the API read shape.
            // Same-project rows carry no url parts (no refetch ran), so
            // the serializer's lazy pair resolves here — FK-guaranteed.
            let slug = store
                .representation_workspace_slug(source.workspace_id)
                .await
                .map_err(|_| Denial::ServerError)?
                .ok_or(Denial::ServerError)?;
            let identifier = store
                .representation_project_identifier(source.project_id)
                .await
                .map_err(|_| Denial::ServerError)?
                .ok_or(Denial::ServerError)?;
            let body = render_moved_issue_api(
                &store,
                &source,
                &slug,
                &identifier,
                web_base.as_deref(),
                &tz,
            )
            .await?;
            Ok(json_response(StatusCode::OK, body))
        }
        MoveResult::Moved(moved) => {
            run_post_commit(state, &pre.pool, &moved.post_commit).await?;
            fire_after_move(
                &pre.pool,
                *pk,
                prev,
                moved.issue.row.state_id,
                moved.dispatch_immediate,
                Some(pre.actor.id),
                now,
            )
            .await?;
            for enqueue in &moved.enqueues {
                let job = pidash_jobs::queue::NewJob::new(
                    enqueue.task,
                    Value::Array(enqueue.args.clone()),
                    Value::Object(enqueue.kwargs.clone()),
                );
                // `.delay` past the commit: best-effort, the response
                // stands on enqueue failure, as in Django (where the
                // broker is up so `.delay` never raises here) and as in
                // the D-26 twin (PIDASHCONV-793: serve-only envs have no
                // `rust_job_queue` table, so a mapped 500 would fail an
                // already-committed move).
                if let Err(error) = pidash_jobs::queue::enqueue(&pre.pool, &job).await {
                    tracing::warn!(%error, task = enqueue.task, "task enqueue failed; response stands");
                }
            }
            let body = render_moved_issue_api(
                &store,
                &moved.issue.row,
                &moved.issue.workspace_slug,
                &moved.issue.project_identifier,
                web_base.as_deref(),
                &tz,
            )
            .await?;
            Ok(json_response(StatusCode::OK, body))
        }
    }
}

/// The move's `target_ref` (`views/issue.py:920-924`): `request.data`
/// coerced to `{}` when it is not a dict, then `.get("project")`
/// (missing reads JSON null, which the driver blanks to the
/// required-400).
fn read_move_target(headers: &HeaderMap, bytes: &[u8]) -> Result<Value, Denial> {
    match shared_body::negotiate_body(headers, bytes, &MOVE_BODY_SPEC) {
        Ok(shared_body::NegotiatedBody::Empty) => Ok(Value::Null),
        Ok(shared_body::NegotiatedBody::JsonText { text, .. }) => match parse_json_text(&text) {
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
        },
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

/// `base_host(request, is_app=True)` (`utils/host.py:17-60`): the app
/// base URL when set, else the web origin — unset everywhere is a 500
/// (`ImproperlyConfigured`). Empty strings fall through like Python's
/// `or` (the D-26 twin passes them to the origin verbatim; unpinned).
fn move_origin(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    if let Some(url) = urls.app_base_url.as_deref().filter(|s| !s.is_empty()) {
        return Ok(url.to_owned());
    }
    if let Some(url) = urls.web_url.as_deref().filter(|s| !s.is_empty()) {
        return Ok(url.to_owned());
    }
    Err(Denial::ServerError)
}

/// Fetch the `ProjectEntityPermission` POST facts
/// (`app/permissions/project.py:85-116`): active project membership with
/// role Admin/Member, workspace- and project-scoped. The entity gate's
/// POST branch only consults `has_project_admin_or_member`.
async fn entity_facts(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
) -> Result<project::ProjectFacts, Denial> {
    // `role` is `smallint`: sqlx does not widen `INT2` into `i32` on
    // decode, so read `i16` and compare as integers. `deleted_at IS NULL`
    // is the `SoftDeletionManager` scope (`db/mixins.py:56-58`).
    let roles: Vec<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "project_members" WHERE "workspace_id" = $1 AND "member_id" = $2 AND "project_id" = $3 AND "is_active" AND "deleted_at" IS NULL"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "entity-roles"))?;
    Ok(project::ProjectFacts {
        workspace: pidash_types::WorkspaceId::from(workspace_slug.to_owned()),
        project_id: pidash_types::ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: false,
        has_workspace_admin_or_member: false,
        is_workspace_admin: false,
        is_project_member: !roles.is_empty(),
        is_project_admin: roles.contains(&20),
        has_project_admin_or_member: roles.iter().any(|r| *r == 20 || *r == 15),
        has_identifier_membership: false,
        has_project_identifier: false,
    })
}

/// Run the move route's gate; deny 403 on failure. The move route
/// carries `ProjectEntityPermission` (POST → project Admin/Member).
async fn require_gate(
    pool: &PgPool,
    workspace_id: &uuid::Uuid,
    workspace_slug: &str,
    user_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    route: V1WorkItemsRoute,
) -> Result<(), Denial> {
    let gate = gate_for(route, "POST");
    let facts = entity_facts(pool, workspace_id, workspace_slug, user_id, project_id).await?;
    let scope = TenantScope::new(pidash_types::WorkspaceId::from(workspace_slug.to_owned()));
    if super::perms::decide(gate, "POST", &scope, &facts) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// Map the 650 driver's failures onto the view's responses: the
/// recoverable `{"error"}` + status, the source-miss 404, the
/// resolve-miss 404 `detail`, anything else the 500.
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

/// The move's 200 body: `IssueSerializer(issue).data` (`views/issue.py:937`)
/// over the refreshed (or same-project) issue — a bare instance, so no
/// `fields`/`expand` context and no relations viewer (the `relations`
/// block omits), while the blocker summary always queries
/// (`serializers/issue.py:436-493`).
async fn render_moved_issue_api(
    store: &PoolMoveStore,
    issue: &SourceIssueRow,
    workspace_slug: &str,
    project_identifier: &str,
    web_base: Option<&str>,
    tz: &Tz,
) -> Result<String, Denial> {
    // Serializer read order (`to_representation`): assignees, labels,
    // then the blocker summary (blocked_by, blocking, has_open) — the
    // same seam reads the driver used pre-move, now past the commit.
    let assignee_ids: Vec<String> = store
        .representation_assignee_ids(issue.id)
        .await
        .map_err(|_| Denial::ServerError)?
        .iter()
        .map(Uuid::to_string)
        .collect();
    let label_ids: Vec<String> = store
        .representation_label_ids(issue.id)
        .await
        .map_err(|_| Denial::ServerError)?
        .iter()
        .map(Uuid::to_string)
        .collect();
    let blocked_by = store
        .representation_blocked_by(issue.id)
        .await
        .map_err(|_| Denial::ServerError)?;
    let blocking = store
        .representation_blocking(issue.id)
        .await
        .map_err(|_| Denial::ServerError)?;
    let has_open_blockers = store
        .representation_has_open_blockers(issue.id)
        .await
        .map_err(|_| Denial::ServerError)?;
    let blocked_by_items: Vec<SummaryItem> = blocked_by.iter().map(summary_item).collect();
    let blocking_items: Vec<SummaryItem> = blocking.iter().map(summary_item).collect();
    let blockers = BlockerSummary {
        blocked_by: blocked_by_items,
        blocking: blocking_items,
        has_open_blockers,
    };

    let id = issue.id.to_string();
    let created_at = render_datetime_in(&issue.created_at, tz);
    let updated_at = render_datetime_in(&issue.updated_at, tz);
    let deleted_at = issue
        .deleted_at
        .as_ref()
        .map(|dt| render_datetime_in(dt, tz));
    let completed_at = issue
        .completed_at
        .as_ref()
        .map(|dt| render_datetime_in(dt, tz));
    let archived_at = issue
        .archived_at
        .as_ref()
        .map(|dt| dt.date_naive().to_string());
    let start_date = issue.start_date.as_ref().map(ToString::to_string);
    let target_date = issue.target_date.as_ref().map(ToString::to_string);
    let project = issue.project_id.to_string();
    let workspace = issue.workspace_id.to_string();
    let parent = issue.parent_id.map(|id| id.to_string());
    let state = issue.state_id.map(|id| id.to_string());
    let estimate_point = issue.estimate_point_id.map(|id| id.to_string());
    let assigned_pod = issue.assigned_pod_id.map(|id| id.to_string());
    let created_by = issue.created_by_id.map(|id| id.to_string());
    let updated_by = issue.updated_by_id.map(|id| id.to_string());
    let url = pidash_services::v1_work_items::shape_issue::issue_url(
        web_base,
        Some(workspace_slug),
        Some(project_identifier),
        Some(issue.sequence_id),
    );
    let assignee_refs: Vec<&str> = assignee_ids.iter().map(String::as_str).collect();
    let label_refs: Vec<&str> = label_ids.iter().map(String::as_str).collect();
    let row = ApiIssueRow {
        id: &id,
        type_id: issue.type_id.as_deref(),
        url,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        point: issue.point,
        name: &issue.name,
        description_html: &issue.description_html,
        description_binary: issue.description_binary.as_deref(),
        priority: &issue.priority,
        complexity_score: issue.complexity_score,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        sequence_id: issue.sequence_id,
        sort_order: issue.sort_order,
        completed_at: completed_at.as_deref(),
        archived_at: archived_at.as_deref(),
        is_draft: issue.is_draft,
        external_source: issue.external_source.as_deref(),
        external_id: issue.external_id.as_deref(),
        git_work_branch: &issue.git_work_branch,
        created_via: issue.created_via.as_deref(),
        agent_executor: issue.agent_executor.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        project: &project,
        workspace: &workspace,
        parent: parent.as_deref(),
        state: state.as_deref(),
        estimate_point: estimate_point.as_deref(),
        assigned_pod: assigned_pod.as_deref(),
    };
    let input = RepresentationInput {
        row: &row,
        fields: None,
        expand: &[],
        is_list: false,
        assignee_ids: assignee_refs.as_slice(),
        assignee_rows: &[],
        label_ids: label_refs.as_slice(),
        expanded_labels: &[],
        blockers: Some(&blockers),
        relations: None,
        expansions: &[],
    };
    // The binary/NaN arms reproduce Django 500s; the `Missing*` arms are
    // unreachable (blockers supplied, no fields filter).
    let view = pidash_services::v1_work_items::shape_issue::render_issue(&input)
        .map_err(|_| Denial::ServerError)?;
    serde_json::to_string(&view).map_err(|_| Denial::ServerError)
}

/// One `relations_summary` item (`orchestration/blockers.py:184-198`):
/// `{identifier, state, state_group}` — the composed `PROJ-42`
/// identifier, never the bare sequence.
fn summary_item(row: &BlockerRow) -> SummaryItem<'_> {
    SummaryItem {
        identifier: format!("{}-{}", row.project_identifier, row.sequence_id),
        state: row.state_name.as_deref(),
        state_group: row.state_group.as_deref(),
    }
}

// ---------------------------------------------------------------------------
// Pool `MoveStore`
// ---------------------------------------------------------------------------

/// The 650 [`MoveStore`] over the pool: guard reads run on pool
/// checkouts, the `:173-373` span runs on one held connection (`BEGIN`
/// at [`MoveStore::lock_source_issue`], `COMMIT` at
/// [`MoveStore::moved_issue`], `ROLLBACK` via [`PoolMoveStore::abort`]
/// on any driver error), and every statement is the 650 SQL text
/// executed verbatim.
///
/// The mutex only bridges `&self` to the held connection — the store
/// is per-request, so it is never contended. It is a tokio mutex so
/// the guard stays `Send` across awaits (a std guard would poison the
/// handler future's `Send` bound).
struct PoolMoveStore {
    pool: PgPool,
    txn: Mutex<Option<PoolConnection<Postgres>>>,
}

impl PoolMoveStore {
    fn new(pool: PgPool) -> Self {
        Self {
            pool,
            txn: Mutex::new(None),
        }
    }

    /// Roll back the span when the driver failed (Django's
    /// `transaction.atomic` exit on raise). A no-op when no span is
    /// open (pre-span failures).
    async fn abort(&self) {
        let conn = self.txn.lock().await.take();
        if let Some(mut conn) = conn {
            if let Err(error) = sqlx::query("ROLLBACK").execute(&mut *conn).await {
                tracing::warn!(%error, "move store: rollback failed");
            }
        }
    }

    /// Open the span: hold one connection and `BEGIN`.
    async fn begin_span(&self) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        sqlx::query("BEGIN")
            .execute(&mut *conn)
            .await
            .map_err(|error| StoreError(error.to_string()))?;
        self.txn.lock().await.replace(conn);
        Ok(())
    }

    /// Close the span: `COMMIT` and release the connection.
    async fn commit_span(&self) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        use pidash_services::app_issues::issue_move::StoreError;
        let mut conn = self
            .txn
            .lock()
            .await
            .take()
            .ok_or_else(|| StoreError("move store: commit without a span".to_owned()))?;
        sqlx::query("COMMIT")
            .execute(&mut *conn)
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
impl MoveStore for PoolMoveStore {
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
            .fetch_optional(&self.pool)
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
                .fetch_optional(&self.pool)
                .await
                .map_err(store_error)?,
            ProjectLookup::Identifier(identifier) => {
                sqlx::query_as(PROJECT_RESOLVE_BY_IDENTIFIER_SQL)
                    .bind(identifier)
                    .bind(slug)
                    .fetch_optional(&self.pool)
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
            .fetch_optional(&self.pool)
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
            .fetch_optional(&self.pool)
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
            .fetch_optional(&self.pool)
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
            .fetch_all(&self.pool)
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
            .fetch_all(&self.pool)
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
            .fetch_all(&self.pool)
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
            .fetch_all(&self.pool)
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
            .fetch_optional(&self.pool)
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
            .fetch_optional(&self.pool)
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
            .fetch_optional(&self.pool)
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
            .fetch_optional(&mut **conn)
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
            .execute(&mut **conn)
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
            .fetch_all(&mut **conn)
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
            .fetch_optional(&mut **conn)
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
            .fetch_optional(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .execute(&mut **conn)
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
            .fetch_all(&mut **conn)
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
            .fetch_all(&mut **conn)
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
            .execute(&mut **conn)
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| StoreError(error.to_string()))
    }

    async fn create_handoff_run(
        &self,
        _issue_id: Uuid,
        _parent_run: &HandoffRunRow,
        _pod_id: Uuid,
        _now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), pidash_services::app_issues::issue_move::StoreError> {
        // Documented gap (see the module docs): the immediate-handoff
        // insert needs the D-12 creation seam, which this mirror has
        // not adopted yet (the D-26 twin has, via PIDASHCONV-743).
        // Loud 500, never a silent skip.
        Err(pidash_services::app_issues::issue_move::StoreError(
            "move store: immediate handoff creation needs the D-12 creation seam (not adopted here yet)".to_owned(),
        ))
    }

    async fn moved_issue(
        &self,
        pk: Uuid,
    ) -> Result<Option<MovedIssueRow>, pidash_services::app_issues::issue_move::StoreError> {
        self.commit_span().await?;
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(MOVED_ISSUE_REFETCH_SQL)
            .bind(pk)
            .fetch_optional(&self.pool)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// F18-11 `calls.search.body`, compact separators (DRF wire bytes).
    const SEARCH_BODY: &str = "{\"issues\":[{\"name\":\"zxcvsearchtoken login flow\",\"id\":\"a7509d00-345f-47fb-bee3-6bcf7d3339e2\",\"sequence_id\":1,\"project__identifier\":\"CT00003\",\"project_id\":\"d715be3d-234f-46ef-89a3-97f0c7c04b7e\",\"workspace__slug\":\"ws-conv659-2\"}]}";

    /// F18-11 `calls.search_advanced.results_full[0]`, compact separators.
    const ADVANCED_ROW_BODY: &str = "{\"id\":\"a7509d00-345f-47fb-bee3-6bcf7d3339e2\",\"sequence_id\":1,\"identifier\":\"CT00003-1\",\"name\":\"zxcvsearchtoken login flow\",\"snippet\":\"zxcvsearchtoken the login flow breaks on retry with session expiry\",\"state\":{\"name\":\"Todo\",\"group\":\"unstarted\"},\"project\":{\"id\":\"d715be3d-234f-46ef-89a3-97f0c7c04b7e\",\"identifier\":\"CT00003\",\"name\":\"Conv659 Project\"},\"workspace_slug\":\"ws-conv659-2\",\"created_at\":\"2026-10-02T23:17:12.025108Z\",\"updated_at\":\"2026-10-02T23:17:12.025108Z\",\"completed_at\":null,\"rank\":0.075990885,\"url\":\"http://127.0.0.1:18359/ws-conv659-2/browse/CT00003-1\"}";

    /// F18-11 `calls.search_advanced` envelope + results, compact separators.
    const ADVANCED_ENVELOPE_BODY: &str = "{\"query\":\"zxcvsearchtoken\",\"count\":1,\"results\":[{\"id\":\"a7509d00-345f-47fb-bee3-6bcf7d3339e2\",\"sequence_id\":1,\"identifier\":\"CT00003-1\",\"name\":\"zxcvsearchtoken login flow\",\"snippet\":\"zxcvsearchtoken the login flow breaks on retry with session expiry\",\"state\":{\"name\":\"Todo\",\"group\":\"unstarted\"},\"project\":{\"id\":\"d715be3d-234f-46ef-89a3-97f0c7c04b7e\",\"identifier\":\"CT00003\",\"name\":\"Conv659 Project\"},\"workspace_slug\":\"ws-conv659-2\",\"created_at\":\"2026-10-02T23:17:12.025108Z\",\"updated_at\":\"2026-10-02T23:17:12.025108Z\",\"completed_at\":null,\"rank\":0.075990885,\"url\":\"http://127.0.0.1:18359/ws-conv659-2/browse/CT00003-1\"}]}";

    #[test]
    fn legacy_statement_numbers_binds_without_project() {
        let stmt = legacy_statement("zxcvsearchtoken", None, None, None).expect("statement");
        assert!(
            stmt.sql.contains("\"project_members\".\"member_id\" = $1"),
            "{}",
            stmt.sql
        );
        assert!(
            stmt.sql.contains("\"workspaces\".\"slug\" = $2"),
            "{}",
            stmt.sql
        );
        assert!(!stmt.sql.contains("$6"), "{}", stmt.sql);
        assert!(!stmt.sql.contains(":fts"), "{}", stmt.sql);
        assert!(!stmt.sql.contains(":seq"), "{}", stmt.sql);
        assert!(!stmt.sql.contains(":like"), "{}", stmt.sql);
        assert!(stmt.sql.ends_with("LIMIT 10"), "{}", stmt.sql);
        assert_eq!(stmt.project_id, None);
        assert_eq!(stmt.fts, "zxcvsearchtoken");
    }

    #[test]
    fn legacy_statement_numbers_project_as_third_bind() {
        let pid = "d715be3d-234f-46ef-89a3-97f0c7c04b7e";
        let stmt = legacy_statement("q", Some("false"), Some(pid), Some("5")).expect("statement");
        assert_eq!(
            stmt.project_id,
            Some(pid.parse::<Uuid>().expect("fixture uuid"))
        );
        assert!(
            stmt.sql.contains("\"issues\".\"project_id\" = $3"),
            "{}",
            stmt.sql
        );
        assert!(stmt.sql.contains("$6"), "{}", stmt.sql);
        assert!(!stmt.sql.contains(":project_id"), "{}", stmt.sql);
        assert!(stmt.sql.ends_with("LIMIT 5"), "{}", stmt.sql);
    }

    #[test]
    fn legacy_statement_empty_workspace_search_disables_filter() {
        // BUG-6: `workspace_search` matches the exact string `"false"`.
        let stmt = legacy_statement(
            "q",
            Some(""),
            Some("d715be3d-234f-46ef-89a3-97f0c7c04b7e"),
            None,
        )
        .expect("statement");
        assert_eq!(stmt.project_id, None);
        assert!(!stmt.sql.contains("$6"), "{}", stmt.sql);
    }

    #[test]
    fn legacy_statement_invalid_project_and_limit_500() {
        // BUG-2: non-UUID `project_id` 500s (render-time ValidationError).
        assert_eq!(
            legacy_statement("q", Some("false"), Some("NOPE"), None),
            Err(Denial::ServerError)
        );
        // BUG-1: garbage and negative limits 500.
        assert_eq!(
            legacy_statement("q", None, None, Some("abc")),
            Err(Denial::ServerError)
        );
        assert_eq!(
            legacy_statement("q", None, None, Some("-3")),
            Err(Denial::ServerError)
        );
    }

    #[test]
    fn legacy_query_present_is_unstripped() {
        // BUG-5: legacy `?search=` is not stripped.
        assert!(!q::legacy_query_present(None));
        assert!(!q::legacy_query_present(Some("")));
        assert!(q::legacy_query_present(Some(" ")));
        assert!(q::legacy_query_present(Some("x")));
    }

    #[test]
    fn render_legacy_row_replays_fixture_bytes() {
        let row = LegacyRow {
            name: "zxcvsearchtoken login flow".to_owned(),
            id: "a7509d00-345f-47fb-bee3-6bcf7d3339e2"
                .parse::<Uuid>()
                .expect("fixture uuid"),
            sequence_id: 1,
            project_identifier: "CT00003".to_owned(),
            project_id: "d715be3d-234f-46ef-89a3-97f0c7c04b7e"
                .parse::<Uuid>()
                .expect("fixture uuid"),
            workspace_slug: "ws-conv659-2".to_owned(),
        };
        let mut envelope = Map::with_capacity(1);
        envelope.insert(
            "issues".to_owned(),
            Value::Array(vec![Value::Object(render_legacy_row(&row))]),
        );
        assert_eq!(serde_json::to_string(&envelope).expect("json"), SEARCH_BODY);
    }

    #[test]
    fn rank_f64_matches_postgres_float4_spelling() {
        // The fixture rank came out of `float4out` as `0.075990885`; the
        // binary `real` roundtrips through `f32` Display to the same digits.
        let from_wire = 0.075990885f64 as f32;
        assert_eq!(rank_f64(Some(from_wire)).to_string(), "0.075990885");
        assert_eq!(rank_f64(None), 0.0);
    }

    #[test]
    fn advanced_result_replays_fixture_bytes() {
        // The recorded headline is not in the fixture — only its
        // `extract_snippet` output — so the markers are reconstructed
        // around the matched token (the strip is what the test pins).
        let headline = "<<zxcvsearchtoken>> the login flow breaks on retry with session expiry";
        let rank = rank_f64(Some(0.075990885f64 as f32));
        let result = q::advanced_result(
            &q::AdvancedRow {
                id: "a7509d00-345f-47fb-bee3-6bcf7d3339e2",
                sequence_id: 1,
                name: "zxcvsearchtoken login flow",
                headline: Some(headline),
                rank: Some(rank),
                created_at: "2026-10-02T23:17:12.025108Z",
                updated_at: "2026-10-02T23:17:12.025108Z",
                completed_at: None,
                state_name: Some("Todo"),
                state_group: Some("unstarted"),
                project_id: "d715be3d-234f-46ef-89a3-97f0c7c04b7e",
                project_identifier: "CT00003",
                project_name: "Conv659 Project",
                workspace_slug: "ws-conv659-2",
            },
            Some("http://127.0.0.1:18359/ws-conv659-2/browse/CT00003-1"),
        );
        assert_eq!(
            serde_json::to_string(&result).expect("json"),
            ADVANCED_ROW_BODY
        );
        let envelope = q::advanced_envelope("zxcvsearchtoken", vec![result]);
        assert_eq!(
            serde_json::to_string(&envelope).expect("json"),
            ADVANCED_ENVELOPE_BODY
        );
    }

    #[test]
    fn advanced_numbering_places_optional_arms_in_order() {
        let base = q::advanced_search_sql(&q::AdvancedParts {
            query: "q",
            project_uuid_filter: true,
            status: q::StatusFilter::All,
            since_filter: true,
            sort: q::Sort::Rank,
        });
        let sql = number_advanced_placeholders(&base, true, true, 10);
        assert!(sql.contains("\"issues\".\"project_id\" = $3"), "{sql}");
        assert!(sql.contains("\"issues\".\"updated_at\" >= $4"), "{sql}");
        assert!(sql.contains("$7"), "{sql}");
        assert!(!sql.contains(":fts"), "{sql}");
        assert!(sql.ends_with("LIMIT 10"), "{sql}");

        let base = q::advanced_search_sql(&q::AdvancedParts {
            query: "q",
            project_uuid_filter: false,
            status: q::StatusFilter::Open,
            since_filter: false,
            sort: q::Sort::Created,
        });
        let sql = number_advanced_placeholders(&base, false, false, 50);
        assert!(!sql.contains(":project_id"), "{sql}");
        assert!(!sql.contains(":since"), "{sql}");
        assert!(!sql.contains("$6"), "{sql}");
        assert!(
            sql.contains("\"issues\".\"created_at\" DESC LIMIT 50"),
            "{sql}"
        );
        assert!(sql.contains("\"states\".\"group\" IN ("), "{sql}");
    }

    #[test]
    fn advanced_sort_and_since_bodies_are_exact() {
        let error = q::parse_sort(Some("nope")).expect_err("sort 400");
        assert_eq!(
            error.body(),
            "{\"error\":\"Unknown sort 'nope'. Valid: rank, -created, -updated.\"}"
        );
        let outcome = q::parse_since(Some("not-a-datetime!!"));
        match outcome {
            q::SinceOutcome::BadFormat(error) => assert_eq!(
                error.body(),
                "{\"error\":\"Invalid 'since' \u{2014} expected ISO 8601 datetime (e.g. 2025-01-01T00:00:00Z).\"}"
            ),
            other => panic!("expected BadFormat, got {other:?}"),
        }
    }

    #[test]
    fn unknown_timezone_400s() {
        assert_eq!(
            activate_timezone(Some("Mars/Olympus")),
            Err(Denial::BadError(
                r#"{"error":"The required key does not exist."}"#.to_owned()
            ))
        );
        assert!(activate_timezone(None).is_ok());
        // Empty stored zone: `ZoneInfo('')` is a `ValueError` → 500.
        assert_eq!(activate_timezone(Some("")), Err(Denial::ServerError));
    }

    #[test]
    fn move_denial_bodies_are_exact() {
        // `IssueMoveError` → `{"error"}` + status, verbatim.
        assert_eq!(
            Denial::MoveRejection {
                status: 400,
                message: "project is required".to_owned(),
            }
            .status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"project is required"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::MoveRejection {
                status: 409,
                message: "Target project does not have a default runner pod".to_owned(),
            }
            .status_and_body()
            .0,
            StatusCode::CONFLICT
        );
        // Source miss → the `ObjectDoesNotExist` branch body.
        assert_eq!(
            Denial::NotFound.status_and_body(),
            (
                StatusCode::NOT_FOUND,
                r#"{"error":"The requested resource does not exist."}"#.to_owned()
            )
        );
        // DRF `ParseError` / `UnsupportedMediaType` render lowercase
        // `detail` (the DRF default, via `auth_exception_handler`).
        assert_eq!(
            Denial::BadDetail("JSON parse error - x".to_owned()).status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"detail":"JSON parse error - x"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::UnsupportedMediaType(
                "Unsupported media type \"text/plain\" in request.".to_owned()
            )
            .status_and_body()
            .0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        // The 413 is a plain `JsonResponse`: default separators (spaces).
        assert_eq!(
            Denial::RequestTooLarge.status_and_body(),
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                r#"{"error": "REQUEST_BODY_TOO_LARGE", "detail": "The size of the request body exceeds the maximum allowed size."}"#.to_owned()
            )
        );
        // The gate denial is the shared class body.
        assert_eq!(
            Denial::Forbidden.status_and_body(),
            (
                StatusCode::FORBIDDEN,
                crate::v1_work_items::perms::CLASS_DENIAL_BODY.to_owned()
            )
        );
    }

    #[test]
    fn map_move_error_maps_driver_failures() {
        use pidash_services::app_issues::IssueMoveError;
        let rejection = map_move_error(MoveError::Issue(IssueMoveError {
            message: "project is required".to_owned(),
            status_code: 400,
        }));
        assert_eq!(
            rejection,
            Denial::MoveRejection {
                status: 400,
                message: "project is required".to_owned(),
            }
        );
        assert_eq!(map_move_error(MoveError::IssueNotFound), Denial::NotFound);
        assert_eq!(
            map_move_error(MoveError::ProjectNotFound),
            Denial::ProjectNotFound
        );
        assert_eq!(
            map_move_error(MoveError::Store(
                pidash_services::app_issues::issue_move::StoreError("boom".to_owned())
            )),
            Denial::ServerError
        );
    }

    #[test]
    fn read_move_target_coerces_non_dict_bodies() {
        use axum::http::header;
        // The negotiator treats a missing `content-length` as empty, so
        // every content-carrying case sets it (as hyper would).
        let headers_for = |content_type: &str, body: &[u8]| {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
            headers.insert(
                header::CONTENT_LENGTH,
                body.len().to_string().parse().unwrap(),
            );
            headers
        };
        let json = |body: &[u8]| headers_for("application/json", body);
        // A dict body reads `.get("project")`.
        assert_eq!(
            read_move_target(&json(br#"{"project": "abc"}"#), br#"{"project": "abc"}"#),
            Ok(Value::String("abc".to_owned()))
        );
        assert_eq!(read_move_target(&json(br#"{}"#), br#"{}"#), Ok(Value::Null));
        // Non-dict JSON (list/primitive) coerces to `{}` → null target →
        // the driver's required-400, never an AttributeError 500.
        assert_eq!(
            read_move_target(&json(br#"[1, 2]"#), br#"[1, 2]"#),
            Ok(Value::Null)
        );
        assert_eq!(read_move_target(&json(br#"42"#), br#"42"#), Ok(Value::Null));
        // Empty bodies read null (missing `.get("project")`).
        assert_eq!(read_move_target(&HeaderMap::new(), b""), Ok(Value::Null));
        // Malformed JSON 400s with the DRF `ParseError` prefix.
        let err = read_move_target(&json(b"{oops"), b"{oops").expect_err("parse 400");
        match err {
            Denial::BadDetail(message) => {
                assert!(message.starts_with(JSON_PARSE_PREFIX), "{message}");
            }
            other => panic!("expected BadDetail, got {other:?}"),
        }
        // Forms read last-value-wins.
        let form = headers_for("application/x-www-form-urlencoded", b"project=abc");
        assert_eq!(
            read_move_target(&form, b"project=abc"),
            Ok(Value::String("abc".to_owned()))
        );
    }

    #[test]
    fn move_gate_is_project_entity_post() {
        use pidash_auth::permissions::project::ProjectFacts;
        use pidash_types::{ProjectId, WorkspaceId};
        assert_eq!(
            gate_for(V1WorkItemsRoute::IssueMove, "POST"),
            crate::v1_work_items::perms::V1WorkItemsGate::ProjectEntity
        );
        let scope = TenantScope::new(WorkspaceId::from("acme"));
        let facts = |member: bool| ProjectFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("11111111-1111-1111-1111-111111111111"),
            authenticated: true,
            is_workspace_member: false,
            has_workspace_admin_or_member: false,
            is_workspace_admin: false,
            is_project_member: member,
            is_project_admin: false,
            has_project_admin_or_member: member,
            has_identifier_membership: false,
            has_project_identifier: false,
        };
        assert!(crate::v1_work_items::perms::decide(
            crate::v1_work_items::perms::V1WorkItemsGate::ProjectEntity,
            "POST",
            &scope,
            &facts(true)
        ));
        assert!(!crate::v1_work_items::perms::decide(
            crate::v1_work_items::perms::V1WorkItemsGate::ProjectEntity,
            "POST",
            &scope,
            &facts(false)
        ));
    }

    #[test]
    fn summary_item_composes_identifier() {
        let row = BlockerRow {
            issue_id: Uuid::nil(),
            sequence_id: 42,
            project_identifier: "PROJ".to_owned(),
            state_name: Some("Todo".to_owned()),
            state_group: Some("unstarted".to_owned()),
        };
        let item = summary_item(&row);
        assert_eq!(item.identifier, "PROJ-42");
        assert_eq!(item.state, Some("Todo"));
        assert_eq!(item.state_group, Some("unstarted"));
    }

    #[test]
    fn social_project_not_found_maps_through() {
        // The identifier-rewrite miss must 404, not 500: `rewrite_project_id`
        // fails `Social::ProjectNotFound` and `?` converts it here.
        let local: Denial = crate::v1_work_items::handlers_social::Denial::ProjectNotFound.into();
        assert_eq!(local, Denial::ProjectNotFound);
    }
}
