//! Move + search handlers (D-18 handlers E, PIDASHCONV-677).
//!
//! Ports `apps/api/pi_dash/api/views/issue.py` (3 units):
//!
//! * `IssueMoveAPIEndpoint.post` (`:920-937`) — move a work item to
//!   another project, with `utils/issue_move.py:122-393` as its closure:
//!   the non-dict-body `{}` coercion,
//!   `domain::issues::move_to_project` over the 650 kernel, the
//!   post-commit cancel/drain, the 594 state-transition fire and the
//!   two enqueues folded into the commit chain, then the S1 API read
//!   shape.
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
//! Layering (all foundation use is read-only): the move reuses the
//! move domain service (`domain::issues::move_to_project` over the 650
//! kernel — the ported util, shared with the D-26 app surface — with
//! the 594 fire and the D-14 pod drains behind its hooks); the search SQL
//! text and param parsers live in
//! `pidash_services::v1_work_items::queries_search` (Q3, PIDASHCONV-670)
//! over `app_views_search::fts`; the advanced row assembly is
//! `queries_search::{advanced_result, advanced_envelope}`; the move's
//! success datetime rendering is
//! `crate::serializer::render_datetime_in` (the `IssueSerializer`
//! `DateTimeField` path), while the advanced rows render UTC
//! (`crate::serializer::render_datetime` — raw `.values()` datetimes,
//! no `DateTimeField`); auth/preamble/rewrite/query helpers are reused from
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
//! * BUG-2 (Q3-2, `:2709`): legacy `?project_id=` with a non-UUID 400s.
//!   The `UUIDField.get_prep_value` `ValidationError` raises at
//!   `filter()` time, inside the handler — inside DRF dispatch's `try`
//!   — so `handle_exception`'s `ValidationError` branch
//!   (`api/views/base.py:148-152`) answers 400
//!   `{"error":"Please provide valid detail"}` (39 bytes, live-probed
//!   byte-identical on Django). Mapped here to 400
//!   `handlers_social::VALID_DETAIL_BODY` (the earlier render-time-500
//!   theory was wrong; the live probe refuted it).
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
//!   QUEUED/PAUSED) runs through the D-12 creation seam
//!   (`create_project_move_handoff_run`), mirroring the D-26 twin's
//!   adoption (PIDASHCONV-743): the span is the SeaORM transaction the
//!   seam writes through in-span, and the creation outboxes drain in
//!   the commit chain before the move's own post-commit work.
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/` —
//! `search`, `search_advanced`, the deprecated `search` twin, and the
//! `move` SQL + DB record).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_services::app_issues::{SourceIssueRow, ASSIGNEE_IDS_SQL, LABEL_IDS_SQL};
use pidash_services::domain;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues as domain_issues;
use pidash_services::domain::issues::{
    CreationDeps, MoveDomainOutcome, MovePostCommitHook, MoveRejection,
};
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{WriteActor, WriteCtx, WriteScope};
use pidash_services::orchestration::blockers::{has_open_blockers_sql, summary_sql, BlockerRow};
use pidash_services::v1_work_items::queries_search as q;
use pidash_services::v1_work_items::shape_issue::{
    BlockerSummary, IssueRow as ApiIssueRow, RepresentationInput, SummaryItem,
};

use crate::domain_hooks::{AppMoveAdmissionCache, AppMovePostCommitHook, AppTaskEnqueueHook};
use crate::issue_transition::AppTransitionHook;
use crate::middleware::body_limit::read_request_bytes;
use crate::orchestration::SpanCreationDeps;
use crate::serializer::{render_datetime, render_datetime_in};
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json::{
    parse_json_text, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};

use super::handlers_social::{preamble, query_last, rewrite_project_id, QueryMap};
use super::perms::{gate_for, V1WorkItemsRoute};
pub use pidash_compat::response::SERVER_ERROR_BODY;

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
    pidash_compat::response::json_response(status, pidash_compat::response::escape_u2028(&body))
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
/// `LegacyProject::InvalidUuid` 400s (BUG-2: the `ValidationError` raises
/// at `filter()` time inside the handler, so `handle_exception` answers
/// 400 `handlers_social::VALID_DETAIL_BODY`, byte-identical to Django).
fn legacy_statement(
    query: &str,
    workspace_search: Option<&str>,
    project_id: Option<&str>,
    limit: Option<&str>,
) -> Result<LegacyStatement, Denial> {
    let project = q::legacy_project_filter(workspace_search, project_id);
    if matches!(project, q::LegacyProject::InvalidUuid) {
        return Err(Denial::BadError(
            super::handlers_social::VALID_DETAIL_BODY.to_owned(),
        ));
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
/// would collide). Datetimes render UTC with a `Z` suffix: the view
/// copies raw `.values()` datetimes into a plain dict (`:2912-2914`)
/// with no DRF `DateTimeField`, so `TimezoneMixin` has no effect and
/// DRF's `JSONEncoder` emits `isoformat()` with `+00:00` rewritten to
/// `Z`; `_rank` is `ts_rank`'s `real`.
fn decode_advanced_row(
    row: &sqlx::postgres::PgRow,
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
            created_at: render_datetime(&created_at),
            updated_at: render_datetime(&updated_at),
            completed_at: completed_at.as_ref().map(render_datetime),
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
    // Timezone activation still gates unknown zones (400); the decoded
    // datetimes render UTC (see `decode_advanced_row`).
    let _tz = activate_timezone(pre.actor.timezone.as_deref())?;
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
        let (decoded, sequence_id, rank) = decode_advanced_row(row)?;
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

/// Move body shape for the shared negotiator: a single scalar,
/// last-value-wins; uploads are dropped (the view reads `request.data`
/// only, and no contract body is multipart).
const MOVE_BODY_SPEC: shared_body::BodySpec = shared_body::BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// `POST .../projects/<project_id>/work-items/<pk>/move/`
/// (`views/issue.py:920-937`): the entity gate, the body read, the
/// pre-save snapshot, `domain::issues::move_to_project` inside a
/// write transaction (the 650 kernel over the SeaORM move store,
/// with the post-commit cancel/drain, the 594 fire, the completion
/// comment and the enqueues folded into the commit chain), then the
/// refreshed issue in the S1 API read shape.
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
    // The serve-wide `BodyLimitLayer` already 413'd over-limit bodies
    // before the view ran; the read below cannot re-cap. A mid-stream
    // transport failure fails closed with the truncated-body 502.
    let bytes = match read_request_bytes(body).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
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
    let prev = crate::issue_transition::capture_prior_state_pool(&pre.pool, Some(*pk))
        .await
        .map_err(|_| Denial::ServerError)?;
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = WriteCtx::new(WriteActor {
        user_id: pre.actor.id,
        timezone: pre.actor.timezone.clone(),
    })
    .with_scope(WriteScope {
        workspace_id: Some(workspace_id),
        project_id: Some(project_id),
    });
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(state, &pre.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pre.pool));
    let span_deps = SpanCreationDeps::from_state(state);
    let drain: Arc<dyn MovePostCommitHook> = Arc::new(AppMovePostCommitHook::new(
        &pre.pool,
        state,
        span_deps.cache.clone(),
    ));
    let creation = CreationDeps {
        pool: pre.pool.clone(),
        cloud: span_deps.cloud.clone(),
        managed: span_deps.managed.clone(),
        now_unix_secs: span_deps.now_unix_secs,
        toolsets: span_deps.toolsets.clone(),
        assistant_seam: span_deps.assistant_seam.clone(),
        admission: Arc::new(AppMoveAdmissionCache::new(span_deps.cache)),
    };
    // Owned copies for the run closure: the `for<'t>` work bound
    // unifies only moved values and local borrows, not the caller's
    // `&str` / `&Uuid` params — and the render below still needs
    // `web_base` afterwards.
    let slug_owned = slug.to_owned();
    let pk_owned = *pk;
    let web_base_owned = web_base.clone();
    let outcome = domain::run(&ctx, orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let drain = drain.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            txn.hooks_mut().register(drain);
            domain_issues::move_to_project(
                &ctx,
                txn,
                domain_issues::MoveInput {
                    slug: &slug_owned,
                    project_id,
                    pk: pk_owned,
                    target_ref: &target_ref,
                    origin: origin.clone(),
                    web_base_url: web_base_owned.as_deref(),
                    now,
                    prev_state: prev,
                    options: domain_issues::MoveOptions::django(),
                    creation,
                },
            )
            .await
        })
    })
    .await
    .map_err(map_move_domain_error)?;
    match outcome {
        MoveDomainOutcome::AlreadyThere {
            row,
            workspace_slug,
            project_identifier,
        } => {
            // Same project: no writes at all, no signals, no enqueues —
            // just the issue in the API read shape. The domain resolved
            // the serializer's lazy url pair past the commit —
            // FK-guaranteed.
            let slug = workspace_slug.ok_or(Denial::ServerError)?;
            let identifier = project_identifier.ok_or(Denial::ServerError)?;
            let body = render_moved_issue_api(
                &pre.pool,
                &row,
                &slug,
                &identifier,
                web_base.as_deref(),
                &tz,
            )
            .await?;
            Ok(json_response(StatusCode::OK, body))
        }
        MoveDomainOutcome::Moved { row, chain } => {
            if chain.stopped().is_some() {
                return Err(Denial::ServerError);
            }
            let body = render_moved_issue_api(
                &pre.pool,
                &row.row,
                &row.workspace_slug,
                &row.project_identifier,
                web_base.as_deref(),
                &tz,
            )
            .await?;
            Ok(json_response(StatusCode::OK, body))
        }
        MoveDomainOutcome::Rejected(rejection) => Err(map_move_rejection(rejection)),
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
fn map_move_rejection(rejection: MoveRejection) -> Denial {
    match rejection {
        MoveRejection::Issue {
            message,
            status_code,
        } => Denial::MoveRejection {
            status: status_code,
            message,
        },
        MoveRejection::SourceNotFound => Denial::NotFound,
        MoveRejection::TargetNotFound => Denial::ProjectNotFound,
        // Unreachable: this driver passes `TargetOnly` (the gate checked
        // the source project up front).
        MoveRejection::SourceDenied => Denial::Forbidden,
    }
}

/// Map a domain-run failure: a store error is the generic 500; a work
/// error is the kernel's post-save 409 (the multiple-executing-runs
/// arm — the span rolled back, as Django's `atomic` exit on raise).
fn map_move_domain_error(error: domain::DomainError) -> Denial {
    match error {
        domain::DomainError::Db(_) => Denial::ServerError,
        domain::DomainError::Work(message) => Denial::MoveRejection {
            status: 409,
            message,
        },
    }
}

/// The move's 200 body: `IssueSerializer(issue).data` (`views/issue.py:937`)
/// over the refreshed (or same-project) issue — a bare instance, so no
/// `fields`/`expand` context and no relations viewer (the `relations`
/// block omits), while the blocker summary always queries
/// (`serializers/issue.py:436-493`).
async fn render_moved_issue_api(
    pool: &PgPool,
    issue: &SourceIssueRow,
    workspace_slug: &str,
    project_identifier: &str,
    web_base: Option<&str>,
    tz: &Tz,
) -> Result<String, Denial> {
    // Serializer read order (`to_representation`): assignees, labels,
    // then the blocker summary (blocked_by, blocking, has_open) — the
    // same seam reads the driver used pre-move, now past the commit.
    let assignee_rows: Vec<(Uuid,)> = sqlx::query_as(ASSIGNEE_IDS_SQL)
        .bind(issue.id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let assignee_ids: Vec<String> = assignee_rows
        .into_iter()
        .map(|row| row.0.to_string())
        .collect();
    let label_rows: Vec<(Uuid,)> = sqlx::query_as(LABEL_IDS_SQL)
        .bind(issue.id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let label_ids: Vec<String> = label_rows
        .into_iter()
        .map(|row| row.0.to_string())
        .collect();
    let blocked_by_rows = sqlx::query(&issue_param(summary_sql(false)))
        .bind(issue.id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let blocked_by: Vec<BlockerRow> = blocked_by_rows
        .iter()
        .map(map_blocker_row)
        .collect::<Result<_, _>>()
        .map_err(|_| Denial::ServerError)?;
    let blocking_rows = sqlx::query(&issue_param(summary_sql(true)))
        .bind(issue.id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let blocking: Vec<BlockerRow> = blocking_rows
        .iter()
        .map(map_blocker_row)
        .collect::<Result<_, _>>()
        .map_err(|_| Denial::ServerError)?;
    let open_hit: Option<(i32,)> = sqlx::query_as(&issue_param(has_open_blockers_sql()))
        .bind(issue.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let has_open_blockers = open_hit.is_some();
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
    fn legacy_statement_invalid_project_is_400_valid_detail() {
        // BUG-2: non-UUID `project_id` 400s — the `ValidationError`
        // raises at `filter()` time inside the handler, so Django's
        // `handle_exception` answers 400 (live-probed, byte-identical).
        let denial = legacy_statement("q", Some("false"), Some("NOPE"), None)
            .expect_err("non-UUID project_id rejects");
        assert_eq!(
            denial.status_and_body(),
            (
                StatusCode::BAD_REQUEST,
                r#"{"error":"Please provide valid detail"}"#.to_owned()
            )
        );
    }

    #[test]
    fn legacy_statement_bad_limit_500() {
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
    fn map_move_rejection_maps_driver_failures() {
        let rejection = map_move_rejection(MoveRejection::Issue {
            message: "project is required".to_owned(),
            status_code: 400,
        });
        assert_eq!(
            rejection,
            Denial::MoveRejection {
                status: 400,
                message: "project is required".to_owned(),
            }
        );
        assert_eq!(
            map_move_rejection(MoveRejection::SourceNotFound),
            Denial::NotFound
        );
        assert_eq!(
            map_move_rejection(MoveRejection::TargetNotFound),
            Denial::ProjectNotFound
        );
        assert_eq!(
            map_move_rejection(MoveRejection::SourceDenied),
            Denial::Forbidden
        );
        let rejection = map_move_domain_error(domain::DomainError::Work("busy".to_owned()));
        assert_eq!(
            rejection,
            Denial::MoveRejection {
                status: 409,
                message: "busy".to_owned(),
            }
        );
        assert_eq!(
            map_move_domain_error(domain::DomainError::Db(sea_orm::DbErr::Custom(
                "boom".to_owned()
            ))),
            Denial::ServerError
        );
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
