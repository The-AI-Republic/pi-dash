//! Space public issue list + retrieve handlers (PIDASHCONV-175, stage 4).
//!
//! Port of `ProjectIssuesPublicEndpoint.get`
//! (`apps/api/pi_dash/space/views/issue.py:76-211`, route
//! `anchor/<anchor>/issues/` from `space/urls/project.py`) and
//! `IssueRetrievePublicEndpoint.get` (`views/issue.py:597-773`, route
//! `anchor/<anchor>/issues/<uuid:issue_id>/` from `space/urls/issue.py`).
//! Both are `AllowAny` GETs (`:74`, `:595`); every other method on the two
//! paths proxies to Django through [`owned`](super::owned) (DRF
//! authenticates before it checks the method, so answering 405 in Rust
//! would break the contract).
//!
//! Layering: SQL fragments come from
//! [`issue_list`](pidash_services::space::queries::issue_list) and
//! [`issue_retrieve`](pidash_services::space::queries::issue_retrieve),
//! anchor/error mapping from [`guards`](pidash_services::space::guards),
//! and window/grouping math from the F-07 paginator kernel
//! ([`crate::paginator`]). This module owns the HTTP shell (routes,
//! session timezone, row fetching, envelope) plus the SQL assembly the
//! builders leave to the handler (documented `$N` binding order, the
//! `{{base}}` join/where block, group-value branches).
//!
//! Reads go through `row_to_json` (the `project_meta` pattern): Postgres
//! renders every column and the handlers shape typed values from the JSON
//! object, so key order and scalar bytes stay under handler control. No
//! datetimes cross either response (the list projection and the 23-key
//! retrieve values carry dates, never timestamps), so the request timezone
//! is resolved only for its failure edge (an invalid stored
//! `user_timezone` is a 500, `views/base.py:37-42`).
//!
//! Ported bugs and quirks (translate, don't redesign; also listed in the PR):
//!
//! * BUG-avatar-list (`grouper.py:124-135,158-169` vs
//!   `issue_list::{vote_items,reaction_items}_annotation_sql`): the
//!   builders read `"vote_actor"."avatar_asset"` /
//!   `"reaction_actor"."avatar_asset"`, but `User.avatar_asset` is a
//!   ForeignKey (`db/models/user.py:69`) so the column is
//!   `avatar_asset_id` (cf. the R1 vote branch, the license
//!   `USER_SHAPE_SELECT`, `db/src/space/columns.rs:332`). The builders'
//!   text cannot execute; [`list_vote_items_sql`] /
//!   [`list_reaction_items_sql`] inline the corrected aggregate (same
//!   `CASE`/filter shape, `_id` columns). Reconciliation is tracked by
//!   PIDASHCONV-234.
//! * BUG-avatar-retrieve (`views/issue.py:713,716,722`): the
//!   `reaction_items` `avatar_url` `When`s read the VOTE actor
//!   (`votes__actor__*`) instead of `issue_reactions__actor__*`. Live
//!   Django resolves that traversal through the users join it reuses from
//!   `vote_items` (same `votes__actor` prefix), i.e. the `vote_actor`
//!   alias — never a column on `votes` itself (`issue_votes` has only
//!   `issue`/`actor`/`vote`, `db/models/issue.py:780-800`). The
//!   `issue_retrieve` builder and the `issue_retrieve.sql` fixture pin the
//!   unexecutable `"votes"."actor_avatar_asset"` / `"votes"."actor_avatar"`
//!   refs; [`retrieve_reaction_items_sql`] emits the executable equivalent
//!   (`vote_actor.avatar_asset_id` / `vote_actor.avatar`), preserving the
//!   copy-paste semantics. Reconciliation is tracked by PIDASHCONV-234.
//! * BUG-always-annotate (`space/utils/grouper.py:67`): the
//!   `default_annotations` guard is `or`, not `and`, so all three id-list
//!   annotations apply on every path — ported via
//!   [`default_annotation_keys`](pidash_services::space::queries::issue_list::default_annotation_keys).
//! * BUG-group-values-no-queryset (`grouper.py:233-250`): grouping by
//!   `target_date` / `start_date` / `created_by` calls
//!   `queryset.values_list(...)` on the default `queryset=None`, raising
//!   `AttributeError` (500 via the dispatch path). Ported: those fields
//!   answer the 500 envelope (see [`group_values_requires_queryset`](pidash_services::space::queries::issue_list::group_values_requires_queryset)).
//! * QUIRK-unscoped-board-get (`views/issue.py:598`): the retrieve board
//!   lookup is `.get(anchor=anchor)` with no `entity_name` scoping — kept
//!   via [`retrieve_board_sql`].
//! * QUIRK-first-null (`views/issue.py:771-773`): a retrieve miss is
//!   `.first()` → `None`, returned as `Response(None)` 200 — which DRF
//!   renders as an EMPTY body (`BaseRenderer.render(None)` returns
//!   `b''`, pinned by `test_issue_retrieve_other_tenant_is_not_found`),
//!   not JSON `null` and not a 404. Ported as 200 + empty body.
//! * QUIRK-dispatch-computed (intake precedent, verified live there): paths
//!   that raise through DRF's dispatch answer the dispatch-computed
//!   envelope. The retrieve board miss (`.get()` → `DoesNotExist`) answers
//!   the `ObjectDoesNotExist` 404 envelope via
//!   [`AnchorLookup::GetRaises`](pidash_services::space::guards::AnchorLookup).
//!
//! Non-obvious faithful corners:
//!
//! * The view checks `group_by == sub_group_by` (400) BEFORE `paginate`
//!   parses `per_page`/cursor (`views/issue.py:141` vs `paginator.py:654`)
//!   — the mismatch wins over malformed pagination input.
//! * `order_issue_queryset`'s `ORDER BY` is dead on every path: all three
//!   paginators re-order by the returned key (`paginator.py:136-139,
//!   :253-267, :454-472`). Only the returned key matters, mapped by
//!   [`order_key`](pidash_services::space::queries::issue_list::order_key);
//!   the base statement carries no `ORDER BY`.
//! * The base `.distinct()` (`views/issue.py:124`) is subsumed by
//!   `GROUP BY "issues"."id"` (+ the state group and any m2m group keys):
//!   same row multiset, and every m2m fanout collapses in the grouping.
//! * `link_count` / `attachment_count` / `sub_issues_count` are annotated
//!   on the base queryset but dropped by `issue_on_results`' `.values()`
//!   (`grouper.py:101-109`) — computed, never rendered. They are selected
//!   (builders verbatim) so the statement shape matches, and stripped
//!   before shaping.
//! * `issue_group_values`' `filters` argument is never read (`grouper.py:
//!   185-252`) — not threaded through.
//! * `?group_by=` (empty) is falsy → the ungrouped path, with the grouper
//!   still annotating (same as `False`).
//! * Relative date terms (`N_weeks`/`N_months`, `issue_filters.py:30-83`)
//!   resolve against `timezone.now().date()` (`TIME_ZONE = "UTC"`,
//!   `settings/common.py:362`); the queries layer skips them, so the
//!   handler expands them to absolute `;after`/plain bounds before
//!   compiling (last-wins among relatives, mirroring the dict overwrite).
//! * Filter params bind typed: psycopg sends untyped literals (Postgres
//!   coerces `integer = '1'`, `uuid = '…'`), while sqlx binds typed values
//!   (`integer = text` errors). The handler infers per value — UUID, then
//!   date, then integer, then text — which matches Django for every
//!   realistic input (`estimate_point` is a UUID FK, `status` is an int,
//!   date bounds are dates, `ILIKE` patterns stay text).

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::{Map, Value};
use sqlx::Row;

use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_services::space::guards::{self, AnchorLookup, ErrorBody, ExceptionKind};
use pidash_services::space::queries::issue_list as list_q;
use pidash_services::space::queries::project_meta::BOARD_COLUMNS;

use super::{guards_error, owned, QueryMap};
use crate::paginator::{self, Cursor, PageError, PageResponse};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the two public-issue GET routes under `api/public/`.
///
/// Nothing else: the sibling social paths in `urls/issue.py`
/// (PIDASHCONV-176) and every non-GET method stay unmatched and proxy to
/// Django. `HEAD` rides axum's `get` handling like Django's `GET`-backed
/// `HEAD`; `OPTIONS` proxies so DRF metadata is preserved.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/public/anchor/{anchor}/issues/",
            owned(axum::routing::get(list_issues), &["GET"]),
        )
        .route(
            "/api/public/anchor/{anchor}/issues/{issue_id}/",
            owned(axum::routing::get(retrieve_issue), &["GET"]),
        )
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Handler failure: the exact status + body Python answers. Variants map
/// one-to-one onto the guards layer (`resolve_anchor` for board misses,
/// `handle_exception` for the envelope): the view-inline 404/400s plus
/// DRF `ParseError` details for malformed pagination input.
#[derive(Debug)]
enum HandlerError {
    /// 404 `{"error":"Project is not published"}` — list board miss
    /// (`views/issue.py:80-82`, inline return, no dispatch involved).
    NotPublished,
    /// 404 `{"error":"The required object does not exist."}` — retrieve
    /// board miss (`.get()` → `DoesNotExist` → dispatch-computed).
    NotFound,
    /// 400 `{"detail": ...}` — malformed `per_page`/cursor
    /// (`ParseError`, `paginator.py:642-652,678`).
    BadDetail(String),
    /// 400 `{"error":"The required key does not exist."}` — sub-grouped
    /// plain grouper cell miss (`KeyError`, `paginator.py:596-604`).
    KeyError,
    /// 500 `{"error":"Something went wrong please try again later"}` —
    /// the fallback envelope (dispatch-computed responses, DB/row-shape
    /// failures, the `AttributeError` group-value branches).
    ServerError,
}

impl HandlerError {
    fn error_body(&self) -> ErrorBody {
        match self {
            HandlerError::NotPublished => guards::project_not_published(),
            HandlerError::NotFound => guards::handle_exception(ExceptionKind::ObjectDoesNotExist),
            HandlerError::BadDetail(message) => ErrorBody {
                status: 400,
                body: serde_json::json!({"detail": message}),
            },
            HandlerError::KeyError => guards::handle_exception(ExceptionKind::KeyError),
            HandlerError::ServerError => guards::handle_exception(ExceptionKind::Other),
        }
    }
}

impl IntoResponse for HandlerError {
    fn into_response(self) -> Response {
        if matches!(self, HandlerError::ServerError) {
            // `log_exception(e)` (`views/base.py:182-186`).
            tracing::warn!("space issues handler: internal error");
        }
        guards_error(self.error_body())
    }
}

/// Map a paginator-kernel error to its HTTP fate, mirroring the
/// `app_issues` `page_denial`: `BadPaginationError` subclasses become
/// `ParseError` 400s; lazy-queryset `ValueError`s and arithmetic errors
/// propagate to the generic 500.
fn page_denial(error: PageError) -> HandlerError {
    use PageError as E;
    match error {
        E::InvalidCursor
        | E::InvalidPerPage
        | E::PerPageTooLarge(_)
        | E::OffsetTooLarge
        | E::NegativeOffset => HandlerError::BadDetail(error.detail()),
        E::NegativeSlice | E::ZeroLimit | E::NonFiniteCursor | E::MissingOrderKey => {
            HandlerError::ServerError
        }
    }
}

/// Map a kernel grouping failure: an undeclared group cell is Python's
/// `KeyError` (400); a malformed row or seeded structure is a 500.
fn group_denial(error: paginator::GroupError) -> HandlerError {
    use paginator::GroupError as E;
    match error {
        E::UnknownGroup(_) => HandlerError::KeyError,
        E::MissingId | E::MissingGroupField(_) | E::MalformedCells => HandlerError::ServerError,
    }
}

// ---------------------------------------------------------------------------
// Shared plumbing (pool, session timezone, row fetching, typed binds)
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, HandlerError> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(HandlerError::ServerError)
}

