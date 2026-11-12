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
//! declare nothing else), the sea-query search statements (built here
//! over the Q3 param parsers and semantics; the Q3 SQL text stays as the
//! documented reference), their by-name row decoding, and the envelopes.
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
use sqlx::PgPool;
use uuid::Uuid;

use sea_orm::sea_query::extension::postgres::{PgExpr, PgFunc};
use sea_orm::sea_query::{
    Alias, BinOper, Condition, Expr, Func, JoinType, Order, Query as SeaQuery, SelectStatement,
    SimpleExpr, Value as SqlValue,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, DbErr, EntityTrait, FromQueryResult, QueryFilter,
    QueryOrder, QuerySelect, RelationTrait,
};

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_db::entity::{issue_comments, issues, project_members, projects, states, workspaces};
use pidash_services::app_views_search::fts;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::issues as domain_issues;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{WriteActor, WriteCtx, WriteScope};
use pidash_services::operations;
use pidash_services::v1_work_items::queries_search as q;

use super::handlers_social::{preamble, query_last, rewrite_project_id, QueryMap};
use super::perms::{gate_for, V1WorkItemsRoute};
use crate::domain_hooks::{AppMoveAdmissionCache, AppMovePostCommitHook, AppTaskEnqueueHook};
use crate::issue_transition::AppTransitionHook;
use crate::middleware::body_limit::read_request_bytes;
use crate::orchestration::SpanCreationDeps;
use crate::serializer::render_datetime;
use crate::state::AppState;
use crate::v1_cycles_modules::body as shared_body;
use crate::v1_cycles_modules::json::{
    parse_json_text, to_serde_publish, JsonFail, JSON_PARSE_PREFIX,
};
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

/// Map an operation failure onto the `Denial` arms the moved move handler
/// used (OVL-14, PIDASHCONV-1649). The move operation never returns the
/// issues-family 400/404/409 bodies, so those arms 500 instead of
/// rendering a body this module cannot spell.
impl From<operations::Error> for Denial {
    fn from(error: operations::Error) -> Self {
        match error {
            operations::Error::NotFound => Denial::NotFound,
            operations::Error::ProjectNotFound => Denial::ProjectNotFound,
            operations::Error::Rejection { status, message } => {
                Denial::MoveRejection { status, message }
            }
            operations::Error::ServerError => Denial::ServerError,
            operations::Error::BadError(_)
            | operations::Error::FieldErrors(_)
            | operations::Error::NotFoundBody(_)
            | operations::Error::Conflict(_) => Denial::ServerError,
        }
    }
}