/// The request's time zone (`TimezoneMixin.initial`, `views/base.py:31-42`).
/// Anonymous (no session, unknown/inactive user) → `deactivate()` → UTC
/// (`:42`); authenticated → `activate(ZoneInfo(user_timezone))` (`:39-40`)
/// with no `try/except`, so an unknown zone is a 500 through the fallback
/// branch. Neither response renders datetimes, so the value is only
/// resolved for that failure edge. Mirrors the merged `project_meta`
/// `request_tz` (same session shape; these routes are `AllowAny`).
async fn request_tz(
    pool: &sqlx::PgPool,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<chrono_tz::Tz, HandlerError> {
    let raw = extension
        .and_then(|axum::Extension(handle)| {
            handle
                .snapshot()
                .get("_auth_user_id")
                .and_then(|v| v.as_str().map(str::to_owned))
        })
        .and_then(|raw| raw.parse::<uuid::Uuid>().ok());
    let id = match raw {
        Some(id) => id,
        None => return Ok(chrono_tz::UTC),
    };
    let row: Option<(bool, Option<String>)> =
        sqlx::query_as("SELECT is_active, user_timezone FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    match row {
        Some((true, timezone)) => timezone
            .as_deref()
            .unwrap_or("")
            .parse::<chrono_tz::Tz>()
            .map_err(|_| HandlerError::ServerError),
        _ => Ok(chrono_tz::UTC),
    }
}

/// One bound `$N` parameter. UUID columns must bind typed `Uuid`
/// (`uuid = text` fails at runtime); integer columns must bind `i64`;
/// date bounds bind `NaiveDate`; everything else binds text. See
/// [`bind_typed`].
#[derive(Debug, Clone, PartialEq)]
enum SqlParam {
    Text(String),
    Uuid(uuid::Uuid),
    /// Typed SQL `NULL` (for nullable board ids: `= NULL` matches nothing,
    /// exactly like Django's `IS NULL` here since the columns compared are
    /// non-nullable).
    NullUuid,
    Int(i64),
    Date(chrono::NaiveDate),
    Timestamp(chrono::NaiveDateTime),
}

fn bind_query<'q>(
    sql: &'q str,
    params: &'q [SqlParam],
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    let mut query = sqlx::query(sql);
    for param in params {
        query = match param {
            SqlParam::Text(text) => query.bind(text),
            SqlParam::Uuid(id) => query.bind(*id),
            SqlParam::NullUuid => query.bind(None::<uuid::Uuid>),
            SqlParam::Int(value) => query.bind(*value),
            SqlParam::Date(value) => query.bind(*value),
            SqlParam::Timestamp(value) => query.bind(*value),
        };
    }
    query
}

/// Infer the bind type for one compiled filter value. Django (psycopg)
/// sends untyped literals so Postgres coerces (`integer = '1'`,
/// `uuid = '…'`); sqlx binds typed values, so the handler must pick the
/// type. Order is load-bearing: UUID-shaped tokens are always UUIDs
/// (`estimate_point` is a UUID FK, every `uuid_in` output parses);
/// strict `YYYY-MM-DD` tokens are date bounds (no other branch emits
/// them bare — `ILIKE` patterns wrap in `%`); integer tokens are
/// `status`/`estimate`… — `status` ints (`issue_intake.status` is an
/// `IntegerField`); the rest stay text. Mismatches (e.g. `?priority=1`)
/// bind text and fail in Postgres exactly like Django's `DataError`
/// (500 either way).
fn bind_typed(raw: &str) -> SqlParam {
    if let Ok(id) = raw.parse::<uuid::Uuid>() {
        return SqlParam::Uuid(id);
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return SqlParam::Date(date);
    }
    if let Ok(value) = raw.parse::<i64>() {
        return SqlParam::Int(value);
    }
    // Timestamp bounds (`created_at` terms accept datetimes): a typed
    // timestamp compares against `::date` through Postgres' implicit
    // `date → timestamp` cast, like Django's unknown literal.
    const STAMP_FORMATS: &[&str] = &[
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
    ];
    for format in STAMP_FORMATS {
        if let Ok(stamp) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return SqlParam::Timestamp(stamp);
        }
    }
    SqlParam::Text(raw.to_owned())
}

/// Fetch zero or one row of `inner` as a JSON object (`project_meta`
/// pattern: `row_to_json` keeps rendering under Postgres).
async fn fetch_optional_object(
    pool: &sqlx::PgPool,
    inner: &str,
    params: &[SqlParam],
) -> Result<Option<Value>, HandlerError> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let row: Option<sqlx::postgres::PgRow> = bind_query(&sql, params)
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    match row {
        None => Ok(None),
        Some(row) => {
            let text: String = row
                .try_get("__row")
                .map_err(|_| HandlerError::ServerError)?;
            let value: Value =
                serde_json::from_str(&text).map_err(|_| HandlerError::ServerError)?;
            match value {
                Value::Object(_) => Ok(Some(value)),
                _ => Err(HandlerError::ServerError),
            }
        }
    }
}

/// Fetch every row of `inner` as JSON objects.
async fn fetch_all_objects(
    pool: &sqlx::PgPool,
    inner: &str,
    params: &[SqlParam],
) -> Result<Vec<Value>, HandlerError> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let rows: Vec<sqlx::postgres::PgRow> = bind_query(&sql, params)
        .fetch_all(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row
            .try_get("__row")
            .map_err(|_| HandlerError::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| HandlerError::ServerError)?;
        match value {
            Value::Object(_) => out.push(value),
            _ => return Err(HandlerError::ServerError),
        }
    }
    Ok(out)
}

fn obj(value: &Value) -> Result<&serde_json::Map<String, Value>, HandlerError> {
    value.as_object().ok_or(HandlerError::ServerError)
}

fn opt_uuid(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<uuid::Uuid>, HandlerError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => s
            .parse::<uuid::Uuid>()
            .map(Some)
            .map_err(|_| HandlerError::ServerError),
        _ => Err(HandlerError::ServerError),
    }
}

// ---------------------------------------------------------------------------
// Anchor boards
// ---------------------------------------------------------------------------

/// The resolved tenant scope: the board's workspace and project. Both
/// board id columns are nullable (`DeployBoard.entity_identifier` and
/// `WorkspaceBaseModel.project` are `null=True`); a null id filters
/// `IS NULL`, matching nothing since the compared issue columns are
/// non-nullable.
struct BoardScope {
    workspace_id: uuid::Uuid,
    project_id: Option<uuid::Uuid>,
}

fn opt_project_param(project_id: &Option<uuid::Uuid>) -> SqlParam {
    match project_id {
        Some(id) => SqlParam::Uuid(*id),
        None => SqlParam::NullUuid,
    }
}

/// List board lookup: `DeployBoard.objects.filter(anchor=anchor,
/// entity_name="project").first()` (`views/issue.py:80`) with the default
/// `Meta.ordering = ("-created_at",)` (`db/models/deploy_board.py:57`) —
/// [`issue_list_board_sql`](list_q::issue_list_board_sql). A miss answers
/// the inline 404 (`:81-82`), never the dispatch path. `project_id` is the
/// board's `entity_identifier` (`:84`); the workspace id comes from the
/// board row (`:85` resolves `workspace.slug`, but the list only ever uses
/// the id — the slug itself is unread).
///
/// A live board on a soft-deleted workspace answers the 500 envelope:
/// Django's `deploy_board.workspace` hop (forward FK through the
/// soft-deletion manager) raises there.
async fn load_list_board(pool: &sqlx::PgPool, anchor: &str) -> Result<BoardScope, HandlerError> {
    let row = fetch_optional_object(
        pool,
        &list_q::issue_list_board_sql(),
        &[SqlParam::Text(anchor.to_owned())],
    )
    .await?
    .ok_or(HandlerError::NotPublished)?;
    let row = obj(&row)?;
    let workspace_id = opt_uuid(row, "workspace_id")?.ok_or(HandlerError::ServerError)?;
    // `entity_identifier` is the project id UUID (`:84`, nullable).
    let project_id = opt_uuid(row, "entity_identifier")?;
    // Forward-FK liveness (`deploy_board.workspace`, soft-deletion
    // manager): a dead workspace is a 500, not a 404.
    let live: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM \"workspaces\" WHERE (\"workspaces\".\"id\" = $1 AND \"workspaces\".\"deleted_at\" IS NULL)",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    if live.is_none() {
        return Err(HandlerError::ServerError);
    }
    Ok(BoardScope {
        workspace_id,
        project_id,
    })
}