/// Wrap an operation outcome in the v1 response.
fn op_response(outcome: operations::Outcome) -> Result<Response, Denial> {
    let status = StatusCode::from_u16(outcome.status).map_err(|_| Denial::ServerError)?;
    Ok(json_response(status, outcome.body))
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
// Search statements (sea-query over the Q3 semantics)
// ---------------------------------------------------------------------------

/// Run a built statement into typed rows, decoded by name.
async fn query_built<T: FromQueryResult>(
    db: &impl ConnectionTrait,
    stmt: &SelectStatement,
) -> Result<Vec<T>, DbErr> {
    let rows = db.query_all(DbBackend::Postgres.build(stmt)).await?;
    rows.iter()
        .map(|row| T::from_query_result(row, ""))
        .collect()
}

fn issue(column: issues::Column) -> Expr {
    Expr::col((issues::Entity, column))
}

fn state(column: states::Column) -> Expr {
    Expr::col((states::Entity, column))
}

fn project(column: projects::Column) -> Expr {
    Expr::col((projects::Entity, column))
}

fn workspace(column: workspaces::Column) -> Expr {
    Expr::col((workspaces::Entity, column))
}

fn member(column: project_members::Column) -> Expr {
    Expr::col((project_members::Entity, column))
}

/// An inline SQL literal, not a bind: the FTS vectors must parse to the
/// same expression tree as the `issues_fts_idx` / `issue_comments_fts_idx`
/// index expressions (`fts::issue_vector_sql`), and a bound parameter is
/// not a constant to the planner's generic plan.
fn literal<V: Into<SqlValue>>(value: V) -> SimpleExpr {
    SimpleExpr::Constant(value.into())
}

/// `'english'::regconfig` ([`fts::FTS_CONFIG`]).
fn regconfig() -> SimpleExpr {
    literal(fts::FTS_CONFIG).cast_as(Alias::new("regconfig"))
}

/// `websearch_to_tsquery('english'::regconfig, <query>)`
/// ([`fts::websearch_sql`]); websearch operators flow through untouched.
fn websearch(query: &str) -> SimpleExpr {
    Func::cust(Alias::new("websearch_to_tsquery"))
        .arg(regconfig())
        .arg(Expr::val(query))
        .into()
}

/// `ISSUE_SEARCH_VECTOR` ([`fts::issue_vector_sql`]): `to_tsvector(
/// 'english'::regconfig, COALESCE(name, '') || ' ' ||
/// COALESCE(description_stripped, ''))` over the outer `issues` row.
fn issue_vector() -> SimpleExpr {
    let text = SimpleExpr::from(Func::coalesce([
        issue(issues::Column::Name).into(),
        literal(""),
    ]))
    .concat(literal(" "))
    .concat(Func::coalesce([
        issue(issues::Column::DescriptionStripped).into(),
        literal(""),
    ]));
    Func::cust(Alias::new("to_tsvector"))
        .arg(regconfig())
        .arg(text)
        .into()
}

/// `ISSUE_COMMENT_SEARCH_VECTOR` ([`fts::comment_vector_sql`]).
fn comment_vector() -> SimpleExpr {
    let text = Func::coalesce([
        Expr::col((
            issue_comments::Entity,
            issue_comments::Column::CommentStripped,
        ))
        .into(),
        literal(""),
    ]);
    Func::cust(Alias::new("to_tsvector"))
        .arg(regconfig())
        .arg(text)
        .into()
}

/// `<vector> @@ websearch_to_tsquery(...)` ([`fts::fts_match_sql`]).
fn fts_match(vector: SimpleExpr, query: &str) -> SimpleExpr {
    vector.binary(BinOper::Custom("@@"), websearch(query))
}

/// The `icontains` arm ([`fts::name_fallback_sql`]): `ILIKE '%<escaped>%'`
/// with the pattern bound whole (`like` is already `escape_icontains`
/// output, so the added `%` are the only wildcards). No `ESCAPE` clause:
/// the backslash the helper escapes with is Postgres's default `LIKE`
/// escape, exactly as Django renders it.
fn icontains(column: Expr, like: &str) -> SimpleExpr {
    column.ilike(format!("%{like}%"))
}

/// The `_build_search_filter` OR-chain ([`fts::search_filter_sql`]) in
/// Django arm order: FTS, name fallback, optional comment arm (soft-deleted
/// comments excluded, no `access` filter — BUG-7), sequence ints,
/// project identifier. `None` when the query is empty
/// ([`fts::search_applies`]).
fn search_filter(query: &str, include_comments: bool) -> Option<Condition> {
    if !fts::search_applies(query) {
        return None;
    }
    let binds = q::search_text_binds(query);
    let mut chain = Condition::any()
        .add(fts_match(issue_vector(), &binds.fts))
        .add(icontains(issue(issues::Column::Name), &binds.like));
    if include_comments {
        chain = chain.add(
            issue(issues::Column::Id).in_subquery(
                SeaQuery::select()
                    .column((issue_comments::Entity, issue_comments::Column::IssueId))
                    .from(issue_comments::Entity)
                    .cond_where(
                        Condition::all()
                            .add(
                                Expr::col((
                                    issue_comments::Entity,
                                    issue_comments::Column::DeletedAt,
                                ))
                                .is_null(),
                            )
                            .add(fts_match(comment_vector(), &binds.fts)),
                    )
                    .take(),
            ),
        );
    }
    Some(
        chain
            .add(issue(issues::Column::SequenceId).eq(PgFunc::any(
                Expr::val(binds.seq).cast_as(Alias::new("int[]")),
            )))
            .add(Expr::exists(
                SeaQuery::select()
                    .expr(literal(1))
                    .from(projects::Entity)
                    .cond_where(
                        Condition::all()
                            .add(
                                project(projects::Column::Id)
                                    .equals((issues::Entity, issues::Column::ProjectId)),
                            )
                            .add(icontains(
                                project(projects::Column::Identifier),
                                &binds.like,
                            )),
                    )
                    .take(),
            )),
    )
}

/// The `FROM` + joins both search endpoints share
/// ([`q::SEARCH_JOINS_SQL`]: `LEFT` states, `INNER` projects,
/// `INNER` project_members with no soft-delete arm, `INNER` workspaces)
/// and the manager + view scope ([`q::search_base_where`]): live,
/// non-triage (NULL-tolerant), unarchived issue and project, non-draft,
/// active membership of the caller, the workspace slug.
fn search_scope(slug: &str, member_id: Uuid) -> SelectStatement {
    let group = || state(states::Column::Group);
    let mut stmt = SeaQuery::select();
    stmt.from(issues::Entity)
        .join(
            JoinType::LeftJoin,
            states::Entity,
            issue(issues::Column::StateId).equals((states::Entity, states::Column::Id)),
        )
        .join(
            JoinType::InnerJoin,
            projects::Entity,
            issue(issues::Column::ProjectId).equals((projects::Entity, projects::Column::Id)),
        )
        .join(
            JoinType::InnerJoin,
            project_members::Entity,
            project(projects::Column::Id)
                .equals((project_members::Entity, project_members::Column::ProjectId)),
        )
        .join(
            JoinType::InnerJoin,
            workspaces::Entity,
            issue(issues::Column::WorkspaceId).equals((workspaces::Entity, workspaces::Column::Id)),
        )
        .cond_where(
            Condition::all()
                .add(issue(issues::Column::DeletedAt).is_null())
                .add(group().is_null().or(group().eq("triage").not()))
                .add(issue(issues::Column::ArchivedAt).is_null())
                .add(project(projects::Column::ArchivedAt).is_null())
                .add(issue(issues::Column::IsDraft).eq(false))
                .add(member(project_members::Column::IsActive).eq(true))
                .add(member(project_members::Column::MemberId).eq(member_id))
                .add(workspace(workspaces::Column::Slug).eq(slug)),
        );
    stmt
}

// ---------------------------------------------------------------------------
// Legacy search
// ---------------------------------------------------------------------------

/// The legacy statement's inputs (`views/issue.py:2696-2718`), parsed
/// before any statement runs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LegacyPlan {
    query: String,
    /// `AND "issues"."project_id" = ...` when the gate applies.
    project_id: Option<Uuid>,
    /// `[: int(limit)]` (quirks 1, 14).
    limit: i64,
}

/// Parse the legacy inputs. `LegacyProject::InvalidUuid` 400s (BUG-2: the
/// `ValidationError` raises at `filter()` time inside the handler, so
/// `handle_exception` answers 400 `handlers_social::VALID_DETAIL_BODY`,
/// byte-identical to Django); bad limits 500 (BUG-1).
fn legacy_plan(
    query: &str,
    workspace_search: Option<&str>,
    project_id: Option<&str>,
    limit: Option<&str>,
) -> Result<LegacyPlan, Denial> {
    let project = q::legacy_project_filter(workspace_search, project_id);
    if matches!(project, q::LegacyProject::InvalidUuid) {
        return Err(Denial::BadError(
            super::handlers_social::VALID_DETAIL_BODY.to_owned(),
        ));
    }
    let limit = legacy_limit(limit)?;
    Ok(LegacyPlan {
        query: query.to_owned(),
        project_id: match project {
            q::LegacyProject::Filter(id) => Some(id),
            _ => None,
        },
        limit,
    })
}