/// Retrieve board lookup: `DeployBoard.objects.get(anchor=anchor)`
/// (`views/issue.py:598`) — deliberately NO `entity_name` scoping
/// (QUIRK-unscoped-board-get). A miss raises `DoesNotExist`, which the
/// dispatch path maps to the 404 envelope
/// ([`AnchorLookup::GetRaises`]); two rows would be
/// `MultipleObjectsReturned` (500), impossible while `anchor` stays
/// globally unique (`db/models/deploy_board.py:32`).
///
/// `workspace.slug` (`:600`) is read off the joined workspace row with the
/// same soft-deletion guard Django's forward-FK fetch applies — a dead
/// workspace answers the same 404 envelope (the hop raises
/// `ObjectDoesNotExist` there too). `project_id` is the board's
/// `project_id` FK (`:601`), NOT `entity_identifier`.
fn retrieve_board_sql() -> String {
    format!(
        "SELECT {} FROM \"deploy_boards\" INNER JOIN \"workspaces\" ON (\"deploy_boards\".\"workspace_id\" = \"workspaces\".\"id\" AND \"workspaces\".\"deleted_at\" IS NULL) WHERE (\"deploy_boards\".\"deleted_at\" IS NULL AND \"deploy_boards\".\"anchor\" = $1)",
        BOARD_COLUMNS
            .iter()
            .map(|col| format!("\"deploy_boards\".\"{col}\""))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

struct RetrieveBoard {
    scope: BoardScope,
    slug: String,
}

async fn load_retrieve_board(
    pool: &sqlx::PgPool,
    anchor: &str,
) -> Result<RetrieveBoard, HandlerError> {
    let sql = format!(
        "SELECT row_to_json(__r)::text AS __row FROM ({}) AS __r",
        retrieve_board_sql()
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(anchor)
        .fetch_all(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    if rows.len() > 1 {
        // `MultipleObjectsReturned` → the 500 fallback.
        return Err(HandlerError::ServerError);
    }
    let Some(row) = rows.first() else {
        // `.get()` miss → `DoesNotExist` → the dispatch-computed 404
        // envelope ([`AnchorLookup::GetRaises`]); `resolve_anchor` with
        // `board_found = false` returns exactly it.
        let _ = guards::resolve_anchor(AnchorLookup::GetRaises, false)
            .expect_err("board miss always denies");
        return Err(HandlerError::NotFound);
    };
    let text: String = row
        .try_get("__row")
        .map_err(|_| HandlerError::ServerError)?;
    let value: Value = serde_json::from_str(&text).map_err(|_| HandlerError::ServerError)?;
    let row = obj(&value)?;
    let workspace_id = opt_uuid(row, "workspace_id")?.ok_or(HandlerError::ServerError)?;
    // Nullable board FK (`WorkspaceBaseModel.project`, `null=True`).
    let project_id = opt_uuid(row, "project_id")?;
    // `workspace.slug` for the `workspace__slug` filter (`:600`).
    let slug = fetch_workspace_slug(pool, workspace_id).await?;
    Ok(RetrieveBoard {
        scope: BoardScope {
            workspace_id,
            project_id,
        },
        slug,
    })
}

async fn fetch_workspace_slug(
    pool: &sqlx::PgPool,
    workspace_id: uuid::Uuid,
) -> Result<String, HandlerError> {
    // Dead workspace: Django's `.workspace` hop raises
    // `ObjectDoesNotExist` → the same 404 envelope as a board miss.
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT \"slug\" FROM \"workspaces\" WHERE (\"id\" = $1 AND \"deleted_at\" IS NULL)",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    row.map(|row| row.0).ok_or(HandlerError::NotFound)
}

// ---------------------------------------------------------------------------
// Relative dates (`issue_filters.py:30-83`)
// ---------------------------------------------------------------------------

/// Date params carrying relative terms (`issue_filters.py:212-290`).
const DATE_PARAMS: &[&str] = &[
    "created_at",
    "updated_at",
    "completed_at",
    "start_date",
    "target_date",
];

/// Resolve one `N_weeks`/`N_months` head (`pattern = \d+_(weeks|months)$`,
/// `issue_filters.py:15`) against today, mirroring `string_date_filter`
/// (`:30-55`): months are `duration * 30` days (not calendar months).
/// Returns `(bound, after)` where `after` selects `>=`, else `<=`.
fn resolve_relative(
    head: &str,
    subsequent: &str,
    offset: &str,
    today: chrono::NaiveDate,
) -> Option<(String, bool)> {
    let (digit, term) = head.split_once('_')?;
    if term != "weeks" && term != "months" {
        return None;
    }
    let duration: i64 = digit.parse().ok()?;
    let days = if term == "weeks" {
        duration.checked_mul(7)?
    } else {
        duration.checked_mul(30)?
    };
    let from_now = offset == "fromnow";
    let bound = if subsequent == "after" {
        if from_now {
            today.checked_add_days(chrono::Days::new(days as u64))?
        } else {
            today.checked_sub_days(chrono::Days::new(days as u64))?
        }
    } else if from_now {
        today.checked_add_days(chrono::Days::new(days as u64))?
    } else {
        today.checked_sub_days(chrono::Days::new(days as u64))?
    };
    Some((bound.format("%Y-%m-%d").to_string(), subsequent == "after"))
}

/// Expand relative 3-part date terms (`head;subsequent;offset`,
/// `issue_filters.py:65-83`) into absolute 2-part terms against `today`
/// before compiling. Non-relative terms pass through untouched (the
/// queries layer compiles them, including the 3-part non-matching
/// `after`-else-`lte` rule).
///
/// Python accumulates bounds into one dict (`issue_filters.py:432`), so
/// same-direction relatives overwrite (last wins); the expansion keeps
/// that rule among relatives. Absolute terms keep the compiled AND
/// behavior (queries-layer territory).
fn expand_relative_dates(
    pairs: &[(String, String)],
    today: chrono::NaiveDate,
) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, raw)| {
            if !DATE_PARAMS.contains(&key.as_str()) {
                return (key.clone(), raw.clone());
            }
            let mut gte: Option<String> = None;
            let mut lte: Option<String> = None;
            let mut rest = Vec::new();
            for term in raw.split(',') {
                let parts: Vec<&str> = term.split(';').collect();
                if parts.len() == 3 {
                    if let Some((bound, after)) =
                        resolve_relative(parts[0], parts[1], parts[2], today)
                    {
                        if after {
                            gte = Some(bound);
                        } else {
                            lte = Some(bound);
                        }
                        continue;
                    }
                }
                rest.push(term.to_owned());
            }
            // Relatives resolve first (dict order is term order in
            // `date_filter`, but same-key overwrites make position
            // moot); absolute survivors keep their relative order.
            let mut out = Vec::new();
            if let Some(bound) = gte {
                out.push(format!("{bound};after"));
            }
            if let Some(bound) = lte {
                out.push(format!("{bound};before"));
            }
            out.extend(rest);
            (key.clone(), out.join(","))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// List statement assembly
// ---------------------------------------------------------------------------

/// The fixed join block. Every table a filter conjunct, group prefilter,
/// group key, or totals predicate can reference is always joined (as
/// `LEFT OUTER`, matching Django's nullable traversals); unconditional
/// `WHERE` conjuncts recover the inner-join semantics where Django uses
/// them. Fanout collapses in `GROUP BY`. Table names are the real ones
/// (`db/models/`); the aliases are the ones the compiled conjuncts cite
/// (`issue_list.rs`, recorded from live Django SQL).
fn list_from_sql() -> String {
    [
        "FROM \"issues\"",
        "INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\")",
        "LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")",
        // Single-row ordering support (`order_by=state__name` family):
        // Django resolves one-level FK traversals with `LEFT OUTER`
        // joins; these add no fanout when unordered-upon.
        "LEFT OUTER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\")",
        "LEFT OUTER JOIN \"issues\" T_parent ON (\"issues\".\"parent_id\" = T_parent.\"id\")",
        "LEFT OUTER JOIN \"issue_labels\" \"label_issue\" ON (\"issues\".\"id\" = \"label_issue\".\"issue_id\")",
        "LEFT OUTER JOIN \"labels\" ON (\"label_issue\".\"label_id\" = \"labels\".\"id\")",
        "LEFT OUTER JOIN \"issue_assignees\" \"issue_assignee\" ON (\"issues\".\"id\" = \"issue_assignee\".\"issue_id\")",
        "LEFT OUTER JOIN \"users\" ON (\"issue_assignee\".\"assignee_id\" = \"users\".\"id\")",
        "LEFT OUTER JOIN \"issue_mentions\" \"issue_mention\" ON (\"issues\".\"id\" = \"issue_mention\".\"issue_id\")",
        "LEFT OUTER JOIN \"cycle_issues\" \"issue_cycle\" ON (\"issues\".\"id\" = \"issue_cycle\".\"issue_id\")",
        "LEFT OUTER JOIN \"module_issues\" \"issue_module\" ON (\"issues\".\"id\" = \"issue_module\".\"issue_id\")",
        "LEFT OUTER JOIN \"intake_issues\" \"issue_intake\" ON (\"issues\".\"id\" = \"issue_intake\".\"issue_id\")",
        "LEFT OUTER JOIN \"issue_subscribers\" \"issue_subscribers\" ON (\"issues\".\"id\" = \"issue_subscribers\".\"issue_id\")",
    ]
    .join(" ")
}

/// `WHERE` before user filters: manager exclusions + tenant scoping
/// ([`base_where_sql`](list_q::base_where_sql): `$1` = workspace id,
/// `$2` = project id), user conjuncts (from `$3`), and the grouper
/// prefilters for m2m group axes (`grouper.py:37-45`).
fn list_where_sql(filter_conjuncts: &[String], group_by: &str, sub_group_by: &str) -> String {
    let mut parts = vec![list_q::base_where_sql()];
    parts.extend(filter_conjuncts.iter().cloned());
    for key in [group_by, sub_group_by] {
        if let Some(prefilter) = list_q::group_prefilter_sql(key) {
            parts.push(prefilter.to_owned());
        }
    }
    format!("WHERE ({})", parts.join(" AND "))
}

/// The order-key expression the paginator sorts by, for the mapped key
/// (`order_queryset.py:14-58` via
/// [`order_key`](list_q::order_key)): `priority_order` / `state_order`
/// `CASE`s, the `min_values` per-issue `MIN` subquery (Django's
/// annotation is unguarded — plain joins, no `deleted_at` filter), or a
/// plain column. Returns `(expression, descending)` for
/// `ORDER BY {expr} {DIR} NULLS LAST`.
fn order_key_expr(mapped: &str) -> (String, bool) {
    let (key, desc) = match mapped.strip_prefix('-') {
        Some(rest) => (rest, true),
        None => (mapped, false),
    };
    let expr = match key {
        "priority_order" => {
            let cases: Vec<String> = list_q::PRIORITY_ORDER
                .iter()
                .enumerate()
                .map(|(i, p)| format!("WHEN \"issues\".\"priority\" = '{p}' THEN {i}"))
                .collect();
            format!(
                "CASE {} ELSE {} END",
                cases.join(" "),
                list_q::PRIORITY_ORDER.len()
            )
        }
        "state_order" => {
            // `order_queryset.py:26`: `STATE_ORDER` as-is for the ascending
            // spelling, reversed for `-state__group` (same shape as
            // `order_by_sql`); the mapped key's direction applies on top.
            let order: Vec<&&str> = if desc {
                list_q::STATE_GROUP_ORDER.iter().rev().collect()
            } else {
                list_q::STATE_GROUP_ORDER.iter().collect()
            };
            let cases: Vec<String> = order
                .iter()
                .enumerate()
                .map(|(i, g)| format!("WHEN \"states\".\"group\" = '{g}' THEN {i}"))
                .collect();
            format!(
                "CASE {} ELSE {} END",
                cases.join(" "),
                list_q::STATE_GROUP_ORDER.len()
            )
        }
        "min_values" => {
            // Set by the caller per `order_by` input (m2m-name ordering);
            // replaced with the concrete subquery via `min_values_sql`.
            "MIN_VALUES".to_owned()
        }
        other => related_order_expr(other),
    };
    (expr, desc)
}

/// Plain-column ordering for the `else` branch of `order_issue_queryset`
/// (`order_queryset.py:49-57`): local columns read off `issues`, while a
/// one-level FK traversal (`state__name`, `project__identifier`, …)
/// resolves through the fixed single-row joins — the same `LEFT OUTER`
/// shape Django's `order_by` uses. Deeper or m2m traversals render an
/// invalid column and fail closed (500), like Django's `FieldError`.
fn related_order_expr(field: &str) -> String {
    if let Some((head, column)) = field.split_once("__") {
        let table = match head {
            "state" => Some("\"states\""),
            "project" => Some("\"projects\""),
            "parent" => Some("T_parent"),
            "workspace" => Some("\"workspaces\""),
            _ => None,
        };
        if let Some(table) = table {
            if !column.contains("__") {
                return format!("{table}.\"{column}\"");
            }
        }
    }
    format!("\"issues\".\"{field}\"")
}

/// `min_values` per-issue subquery for m2m-name orderings
/// (`order_queryset.py:36-47`): `Min(...)` over plain (unguarded) joins.
/// `order_by_param` is the raw input (`labels__name` family, `:38-44`).
fn min_values_sql(order_by_param: &str) -> Option<String> {
    let field = order_by_param.trim_start_matches('-');
    let (through, target, column) = match field {
        "labels__name" => ("\"issue_labels\"", "\"labels\"", "\"labels\".\"name\""),
        "assignees__first_name" => (
            "\"issue_assignees\"",
            "\"users\"",
            "\"users\".\"first_name\"",
        ),
        "issue_module__module__name" => {
            ("\"module_issues\"", "\"modules\"", "\"modules\".\"name\"")
        }
        _ => return None,
    };
    let link = match field {
        "labels__name" => {
            "\"ml\".\"label_id\" = \"t\".\"id\" AND \"ml\".\"issue_id\" = (\"issues\".\"id\")"
        }
        "assignees__first_name" => {
            "\"ml\".\"assignee_id\" = \"t\".\"id\" AND \"ml\".\"issue_id\" = (\"issues\".\"id\")"
        }
        _ => "\"ml\".\"module_id\" = \"t\".\"id\" AND \"ml\".\"issue_id\" = (\"issues\".\"id\")",
    };
    Some(format!(
        "(SELECT MIN({column}) FROM {through} \"ml\" INNER JOIN {target} \"t\" ON ({link}))"
    ))
}

/// Bare value expression for one group axis: the through FK for m2m axes
/// (`group_lookup`), the cycle subquery for `cycle_id`, plain columns
/// otherwise. Window partitions reference the projected output names;
/// totals embed this expression in their own `SELECT`.
/// Returns `None` for unknown fields (Django's `F()` raises → 500) and for
/// `target_date` / `start_date` / `created_by` (BUG-group-values-no-
/// queryset: `issue_group_values` raises `AttributeError` while the view
/// builds the `paginate` call, before any SQL runs → 500).
fn group_inner_expr(group_by: &str) -> Option<String> {
    if group_by.is_empty() {
        return None;
    }
    match group_by {
        "labels__id" => Some("\"label_issue\".\"label_id\"".to_owned()),
        "assignees__id" => Some("\"issue_assignee\".\"assignee_id\"".to_owned()),
        "issue_module__module_id" => Some("\"issue_module\".\"module_id\"".to_owned()),
        "cycle_id" => Some(bare_cycle_expr()),
        // `F("state")` resolves through the `state` FK to `state_id`
        // (verified live: `group_by=state` partitions by state id).
        "state" => Some("\"issues\".\"state_id\"".to_owned()),
        "state_id" => Some("\"issues\".\"state_id\"".to_owned()),
        "project_id" => Some("\"issues\".\"project_id\"".to_owned()),
        "priority" => Some("\"issues\".\"priority\"".to_owned()),
        "state__group" => Some("\"states\".\"group\"".to_owned()),
        "target_date" => Some("\"issues\".\"target_date\"".to_owned()),
        "start_date" => Some("\"issues\".\"start_date\"".to_owned()),
        "created_by" => Some("\"issues\".\"created_by_id\"".to_owned()),
        _ => None,
    }
}

/// The cycle subquery without its `AS "cycle_id"` tail (the builder
/// carries the alias for `SELECT` use; partitions and totals need the
/// bare expression).
fn bare_cycle_expr() -> String {
    let full = list_q::cycle_id_annotation_sql();
    full.strip_suffix(" AS \"cycle_id\"")
        .unwrap_or(&full)
        .to_owned()
}

/// Extra `SELECT` for one group axis beyond the `on_results` projection:
/// m2m axes project the through FK under the lookup name, `cycle_id`
/// reuses the annotation builder as-is, plain axes are already projected
/// (their output name is the field itself).
fn group_select(group_by: &str) -> Option<Option<String>> {
    group_inner_expr(group_by)?;
    if group_by == "cycle_id" {
        return Some(Some(list_q::cycle_id_annotation_sql()));
    }
    match group_by {
        "labels__id" | "assignees__id" | "issue_module__module_id" => {
            let expr = group_inner_expr(group_by).expect("checked above");
            Some(Some(format!("{expr} AS \"{group_by}\"")))
        }
        // `F("state")` partitions by the FK target: project the FK column
        // under the URL name so `__b."state"` resolves. `GROUP BY` needs no
        // addition (`issues.state_id` is functionally dependent on the
        // `"issues"."id"` PK already grouped).
        "state" => Some(Some("\"issues\".\"state_id\" AS \"state\"".to_owned())),
        _ => Some(None),
    }
}

/// Whether the axis needs the caller's queryset (and therefore 500s on
/// this endpoint). See
/// [`group_values_requires_queryset`](list_q::group_values_requires_queryset).
fn group_needs_queryset(field: &str) -> bool {
    list_q::group_values_requires_queryset(field)
}

/// `vote_items` aggregate for the list projection (`grouper.py:112-145`):
/// one `{"vote", "actor_details"}` object per live vote, `NULL` when
/// voteless (no `Coalesce`).
///
/// BUG-avatar-list: the queries-layer builder reads the nonexistent
/// `"vote_actor"."avatar_asset"`; the executable text below reads
/// `avatar_asset_id` (the `User.avatar_asset` FK column). Tracked by
/// PIDASHCONV-234.
fn list_vote_items_sql() -> String {
    "ARRAY_AGG(DISTINCT CASE WHEN (\"votes\".\"id\" IS NOT NULL AND \"votes\".\"deleted_at\" IS NULL) THEN JSONB_BUILD_OBJECT('vote', \"votes\".\"vote\", 'actor_details', JSONB_BUILD_OBJECT('id', \"vote_actor\".\"id\", 'first_name', \"vote_actor\".\"first_name\", 'last_name', \"vote_actor\".\"last_name\", 'avatar', \"vote_actor\".\"avatar\", 'avatar_url', CASE WHEN \"vote_actor\".\"avatar_asset_id\" IS NOT NULL THEN CONCAT('/api/assets/v2/static/', \"vote_actor\".\"avatar_asset_id\", '/') ELSE \"vote_actor\".\"avatar\" END, 'display_name', \"vote_actor\".\"display_name\")) ELSE NULL END) FILTER (WHERE (\"votes\".\"id\" IS NOT NULL AND \"votes\".\"deleted_at\" IS NULL)) AS \"vote_items\"".to_string()
}

/// `reaction_items` aggregate for the list projection
/// (`grouper.py:146-179`). Same BUG-avatar-list correction as above
/// (tracked by PIDASHCONV-234).
fn list_reaction_items_sql() -> String {
    "ARRAY_AGG(DISTINCT CASE WHEN (\"issue_reactions\".\"id\" IS NOT NULL AND \"issue_reactions\".\"deleted_at\" IS NULL) THEN JSONB_BUILD_OBJECT('reaction', \"issue_reactions\".\"reaction\", 'actor_details', JSONB_BUILD_OBJECT('id', \"reaction_actor\".\"id\", 'first_name', \"reaction_actor\".\"first_name\", 'last_name', \"reaction_actor\".\"last_name\", 'avatar', \"reaction_actor\".\"avatar\", 'avatar_url', CASE WHEN \"reaction_actor\".\"avatar_asset_id\" IS NOT NULL THEN CONCAT('/api/assets/v2/static/', \"reaction_actor\".\"avatar_asset_id\", '/') ELSE \"reaction_actor\".\"avatar\" END, 'display_name', \"reaction_actor\".\"display_name\")) ELSE NULL END) FILTER (WHERE (\"issue_reactions\".\"id\" IS NOT NULL AND \"issue_reactions\".\"deleted_at\" IS NULL)) AS \"reaction_items\"".to_string()
}

/// Vote/reaction source joins for the list aggregates (aliased exactly as
/// the annotations cite them).
fn votes_reactions_joins_sql() -> String {
    [
        "LEFT OUTER JOIN \"issue_votes\" \"votes\" ON (\"issues\".\"id\" = \"votes\".\"issue_id\")",
        "LEFT OUTER JOIN \"users\" \"vote_actor\" ON (\"votes\".\"actor_id\" = \"vote_actor\".\"id\")",
        "LEFT OUTER JOIN \"issue_reactions\" ON (\"issues\".\"id\" = \"issue_reactions\".\"issue_id\")",
        "LEFT OUTER JOIN \"users\" \"reaction_actor\" ON (\"issue_reactions\".\"actor_id\" = \"reaction_actor\".\"id\")",
    ]
    .join(" ")
}

/// `SELECT` expression (with its `AS`) for one `on_results` field.
/// Mirrors `issue_on_results`' `.values(*required_fields, "vote_items",
/// "reaction_items")` (`grouper.py:101-109,180`) via
/// [`on_results_fields`](list_q::on_results_fields). `estimate_point`
/// and `created_by` select their `_id` columns under the public names
/// (Django renders FKs as PKs in `.values()`).
fn field_select(field: &str) -> Option<String> {
    let select = match field {
        "id" => "\"issues\".\"id\" AS \"id\"",
        "name" => "\"issues\".\"name\" AS \"name\"",
        "state_id" => "\"issues\".\"state_id\" AS \"state_id\"",
        "sort_order" => "\"issues\".\"sort_order\" AS \"sort_order\"",
        "estimate_point" => "\"issues\".\"estimate_point_id\" AS \"estimate_point\"",
        "priority" => "\"issues\".\"priority\" AS \"priority\"",
        "start_date" => "\"issues\".\"start_date\" AS \"start_date\"",
        "target_date" => "\"issues\".\"target_date\" AS \"target_date\"",
        "sequence_id" => "\"issues\".\"sequence_id\" AS \"sequence_id\"",
        "project_id" => "\"issues\".\"project_id\" AS \"project_id\"",
        "parent_id" => "\"issues\".\"parent_id\" AS \"parent_id\"",
        "cycle_id" => return Some(list_q::cycle_id_annotation_sql()),
        "created_by" => "\"issues\".\"created_by_id\" AS \"created_by\"",
        "state__group" => "\"states\".\"group\" AS \"state__group\"",
        "assignee_ids" => return list_q::default_annotation_sql("assignee_ids"),
        "label_ids" => return list_q::default_annotation_sql("label_ids"),
        "module_ids" => return list_q::default_annotation_sql("module_ids"),
        "labels__id" => "\"label_issue\".\"label_id\" AS \"labels__id\"",
        "assignees__id" => "\"issue_assignee\".\"assignee_id\" AS \"assignees__id\"",
        "issue_module__module_id" => {
            "\"issue_module\".\"module_id\" AS \"issue_module__module_id\""
        }
        "vote_items" => return Some(list_vote_items_sql()),
        "reaction_items" => return Some(list_reaction_items_sql()),
        _ => return None,
    };
    Some(select.to_owned())
}

// ---------------------------------------------------------------------------
// Group values (`issue_group_values`, grouper.py:185-252)
// ---------------------------------------------------------------------------

/// How one axis' declared groups are read.
enum GroupValuesPlan {
    /// Static branch (`priority`, `state__group`).
    Static(Vec<String>),
    /// One DB branch; the bool records the `"None"` sentinel.
    Sql {
        sql: String,
        params: Vec<SqlParam>,
        sentinel: bool,
    },
    /// Unknown field (`grouper.py:252`).
    Empty,
}

/// Statement plan for
/// [`group_values_query`](list_q::group_values_query): static branches,
/// DB branches with workspace scoping (`workspace__slug=slug` in Python,
/// `grouper.py:193-226` — the handler binds the board row's workspace id
/// instead, same row, no extra hop), and the null-project rules
/// (`grouper.py:236-251`: every branch drops its project filter; the
/// assignees branch switches tables to `WorkspaceMember`).
fn group_values_plan(field: &str, scope: &BoardScope) -> GroupValuesPlan {
    if let Some(statics) = list_q::static_group_values(field) {
        return GroupValuesPlan::Static(statics.iter().map(|s| s.to_string()).collect());
    }
    let Some(query) = list_q::group_values_query(field) else {
        return GroupValuesPlan::Empty;
    };
    if field == "assignees__id" && scope.project_id.is_none() {
        return GroupValuesPlan::Sql {
            sql: "SELECT \"workspace_members\".\"member_id\" AS \"v\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"workspace_id\" = $1 AND \"workspace_members\".\"is_active\" = true)".to_owned(),
            params: vec![SqlParam::Uuid(scope.workspace_id)],
            sentinel: query.none_sentinel,
        };
    }
    // `workspace_column` is already a quoted dotted path
    // (`"states"."workspace_id"`); only `table`/`column`/`project_filter`
    // arrive with trimmable quotes. Quoting it again emits `""states""`
    // and 500s every DB-backed axis at plan time.
    let mut sql = format!(
        "SELECT \"{table}\".\"{column}\" AS \"v\" FROM \"{table}\" WHERE ({workspace} = $1",
        table = query.table.trim_matches('"'),
        column = query.column.trim_matches('"'),
        workspace = query.workspace_column,
    );
    let mut params = vec![SqlParam::Uuid(scope.workspace_id)];
    if !query.project_filter.is_empty() {
        if let Some(project_id) = scope.project_id {
            sql.push_str(&format!(
                " AND (\"{}\".\"{}\" = $2)",
                query.table.trim_matches('"'),
                query.project_filter.trim_matches('"')
            ));
            params.push(SqlParam::Uuid(project_id));
        }
    }
    if !query.extra_where.is_empty() {
        sql.push_str(&format!(" AND ({})", query.extra_where));
    }
    sql.push(')');
    GroupValuesPlan::Sql {
        sql,
        params,
        sentinel: query.none_sentinel,
    }
}

/// Declared groups for one axis: static branches, one DB branch, or `[]`
/// for unknown fields. Queryset-needing branches never reach here (the
/// caller 500s first — eager `paginate`-kwarg evaluation order).
async fn group_values(
    pool: &sqlx::PgPool,
    field: &str,
    scope: &BoardScope,
) -> Result<Vec<String>, HandlerError> {
    match group_values_plan(field, scope) {
        GroupValuesPlan::Static(statics) => Ok(statics),
        GroupValuesPlan::Empty => Ok(Vec::new()),
        GroupValuesPlan::Sql {
            sql,
            params,
            sentinel,
        } => {
            let rows = fetch_all_objects(pool, &sql, &params).await?;
            let mut out: Vec<String> = rows
                .iter()
                .filter_map(|row| row.get("v"))
                .map(paginator::py_str)
                .collect();
            if sentinel {
                out.push("None".to_owned());
            }
            Ok(out)
        }
    }
}

// ---------------------------------------------------------------------------
// Totals (grouped `__get_total_queryset`s, paginator.py:297-303,505-527)
// ---------------------------------------------------------------------------

/// Per-group totals over the filtered set with the intake/archived/draft
/// `count_filter` (`views/issue.py:170-177,196-203`,
/// [`count_filter_sql`](list_q::count_filter_sql)): one `(group, count)`
/// row per group value, raw counts (the `1-if-zero` rule lives in the
/// kernel [`total_dict`](paginator::total_dict)).
async fn group_total_pairs(
    pool: &sqlx::PgPool,
    group_expr: &str,
    from_sql: &str,
    where_sql: &str,
    params: &[SqlParam],
) -> Result<Vec<(String, i64)>, HandlerError> {
    let sql = format!(
        "SELECT ({group_expr}) AS \"g\", COUNT(DISTINCT \"issues\".\"id\") FILTER (WHERE {}) AS \"count\" {from_sql} {where_sql} GROUP BY ({group_expr})",
        list_q::count_filter_sql(),
    );
    let rows = fetch_all_objects(pool, &sql, params).await?;
    rows.iter()
        .map(|row| {
            let group = row
                .get("g")
                .map(paginator::py_str)
                .unwrap_or("None".to_owned());
            let count = row
                .get("count")
                .and_then(|v| v.as_i64())
                .ok_or(HandlerError::ServerError)?;
            Ok((group, count))
        })
        .collect()
}

/// Envelope total over the filtered set for the grouped branches:
/// `queryset.count()` counts base rows (one per issue after grouping),
/// so the joins' fanout must collapse in `COUNT(DISTINCT ...)`.
fn grouped_total_sql(from_sql: &str, where_sql: &str) -> String {
    format!("SELECT COUNT(DISTINCT \"issues\".\"id\") AS \"count\" {from_sql} {where_sql}")
}

/// Raw top count behind grouped `max_hits`
/// (`...order_by("-count")[0]["count"]`, `paginator.py:280-287,467-474`):
/// the `1-if-zero` adjustment applies to per-cell totals only, never here.
fn raw_top_count(group_totals: &[(String, i64)]) -> i64 {
    group_totals
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(0)
}

/// Per-group/sub-group totals (`paginator.py:511-518`): sub counts
/// overwrite plainly (no `1-if-zero` — kernel
/// [`sub_total_dicts`](paginator::sub_total_dicts)).
async fn subgroup_total_pairs(
    pool: &sqlx::PgPool,
    group_expr: &str,
    sub_expr: &str,
    from_sql: &str,
    where_sql: &str,
    params: &[SqlParam],
) -> Result<Vec<(String, String, i64)>, HandlerError> {
    let sql = format!(
        "SELECT ({group_expr}) AS \"g\", ({sub_expr}) AS \"s\", COUNT(DISTINCT \"issues\".\"id\") FILTER (WHERE {}) AS \"count\" {from_sql} {where_sql} GROUP BY ({group_expr}), ({sub_expr})",
        list_q::count_filter_sql(),
    );
    let rows = fetch_all_objects(pool, &sql, params).await?;
    rows.iter()
        .map(|row| {
            let group = row
                .get("g")
                .map(paginator::py_str)
                .unwrap_or("None".to_owned());
            let sub = row
                .get("s")
                .map(paginator::py_str)
                .unwrap_or("None".to_owned());
            let count = row
                .get("count")
                .and_then(|v| v.as_i64())
                .ok_or(HandlerError::ServerError)?;
            Ok((group, sub, count))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Row shaping
// ---------------------------------------------------------------------------

/// Re-render every `"sort_order":<number>` literal in a serialized body
/// through [`py_float_str`](paginator::py_float_str). Two renderers miss
/// Python's `repr` here: Postgres `float8out` prints integral floats bare
/// (`65535`, fixed earlier via `Number::from_f64`), while serde's ryu
/// drops the exponent `+` (`1e16` vs `1e+16`). `sort_order` is the only
/// float anywhere in these responses (counts/sequences/votes are ints;
/// `description_json` numerics are jsonb territory, out of scope), so the
/// rewrite is scoped to that key.
fn fix_sort_order_numbers(body: &str) -> String {
    const KEY: &str = "\"sort_order\":";
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(index) = rest.find(KEY) {
        let (head, tail) = rest.split_at(index + KEY.len());
        out.push_str(head);
        let end = tail
            .char_indices()
            .take_while(|(_, ch)| ch.is_ascii_digit() || matches!(ch, '.' | 'e' | 'E' | '+' | '-'))
            .map(|(i, ch)| i + ch.len_utf8())
            .last()
            .unwrap_or(0);
        let (literal, remaining) = tail.split_at(end);
        match literal.parse::<f64>() {
            Ok(number) => out.push_str(&paginator::py_float_str(number)),
            Err(_) => out.push_str(literal),
        }
        rest = remaining;
    }
    out.push_str(rest);
    out
}

/// Shape one fetched row into the `on_results` field order
/// (`grouper.py:101-109`). `sort_order` normalizes integral floats to
/// `Number` (`65535` → `65535.0`); exponent formatting is finished by
/// [`fix_sort_order_numbers`] at serialization. Everything else passes
/// through as rendered (UUIDs/dates as strings, id-lists as arrays,
/// vote/reaction items as arrays-or-null).
/// Model/join columns of the list projection in `.values()` order
/// (`grouper.py:84-97` minus the `cycle_id` annotation).
const LIST_MODEL_ORDER: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "created_by",
    "state__group",
];

/// Annotation columns of the list projection in `.annotate()` order: the
/// view's `cycle_id`, the grouper's id lists, the `on_results`
/// aggregates (`views/issue.py:98-102`, `grouper.py:48-66,112-179`).
/// Django emits concrete columns first, annotations after — verified live
/// per mode (ungrouped and grouped alike).
const LIST_ANNOT_ORDER: &[&str] = &[
    "cycle_id",
    "assignee_ids",
    "label_ids",
    "module_ids",
    "vote_items",
    "reaction_items",
];

/// A group traversal (`labels__id`, `assignees__id`,
/// `issue_module__module_id`): a join column, so it sorts with the model
/// block in `rest` order — never with the annotations.
fn is_traversal(field: &str) -> bool {
    matches!(
        field,
        "labels__id" | "assignees__id" | "issue_module__module_id"
    )
}

/// Django column order for one shaped list row: model/join columns in
/// `.values()` order (required columns, then any group traversals in
/// `rest` order), then annotations in `.annotate()` order. A swapped-out
/// m2m id list is NOT kept here: the multi-grouper appends it at the end
/// itself (`result[mapped] = ...` on a missing key), and pre-keeping it
/// would pin it in annotation position instead.
fn list_wire_order(fields: &[String]) -> Vec<String> {
    let mut ordered = Vec::with_capacity(fields.len() + 1);
    for field in fields {
        if LIST_MODEL_ORDER.contains(&field.as_str()) {
            ordered.push(field.clone());
        }
    }
    for field in fields {
        if is_traversal(field) {
            ordered.push(field.clone());
        }
    }
    for field in LIST_ANNOT_ORDER {
        if fields.iter().any(|f| f == field) {
            ordered.push(field.to_string());
        }
    }
    for field in fields {
        if !ordered.iter().any(|f| f == field) {
            ordered.push(field.clone());
        }
    }
    ordered
}

fn shape_row(
    row: &Map<String, Value>,
    fields: &[String],
) -> Result<Map<String, Value>, HandlerError> {
    let mut out = Map::new();
    for field in list_wire_order(fields) {
        let value = row.get(&field).unwrap_or(&Value::Null);
        if field == "sort_order" {
            if let Some(number) = value.as_f64() {
                let rendered = serde_json::Number::from_f64(number)
                    .map(Value::Number)
                    .unwrap_or_else(|| value.clone());
                out.insert(field.clone(), rendered);
                continue;
            }
        }
        // Django's `ArrayAgg` deserializes a NULL aggregate as `[]`
        // (verified live for voteless issues).
        if (field == "vote_items" || field == "reaction_items") && value.is_null() {
            out.insert(field.clone(), Value::Array(Vec::new()));
            continue;
        }
        out.insert(field.clone(), value.clone());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// List handler (`ProjectIssuesPublicEndpoint.get`, views/issue.py:76-211)
// ---------------------------------------------------------------------------

/// Plain multi-map for filter compilation (Django `QueryDict.get` reads
/// last-wins; repeats collapse before compiling).
fn multi_map(query: &QueryMap) -> HashMap<String, Vec<String>> {
    query
        .keys()
        .filter_map(|key| super::query_values(query, key).map(|values| (key.clone(), values)))
        .collect()
}

fn multi_last(multi: &HashMap<String, Vec<String>>, key: &str) -> Option<String> {
    multi
        .get(key)
        .and_then(|values| values.iter().last().cloned())
}

/// Render a 200 JSON response with exact bytes.
fn json_ok(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("handler json response")
}

/// Envelope metadata: everything [`PageResponse`] carries besides the
/// cursors and the results value.
struct PageMeta<'a> {
    grouped_by: Option<&'a str>,
    sub_grouped_by: Option<&'a str>,
    total_count: i64,
    count: usize,
    total_pages: i64,
}

/// The envelope `BasePaginator.paginate` returns (`paginator.py:714-731`),
/// keys in order via the kernel [`PageResponse`].
fn envelope_response(
    meta: &PageMeta<'_>,
    next: &Cursor,
    prev: &Cursor,
    results: Value,
) -> Response {
    let page = PageResponse {
        grouped_by: meta.grouped_by.map(str::to_owned),
        sub_grouped_by: meta.sub_grouped_by.map(str::to_owned),
        total_count: meta.total_count,
        next_cursor: next.to_string(),
        prev_cursor: prev.to_string(),
        next_page_results: next.has_results_or_false(),
        prev_page_results: prev.has_results_or_false(),
        count: meta.count,
        total_pages: meta.total_pages,
        total_results: meta.total_count,
        extra_stats: None,
        results,
    };
    let body = serde_json::to_string(&page).expect("serializable envelope");
    json_ok(fix_sort_order_numbers(&body))
}

/// `GET anchor/<anchor>/issues/` (`ProjectIssuesPublicEndpoint`,
/// `views/issue.py:76-211`).
async fn list_issues(
    State(state): State<AppState>,
    Path(anchor): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    match list_issues_inner(&state, &anchor, &query, extension).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn list_issues_inner(
    state: &AppState,
    anchor: &str,
    query: &QueryMap,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, HandlerError> {
    let pool = pool_of(state)?;
    let multi = multi_map(query);
    // Timezone first: `TimezoneMixin.initial` runs before the view, so an
    // invalid stored zone 500s even when the anchor is bad.
    let _tz = request_tz(pool, extension).await?;
    let board = load_list_board(pool, anchor).await?;

    let group_by = multi_last(&multi, "group_by").unwrap_or_default();
    let sub_group_by = multi_last(&multi, "sub_group_by").unwrap_or_default();
    // Mismatch before pagination parsing (`views/issue.py:141-146`).
    if !group_by.is_empty() && !sub_group_by.is_empty() && group_by == sub_group_by {
        return Ok(guards_error(guards::same_group_by()));
    }

    // Eager `paginate`-kwarg order: `issue_group_values` evaluates before
    // `paginate` parses `per_page`/cursor, so queryset-needing axes
    // (`target_date`/`start_date`/`created_by`, `grouper.py:233-250`) 500
    // before malformed pagination input can 400.
    for axis in [&group_by, &sub_group_by] {
        if !axis.is_empty() && group_needs_queryset(axis) {
            return Err(HandlerError::ServerError);
        }
    }

    let per_page = paginator::parse_per_page(multi_last(&multi, "per_page").as_deref(), 1000, 1000)
        .map_err(page_denial)?;
    let cursor_raw = multi_last(&multi, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor = Cursor::from_string(&cursor_raw).map_err(page_denial)?;

    // Unknown axes: `issue_group_values` answers `[]` without raising
    // (`grouper.py:252`), so malformed pagination input still wins the
    // 400 here; the 500 lands at evaluation like Django's `FieldError`.
    for axis in [&group_by, &sub_group_by] {
        if !axis.is_empty() && group_inner_expr(axis).is_none() {
            return Err(HandlerError::ServerError);
        }
    }

    let order_input = multi_last(&multi, "order_by").unwrap_or_else(|| "-created_at".to_owned());
    let mapped = list_q::order_key(&order_input).to_owned();
    let (mut key_expr, descending) = order_key_expr(&mapped);
    if key_expr == "MIN_VALUES" {
        key_expr = min_values_sql(&order_input).ok_or(HandlerError::ServerError)?;
    }
    let direction = if descending { "DESC" } else { "ASC" };

    // Filters (`issue_filters(request.query_params, "GET")`, `:77`) with
    // relative dates expanded against UTC today. Sorted by key so the
    // `$N` numbering (and the statement text) is deterministic.
    let today = chrono::Utc::now().date_naive();
    let mut pairs: Vec<(String, String)> = multi
        .iter()
        .map(|(key, values)| {
            (
                key.clone(),
                values.iter().last().cloned().unwrap_or_default(),
            )
        })
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let compiled = list_q::compile_issue_filters(&expand_relative_dates(&pairs, today), 3);
    let mut params = vec![
        SqlParam::Uuid(board.workspace_id),
        opt_project_param(&board.project_id),
    ];
    params.extend(compiled.params.iter().map(|raw| bind_typed(raw)));

    let from_sql = format!("{} {}", list_from_sql(), votes_reactions_joins_sql());
    let where_sql = list_where_sql(&compiled.conjuncts, &group_by, &sub_group_by);

    let list = ListQuery {
        from_sql: &from_sql,
        where_sql: &where_sql,
        params: &params,
        key_expr: &key_expr,
        direction,
        per_page,
        cursor: &cursor,
    };
    if sub_group_by.is_empty() && group_by.is_empty() {
        return ungrouped_response(pool, &list).await;
    }
    grouped_response(pool, &board, &group_by, &sub_group_by, &list).await
}

/// One filtered list read: everything the window/total queries share.
struct ListQuery<'a> {
    from_sql: &'a str,
    where_sql: &'a str,
    params: &'a [SqlParam],
    key_expr: &'a str,
    direction: &'a str,
    per_page: i64,
    cursor: &'a Cursor,
}

/// The projected `SELECT` list + `GROUP BY` for one filtered base:
/// exactly [`on_results_fields`](list_q::on_results_fields) plus the
/// internal order/tiebreak columns. Group extras ride along when the
/// field list does not already project them.
fn base_selects(
    group_by: &str,
    sub_group_by: &str,
    key_expr: &str,
) -> Result<(Vec<String>, Vec<String>), HandlerError> {
    let fields = list_q::on_results_fields(group_by, sub_group_by);
    let mut selects = Vec::with_capacity(fields.len() + 4);
    for field in &fields {
        selects.push(field_select(field).ok_or(HandlerError::ServerError)?);
    }
    for axis in [group_by, sub_group_by] {
        if axis.is_empty() {
            continue;
        }
        if let Some(Some(extra)) = group_select(axis) {
            if !selects.contains(&extra) {
                selects.push(extra);
            }
        }
    }
    selects.push(format!("({key_expr}) AS __order_key"));
    selects.push("\"issues\".\"created_at\" AS __created_at".to_owned());

    let mut group = vec![
        "\"issues\".\"id\"".to_owned(),
        "\"states\".\"group\"".to_owned(),
    ];
    for axis in [group_by, sub_group_by] {
        match axis {
            "labels__id" => group.push("\"label_issue\".\"label_id\"".to_owned()),
            "assignees__id" => group.push("\"issue_assignee\".\"assignee_id\"".to_owned()),
            "issue_module__module_id" => group.push("\"issue_module\".\"module_id\"".to_owned()),
            "cycle_id" => group.push("\"cycle_id\"".to_owned()),
            _ => {}
        }
    }
    Ok((selects, group))
}

/// Ungrouped branch: `OffsetPaginator` through `paginate` with
/// `on_results` (`views/issue.py:205-211`, `paginator.py:121-192,654+`).
async fn ungrouped_response(
    pool: &sqlx::PgPool,
    list: &ListQuery<'_>,
) -> Result<Response, HandlerError> {
    use paginator::{apply_offset_window, max_hits, next_cursor, offset_window, prev_cursor};
    let limit = list.per_page.min(1000);
    let window = offset_window(
        limit,
        list.cursor.offset,
        list.cursor.value,
        list.cursor.is_prev,
        None,
    )
    .map_err(page_denial)?;
    let fields = list_q::on_results_fields("", "");
    let (selects, group) = base_selects("", "", list.key_expr)?;
    let inner = format!(
        "SELECT {} {} {} GROUP BY {} ORDER BY __order_key {} NULLS LAST, __created_at DESC LIMIT {} OFFSET {}",
        selects.join(", "),
        list.from_sql,
        list.where_sql,
        group.join(", "),
        list.direction,
        window.stop - window.offset,
        window.offset
    );
    let rows = fetch_all_objects(pool, &inner, list.params).await?;
    let has_more = rows.len() as i64 > limit;
    let page: Vec<Value> = apply_offset_window(&rows, limit)
        .map_err(page_denial)?
        .into_iter()
        .collect();
    let total_count = {
        let sql = format!(
            "SELECT COUNT(DISTINCT \"issues\".\"id\") AS \"count\" {} {}",
            list.from_sql, list.where_sql
        );
        let rows = fetch_all_objects(pool, &sql, list.params).await?;
        rows.first()
            .and_then(|row| row.get("count"))
            .and_then(|v| v.as_i64())
            .ok_or(HandlerError::ServerError)?
    };
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let mut shaped = Vec::with_capacity(page.len());
    for row in &page {
        let row = obj(row)?;
        shaped.push(Value::Object(shape_row(row, &fields)?));
    }
    let count = shaped.len();
    Ok(envelope_response(
        &PageMeta {
            grouped_by: None,
            sub_grouped_by: None,
            total_count,
            count,
            total_pages: max_hits(total_count, limit).map_err(page_denial)?,
        },
        &next,
        &prev,
        Value::Array(shaped),
    ))
}

/// Grouped / sub-grouped branch (`views/issue.py:140-204`,
/// `GroupedOffsetPaginator` / `SubGroupedOffsetPaginator`,
/// `paginator.py:194-632`).
async fn grouped_response(
    pool: &sqlx::PgPool,
    board: &BoardScope,
    group_by: &str,
    sub_group_by: &str,
    list: &ListQuery<'_>,
) -> Result<Response, HandlerError> {
    use paginator::{
        grouped_max_hits, grouped_window, next_cursor, prev_cursor, process_grouped_results,
        process_sub_grouped_results, sub_field_dict, sub_total_dicts, total_dict,
    };
    let limit = list.per_page.min(1000);
    let window =
        grouped_window(limit, list.cursor.offset, list.cursor.value, None).map_err(page_denial)?;
    let direction = list.direction;
    let fields = list_q::on_results_fields(group_by, sub_group_by);
    let (selects, group) = base_selects(group_by, sub_group_by, list.key_expr)?;
    let base = format!(
        "SELECT {} {} {} GROUP BY {}",
        selects.join(", "),
        list.from_sql,
        list.where_sql,
        group.join(", "),
    );
    let partition = if sub_group_by.is_empty() {
        format!("__b.\"{group_by}\"")
    } else {
        format!("__b.\"{group_by}\", __b.\"{sub_group_by}\"")
    };
    let windowed = format!(
        "SELECT __b.*, ROW_NUMBER() OVER (PARTITION BY {partition} ORDER BY __b.__order_key {direction} NULLS LAST, __b.__created_at DESC) AS __rn FROM ({base}) __b"
    );
    let outer = format!(
        "SELECT * FROM ({windowed}) __w WHERE __w.__rn > {} AND __w.__rn < {} ORDER BY __w.__order_key {direction} NULLS LAST, __w.__created_at DESC",
        window.offset, window.stop
    );
    let rows = fetch_all_objects(pool, &outer, list.params).await?;
    let window_len = rows.len();
    let window_empty = rows.is_empty();
    let has_more = {
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM ({windowed}) __e WHERE __e.__rn >= {}) AS \"more\"",
            window.stop
        );
        fetch_all_objects(pool, &sql, list.params)
            .await?
            .first()
            .and_then(|row| row.get("more"))
            .and_then(|v| v.as_bool())
            .ok_or(HandlerError::ServerError)?
    };
    // Totals over the filtered set (`__get_total_queryset`,
    // `__get_subgroup_total_queryset`).
    let group_expr = group_inner_expr(group_by).ok_or(HandlerError::ServerError)?;
    let group_totals = group_total_pairs(
        pool,
        &group_expr,
        list.from_sql,
        list.where_sql,
        list.params,
    )
    .await?;
    if !window_empty && group_totals.is_empty() {
        // `...order_by("-count")[0]` on an empty group list: IndexError.
        return Err(HandlerError::ServerError);
    }
    let totals = total_dict(
        &group_totals
            .iter()
            .map(|(group, count)| (group.clone(), *count))
            .collect::<Vec<_>>(),
    );
    // `max_hits` runs over the RAW top count (`...order_by("-count")[0]`,
    // `paginator.py:280-287,467-474`); the `1-if-zero` rule (`:291-295`)
    // feeds the per-cell `total_results` only.
    let top_group_count = raw_top_count(&group_totals);
    // `hits = queryset.count()` (`paginator.py:276,462`): distinct issues,
    // not joined rows — every m2m/vote/reaction join fans out, so this
    // must stay `COUNT(DISTINCT ...)` like the ungrouped total.
    let total_count = {
        let sql = grouped_total_sql(list.from_sql, list.where_sql);
        fetch_all_objects(pool, &sql, list.params)
            .await?
            .first()
            .and_then(|row| row.get("count"))
            .and_then(|v| v.as_i64())
            .ok_or(HandlerError::ServerError)?
    };
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let mut shaped = Vec::with_capacity(rows.len());
    for row in &rows {
        let row = obj(row)?;
        shaped.push(shape_row(row, &fields)?);
    }
    let group_fields = group_values(pool, group_by, board).await?;
    let results_value = if sub_group_by.is_empty() {
        process_grouped_results(&shaped, group_by, &group_fields, &totals).map_err(group_denial)?
    } else {
        let sub_expr = group_inner_expr(sub_group_by).ok_or(HandlerError::ServerError)?;
        let sub_pairs = subgroup_total_pairs(
            pool,
            &group_expr,
            &sub_expr,
            list.from_sql,
            list.where_sql,
            list.params,
        )
        .await?;
        let (_, sub_totals_map) = sub_total_dicts(
            &group_totals
                .iter()
                .map(|(group, count)| (group.clone(), *count))
                .collect::<Vec<_>>(),
            &sub_pairs,
        );
        // Declared sub groups are fetched (the view evaluates the kwarg)
        // but never read downstream: `__get_field_dict` seeds cells from
        // the query-driven sub totals only (`paginator.py:552-562`).
        let _declared_sub_fields = group_values(pool, sub_group_by, board).await?;
        let seeded = sub_field_dict(&group_fields, &totals, &sub_totals_map)
            .map_err(|_| HandlerError::ServerError)?;
        let Value::Object(cells) = seeded else {
            return Err(HandlerError::ServerError);
        };
        process_sub_grouped_results(&shaped, group_by, sub_group_by, cells).map_err(group_denial)?
    };
    Ok(envelope_response(
        &PageMeta {
            grouped_by: Some(group_by),
            sub_grouped_by: if sub_group_by.is_empty() {
                None
            } else {
                Some(sub_group_by)
            },
            total_count,
            count: window_len,
            total_pages: grouped_max_hits(window_empty, top_group_count, limit)
                .map_err(page_denial)?,
        },
        &next,
        &prev,
        results_value,
    ))
}

// ---------------------------------------------------------------------------
// Retrieve handler (`IssueRetrievePublicEndpoint.get`, views/issue.py:597-773)
// ---------------------------------------------------------------------------

/// `vote_items` annotation for retrieve (`views/issue.py:652-698`):
/// verbatim from the queries layer (its vote branch is correct).
fn retrieve_vote_items_sql() -> String {
    "ARRAY_AGG(DISTINCT (CASE WHEN (\"votes\".\"id\" IS NOT NULL AND \"votes\".\"deleted_at\" IS NULL) THEN JSONB_BUILD_OBJECT('vote', \"votes\".\"vote\", 'actor_details', JSONB_BUILD_OBJECT('id', \"vote_actor\".\"id\", 'first_name', \"vote_actor\".\"first_name\", 'last_name', \"vote_actor\".\"last_name\", 'avatar', \"vote_actor\".\"avatar\", 'avatar_url', (CASE WHEN (\"vote_actor\".\"avatar_asset_id\" IS NOT NULL) THEN CONCAT('/api/assets/v2/static/', \"vote_actor\".\"avatar_asset_id\", '/') WHEN (\"vote_actor\".\"avatar_asset_id\" IS NULL) THEN \"vote_actor\".\"avatar\" ELSE NULL END), 'display_name', \"vote_actor\".\"display_name\") ) ELSE NULL END)) FILTER (WHERE CASE WHEN (\"votes\".\"id\" IS NOT NULL AND \"votes\".\"deleted_at\" IS NULL) THEN true ELSE false END) AS \"vote_items\"".to_string()
}

/// `reaction_items` annotation for retrieve (`views/issue.py:699-745`).
///
/// BUG-avatar-retrieve: the inner `When`s read the VOTE actor
/// (`votes__actor__*`, `:713,716,722`) instead of
/// `issue_reactions__actor__*`. Live Django resolves that traversal
/// through the users join reused from `vote_items` (identical
/// `votes__actor` prefix), so the executable equivalent references the
/// `vote_actor` alias — the queries-layer builder's
/// `"votes"."actor_avatar_asset"` / `"votes"."actor_avatar"` name columns
/// `issue_votes` does not have and cannot execute. Tracked by
/// PIDASHCONV-234.
fn retrieve_reaction_items_sql() -> String {
    "ARRAY_AGG(DISTINCT (CASE WHEN (\"issue_reactions\".\"id\" IS NOT NULL AND \"issue_reactions\".\"deleted_at\" IS NULL) THEN JSONB_BUILD_OBJECT('reaction', \"issue_reactions\".\"reaction\", 'actor_details', JSONB_BUILD_OBJECT('id', \"reaction_actor\".\"id\", 'first_name', \"reaction_actor\".\"first_name\", 'last_name', \"reaction_actor\".\"last_name\", 'avatar', \"reaction_actor\".\"avatar\", 'avatar_url', (CASE WHEN (\"vote_actor\".\"avatar_asset_id\" IS NOT NULL) THEN CONCAT('/api/assets/v2/static/', \"vote_actor\".\"avatar_asset_id\", '/') WHEN (\"vote_actor\".\"avatar_asset_id\" IS NULL) THEN \"vote_actor\".\"avatar\" ELSE NULL END), 'display_name', \"reaction_actor\".\"display_name\") ) ELSE NULL END)) FILTER (WHERE CASE WHEN (\"issue_reactions\".\"id\" IS NOT NULL AND \"issue_reactions\".\"deleted_at\" IS NULL) THEN true ELSE false END) AS \"reaction_items\"".to_string()
}

/// R1 single-issue read (`views/issue.py:600-771`): the manager scope,
/// `select_related` joins, through-table joins behind the id-list
/// annotations, actor joins behind the vote/reaction aggregates, and the
/// 23-key `.values(...)` list with `.first()` (`LIMIT 1`).
/// `GROUP BY "issues"."id", "states"."group"` with `LIMIT 1` is the
/// `.first()` at `:771`.
///
/// `$1` = issue id, `$2` = workspace slug, `$3` = project id. The joins
/// mirror `issue_retrieve_sql` in the queries layer; only the
/// reaction `avatar_url` refs differ (see [`retrieve_reaction_items_sql`]).
fn issue_retrieve_sql() -> String {
    format!(
        "SELECT \"issues\".\"id\", \"issues\".\"name\", \"issues\".\"state_id\", \"issues\".\"sort_order\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"description_stripped\", \"issues\".\"description_binary\", {module_ids}, {label_ids}, {assignee_ids}, \"issues\".\"estimate_point_id\" AS \"estimate_point\", \"issues\".\"priority\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"project_id\", \"issues\".\"parent_id\", {cycle_id}, \"issues\".\"created_by_id\" AS \"created_by\", \"states\".\"group\" AS \"state__group\", {vote_items}, {reaction_items} FROM \"issues\" INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") LEFT OUTER JOIN \"issues\" T_parent ON (\"issues\".\"parent_id\" = T_parent.\"id\") LEFT OUTER JOIN \"issue_labels\" \"label_through\" ON (\"issues\".\"id\" = \"label_through\".\"issue_id\") LEFT OUTER JOIN \"labels\" ON (\"label_through\".\"label_id\" = \"labels\".\"id\") LEFT OUTER JOIN \"issue_assignees\" \"assignee_through\" ON (\"issues\".\"id\" = \"assignee_through\".\"issue_id\") LEFT OUTER JOIN \"users\" \"assignees\" ON (\"assignee_through\".\"assignee_id\" = \"assignees\".\"id\") LEFT OUTER JOIN \"project_members\" ON (\"assignees\".\"id\" = \"project_members\".\"member_id\") LEFT OUTER JOIN \"module_issues\" \"module_through\" ON (\"issues\".\"id\" = \"module_through\".\"issue_id\") LEFT OUTER JOIN \"modules\" ON (\"module_through\".\"module_id\" = \"modules\".\"id\") LEFT OUTER JOIN \"issue_votes\" \"votes\" ON (\"issues\".\"id\" = \"votes\".\"issue_id\") LEFT OUTER JOIN \"users\" \"vote_actor\" ON (\"votes\".\"actor_id\" = \"vote_actor\".\"id\") LEFT OUTER JOIN \"issue_reactions\" ON (\"issues\".\"id\" = \"issue_reactions\".\"issue_id\") LEFT OUTER JOIN \"users\" \"reaction_actor\" ON (\"issue_reactions\".\"actor_id\" = \"reaction_actor\".\"id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND \"states\".\"group\" != 'triage' AND \"issues\".\"archived_at\" IS NULL AND \"projects\".\"archived_at\" IS NULL AND \"issues\".\"is_draft\" = false AND \"issues\".\"id\" = $1 AND \"workspaces\".\"slug\" = $2 AND \"issues\".\"project_id\" = $3) GROUP BY \"issues\".\"id\", \"states\".\"group\" LIMIT 1",
        module_ids = "COALESCE(ARRAY_AGG(DISTINCT \"modules\".\"id\") FILTER (WHERE NOT (\"modules\".\"id\" IS NULL) AND \"modules\".\"archived_at\" IS NULL AND \"module_through\".\"deleted_at\" IS NULL), '{}') AS \"module_ids\"",
        label_ids = "COALESCE(ARRAY_AGG(DISTINCT \"labels\".\"id\") FILTER (WHERE NOT (\"labels\".\"id\" IS NULL) AND \"label_through\".\"deleted_at\" IS NULL), '{}') AS \"label_ids\"",
        assignee_ids = "COALESCE(ARRAY_AGG(DISTINCT \"assignees\".\"id\") FILTER (WHERE NOT (\"assignees\".\"id\" IS NULL) AND \"project_members\".\"is_active\" = true AND \"assignee_through\".\"deleted_at\" IS NULL), '{}') AS \"assignee_ids\"",
        cycle_id = "(SELECT U0.\"cycle_id\" FROM \"cycle_issues\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"issue_id\" = (\"issues\".\"id\")) LIMIT 1) AS \"cycle_id\"",
        vote_items = retrieve_vote_items_sql(),
        reaction_items = retrieve_reaction_items_sql(),
    )
}

/// `GET anchor/<anchor>/issues/<issue_id>/`
/// (`IssueRetrievePublicEndpoint`, `views/issue.py:597-773`).
async fn retrieve_issue(
    State(state): State<AppState>,
    Path((anchor, issue_id_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Response {
    match retrieve_issue_inner(&state, &anchor, &issue_id_raw, extension).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn retrieve_issue_inner(
    state: &AppState,
    anchor: &str,
    issue_id_raw: &str,
    extension: Option<axum::Extension<SessionHandle>>,
) -> Result<Response, HandlerError> {
    let pool = pool_of(state)?;
    // Timezone first (`TimezoneMixin.initial` runs before the view).
    let _tz = request_tz(pool, extension).await?;
    let board = load_retrieve_board(pool, anchor).await?;
    // Django's `<uuid:issue_id>` converter 404s on garbage before the view
    // runs; the intake precedent maps that to the 404 envelope.
    let issue_id = issue_id_raw
        .parse::<uuid::Uuid>()
        .map_err(|_| HandlerError::NotFound)?;
    let row = fetch_optional_object(
        pool,
        &issue_retrieve_sql(),
        &[
            SqlParam::Uuid(issue_id),
            SqlParam::Text(board.slug),
            opt_project_param(&board.scope.project_id),
        ],
    )
    .await?;
    let Some(row) = row else {
        // `.first()` miss → `Response(None)` 200 with an empty body
        // (QUIRK-first-null; DRF renders `None` as `b''`).
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::empty())
            .expect("empty retrieve response"));
    };
    let row = obj(&row)?;
    let mut out = Map::new();
    for field in RETRIEVE_WIRE_FIELDS {
        let value = row.get(*field).cloned().unwrap_or(Value::Null);
        out.insert((*field).to_owned(), shape_retrieve_value(field, value));
    }
    let body = serde_json::to_string(&out).expect("serializable row");
    Ok(json_ok(fix_sort_order_numbers(&body)))
}

/// Wire order of the 23 retrieve keys. This is NOT the `.values(...)`
/// source order (`views/issue.py:746-770`, mirrored by the read-only
/// `RETRIEVE_VALUES_FIELDS` in the queries layer): Django emits the concrete
/// columns first and the six annotations afterwards in `.annotate()`
/// order (`cycle_id`, then `label_ids`/`assignee_ids`/`module_ids`, then
/// `vote_items`/`reaction_items`) — verified against the live SQL
/// (`SELECT "issues"."id", COALESCE(...) AS "label_ids", ...` for
/// `.values('id', 'vote_items', 'label_ids')`). The queryset is fixed, so
/// this order is deterministic; byte identity depends on it.
const RETRIEVE_WIRE_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "description_json",
    "description_html",
    "description_stripped",
    "description_binary",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "created_by",
    "state__group",
    "cycle_id",
    "label_ids",
    "assignee_ids",
    "module_ids",
    "vote_items",
    "reaction_items",
];

/// Shape one retrieved column the way Django's field deserialization does:
/// `sort_order` is the only float among the 23 keys (integral floats render
/// `65535.0` like the list path; exponents are finished by
/// `fix_sort_order_numbers`), and a `NULL` vote/reaction aggregate reads
/// back as `[]` (Django's `ArrayAgg` returns the empty list, never null —
/// verified live: `{'vote_items': []}` for a voteless issue).
fn shape_retrieve_value(field: &str, value: Value) -> Value {
    if field == "sort_order" {
        if let Some(number) = value.as_f64() {
            return serde_json::Number::from_f64(number)
                .map(Value::Number)
                .unwrap_or(value);
        }
        return value;
    }
    if (field == "vote_items" || field == "reaction_items") && value.is_null() {
        return Value::Array(Vec::new());
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 9, 28).expect("fixed today")
    }

    #[test]
    fn relative_weeks_months_resolve_like_string_date_filter() {
        // `string_date_filter`: months are duration * 30 days.
        assert_eq!(
            resolve_relative("2_weeks", "after", "fromnow", today()),
            Some(("2026-10-12".to_owned(), true))
        );
        assert_eq!(
            resolve_relative("2_weeks", "before", "ago", today()),
            Some(("2026-09-14".to_owned(), false))
        );
        assert_eq!(
            resolve_relative("1_months", "after", "fromnow", today()),
            Some(("2026-10-28".to_owned(), true))
        );
        assert_eq!(
            resolve_relative("1_months", "before", "fromnow", today()),
            Some(("2026-10-28".to_owned(), false))
        );
        // Non-matching heads are not relative.
        assert_eq!(resolve_relative("soon", "after", "fromnow", today()), None);
        assert_eq!(
            resolve_relative("2_years", "after", "fromnow", today()),
            None
        );
        assert_eq!(
            resolve_relative("x_weeks", "after", "fromnow", today()),
            None
        );
    }

    #[test]
    fn relative_expansion_last_wins_and_passes_through() {
        let pairs = vec![
            (
                "created_at".to_owned(),
                "2_weeks;after;fromnow,1_weeks;after;fromnow".to_owned(),
            ),
            ("priority".to_owned(), "high".to_owned()),
            ("target_date".to_owned(), "2026-10-01;after".to_owned()),
        ];
        let expanded = expand_relative_dates(&pairs, today());
        let get = |key: &str| {
            expanded
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        // Same-direction relatives: last wins (dict overwrite).
        assert_eq!(get("created_at"), "2026-10-05;after");
        // Other params untouched.
        assert_eq!(get("priority"), "high");
        assert_eq!(get("target_date"), "2026-10-01;after");
    }

    #[test]
    fn bind_inference_matches_django_coercion() {
        // UUID FKs (`estimate_point`, every `uuid_in` output).
        assert!(matches!(
            bind_typed("11111111-1111-1111-1111-111111111111"),
            SqlParam::Uuid(_)
        ));
        // Date bounds.
        assert!(matches!(bind_typed("2026-10-01"), SqlParam::Date(_)));
        // Integer columns (`issue_intake.status`).
        assert_eq!(bind_typed("-1"), SqlParam::Int(-1));
        assert_eq!(bind_typed("2"), SqlParam::Int(2));
        // Text stays text (`ILIKE` patterns wrap in `%`, never bare).
        assert_eq!(bind_typed("%login%"), SqlParam::Text("%login%".to_owned()));
        assert_eq!(bind_typed("backlog"), SqlParam::Text("backlog".to_owned()));
        // Garbage stays text (Postgres errors like Django's DataError).
        assert_eq!(bind_typed("abc"), SqlParam::Text("abc".to_owned()));
    }

    #[test]
    fn cycle_bare_expr_strips_alias() {
        let bare = bare_cycle_expr();
        assert!(!bare.ends_with("AS \"cycle_id\""));
        assert!(bare.starts_with("(SELECT"));
    }

    #[test]
    fn null_project_param_binds_typed_null() {
        let id = uuid::Uuid::nil();
        assert_eq!(opt_project_param(&Some(id)), SqlParam::Uuid(id));
        assert_eq!(opt_project_param(&None), SqlParam::NullUuid);
    }

    #[test]
    fn group_values_plan_covers_null_project_rules() {
        let ws = uuid::Uuid::nil();
        let scoped = BoardScope {
            workspace_id: ws,
            project_id: Some(uuid::Uuid::nil()),
        };
        let unscoped = BoardScope {
            workspace_id: ws,
            project_id: None,
        };
        // Scoped state branch carries the project conjunct + bind.
        let GroupValuesPlan::Sql { sql, params, .. } = group_values_plan("state_id", &scoped)
        else {
            panic!("state branch is a DB plan");
        };
        assert!(sql.contains("\"states\".\"project_id\" = $2"));
        assert_eq!(params.len(), 2);
        // Null-project state branch drops the conjunct and the bind.
        let GroupValuesPlan::Sql { sql, params, .. } = group_values_plan("state_id", &unscoped)
        else {
            panic!("state branch is a DB plan");
        };
        assert!(!sql.contains("$2"));
        assert_eq!(params.len(), 1);
        // Null-project assignees branch switches to WorkspaceMember.
        let GroupValuesPlan::Sql { sql, params, .. } =
            group_values_plan("assignees__id", &unscoped)
        else {
            panic!("assignees branch is a DB plan");
        };
        assert!(sql.contains("\"workspace_members\""));
        assert!(!sql.contains("project_members"));
        assert_eq!(params.len(), 1);
        // Scoped assignees branch stays on ProjectMember.
        let GroupValuesPlan::Sql { sql, .. } = group_values_plan("assignees__id", &scoped) else {
            panic!("assignees branch is a DB plan");
        };
        assert!(sql.contains("\"project_members\""));
        // Static + unknown branches.
        assert!(matches!(
            group_values_plan("priority", &scoped),
            GroupValuesPlan::Static(_)
        ));
        assert!(matches!(
            group_values_plan("bogus", &scoped),
            GroupValuesPlan::Empty
        ));
    }

    #[test]
    fn group_axes_known_and_unknown() {
        assert!(group_inner_expr("labels__id").is_some());
        assert!(group_inner_expr("priority").is_some());
        assert!(group_inner_expr("cycle_id").is_some());
        assert!(group_inner_expr("bogus").is_none());
        assert!(group_inner_expr("").is_none());
        assert!(group_needs_queryset("target_date"));
        assert!(group_needs_queryset("start_date"));
        assert!(group_needs_queryset("created_by"));
        assert!(!group_needs_queryset("priority"));
    }

    #[test]
    fn ungrouped_projection_covers_on_results_fields() {
        let fields = list_q::on_results_fields("", "");
        let (selects, group) = base_selects("", "", "\"issues\".\"created_at\"").unwrap();
        let _ = group;
        assert_eq!(selects.len(), fields.len() + 2);
        let joined = selects.join(" ");
        assert!(joined.contains("AS \"vote_items\""));
        assert!(joined.contains("AS \"reaction_items\""));
        assert!(joined.contains("AS __order_key"));
    }

    #[test]
    fn sort_order_numbers_render_like_python_repr() {
        let body = r#"{"results":[{"sort_order":65535,"sequence_id":1}],"count":1}"#;
        assert!(fix_sort_order_numbers(body).contains("\"sort_order\":65535.0"));
        // Exponent keeps Python's `+` (serde ryu would print `1e16`).
        let body = r#"{"results":[{"sort_order":1e16}]}"#;
        assert!(fix_sort_order_numbers(body).contains("\"sort_order\":1e+16"));
        // Non-target keys untouched.
        let body = r#"{"sequence_id":1e16}"#;
        assert_eq!(fix_sort_order_numbers(body), body);
    }

    #[test]
    fn order_key_shapes() {
        let (expr, desc) = order_key_expr("-priority_order");
        assert!(desc);
        assert!(expr.starts_with("CASE"));
        let (expr, desc) = order_key_expr("state_order");
        assert!(!desc);
        assert!(expr.contains("\"states\".\"group\""));
        let (expr, desc) = order_key_expr("-created_at");
        assert!(desc);
        assert_eq!(expr, "\"issues\".\"created_at\"");
        assert!(min_values_sql("labels__name").is_some());
        assert!(min_values_sql("-assignees__first_name").is_some());
        assert!(min_values_sql("bogus").is_none());
    }

    #[test]
    fn state_order_case_reverses_for_descending() {
        // `order_queryset.py:26`: ascending keeps `STATE_ORDER` as-is,
        // `-state__group` reverses it (the direction applies on top).
        let (asc_expr, asc_desc) = order_key_expr("state_order");
        assert!(!asc_desc);
        let backlog = asc_expr
            .find("WHEN \"states\".\"group\" = 'backlog'")
            .expect("backlog arm");
        let cancelled = asc_expr
            .find("WHEN \"states\".\"group\" = 'cancelled'")
            .expect("cancelled arm");
        assert!(backlog < cancelled);
        let (desc_expr, desc_desc) = order_key_expr("-state_order");
        assert!(desc_desc);
        let backlog = desc_expr
            .find("WHEN \"states\".\"group\" = 'backlog'")
            .expect("backlog arm");
        let cancelled = desc_expr
            .find("WHEN \"states\".\"group\" = 'cancelled'")
            .expect("cancelled arm");
        assert!(cancelled < backlog);
    }

    #[test]
    fn grouped_envelope_total_counts_distinct_issues() {
        // `hits = queryset.count()`: join fanout must collapse, like the
        // ungrouped total.
        let sql = grouped_total_sql("FROM ...", "WHERE (...)");
        assert!(sql.contains("COUNT(DISTINCT \"issues\".\"id\")"));
        assert!(!sql.contains("COUNT(*)"));
    }

    #[test]
    fn grouped_max_hits_uses_raw_top_count() {
        // `...order_by("-count")[0]["count"]` reads the raw DB count: an
        // all-zero group list reports zero pages, not one.
        assert_eq!(raw_top_count(&[]), 0);
        assert_eq!(
            raw_top_count(&[("a".to_owned(), 0), ("b".to_owned(), 0)]),
            0
        );
        assert_eq!(
            raw_top_count(&[("a".to_owned(), 0), ("b".to_owned(), 3)]),
            3
        );
    }

    #[test]
    fn group_values_workspace_column_is_not_requoted() {
        // The queries layer's `workspace_column` is already a quoted dotted
        // path; wrapping it again emits `""states""` and 500s every
        // DB-backed group axis at plan time.
        let ws = uuid::Uuid::nil();
        let scoped = BoardScope {
            workspace_id: ws,
            project_id: Some(uuid::Uuid::nil()),
        };
        let GroupValuesPlan::Sql { sql, .. } = group_values_plan("state_id", &scoped)
        else {
            panic!("state_id branch is a DB plan");
        };
        assert!(sql.contains("(\"states\".\"workspace_id\" = $1"));
        assert!(!sql.contains("\"\""));
    }

    #[test]
    fn retrieve_aggregates_use_jsonb_build_object() {
        // Django's `JSONObject.as_postgresql` emits `JSONB_BUILD_OBJECT`
        // (`comparison.py:169-174`); `ARRAY_AGG(DISTINCT ...)` over plain
        // `json` has no equality operator and 500s at plan time, so the
        // retrieve vote/reaction fragments must never say `JSON_BUILD_OBJECT`.
        let sql = issue_retrieve_sql();
        assert!(sql.contains("JSONB_BUILD_OBJECT"));
        assert!(!sql.contains("JSON_BUILD_OBJECT"));
    }

    #[test]
    fn list_wire_order_puts_traversal_with_model_columns() {
        // Grouped-labels mode: `labels__id` sorts with the model block
        // (after `state__group`, before the `cycle_id` annotation); the
        // swapped-out `label_ids` is absent (the multi-grouper appends it).
        let fields: Vec<String> = [
            "id",
            "name",
            "state_id",
            "sort_order",
            "estimate_point",
            "priority",
            "start_date",
            "target_date",
            "sequence_id",
            "project_id",
            "parent_id",
            "cycle_id",
            "created_by",
            "state__group",
            "assignee_ids",
            "module_ids",
            "labels__id",
            "vote_items",
            "reaction_items",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let order = list_wire_order(&fields);
        let pos = |k: &str| order.iter().position(|f| f == k).expect(k);
        assert!(pos("state__group") < pos("labels__id"));
        assert!(pos("labels__id") < pos("cycle_id"));
        assert!(pos("cycle_id") < pos("assignee_ids"));
        assert!(!order.iter().any(|f| f == "label_ids"));
    }

    #[test]
    fn retrieve_wire_order_matches_django_column_order() {
        // 23 keys, concrete columns first, annotations in `.annotate()`
        // order — the live Django column order, not the `.values()` source
        // order.
        assert_eq!(
            RETRIEVE_WIRE_FIELDS,
            &[
                "id",
                "name",
                "state_id",
                "sort_order",
                "description_json",
                "description_html",
                "description_stripped",
                "description_binary",
                "estimate_point",
                "priority",
                "start_date",
                "target_date",
                "sequence_id",
                "project_id",
                "parent_id",
                "created_by",
                "state__group",
                "cycle_id",
                "label_ids",
                "assignee_ids",
                "module_ids",
                "vote_items",
                "reaction_items",
            ]
        );
    }

    #[test]
    fn retrieve_null_vote_reaction_items_read_back_empty() {
        // Django's `ArrayAgg` deserializes a NULL aggregate as `[]`.
        assert_eq!(
            shape_retrieve_value("vote_items", Value::Null),
            Value::Array(Vec::new())
        );
        assert_eq!(
            shape_retrieve_value("reaction_items", Value::Null),
            Value::Array(Vec::new())
        );
        // Non-null values pass through untouched.
        let items = serde_json::json!([{"vote": 1}]);
        assert_eq!(
            shape_retrieve_value("vote_items", items.clone()),
            items
        );
        assert_eq!(
            shape_retrieve_value("label_ids", Value::Null),
            Value::Null
        );
    }
}