/// [`q::legacy_limit_sql`]'s literal as an integer. A literal past `int8`
/// errors inside Postgres (quirk 1: Django 500s on the overflow); it 500s
/// here before the statement, same status.
fn legacy_limit(raw: Option<&str>) -> Result<i64, Denial> {
    let sql = q::legacy_limit_sql(raw).map_err(|_| Denial::ServerError)?;
    sql.strip_prefix("LIMIT ")
        .and_then(|digits| digits.parse::<i64>().ok())
        .ok_or(Denial::ServerError)
}

/// The legacy statement ([`q::legacy_search_sql`]): `SELECT DISTINCT` the
/// six `values()` keys plus the `created_at` Django appends for
/// `DISTINCT` + `ORDER BY`, the scope, the FTS chain without comments,
/// the optional project arm, `Meta.ordering` (`-created_at`), the limit.
fn legacy_select(slug: &str, member_id: Uuid, plan: &LegacyPlan) -> SelectStatement {
    let mut stmt = search_scope(slug, member_id);
    stmt.distinct()
        .expr_as(issue(issues::Column::Name), Alias::new("name"))
        .expr_as(issue(issues::Column::Id), Alias::new("id"))
        .expr_as(issue(issues::Column::SequenceId), Alias::new("sequence_id"))
        .expr_as(
            project(projects::Column::Identifier),
            Alias::new("project_identifier"),
        )
        .expr_as(issue(issues::Column::ProjectId), Alias::new("project_id"))
        .expr_as(
            workspace(workspaces::Column::Slug),
            Alias::new("workspace_slug"),
        )
        .expr_as(issue(issues::Column::CreatedAt), Alias::new("created_at"))
        .cond_where(
            Condition::all()
                .add_option(search_filter(&plan.query, false))
                .add_option(
                    plan.project_id
                        .map(|id| issue(issues::Column::ProjectId).eq(id)),
                ),
        )
        .order_by((issues::Entity, issues::Column::CreatedAt), Order::Desc)
        .limit(plan.limit as u64);
    stmt
}

/// One decoded legacy `values()` row, in
/// [`LEGACY_VALUES_KEYS`](q::LEGACY_VALUES_KEYS) order.
#[derive(Debug, FromQueryResult)]
struct LegacyRow {
    name: String,
    id: Uuid,
    sequence_id: i32,
    project_identifier: String,
    project_id: Uuid,
    workspace_slug: String,
}

/// Fetch the legacy rows.
async fn fetch_legacy_rows(
    pool: &PgPool,
    slug: &str,
    member_id: &Uuid,
    plan: &LegacyPlan,
) -> Result<Vec<LegacyRow>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    query_built(&db, &legacy_select(slug, *member_id, plan))
        .await
        .map_err(|error| db_error(error, "search-legacy"))
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
    let plan = legacy_plan(
        &text,
        query_last(query, "workspace_search").as_deref(),
        query_last(query, "project_id").as_deref(),
        query_last(query, "limit").as_deref(),
    )?;
    let rows = fetch_legacy_rows(&pre.pool, slug, &pre.actor.id, &plan).await?;
    let rendered: Vec<Value> = rows
        .iter()
        .map(|row| Value::Object(render_legacy_row(row)))
        .collect();
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

/// The advanced statement's inputs (`views/issue.py:2809-2876`), parsed
/// in source order (query → sort → limit → project → status → since)
/// before any statement runs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AdvancedPlan {
    /// The stripped, non-empty `?q=`.
    query: String,
    /// A UUID-form or resolved identifier-form `?project=`.
    project_id: Option<Uuid>,
    status: q::StatusFilter,
    since: Option<DateTime<Utc>>,
    sort: q::Sort,
    /// The clamped `1..=50` limit.
    limit: i64,
}

/// `SearchHeadline` options (`search/issue.py:171-182`), the exact
/// literal Django binds.
const HEADLINE_OPTIONS: &str =
    "StartSel='<<', StopSel='>>', MaxWords=20, MinWords=10, ShortWord=3, HighlightAll=false";

/// Resolve the identifier-form `?project=` (`Project.resolve`,
/// `db/models/project.py:208-217`): strip + upper before the btree
/// equality lookup, live projects only, the workspace traversal, newest
/// first. A miss 404s before any statement runs; the UUID form never
/// 404s (a miss yields empty rows — Q3 quirk 4).
async fn resolve_advanced_project(pool: &PgPool, slug: &str, text: &str) -> Result<Uuid, Denial> {
    let normalized = q::normalize_project_identifier(text);
    let id: Option<Uuid> = projects::Entity::find()
        .select_only()
        .column(projects::Column::Id)
        .join(JoinType::InnerJoin, projects::Relation::Workspaces.def())
        .filter(projects::Column::DeletedAt.is_null())
        .filter(workspaces::Column::Slug.eq(slug))
        .filter(projects::Column::Identifier.eq(normalized))
        .order_by_desc(projects::Column::CreatedAt)
        .into_tuple()
        .one(&pidash_db::pool::sea_orm_connection(pool))
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

/// Parse the advanced inputs (`views/issue.py:2809-2876`) in source
/// order; the identifier-form project resolves here (a miss 404s).
async fn advanced_plan(
    pool: &PgPool,
    slug: &str,
    query: &QueryMap,
) -> Result<AdvancedPlan, Denial> {
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
    Ok(AdvancedPlan {
        query: text,
        project_id,
        status,
        since,
        sort,
        limit,
    })
}

/// The `?status=` predicate ([`q::status_where_sql`]): `closed` is the
/// closed groups; `open` is the open groups OR a NULL state (the positive
/// list keeps stateless issues visible); `all` is none.
fn status_condition(status: q::StatusFilter) -> Option<Condition> {
    let group = || state(states::Column::Group);
    match status {
        q::StatusFilter::All => None,
        q::StatusFilter::Closed => {
            Some(Condition::all().add(group().is_in(q::closed_state_groups().iter().copied())))
        }
        q::StatusFilter::Open => Some(
            Condition::any()
                .add(group().is_in(q::open_state_groups().iter().copied()))
                .add(issue(issues::Column::StateId).is_null()),
        ),
    }
}

/// The advanced statement ([`q::advanced_search_sql`]): `SELECT DISTINCT`
/// the fourteen `values()` keys (`_headline` / `_rank` via `ts_headline` /
/// `ts_rank`, [`fts::headline_sql`] / [`fts::rank_sql`]), the scope, then
/// the arms in source application order (project → status → since → FTS
/// with comments), the sort ([`q::sort_order_sql`]), the clamped limit.
/// Every output column carries a unique name (`name` repeats across
/// issues/states/projects in Django's `values()`), so the row decodes by
/// name.
fn advanced_select(slug: &str, member_id: Uuid, plan: &AdvancedPlan) -> SelectStatement {
    let mut stmt = search_scope(slug, member_id);
    let headline = Func::cust(Alias::new("ts_headline"))
        .arg(regconfig())
        .arg(issue(issues::Column::DescriptionStripped))
        .arg(websearch(&plan.query))
        .arg(literal(HEADLINE_OPTIONS));
    let rank = Func::cust(Alias::new("ts_rank"))
        .arg(issue_vector())
        .arg(websearch(&plan.query));
    stmt.distinct()
        .expr_as(issue(issues::Column::Id), Alias::new("id"))
        .expr_as(issue(issues::Column::SequenceId), Alias::new("sequence_id"))
        .expr_as(issue(issues::Column::Name), Alias::new("name"))
        .expr_as(headline, Alias::new("fts_headline"))
        .expr_as(rank, Alias::new("fts_rank"))
        .expr_as(issue(issues::Column::CreatedAt), Alias::new("created_at"))
        .expr_as(issue(issues::Column::UpdatedAt), Alias::new("updated_at"))
        .expr_as(
            issue(issues::Column::CompletedAt),
            Alias::new("completed_at"),
        )
        .expr_as(state(states::Column::Name), Alias::new("state_name"))
        .expr_as(state(states::Column::Group), Alias::new("state_group"))
        .expr_as(issue(issues::Column::ProjectId), Alias::new("project_id"))
        .expr_as(
            project(projects::Column::Identifier),
            Alias::new("project_identifier"),
        )
        .expr_as(project(projects::Column::Name), Alias::new("project_name"))
        .expr_as(
            workspace(workspaces::Column::Slug),
            Alias::new("workspace_slug"),
        )
        .cond_where(
            Condition::all()
                .add_option(
                    plan.project_id
                        .map(|id| issue(issues::Column::ProjectId).eq(id)),
                )
                .add_option(status_condition(plan.status))
                .add_option(
                    plan.since
                        .map(|since| issue(issues::Column::UpdatedAt).gte(since)),
                )
                .add_option(search_filter(&plan.query, true)),
        );
    match plan.sort {
        q::Sort::Rank => stmt
            .order_by(Alias::new("fts_rank"), Order::Desc)
            .order_by((issues::Entity, issues::Column::CreatedAt), Order::Desc),
        q::Sort::Created => stmt.order_by((issues::Entity, issues::Column::CreatedAt), Order::Desc),
        q::Sort::Updated => stmt.order_by((issues::Entity, issues::Column::UpdatedAt), Order::Desc),
    };
    stmt.limit(plan.limit as u64);
    stmt
}

/// One advanced `values()` row (`:2860-2875`), decoded by name.
#[derive(Debug, FromQueryResult)]
struct AdvancedFetched {
    id: Uuid,
    sequence_id: i32,
    name: String,
    fts_headline: Option<String>,
    /// `ts_rank`'s `real`.
    fts_rank: Option<f32>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    state_name: Option<String>,
    state_group: Option<String>,
    project_id: Uuid,
    project_identifier: String,
    project_name: String,
    workspace_slug: String,
}

/// Fetch the advanced rows.
async fn fetch_advanced_rows(
    pool: &PgPool,
    slug: &str,
    member_id: &Uuid,
    plan: &AdvancedPlan,
) -> Result<Vec<AdvancedFetched>, Denial> {
    let db = pidash_db::pool::sea_orm_connection(pool);
    query_built(&db, &advanced_select(slug, *member_id, plan))
        .await
        .map_err(|error| db_error(error, "search-advanced"))
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

/// Render one advanced row's strings. Datetimes render UTC with a `Z`
/// suffix: the view copies raw `.values()` datetimes into a plain dict
/// (`:2912-2914`) with no DRF `DateTimeField`, so `TimezoneMixin` has
/// no effect and DRF's `JSONEncoder` emits `isoformat()` with `+00:00`
/// rewritten to `Z`.
fn decode_advanced_row(row: AdvancedFetched) -> (AdvancedDecoded, i32, Option<f32>) {
    (
        AdvancedDecoded {
            id: row.id.to_string(),
            name: row.name,
            headline: row.fts_headline,
            created_at: render_datetime(&row.created_at),
            updated_at: render_datetime(&row.updated_at),
            completed_at: row.completed_at.as_ref().map(render_datetime),
            state_name: row.state_name,
            state_group: row.state_group,
            project_id: row.project_id.to_string(),
            project_identifier: row.project_identifier,
            project_name: row.project_name,
            workspace_slug: row.workspace_slug,
        },
        row.sequence_id,
        row.fts_rank,
    )
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
    let plan = advanced_plan(&pre.pool, slug, query).await?;
    let rows = fetch_advanced_rows(&pre.pool, slug, &pre.actor.id, &plan).await?;
    let text = plan.query.clone();
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
    let mut rendered = Vec::with_capacity(rows.len());
    for row in rows {
        let (decoded, sequence_id, rank) = decode_advanced_row(row);
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
    activate_timezone(pre.actor.timezone.as_deref())?;
    // `request.data` parses lazily in the view — after auth, gate, and
    // timezone — so a denying gate answers before a malformed body 400s.
    let target_ref = read_move_target(&parts.headers, bytes)?;
    let origin = move_origin(state)?;
    let web_base = pidash_services::v1_work_items::shape_issue::web_base_url(
        state.settings().urls.web_url.as_deref(),
        state.settings().urls.app_base_url.as_deref(),
    );
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
    let deps = operations::Deps {
        pool: pre.pool.clone(),
        orm: orm.clone(),
        transition,
        enqueue,
    };
    let span_deps = SpanCreationDeps::from_state(state);
    let drain: Arc<dyn domain_issues::MovePostCommitHook> = Arc::new(AppMovePostCommitHook::new(
        &pre.pool,
        state,
        span_deps.cache.clone(),
    ));
    let creation = domain_issues::CreationDeps {
        pool: pre.pool.clone(),
        cloud: span_deps.cloud.clone(),
        managed: span_deps.managed.clone(),
        now_unix_secs: span_deps.now_unix_secs,
        toolsets: span_deps.toolsets.clone(),
        assistant_seam: span_deps.assistant_seam.clone(),
        admission: Arc::new(AppMoveAdmissionCache::new(span_deps.cache)),
    };
    let write = operations::issues::MoveWrite {
        slug,
        project_id,
        pk: *pk,
        target_ref: &target_ref,
        origin: Some(origin),
        web_base,
    };
    // Exactly the Django behavior: the gate checked the source project
    // up front (target-only guards), no `updated_by` stamp, handoff on.
    let outcome = operations::issues::move_to_project(
        &deps,
        &ctx,
        &write,
        domain_issues::MoveOptions::django(),
        drain,
        creation,
    )
    .await?;
    op_response(outcome)
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
    let roles = project_member_roles(
        &pidash_db::pool::sea_orm_connection(pool),
        *workspace_id,
        *user_id,
        *project_id,
    )
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

/// The roles of the user's active, live `project_members` rows on the
/// project, workspace-scoped (`project.py:56-116`).
async fn project_member_roles<C: ConnectionTrait>(
    db: &C,
    workspace_id: Uuid,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<Vec<i16>, DbErr> {
    project_members::Entity::find()
        .select_only()
        .column(project_members::Column::Role)
        .filter(project_members::Column::WorkspaceId.eq(workspace_id))
        .filter(project_members::Column::MemberId.eq(user_id))
        .filter(project_members::Column::ProjectId.eq(project_id))
        .filter(project_members::Column::IsActive.eq(true))
        .filter(project_members::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::sea_query::PostgresQueryBuilder;

    /// F18-11 `calls.search.body`, compact separators (DRF wire bytes).
    const SEARCH_BODY: &str = "{\"issues\":[{\"name\":\"zxcvsearchtoken login flow\",\"id\":\"a7509d00-345f-47fb-bee3-6bcf7d3339e2\",\"sequence_id\":1,\"project__identifier\":\"CT00003\",\"project_id\":\"d715be3d-234f-46ef-89a3-97f0c7c04b7e\",\"workspace__slug\":\"ws-conv659-2\"}]}";

    /// F18-11 `calls.search_advanced.results_full[0]`, compact separators.
    const ADVANCED_ROW_BODY: &str = "{\"id\":\"a7509d00-345f-47fb-bee3-6bcf7d3339e2\",\"sequence_id\":1,\"identifier\":\"CT00003-1\",\"name\":\"zxcvsearchtoken login flow\",\"snippet\":\"zxcvsearchtoken the login flow breaks on retry with session expiry\",\"state\":{\"name\":\"Todo\",\"group\":\"unstarted\"},\"project\":{\"id\":\"d715be3d-234f-46ef-89a3-97f0c7c04b7e\",\"identifier\":\"CT00003\",\"name\":\"Conv659 Project\"},\"workspace_slug\":\"ws-conv659-2\",\"created_at\":\"2026-10-02T23:17:12.025108Z\",\"updated_at\":\"2026-10-02T23:17:12.025108Z\",\"completed_at\":null,\"rank\":0.075990885,\"url\":\"http://127.0.0.1:18359/ws-conv659-2/browse/CT00003-1\"}";

    /// F18-11 `calls.search_advanced` envelope + results, compact separators.
    const ADVANCED_ENVELOPE_BODY: &str = "{\"query\":\"zxcvsearchtoken\",\"count\":1,\"results\":[{\"id\":\"a7509d00-345f-47fb-bee3-6bcf7d3339e2\",\"sequence_id\":1,\"identifier\":\"CT00003-1\",\"name\":\"zxcvsearchtoken login flow\",\"snippet\":\"zxcvsearchtoken the login flow breaks on retry with session expiry\",\"state\":{\"name\":\"Todo\",\"group\":\"unstarted\"},\"project\":{\"id\":\"d715be3d-234f-46ef-89a3-97f0c7c04b7e\",\"identifier\":\"CT00003\",\"name\":\"Conv659 Project\"},\"workspace_slug\":\"ws-conv659-2\",\"created_at\":\"2026-10-02T23:17:12.025108Z\",\"updated_at\":\"2026-10-02T23:17:12.025108Z\",\"completed_at\":null,\"rank\":0.075990885,\"url\":\"http://127.0.0.1:18359/ws-conv659-2/browse/CT00003-1\"}]}";

    const FIXTURE_PROJECT: &str = "d715be3d-234f-46ef-89a3-97f0c7c04b7e";

    fn render(stmt: &SelectStatement) -> String {
        stmt.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn legacy_select_pins_scope_joins_and_values() {
        let plan = legacy_plan("zxcvsearchtoken", None, None, None).expect("plan");
        assert_eq!(plan.project_id, None);
        assert_eq!(plan.limit, 10);
        let sql = render(&legacy_select("acme", Uuid::nil(), &plan));
        assert!(sql.starts_with("SELECT DISTINCT \"issues\".\"name\" AS \"name\", \"issues\".\"id\" AS \"id\", \"issues\".\"sequence_id\" AS \"sequence_id\", \"projects\".\"identifier\" AS \"project_identifier\", \"issues\".\"project_id\" AS \"project_id\", \"workspaces\".\"slug\" AS \"workspace_slug\", \"issues\".\"created_at\" AS \"created_at\" FROM \"issues\""), "{sql}");
        assert!(sql.contains(r#"LEFT JOIN "states" ON "issues"."state_id" = "states"."id" INNER JOIN "projects" ON "issues"."project_id" = "projects"."id" INNER JOIN "project_members" ON "projects"."id" = "project_members"."project_id" INNER JOIN "workspaces" ON "issues"."workspace_id" = "workspaces"."id""#), "{sql}");
        // Manager + view scope, in order.
        assert!(sql.contains(r#""issues"."deleted_at" IS NULL AND ("states"."group" IS NULL OR (NOT "states"."group" = 'triage')) AND "issues"."archived_at" IS NULL AND "projects"."archived_at" IS NULL AND "issues"."is_draft" = FALSE AND "project_members"."is_active" = TRUE AND "project_members"."member_id" = '00000000-0000-0000-0000-000000000000' AND "workspaces"."slug" = 'acme'"#), "{sql}");
        // The FTS vectors carry inline literals (index-expression parity).
        assert!(sql.contains("to_tsvector(CAST('english' AS regconfig), COALESCE(\"issues\".\"name\", '') || ' ' || COALESCE(\"issues\".\"description_stripped\", '')) @@ websearch_to_tsquery(CAST('english' AS regconfig), 'zxcvsearchtoken')"), "{sql}");
        assert!(
            sql.contains(r#"OR ("issues"."name" ILIKE '%zxcvsearchtoken%') OR"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""issues"."sequence_id" = ANY(CAST('{}' AS int[]))"#),
            "{sql}"
        );
        assert!(sql.contains(r#"EXISTS(SELECT 1 FROM "projects" WHERE "projects"."id" = "issues"."project_id" AND ("projects"."identifier" ILIKE"#), "{sql}");
        // Legacy: no comment arm, no project arm.
        assert!(!sql.contains("issue_comments"), "{sql}");
        assert!(!sql.contains(r#""issues"."project_id" = '"#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "issues"."created_at" DESC LIMIT 10"#),
            "{sql}"
        );
    }

    #[test]
    fn legacy_select_adds_project_arm_and_limit() {
        let plan =
            legacy_plan("q 42", Some("false"), Some(FIXTURE_PROJECT), Some("5")).expect("plan");
        assert_eq!(
            plan.project_id,
            Some(FIXTURE_PROJECT.parse().expect("uuid"))
        );
        let sql = render(&legacy_select("acme", Uuid::nil(), &plan));
        assert!(
            sql.contains(&format!(
                r#"AND "issues"."project_id" = '{FIXTURE_PROJECT}'"#
            )),
            "{sql}"
        );
        assert!(
            sql.contains(r#""issues"."sequence_id" = ANY(CAST(ARRAY [42] AS int[]))"#),
            "{sql}"
        );
        assert!(sql.ends_with("LIMIT 5"), "{sql}");
    }

    #[test]
    fn legacy_plan_empty_workspace_search_disables_filter() {
        // BUG-6: `workspace_search` matches the exact string `"false"`.
        let plan = legacy_plan("q", Some(""), Some(FIXTURE_PROJECT), None).expect("plan");
        assert_eq!(plan.project_id, None);
    }

    #[test]
    fn legacy_plan_invalid_project_is_400_valid_detail() {
        // BUG-2: non-UUID `project_id` 400s — the `ValidationError`
        // raises at `filter()` time inside the handler, so Django's
        // `handle_exception` answers 400 (live-probed, byte-identical).
        let denial = legacy_plan("q", Some("false"), Some("NOPE"), None)
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
    fn legacy_plan_bad_limit_500() {
        // BUG-1: garbage, negative and int8-overflowing limits 500.
        assert_eq!(
            legacy_plan("q", None, None, Some("abc")),
            Err(Denial::ServerError)
        );
        assert_eq!(
            legacy_plan("q", None, None, Some("-3")),
            Err(Denial::ServerError)
        );
        assert_eq!(
            legacy_plan("q", None, None, Some("99999999999999999999")),
            Err(Denial::ServerError)
        );
        assert_eq!(
            legacy_plan("q", None, None, Some(" 0_7 ")).map(|p| p.limit),
            Ok(7)
        );
        assert_eq!(
            legacy_plan("q", None, None, Some("-0")).map(|p| p.limit),
            Ok(0)
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

    fn plan(
        project: Option<Uuid>,
        status: q::StatusFilter,
        since: Option<DateTime<Utc>>,
        sort: q::Sort,
        limit: i64,
    ) -> AdvancedPlan {
        AdvancedPlan {
            query: "q".to_owned(),
            project_id: project,
            status,
            since,
            sort,
            limit,
        }
    }

    #[test]
    fn advanced_select_places_optional_arms_in_order() {
        let since = "2025-01-01T00:00:00Z"
            .parse::<DateTime<Utc>>()
            .expect("since");
        let project: Uuid = FIXTURE_PROJECT.parse().expect("uuid");
        let sql = render(&advanced_select(
            "acme",
            Uuid::nil(),
            &plan(
                Some(project),
                q::StatusFilter::All,
                Some(since),
                q::Sort::Rank,
                10,
            ),
        ));
        // The fourteen values() keys, every name unique.
        assert!(sql.contains(r#""issues"."name" AS "name", ts_headline(CAST('english' AS regconfig), "issues"."description_stripped", websearch_to_tsquery(CAST('english' AS regconfig), 'q'), E'StartSel=\'<<\', StopSel=\'>>\', MaxWords=20, MinWords=10, ShortWord=3, HighlightAll=false') AS "fts_headline", ts_rank(to_tsvector("#), "{sql}");
        assert!(sql.contains(r#""states"."name" AS "state_name", "states"."group" AS "state_group", "issues"."project_id" AS "project_id", "projects"."identifier" AS "project_identifier", "projects"."name" AS "project_name", "workspaces"."slug" AS "workspace_slug" FROM"#), "{sql}");
        // project → since → FTS (with the comment arm), after the scope.
        let project_at = sql.find(&format!(r#""workspaces"."slug" = 'acme' AND "issues"."project_id" = '{FIXTURE_PROJECT}' AND "issues"."updated_at" >= "#)).expect("project then since");
        let fts_at = sql.find("@@").expect("fts");
        assert!(project_at < fts_at, "{sql}");
        assert!(sql.contains(r#""issues"."id" IN (SELECT "issue_comments"."issue_id" FROM "issue_comments" WHERE "issue_comments"."deleted_at" IS NULL AND (to_tsvector(CAST('english' AS regconfig), COALESCE("issue_comments"."comment_stripped", '')) @@ websearch_to_tsquery(CAST('english' AS regconfig), 'q')))"#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "fts_rank" DESC, "issues"."created_at" DESC LIMIT 10"#),
            "{sql}"
        );

        let sql = render(&advanced_select(
            "acme",
            Uuid::nil(),
            &plan(None, q::StatusFilter::Open, None, q::Sort::Created, 50),
        ));
        assert!(!sql.contains(r#""issues"."project_id" = '"#), "{sql}");
        assert!(!sql.contains("updated_at\" >="), "{sql}");
        assert!(sql.contains(r#"AND ("states"."group" IN ('backlog', 'unstarted', 'started', 'review', 'test') OR "issues"."state_id" IS NULL) AND"#), "{sql}");
        assert!(
            sql.ends_with(r#"ORDER BY "issues"."created_at" DESC LIMIT 50"#),
            "{sql}"
        );

        let sql = render(&advanced_select(
            "acme",
            Uuid::nil(),
            &plan(None, q::StatusFilter::Closed, None, q::Sort::Updated, 1),
        ));
        assert!(
            sql.contains(r#"AND "states"."group" IN ('completed', 'cancelled') AND"#),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"ORDER BY "issues"."updated_at" DESC LIMIT 1"#),
            "{sql}"
        );
    }

    /// The search statements and the two scalar reads against temp copies
    /// on one connection (FTS runs without the GIN indexes).
    #[tokio::test]
    async fn live_search_reads() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        for table in [
            "issues",
            "states",
            "projects",
            "project_members",
            "workspaces",
            "issue_comments",
        ] {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(&pool)
            .await
            .expect("temp table");
        }
        let (ws, other_ws, user, stranger) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let (acme, zeta, other) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (todo, done, triage) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (a, b, c, archived, draft, triaged, foreign) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        for sql in [
            format!("INSERT INTO workspaces (id, slug) VALUES ('{ws}', 'acme'), ('{other_ws}', 'other')"),
            format!(
                "INSERT INTO projects (id, workspace_id, identifier, name, created_at, deleted_at, archived_at) VALUES \
                 ('{acme}', '{ws}', 'ACME', 'Acme Proj', '2025-01-01T00:00:00Z', NULL, NULL), \
                 ('{zeta}', '{ws}', 'ZETA', 'Zeta Proj', '2025-01-02T00:00:00Z', NULL, NULL), \
                 ('{other}', '{other_ws}', 'ACME', 'Other Acme', '2025-01-03T00:00:00Z', NULL, NULL)"
            ),
            format!(
                "INSERT INTO project_members (id, workspace_id, project_id, member_id, role, is_active, deleted_at) VALUES \
                 (gen_random_uuid(), '{ws}', '{acme}', '{user}', 20, true, NULL), \
                 (gen_random_uuid(), '{ws}', '{acme}', '{user}', 5, true, now()), \
                 (gen_random_uuid(), '{ws}', '{zeta}', '{user}', 15, false, NULL), \
                 (gen_random_uuid(), '{other_ws}', '{other}', '{stranger}', 15, true, NULL)"
            ),
            format!(
                "INSERT INTO states (id, name, \"group\") VALUES ('{todo}', 'Todo', 'unstarted'), ('{done}', 'Done', 'completed'), ('{triage}', 'Triage', 'triage')"
            ),
            format!(
                "INSERT INTO issues (id, name, description_stripped, sequence_id, project_id, workspace_id, state_id, created_at, updated_at, completed_at, deleted_at, archived_at, is_draft) VALUES \
                 ('{a}', 'zxcv login flow', 'the zxcv login breaks on retry', 1, '{acme}', '{ws}', '{todo}', '2025-03-01T00:00:00Z', '2025-03-05T00:00:00Z', NULL, NULL, NULL, false), \
                 ('{b}', 'plain task', 'nothing here', 42, '{acme}', '{ws}', '{done}', '2025-03-02T00:00:00Z', '2025-03-02T00:00:00Z', '2025-03-03T00:00:00Z', NULL, NULL, false), \
                 ('{c}', 'stateless zxcv', NULL, 7, '{acme}', '{ws}', NULL, '2025-03-03T00:00:00Z', '2025-03-03T00:00:00Z', NULL, NULL, NULL, false), \
                 ('{archived}', 'zxcv archived', NULL, 8, '{acme}', '{ws}', '{todo}', '2025-03-04T00:00:00Z', '2025-03-04T00:00:00Z', NULL, NULL, '2025-03-04', false), \
                 ('{draft}', 'zxcv draft', NULL, 9, '{acme}', '{ws}', '{todo}', '2025-03-05T00:00:00Z', '2025-03-05T00:00:00Z', NULL, NULL, NULL, true), \
                 ('{triaged}', 'zxcv triage', NULL, 10, '{acme}', '{ws}', '{triage}', '2025-03-06T00:00:00Z', '2025-03-06T00:00:00Z', NULL, NULL, NULL, false), \
                 ('{foreign}', 'zxcv foreign', NULL, 11, '{other}', '{other_ws}', '{todo}', '2025-03-07T00:00:00Z', '2025-03-07T00:00:00Z', NULL, NULL, NULL, false)"
            ),
            format!(
                "INSERT INTO issue_comments (id, issue_id, comment_stripped, deleted_at) VALUES \
                 (gen_random_uuid(), '{b}', 'a zxcv mention in a comment', NULL), \
                 (gen_random_uuid(), '{c}', 'deleted zxcv comment', now())"
            ),
        ] {
            sqlx::query(&sql).execute(&pool).await.expect("seed");
        }

        // Legacy: FTS + name fallback, newest first; no comment arm; the
        // manager scope drops archived/draft/triage, membership drops the
        // other workspace.
        let plan = legacy_plan("zxcv", None, None, None).expect("plan");
        let rows = fetch_legacy_rows(&pool, "acme", &user, &plan)
            .await
            .expect("legacy");
        let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
        assert_eq!(ids, vec![c, a]);
        assert_eq!(rows[1].name, "zxcv login flow");
        assert_eq!(rows[1].sequence_id, 1);
        assert_eq!(rows[1].project_identifier, "ACME");
        assert_eq!(rows[1].project_id, acme);
        assert_eq!(rows[1].workspace_slug, "acme");
        // Sequence arm and the identifier arm.
        let plan = legacy_plan("42", None, None, None).expect("plan");
        let rows = fetch_legacy_rows(&pool, "acme", &user, &plan)
            .await
            .expect("legacy");
        assert_eq!(rows.iter().map(|row| row.id).collect::<Vec<_>>(), vec![b]);
        let plan = legacy_plan("acm", None, None, Some("2")).expect("plan");
        let rows = fetch_legacy_rows(&pool, "acme", &user, &plan)
            .await
            .expect("legacy");
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![c, b]
        );
        // Project arm; the inactive membership on zeta hides it entirely.
        let plan = legacy_plan("zxcv", Some("false"), Some(&acme.to_string()), None).expect("plan");
        assert_eq!(
            fetch_legacy_rows(&pool, "acme", &user, &plan)
                .await
                .expect("legacy")
                .len(),
            2
        );
        let plan = legacy_plan("zxcv", Some("false"), Some(&zeta.to_string()), None).expect("plan");
        assert!(fetch_legacy_rows(&pool, "acme", &user, &plan)
            .await
            .expect("legacy")
            .is_empty());
        // The stranger sees nothing under this slug; an unknown slug is empty.
        let plan = legacy_plan("zxcv", None, None, None).expect("plan");
        assert!(fetch_legacy_rows(&pool, "acme", &stranger, &plan)
            .await
            .expect("legacy")
            .is_empty());
        assert!(fetch_legacy_rows(&pool, "nope", &user, &plan)
            .await
            .expect("legacy")
            .is_empty());

        // Advanced: the comment arm surfaces b (rank 0, so after the
        // ranked hits; ties by created_at desc), headline/rank decode.
        let rank_plan = |status, since, sort| AdvancedPlan {
            query: "zxcv".to_owned(),
            project_id: None,
            status,
            since,
            sort,
            limit: 10,
        };
        let rows = fetch_advanced_rows(
            &pool,
            "acme",
            &user,
            &rank_plan(q::StatusFilter::All, None, q::Sort::Rank),
        )
        .await
        .expect("advanced");
        let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[2], b, "the comment-only hit ranks 0 and sorts last");
        assert!(ids[..2].contains(&a) && ids[..2].contains(&c));
        let hit_a = rows.iter().find(|row| row.id == a).expect("a");
        assert!(hit_a.fts_rank.expect("rank") > 0.0);
        assert!(hit_a
            .fts_headline
            .as_deref()
            .expect("headline")
            .contains("<<zxcv>>"));
        assert_eq!(hit_a.state_name.as_deref(), Some("Todo"));
        assert_eq!(hit_a.state_group.as_deref(), Some("unstarted"));
        assert_eq!(hit_a.project_name, "Acme Proj");
        assert_eq!(hit_a.updated_at.to_rfc3339(), "2025-03-05T00:00:00+00:00");
        let hit_b = rows.iter().find(|row| row.id == b).expect("b");
        assert_eq!(hit_b.fts_rank, Some(0.0));
        assert!(hit_b.completed_at.is_some());
        let hit_c = rows.iter().find(|row| row.id == c).expect("c");
        assert_eq!(
            (hit_c.state_name.as_deref(), hit_c.fts_headline.as_deref()),
            (None, None)
        );
        // Status: open keeps the stateless row; closed keeps the done one.
        let rows = fetch_advanced_rows(
            &pool,
            "acme",
            &user,
            &rank_plan(q::StatusFilter::Open, None, q::Sort::Created),
        )
        .await
        .expect("advanced");
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![c, a]
        );
        let rows = fetch_advanced_rows(
            &pool,
            "acme",
            &user,
            &rank_plan(q::StatusFilter::Closed, None, q::Sort::Updated),
        )
        .await
        .expect("advanced");
        assert_eq!(rows.iter().map(|row| row.id).collect::<Vec<_>>(), vec![b]);
        // Since: updated_at >=.
        let since = "2025-03-03T00:00:00Z"
            .parse::<DateTime<Utc>>()
            .expect("since");
        let rows = fetch_advanced_rows(
            &pool,
            "acme",
            &user,
            &rank_plan(q::StatusFilter::All, Some(since), q::Sort::Updated),
        )
        .await
        .expect("advanced");
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![a, c]
        );
        // Limit.
        let mut one = rank_plan(q::StatusFilter::All, None, q::Sort::Created);
        one.limit = 1;
        assert_eq!(
            fetch_advanced_rows(&pool, "acme", &user, &one)
                .await
                .expect("advanced")
                .len(),
            1
        );

        // The identifier resolver: strip + upper, live rows, this slug.
        let db = pidash_db::pool::sea_orm_connection(&pool);
        assert_eq!(
            resolve_advanced_project(&pool, "acme", " acme ").await,
            Ok(acme)
        );
        assert_eq!(
            resolve_advanced_project(&pool, "other", "ACME").await,
            Ok(other)
        );
        assert_eq!(
            resolve_advanced_project(&pool, "acme", "NOPE").await,
            Err(Denial::ProjectNotFound)
        );
        // The gate's roles read.
        assert_eq!(
            project_member_roles(&db, ws, user, acme)
                .await
                .expect("roles"),
            vec![20]
        );
        assert!(project_member_roles(&db, ws, user, zeta)
            .await
            .expect("roles")
            .is_empty());
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
    fn social_project_not_found_maps_through() {
        // The identifier-rewrite miss must 404, not 500: `rewrite_project_id`
        // fails `Social::ProjectNotFound` and `?` converts it here.
        let local: Denial = crate::v1_work_items::handlers_social::Denial::ProjectNotFound.into();
        assert_eq!(local, Denial::ProjectNotFound);
    }
}
