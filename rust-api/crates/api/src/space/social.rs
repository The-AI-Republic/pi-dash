//! Space social handlers: comments, issue reactions, comment reactions, votes.
//!
//! Port of the four viewsets in `apps/api/pi_dash/space/views/issue.py`
//! (`IssueCommentPublicViewSet` `:214-339`, `IssueReactionPublicViewSet`
//! `:342-427`, `CommentReactionPublicViewSet` `:430-519`,
//! `IssueVotePublicViewSet` `:522-591`) with route strings from
//! `space/urls/issue.py` (6 of the 8 entries; the two
//! `IssueRetrievePublicEndpoint` routes belong to PIDASHCONV-175).
//!
//! Layering: SQL text for the list halves comes from
//! `pidash_services::space::queries::social`, anchor/enabled mapping from
//! `pidash_services::space::guards`, activity payloads from `pidash_jobs::space`.
//! This module owns the HTTP shell (routes, session auth, row fetching,
//! serializer rendering, writes) plus the reads no query builder covers
//! (single-row gets, inserts, soft-delete updates, member get-or-create,
//! sync-lock probes, description side effects — see below).
//!
//! Reads go through `row_to_json` (the `project_meta` pattern): Postgres
//! renders every column and the handlers shape typed values from the JSON
//! object, so key order and scalar bytes stay under handler control.
//! Datetimes re-render through the serializer kernel in the request's zone
//! (authenticated → the user's zone, anonymous → `deactivate()` → UTC,
//! `views/base.py:37-42`).
//!
//! Method matrix (`views/base.py:48`, `views/issue.py:220-226`): comment
//! list/retrieve are `AllowAny`; every other action on all four viewsets
//! requires a session (`IsAuthenticated`), answered before the board is
//! even read. Reaction and vote lists require auth too (no permission
//! override on those viewsets — `guards::Route::{IssueReactions,
//! CommentReactions, Votes}`, BUG-reaction-list-auth).
//!
//! Response shapes (app serializers, `app/serializers/issue.py:900-989`):
//! `__all__` renders declared fields first (`id` from `BaseSerializer`,
//! then the subclass declarations) in creation order, then model fields
//! in definition order; explicit `Meta.fields` lists render in list order.
//! `is_member` is a read-only annotation that only exists on the list
//! queryset, so create/partial_update responses omit the key (DRF
//! `SkipField`), while list/retrieve include it.
//!
//! Write side effects, in Python order (invisible over HTTP, kept anyway):
//! comment create saves, delays `comment.activity.created` with the
//! validated-output dump, then runs the member get-or-create; reaction
//! create saves, runs the member effect, then delays; vote create
//! get-or-creates, runs the member effect, assigns `vote`, saves, delays.
//! Destroys delay with the pre-delete snapshot *before* the soft delete.
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * BUG-reaction-list-dead (`views/issue.py:348-351`): the issue-reaction
//!   board lookup filters on `slug`/`project_id` kwargs the routes never
//!   supply, so the list always serves `[]`. Ported by executing the dead
//!   board get (`queries::issue_reaction_dead_board_get_sql`) and serving
//!   the empty list on the inevitable miss.
//! * BUG-vote-anchor-as-slug (`views/issue.py:528-530`): the vote board
//!   lookup passes the anchor value to the `workspace__slug` field. Ported
//!   as-is (`queries::vote_dead_board_get_sql`); on a miss the list serves
//!   `[]`, on a (pathological slug-collision) hit the would-be filter
//!   (`queries::vote_list_sql`) runs.
//! * QUIRK-anonymous-member (`views/issue.py:241-250`): `request.user.id`
//!   is evaluated even for anonymous lists. The queries layer binds `$3`
//!   as-is; anonymous binds NULL there (a NULL `$2` project likewise never
//!   matches under `=`, like the queries-layer text).
//! * BUG-comment-reaction-project (`views/issue.py:481`): the
//!   comment-reaction create activity carries `project_id=str(None)` — the
//!   literal string `"None"` — because the routes supply no `project_id`
//!   kwarg. Ported via `jobs::space::comment_reaction_created` with
//!   `"None"`.
//! * QUIRK-vote-always-201 (`views/issue.py:543-571`): vote create answers
//!   201 even when the row already existed, assigns `vote` with no
//!   validation, and never checks `is_votes_enabled`
//!   (`guards::vote_write_gated` pins the last half).
//!
//! Faithful corners:
//!
//! * Instance `.delete()` (all four destroys, plus the comment
//!   save paths through `ProjectBaseModel`) is a soft delete plus a
//!   `soft_delete_related_objects.delay("db", "<model>", pk, "default")`
//!   publish (`db/mixins.py:69-78`), enqueued best-effort like the
//!   activity messages (the intake-handler precedent).
//! * `ProjectBaseModel.save` forces `workspace` from `project.workspace`
//!   (`db/models/project.py:309-311`): written rows resolve the workspace
//!   through the project row, never from the board or the request.
//! * `IssueComment.save` recomputes `comment_stripped` from `comment_html`
//!   on every save and mirrors tracked-field changes into a `descriptions`
//!   row (`db/models/issue.py:601-644`); explicit `comment_stripped` input
//!   is overwritten the same way.
//! * Retrieve of a missing comment answers DRF's `Http404` body
//!   (`{"detail":"Not found."}`) via `get_object_or_404`, while
//!   update/destroy misses answer the `handle_exception`
//!   `ObjectDoesNotExist` envelope — the two 404 bodies differ on purpose.
//! * `request.data.get("vote", 1)` keeps any JSON number that fits an
//!   `int4`; anything else fails like Postgres does (500 envelope), an
//!   explicit null violates the column (400 payload body), and a missing
//!   key means `1`.

use axum::extract::{Extension, Path, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;

use crate::middleware::SessionHandle;
use crate::state::AppState;
use pidash_services::space::{guards, queries};

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Register the seven social route entries under `api/public/` (the six
/// `urls/issue.py` paths this issue owns; the retrieve endpoint's two
/// paths belong to PIDASHCONV-175 and stay unmatched here).
///
/// Owned methods serve from Rust; every other method on these paths falls
/// through to Django (its 405-after-auth and metadata responses live
/// there). `HEAD` rides axum's `get` handling like Django's `GET`-backed
/// `HEAD`; non-UUID path segments match no Django route either, so those
/// proxy and Django's own 404 stays the contract.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/public/anchor/{anchor}/issues/{issue_id}/comments/",
            owned(
                axum::routing::get(comment_list).post(comment_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/public/anchor/{anchor}/issues/{issue_id}/comments/{pk}/",
            owned(
                axum::routing::get(comment_retrieve)
                    .patch(comment_partial_update)
                    .delete(comment_destroy),
                &["GET", "PATCH", "DELETE"],
            ),
        )
        .route(
            "/api/public/anchor/{anchor}/issues/{issue_id}/reactions/",
            owned(
                axum::routing::get(issue_reaction_list).post(issue_reaction_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/public/anchor/{anchor}/issues/{issue_id}/reactions/{reaction_code}/",
            owned(axum::routing::delete(issue_reaction_destroy), &["DELETE"]),
        )
        .route(
            "/api/public/anchor/{anchor}/comments/{comment_id}/reactions/",
            owned(
                axum::routing::get(comment_reaction_list).post(comment_reaction_create),
                &["GET", "POST"],
            ),
        )
        .route(
            "/api/public/anchor/{anchor}/comments/{comment_id}/reactions/{reaction_code}/",
            owned(axum::routing::delete(comment_reaction_destroy), &["DELETE"]),
        )
        .route(
            "/api/public/anchor/{anchor}/issues/{issue_id}/votes/",
            owned(
                axum::routing::get(vote_list)
                    .post(vote_create)
                    .delete(vote_destroy),
                &["GET", "POST", "DELETE"],
            ),
        )
}

/// A social path: the owned methods serve from Rust, everything else falls
/// through to Django. Same cutover shape as the intake handlers.
fn owned(
    methods: axum::routing::MethodRouter<AppState>,
    owned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = methods;
    for other in ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        if owned.contains(&other) {
            continue;
        }
        router = match other {
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

/// Handler failure: the exact status + body Python answers.
#[derive(Debug)]
enum HandlerError {
    /// 401 `{"detail":"Authentication credentials were not provided."}` —
    /// anonymous on a guarded action.
    Unauthorized,
    /// 404 `{"error":"The required object does not exist."}` — bare
    /// `.get()` misses (board on writes, actor-scoped comment/reaction/vote
    /// gets) via `handle_exception`.
    NotFound,
    /// 404 `{"detail":"Not found."}` — DRF's `Http404` body for retrieve
    /// misses through `get_object_or_404`.
    NotFoundDetail,
    /// A guards-layer error body (enabled-flag 400s, validation envelope).
    Guard(guards::ErrorBody),
    /// 400 serializer-errors body (`{"field": [...]}`).
    BadJson(Value),
    /// 400 `{"detail": ...}` (malformed JSON bodies).
    BadDetail(String),
    /// 500 `{"error":"Something went wrong please try again later"}` —
    /// the fallback envelope.
    ServerError,
}

impl HandlerError {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            HandlerError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                r#"{"detail":"Authentication credentials were not provided."}"#.to_owned(),
            ),
            HandlerError::NotFound => (
                StatusCode::NOT_FOUND,
                r#"{"error":"The required object does not exist."}"#.to_owned(),
            ),
            HandlerError::NotFoundDetail => (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Not found."}"#.to_owned(),
            ),
            HandlerError::Guard(error) => (
                StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                serde_json::to_string(&error.body).unwrap_or_else(|_| {
                    r#"{"error":"Something went wrong please try again later"}"#.to_owned()
                }),
            ),
            HandlerError::BadJson(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).unwrap_or_else(|_| {
                    r#"{"error":"Something went wrong please try again later"}"#.to_owned()
                }),
            ),
            HandlerError::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            HandlerError::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":"Something went wrong please try again later"}"#.to_owned(),
            ),
        }
    }
}

impl IntoResponse for HandlerError {
    fn into_response(self) -> Response {
        if matches!(self, HandlerError::ServerError) {
            // `log_exception(e)` (`views/base.py:182`).
            tracing::warn!("space social handler: internal error");
        }
        let (status, body) = self.status_and_body();
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .expect("handler error response")
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
        .expect("handler json response")
}

fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, HandlerError> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(HandlerError::ServerError)
}

/// Map a write failure the way `handle_exception` does: unique and
/// foreign-key violations are the `IntegrityError` 400, anything else the
/// fallback 500.
fn integrity_error(error: sqlx::Error) -> HandlerError {
    if let sqlx::Error::Database(db) = &error {
        let code = db.code().unwrap_or_default();
        if code == "23503" || code == "23505" {
            return HandlerError::Guard(guards::handle_exception(
                guards::ExceptionKind::IntegrityError,
            ));
        }
    }
    let _ = error;
    HandlerError::ServerError
}

// ---------------------------------------------------------------------------
// row_to_json fetching + field access (the project_meta shape)
// ---------------------------------------------------------------------------

/// One bound `$N` parameter. UUID columns bind typed `Uuid`; text columns
/// (`anchor`) bind as text; the anonymous member probe binds NULL.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SqlParam<'a> {
    Text(&'a str),
    Uuid(uuid::Uuid),
    Null,
}

/// Parse a UUID-typed bind. Callers pass path segments Django's `<uuid:>`
/// converter already validated, or ids read back from the database —
/// anything else is a 500 like Python's `ValidationError` → fallback.
fn uuid_param(raw: &str) -> Result<SqlParam<'_>, HandlerError> {
    uuid::Uuid::parse_str(raw)
        .map(SqlParam::Uuid)
        .map_err(|_| HandlerError::ServerError)
}

/// Fetch zero or one row of `inner` as a JSON object.
async fn fetch_optional_object(
    pool: &sqlx::PgPool,
    inner: &str,
    params: &[SqlParam<'_>],
) -> Result<Option<Value>, HandlerError> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let mut query = sqlx::query(&sql);
    for param in params {
        query = match param {
            SqlParam::Text(text) => query.bind(*text),
            SqlParam::Uuid(id) => query.bind(*id),
            SqlParam::Null => query.bind(Option::<uuid::Uuid>::None),
        };
    }
    let row: Option<sqlx::postgres::PgRow> = query
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
    params: &[SqlParam<'_>],
) -> Result<Vec<Value>, HandlerError> {
    let sql = format!("SELECT row_to_json(__r)::text AS __row FROM ({inner}) AS __r");
    let mut query = sqlx::query(&sql);
    for param in params {
        query = match param {
            SqlParam::Text(text) => query.bind(*text),
            SqlParam::Uuid(id) => query.bind(*id),
            SqlParam::Null => query.bind(Option::<uuid::Uuid>::None),
        };
    }
    let rows: Vec<sqlx::postgres::PgRow> = query
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

fn req_str(obj: &serde_json::Map<String, Value>, key: &str) -> Result<String, HandlerError> {
    match obj.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(HandlerError::ServerError),
    }
}

fn opt_str(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, HandlerError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(HandlerError::ServerError),
    }
}

fn req_bool(obj: &serde_json::Map<String, Value>, key: &str) -> Result<bool, HandlerError> {
    match obj.get(key) {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(HandlerError::ServerError),
    }
}

fn req_json(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Value, HandlerError> {
    obj.get(key).cloned().ok_or(HandlerError::ServerError)
}

/// Render a `row_to_json` timestamptz value exactly like DRF `DateTimeField`
/// with the default `iso-8601` format: ISO-8601 in the request's zone with
/// `+00:00` rewritten to `Z` (serializer kernel rule).
fn render_dt(raw: &str, tz: &chrono_tz::Tz) -> Result<String, HandlerError> {
    let dt = chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| HandlerError::ServerError)?;
    Ok(crate::serializer::render_datetime_in(&dt, tz))
}

fn render_dt_opt(raw: Option<String>, tz: &chrono_tz::Tz) -> Result<Option<String>, HandlerError> {
    raw.map(|text| render_dt(&text, tz)).transpose()
}

// ---------------------------------------------------------------------------
// Request context: auth + anchor board (TimezoneMixin + permission layer)
// ---------------------------------------------------------------------------

/// Authenticated actor plus time zone. Anonymous is `None` and proceeds on
/// `AllowAny` actions only (anonymous renders in UTC, `views/base.py:42`);
/// guarded actions 401 through [`require_actor`].
struct Actor {
    id: uuid::Uuid,
    timezone: chrono_tz::Tz,
}

/// `request.user` through Django-session auth (the intake-handler shape:
/// the shared license resolver, which also verifies the session hash).
async fn request_actor(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Option<Actor>, HandlerError> {
    let pool = pool_of(state)?;
    let resolved =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    Ok(resolved.map(|actor| Actor {
        id: actor.id,
        timezone: actor.timezone,
    }))
}

/// `IsAuthenticated` (`views/base.py:48`): anonymous answers the DRF
/// `NotAuthenticated` body before anything else runs.
fn require_actor(actor: Option<Actor>) -> Result<Actor, HandlerError> {
    actor.ok_or(HandlerError::Unauthorized)
}

/// The request's zone for datetime rendering: the actor's zone when
/// authenticated, UTC when anonymous.
fn request_tz(actor: Option<&Actor>) -> chrono_tz::Tz {
    actor.map(|actor| actor.timezone).unwrap_or(chrono_tz::UTC)
}

/// The board columns the social actions read.
struct BoardRow {
    workspace_id: String,
    project_id: Option<String>,
    is_comments_enabled: bool,
    is_reactions_enabled: bool,
}

impl BoardRow {
    fn from_value(value: &Value) -> Result<Self, HandlerError> {
        let o = obj(value)?;
        Ok(Self {
            workspace_id: req_str(o, "workspace_id")?,
            project_id: opt_str(o, "project_id")?,
            is_comments_enabled: req_bool(o, "is_comments_enabled")?,
            is_reactions_enabled: req_bool(o, "is_reactions_enabled")?,
        })
    }
}

/// `str(project_deploy_board.project_id)`: the activity payloads render a
/// null project as the literal `"None"`, like Python.
fn board_project_str(board: &BoardRow) -> String {
    board
        .project_id
        .clone()
        .unwrap_or_else(|| "None".to_owned())
}

/// Resolve `DeployBoard.objects.get(anchor, entity_name="project")`
/// (`queries::social_board_get_sql`). A miss raises through to the
/// `ObjectDoesNotExist` envelope on writes (`AnchorLookup::GetRaises`);
/// list querysets catch it and serve `[]` instead.
async fn fetch_board(pool: &sqlx::PgPool, anchor: &str) -> Result<Option<BoardRow>, HandlerError> {
    let row = fetch_optional_object(
        pool,
        &queries::social::social_board_get_sql(),
        &[SqlParam::Text(anchor)],
    )
    .await?;
    row.map(|value| BoardRow::from_value(&value)).transpose()
}

/// `ProjectBaseModel.save` forces `workspace` from `project.workspace`
/// (`db/models/project.py:309-311`): written rows resolve the workspace
/// through the project row. Forward FK reads use the soft-deletion
/// manager, hence the `deleted_at IS NULL` guard.
async fn project_workspace_id(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<uuid::Uuid, HandlerError> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as("SELECT workspace_id FROM projects WHERE id = $1 AND deleted_at IS NULL")
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    row.map(|row| row.0).ok_or(HandlerError::ServerError)
}

// ---------------------------------------------------------------------------
// `json.dumps` with CPython defaults (Celery `requested_data` payloads)
// ---------------------------------------------------------------------------

/// `json.dumps(value)`: `separators=(', ', ': ')`, `ensure_ascii=True`,
/// insertion-ordered keys. Same implementation as the intake handlers'
/// `python_dumps` (convergent copy; the two dedupe if both merge).
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

/// CPython `py_encode_basestring_ascii`.
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

/// `django.utils.html.strip_tags`: remove every `<...>` span. The comment
/// `save()` computes `comment_stripped` this way (`db/models/issue.py:601`;
/// `""` stays `""`).
fn strip_tags(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tasks: issue_activity publishing through the queue
// ---------------------------------------------------------------------------

/// Enqueue a worker message for the worker to forward to the broker
/// (Python-owned task). Best-effort after commit: without it the response
/// still stands (the intake-handler precedent).
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

/// `soft_delete_related_objects.delay("db", "<model>", pk, "default")`
/// (`db/mixins.py:77`): positional args, no kwargs. Fires on every
/// instance `.delete()` in these views.
async fn enqueue_soft_delete(pool: &sqlx::PgPool, model: &str, pk: &uuid::Uuid) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(
        pidash_jobs::tasks_cleanup::deletion::SOFT_DELETE_TASK,
        vec![
            serde_json::Value::String("db".to_owned()),
            serde_json::Value::String(model.to_owned()),
            serde_json::Value::String(pk.to_string()),
            serde_json::Value::String("default".to_owned()),
        ],
        Default::default(),
    );
    enqueue_message(pool, message).await;
}

/// The member side effect shared by the comment / reaction / vote creates:
/// non-members are tracked via `ProjectPublicMember.get_or_create`
/// (`views/issue.py:283-291` and siblings) using the jobs-layer SQL.
async fn ensure_public_member(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    member_id: &uuid::Uuid,
) -> Result<(), HandlerError> {
    let member: Option<(i32,)> = sqlx::query_as(pidash_jobs::space::MEMBER_EXISTS_SQL)
        .bind(project_id)
        .bind(member_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    if member.is_some() {
        return Ok(());
    }
    let existing: Option<(uuid::Uuid,)> = sqlx::query_as(pidash_jobs::space::PUBLIC_MEMBER_GET_SQL)
        .bind(project_id)
        .bind(member_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    if existing.is_none() {
        sqlx::query(pidash_jobs::space::PUBLIC_MEMBER_INSERT_SQL)
            .bind(uuid::Uuid::new_v4())
            .bind(project_id)
            .bind(workspace_id)
            .bind(member_id)
            .execute(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared nests: users, assets, lite leaves
// ---------------------------------------------------------------------------

/// `SELECT` for the user row behind `actor_detail` nests. Forward-FK
/// semantics carry the soft-deletion guard.
fn user_detail_sql() -> String {
    "SELECT \"users\".\"id\", \"users\".\"first_name\", \"users\".\"last_name\", \"users\".\"avatar\", \"users\".\"avatar_asset_id\", \"users\".\"is_bot\", \"users\".\"display_name\" FROM \"users\" WHERE (\"users\".\"deleted_at\" IS NULL AND \"users\".\"id\" = $1)".to_string()
}

/// `SELECT` for the logo/cover asset row behind `*_url` properties (the
/// project_meta shape).
fn logo_asset_sql() -> String {
    "SELECT \"a\".\"id\", \"a\".\"entity_type\", \"a\".\"workspace_id\", \"a\".\"project_id\", \"a\".\"issue_id\", \"w\".\"slug\" AS \"workspace_slug\" FROM \"file_assets\" AS \"a\" LEFT OUTER JOIN \"workspaces\" AS \"w\" ON (\"a\".\"workspace_id\" = \"w\".\"id\") WHERE (\"a\".\"deleted_at\" IS NULL AND \"a\".\"id\" = $1)".to_string()
}

/// Port of `FileAsset.asset_url` (`db/models/asset.py:80-99`): static-asset
/// branches render `/api/assets/v2/static/<id>/`; attachment/description
/// branches interpolate ids; anything else renders `None`.
fn asset_url(
    asset_id: &str,
    entity_type: Option<&str>,
    workspace_slug: Option<&str>,
    project_id: Option<&str>,
    issue_id: Option<&str>,
) -> Option<String> {
    match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Some(format!("/api/assets/v2/static/{asset_id}/"))
        }
        Some("ISSUE_ATTACHMENT") => {
            let (slug, project, issue) = match (workspace_slug, project_id, issue_id) {
                (Some(s), Some(p), Some(i)) => (s, p, i),
                _ => return None,
            };
            Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/issues/{issue}/attachments/{asset_id}/"
            ))
        }
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => {
            let (slug, project) = match (workspace_slug, project_id) {
                (Some(s), Some(p)) => (s, p),
                _ => return None,
            };
            Some(format!(
                "/api/assets/v2/workspaces/{slug}/projects/{project}/{asset_id}/"
            ))
        }
        _ => None,
    }
}

/// Resolve a logo/cover/avatar URL the way the model properties do: the
/// asset's `asset_url` when the FK is set (even when that computes to
/// `None`), else the raw string column, else `None`. Empty strings are
/// falsy in Python (`if self.avatar`), so they render `None` too.
async fn logo_or_cover_url(
    pool: &sqlx::PgPool,
    raw: Option<String>,
    asset_id: Option<String>,
) -> Result<Option<String>, HandlerError> {
    let Some(asset_id) = asset_id else {
        return Ok(raw.filter(|text| !text.is_empty()));
    };
    let row = fetch_optional_object(pool, &logo_asset_sql(), &[uuid_param(&asset_id)?])
        .await?
        .ok_or(HandlerError::ServerError)?;
    let o = obj(&row)?;
    let entity_type = opt_str(o, "entity_type")?;
    let workspace_slug = opt_str(o, "workspace_slug")?;
    let project_id = opt_str(o, "project_id")?;
    let issue_id = opt_str(o, "issue_id")?;
    let id = req_str(o, "id")?;
    Ok(asset_url(
        &id,
        entity_type.as_deref(),
        workspace_slug.as_deref(),
        project_id.as_deref(),
        issue_id.as_deref(),
    ))
}

/// App `UserLiteSerializer` (`app/serializers/user.py:141-154`), in
/// `Meta.fields` order.
#[derive(Debug, Serialize)]
struct ActorDetailView {
    id: String,
    first_name: String,
    last_name: String,
    avatar: Option<String>,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
}

/// Render one user row as `actor_detail`. A dangling FK is a 500 like
/// Python's `DoesNotExist` → fallback envelope.
async fn render_actor_detail(
    pool: &sqlx::PgPool,
    user_id: &str,
) -> Result<ActorDetailView, HandlerError> {
    let row = fetch_optional_object(pool, &user_detail_sql(), &[uuid_param(user_id)?])
        .await?
        .ok_or(HandlerError::ServerError)?;
    let o = obj(&row)?;
    let avatar = opt_str(o, "avatar")?;
    let avatar_url =
        logo_or_cover_url(pool, avatar.clone(), opt_str(o, "avatar_asset_id")?).await?;
    Ok(ActorDetailView {
        id: req_str(o, "id")?,
        first_name: opt_str(o, "first_name")?.unwrap_or_default(),
        last_name: opt_str(o, "last_name")?.unwrap_or_default(),
        avatar: avatar.filter(|text| !text.is_empty()),
        avatar_url,
        is_bot: req_bool(o, "is_bot")?,
        display_name: opt_str(o, "display_name")?.unwrap_or_default(),
    })
}

// ---------------------------------------------------------------------------
// Query strings (Django QueryDict.get: last value wins)
// ---------------------------------------------------------------------------

/// One query value, repeated or not (the app_issues shape).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

type QueryMap = std::collections::HashMap<String, OneOrMany>;

/// Django `QueryDict.get`: the last value, or `None`.
fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query.get(key).and_then(|value| match value {
        OneOrMany::One(one) => Some(one.clone()),
        OneOrMany::Many(many) => many.last().cloned(),
    })
}

// ---------------------------------------------------------------------------
// Request bodies (DRF JSON parsing)
// ---------------------------------------------------------------------------

/// Django's `DATA_UPLOAD_MAX_MEMORY_SIZE` default.
const MAX_BODY: usize = 2_621_440;

/// Read the request body the way DRF does for JSON posts: empty → `{}`;
/// malformed → `ParseError` 400; non-object JSON → the attribute errors
/// the view code hits (500 envelope).
async fn read_body(req: Request) -> Result<Value, HandlerError> {
    let bytes = axum::body::to_bytes(req.into_body(), MAX_BODY)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    parse_body(&bytes)
}

fn parse_body(raw: &[u8]) -> Result<Value, HandlerError> {
    if raw.is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(Value::Object(map)) => Ok(Value::Object(map)),
        Ok(_) => Err(HandlerError::ServerError),
        Err(error) => Err(HandlerError::BadDetail(format!(
            "JSON parse error - {error}"
        ))),
    }
}

/// Proxy to Django (its own 404 stays the contract) for path segments
/// Django's `<uuid:>` converter would never route to these views.
async fn proxy(state: &AppState, req: Request) -> Response {
    crate::edge::proxy(State(state.clone()), req).await
}

// ---------------------------------------------------------------------------
// Comment shapes (app IssueCommentSerializer, issue.py:948-989)
// ---------------------------------------------------------------------------

/// App `IssueFlatSerializer` for `issue_detail`
/// (`app/serializers/issue.py:105-123`), in `Meta.fields` order.
#[derive(Debug, Serialize)]
struct IssueFlatView {
    id: String,
    name: String,
    description_json: Value,
    description_html: Option<String>,
    priority: String,
    complexity_score: Value,
    start_date: Option<String>,
    target_date: Option<String>,
    sequence_id: i64,
    sort_order: Value,
    is_draft: bool,
}

/// App `ProjectLiteSerializer` for `project_detail`
/// (`app/serializers/project.py:120-133`), in `Meta.fields` order.
#[derive(Debug, Serialize)]
struct ProjectLiteView {
    id: String,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    cover_image_url: Option<String>,
    logo_props: Value,
    description: Option<String>,
    is_default: bool,
}

/// App `WorkspaceLiteSerializer` for `workspace_detail`
/// (`app/serializers/workspace.py:79-83`), in `Meta.fields` order.
#[derive(Debug, Serialize)]
struct WorkspaceLiteView {
    name: String,
    slug: String,
    id: String,
    logo_url: Option<String>,
}

/// App `CommentReactionSerializer` (`app/serializers/issue.py:917-937`),
/// in `Meta.fields` order. `display_name` reads `actor.display_name`.
#[derive(Debug, Serialize)]
struct CommentReactionView {
    id: String,
    actor: String,
    comment: String,
    reaction: String,
    display_name: String,
    deleted_at: Option<String>,
    workspace: Option<String>,
    project: Option<String>,
    created_at: String,
    updated_at: String,
    created_by: Option<String>,
    updated_by: Option<String>,
}

/// App `IssueCommentSerializer` (`app/serializers/issue.py:948-972`):
/// declared fields first (`id`, then the subclass declarations in
/// creation order), then model definition order. `is_member` is only
/// present on queryset-annotated rows (list/retrieve), never on
/// create/update responses.
#[derive(Debug, Serialize)]
struct CommentView {
    id: String,
    actor_detail: Option<ActorDetailView>,
    issue_detail: IssueFlatView,
    project_detail: ProjectLiteView,
    workspace_detail: WorkspaceLiteView,
    comment_reactions: Vec<CommentReactionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_member: Option<bool>,
    is_synced: bool,
    created_at: String,
    updated_at: String,
    created_by: Option<String>,
    updated_by: Option<String>,
    deleted_at: Option<String>,
    project: Option<String>,
    workspace: Option<String>,
    comment_stripped: Option<String>,
    comment_json: Value,
    comment_html: Option<String>,
    description: Option<String>,
    attachments: Value,
    labels: Value,
    issue: Option<String>,
    actor: Option<String>,
    access: Option<String>,
    external_source: Option<String>,
    external_id: Option<String>,
    speaker_type: Option<String>,
    speaker_label: Option<String>,
    speaker_agent_run_id: Option<String>,
    edited_at: Option<String>,
    parent: Option<String>,
}

/// The `issue_comments` columns one render needs, read from the
/// `row_to_json` object. `created_by`/`updated_by` arrive as
/// `created_by_id` from `SELECT *` (the column names); the `or_else`
/// fallback covers aliased selects.
struct CommentRow {
    id: String,
    created_at: String,
    updated_at: String,
    created_by: Option<String>,
    updated_by: Option<String>,
    deleted_at: Option<String>,
    project_id: Option<String>,
    workspace_id: Option<String>,
    comment_stripped: Option<String>,
    comment_json: Value,
    comment_html: Option<String>,
    description_id: Option<String>,
    attachments: Value,
    labels: Value,
    issue_id: Option<String>,
    actor_id: Option<String>,
    access: Option<String>,
    external_source: Option<String>,
    external_id: Option<String>,
    speaker_type: Option<String>,
    speaker_label: Option<String>,
    speaker_agent_run_id: Option<String>,
    edited_at: Option<String>,
    parent_id: Option<String>,
    is_member: Option<bool>,
}

impl CommentRow {
    fn from_value(value: &Value) -> Result<Self, HandlerError> {
        let o = obj(value)?;
        let is_member = match o.get("is_member") {
            None => None,
            Some(Value::Bool(b)) => Some(*b),
            Some(Value::Null) => None,
            _ => return Err(HandlerError::ServerError),
        };
        Ok(Self {
            id: req_str(o, "id")?,
            created_at: req_str(o, "created_at")?,
            updated_at: req_str(o, "updated_at")?,
            created_by: opt_str(o, "created_by_id")?
                .or_else(|| opt_str(o, "created_by").unwrap_or(None)),
            updated_by: opt_str(o, "updated_by_id")?
                .or_else(|| opt_str(o, "updated_by").unwrap_or(None)),
            deleted_at: opt_str(o, "deleted_at")?,
            project_id: opt_str(o, "project_id")?,
            workspace_id: opt_str(o, "workspace_id")?,
            comment_stripped: opt_str(o, "comment_stripped")?,
            comment_json: o.get("comment_json").cloned().unwrap_or(Value::Null),
            comment_html: opt_str(o, "comment_html")?,
            description_id: opt_str(o, "description_id")?,
            attachments: o.get("attachments").cloned().unwrap_or(Value::Null),
            labels: o.get("labels").cloned().unwrap_or(Value::Null),
            issue_id: opt_str(o, "issue_id")?,
            actor_id: opt_str(o, "actor_id")?,
            access: opt_str(o, "access")?,
            external_source: opt_str(o, "external_source")?,
            external_id: opt_str(o, "external_id")?,
            speaker_type: opt_str(o, "speaker_type")?,
            speaker_label: opt_str(o, "speaker_label")?,
            speaker_agent_run_id: opt_str(o, "speaker_agent_run_id")?,
            edited_at: opt_str(o, "edited_at")?,
            parent_id: opt_str(o, "parent_id")?,
            is_member,
        })
    }
}

/// Render one comment row with its nests. `include_member` is true for
/// list/retrieve (annotated queryset) and false for create/update (plain
/// instance: the key is skipped, like DRF's `SkipField`).
async fn render_comment(
    pool: &sqlx::PgPool,
    row: &CommentRow,
    tz: &chrono_tz::Tz,
    include_member: bool,
) -> Result<CommentView, HandlerError> {
    let issue_detail = render_issue_flat(pool, row.issue_id.as_deref()).await?;
    let project_detail = render_project_lite(pool, row.project_id.as_deref()).await?;
    let workspace_detail = render_workspace_lite(pool, row.workspace_id.as_deref()).await?;
    let comment_reactions = render_comment_reactions(pool, &row.id, tz).await?;
    let actor_detail = match row.actor_id.as_deref() {
        None => None,
        Some(actor) => Some(render_actor_detail(pool, actor).await?),
    };
    let is_synced = comment_is_synced(pool, row.external_source.as_deref(), &row.id).await?;
    Ok(CommentView {
        id: row.id.clone(),
        actor_detail,
        issue_detail,
        project_detail,
        workspace_detail,
        comment_reactions,
        is_member: if include_member { row.is_member } else { None },
        is_synced,
        created_at: render_dt(&row.created_at, tz)?,
        updated_at: render_dt(&row.updated_at, tz)?,
        created_by: row.created_by.clone(),
        updated_by: row.updated_by.clone(),
        deleted_at: render_dt_opt(row.deleted_at.clone(), tz)?,
        project: row.project_id.clone(),
        workspace: row.workspace_id.clone(),
        comment_stripped: row.comment_stripped.clone(),
        comment_json: row.comment_json.clone(),
        comment_html: row.comment_html.clone(),
        description: row.description_id.clone(),
        attachments: row.attachments.clone(),
        labels: row.labels.clone(),
        issue: row.issue_id.clone(),
        actor: row.actor_id.clone(),
        access: row.access.clone(),
        external_source: row.external_source.clone(),
        external_id: row.external_id.clone(),
        speaker_type: row.speaker_type.clone(),
        speaker_label: row.speaker_label.clone(),
        speaker_agent_run_id: row.speaker_agent_run_id.clone(),
        edited_at: render_dt_opt(row.edited_at.clone(), tz)?,
        parent: row.parent_id.clone(),
    })
}

/// `issue_detail`: the flat leaf over the comment's issue FK. A dangling
/// FK is a 500 like Python's `DoesNotExist` → fallback envelope.
async fn render_issue_flat(
    pool: &sqlx::PgPool,
    issue_id: Option<&str>,
) -> Result<IssueFlatView, HandlerError> {
    let Some(issue_id) = issue_id else {
        return Err(HandlerError::ServerError);
    };
    let row = fetch_optional_object(
        pool,
        "SELECT \"issues\".\"id\", \"issues\".\"name\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"priority\", \"issues\".\"complexity_score\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"sort_order\", \"issues\".\"is_draft\" FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" = $1)",
        &[uuid_param(issue_id)?],
    )
    .await?
    .ok_or(HandlerError::ServerError)?;
    let o = obj(&row)?;
    let sequence_id = match o.get("sequence_id") {
        Some(Value::Number(n)) => n.as_i64().ok_or(HandlerError::ServerError)?,
        _ => return Err(HandlerError::ServerError),
    };
    // DRF `FloatField` renders `65535.0`, never Postgres `float8out`'s
    // `65535` (the project_meta kernel rule).
    let sort_order = match o.get("sort_order") {
        Some(Value::Number(n)) => {
            let float = n.as_f64().ok_or(HandlerError::ServerError)?;
            serde_json::Number::from_f64(float)
                .map(Value::Number)
                .ok_or(HandlerError::ServerError)?
        }
        _ => return Err(HandlerError::ServerError),
    };
    Ok(IssueFlatView {
        id: req_str(o, "id")?,
        name: req_str(o, "name")?,
        description_json: req_json(o, "description_json")?,
        description_html: opt_str(o, "description_html")?,
        priority: req_str(o, "priority")?,
        complexity_score: req_json(o, "complexity_score")?,
        start_date: opt_str(o, "start_date")?,
        target_date: opt_str(o, "target_date")?,
        sequence_id,
        sort_order,
        is_draft: req_bool(o, "is_draft")?,
    })
}

/// `project_detail`: the app lite leaf over the comment's project FK.
async fn render_project_lite(
    pool: &sqlx::PgPool,
    project_id: Option<&str>,
) -> Result<ProjectLiteView, HandlerError> {
    let Some(project_id) = project_id else {
        return Err(HandlerError::ServerError);
    };
    let row = fetch_optional_object(
        pool,
        "SELECT \"projects\".\"id\", \"projects\".\"identifier\", \"projects\".\"name\", \"projects\".\"cover_image\", \"projects\".\"cover_image_asset_id\", \"projects\".\"logo_props\", \"projects\".\"description\", \"projects\".\"is_default\" FROM \"projects\" WHERE (\"projects\".\"deleted_at\" IS NULL AND \"projects\".\"id\" = $1)",
        &[uuid_param(project_id)?],
    )
    .await?
    .ok_or(HandlerError::ServerError)?;
    let o = obj(&row)?;
    let cover_image_url = logo_or_cover_url(
        pool,
        opt_str(o, "cover_image")?,
        opt_str(o, "cover_image_asset_id")?,
    )
    .await?;
    Ok(ProjectLiteView {
        id: req_str(o, "id")?,
        identifier: req_str(o, "identifier")?,
        name: req_str(o, "name")?,
        cover_image: opt_str(o, "cover_image")?,
        cover_image_url,
        logo_props: req_json(o, "logo_props")?,
        description: opt_str(o, "description")?,
        is_default: req_bool(o, "is_default")?,
    })
}

/// `workspace_detail`: the app lite leaf over the comment's workspace FK.
async fn render_workspace_lite(
    pool: &sqlx::PgPool,
    workspace_id: Option<&str>,
) -> Result<WorkspaceLiteView, HandlerError> {
    let Some(workspace_id) = workspace_id else {
        return Err(HandlerError::ServerError);
    };
    let row = fetch_optional_object(
        pool,
        "SELECT \"workspaces\".\"id\", \"workspaces\".\"name\", \"workspaces\".\"slug\", \"workspaces\".\"logo\", \"workspaces\".\"logo_asset_id\" FROM \"workspaces\" WHERE (\"workspaces\".\"deleted_at\" IS NULL AND \"workspaces\".\"id\" = $1)",
        &[uuid_param(workspace_id)?],
    )
    .await?
    .ok_or(HandlerError::ServerError)?;
    let o = obj(&row)?;
    let logo_url =
        logo_or_cover_url(pool, opt_str(o, "logo")?, opt_str(o, "logo_asset_id")?).await?;
    Ok(WorkspaceLiteView {
        name: req_str(o, "name")?,
        slug: req_str(o, "slug")?,
        id: req_str(o, "id")?,
        logo_url,
    })
}

/// `comment_reactions`: the live reaction rows for one comment, newest
/// first (model `ordering = ("-created_at",)`).
async fn render_comment_reactions(
    pool: &sqlx::PgPool,
    comment_id: &str,
    tz: &chrono_tz::Tz,
) -> Result<Vec<CommentReactionView>, HandlerError> {
    let rows = fetch_all_objects(
        pool,
        "SELECT \"comment_reactions\".* FROM \"comment_reactions\" WHERE (\"comment_reactions\".\"deleted_at\" IS NULL AND \"comment_reactions\".\"comment_id\" = $1) ORDER BY \"comment_reactions\".\"created_at\" DESC",
        &[uuid_param(comment_id)?],
    )
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(render_comment_reaction_row(pool, row, tz).await?);
    }
    Ok(out)
}

async fn render_comment_reaction_row(
    pool: &sqlx::PgPool,
    row: &Value,
    tz: &chrono_tz::Tz,
) -> Result<CommentReactionView, HandlerError> {
    let o = obj(row)?;
    let actor_id = req_str(o, "actor_id")?;
    let display_name = fetch_display_name(pool, &actor_id).await?;
    Ok(CommentReactionView {
        id: req_str(o, "id")?,
        actor: actor_id,
        comment: req_str(o, "comment_id")?,
        reaction: req_str(o, "reaction")?,
        display_name,
        deleted_at: render_dt_opt(opt_str(o, "deleted_at")?, tz)?,
        workspace: opt_str(o, "workspace_id")?,
        project: opt_str(o, "project_id")?,
        created_at: render_dt(&req_str(o, "created_at")?, tz)?,
        updated_at: render_dt(&req_str(o, "updated_at")?, tz)?,
        created_by: opt_str(o, "created_by_id")?
            .or_else(|| opt_str(o, "created_by").unwrap_or(None)),
        updated_by: opt_str(o, "updated_by_id")?
            .or_else(|| opt_str(o, "updated_by").unwrap_or(None)),
    })
}

/// `display_name` for reaction rows (`source="actor.display_name"`).
async fn fetch_display_name(pool: &sqlx::PgPool, user_id: &str) -> Result<String, HandlerError> {
    let id = uuid::Uuid::parse_str(user_id).map_err(|_| HandlerError::ServerError)?;
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT display_name FROM users WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    row.map(|row| row.0.unwrap_or_default())
        .ok_or(HandlerError::ServerError)
}

/// The synced-comment lock (`app/serializers/issue.py:79-91`): native rows
/// (`external_source` empty) short-circuit before any DB probe.
async fn comment_is_synced(
    pool: &sqlx::PgPool,
    external_source: Option<&str>,
    comment_id: &str,
) -> Result<bool, HandlerError> {
    if external_source.unwrap_or("").is_empty() {
        return Ok(false);
    }
    let id = uuid::Uuid::parse_str(comment_id).map_err(|_| HandlerError::ServerError)?;
    let git: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM git_comment_syncs WHERE deleted_at IS NULL AND comment_id = $1 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    if git.is_some() {
        return Ok(true);
    }
    let github: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM github_comment_syncs WHERE deleted_at IS NULL AND comment_id = $1 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    Ok(github.is_some())
}

// ---------------------------------------------------------------------------
// Validation (serializer `is_valid`, error bodies are `serializer.errors`)
// ---------------------------------------------------------------------------

type FieldErrors = serde_json::Map<String, Value>;

fn push_error(errors: &mut FieldErrors, field: &str, message: String) {
    errors
        .entry(field.to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("error entry is an array")
        .push(Value::String(message));
}

fn field_errors(errors: FieldErrors) -> Result<(), HandlerError> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(HandlerError::BadJson(Value::Object(errors)))
    }
}

/// Optional text input: wrong types fail, null fails on non-nullable
/// fields, over-long fails on max-length fields.
fn check_text(
    errors: &mut FieldErrors,
    body: &serde_json::Map<String, Value>,
    field: &str,
    max_length: Option<usize>,
) -> Option<String> {
    match body.get(field) {
        None => None,
        Some(Value::Null) => {
            push_error(errors, field, "This field may not be null.".to_owned());
            None
        }
        Some(Value::String(text)) => {
            if let Some(max) = max_length {
                if text.len() > max {
                    push_error(
                        errors,
                        field,
                        format!("Ensure this field has no more than {max} characters."),
                    );
                }
            }
            Some(text.clone())
        }
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            None
        }
    }
}

/// Required text input (the reaction name): missing, null, blank and
/// wrong-typed inputs each fail their own way.
fn check_required_text(
    errors: &mut FieldErrors,
    body: &serde_json::Map<String, Value>,
    field: &str,
) -> Option<String> {
    match body.get(field) {
        None => {
            push_error(errors, field, "This field is required.".to_owned());
            None
        }
        Some(Value::Null) => {
            push_error(errors, field, "This field may not be null.".to_owned());
            None
        }
        Some(Value::String(text)) => {
            if text.is_empty() {
                push_error(errors, field, "This field may not be blank.".to_owned());
                None
            } else {
                Some(text.clone())
            }
        }
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            None
        }
    }
}

/// Parse a UUID column carried over from the old row (merged updates).
fn parse_old_uuid(value: &Option<String>) -> Result<Option<uuid::Uuid>, HandlerError> {
    value
        .as_deref()
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| HandlerError::ServerError)
}

/// Optional datetime input (`edited_at`, nullable): absent leaves the
/// column alone, explicit null clears it, otherwise the text must parse.
fn check_nullable_datetime(
    errors: &mut FieldErrors,
    body: &serde_json::Map<String, Value>,
    field: &str,
) -> Option<Option<String>> {
    match body.get(field) {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(text)) => {
            if parse_input_datetime(text).is_none() {
                push_error(
                    errors,
                    field,
                    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].".to_owned(),
                );
                None
            } else {
                Some(Some(text.clone()))
            }
        }
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            None
        }
    }
}

/// Optional nullable text input (`external_source`, `external_id`):
/// absent leaves the column alone, explicit null clears it.
fn check_nullable_text(
    errors: &mut FieldErrors,
    body: &serde_json::Map<String, Value>,
    field: &str,
    max_length: usize,
) -> Option<Option<String>> {
    match body.get(field) {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(text)) => {
            if text.len() > max_length {
                push_error(
                    errors,
                    field,
                    format!("Ensure this field has no more than {max_length} characters."),
                );
                None
            } else {
                Some(Some(text.clone()))
            }
        }
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            None
        }
    }
}

/// Optional nullable UUID input (`speaker_agent_run_id`).
fn check_nullable_uuid(
    errors: &mut FieldErrors,
    body: &serde_json::Map<String, Value>,
    field: &str,
) -> Option<Option<uuid::Uuid>> {
    match body.get(field) {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(text)) => match uuid::Uuid::parse_str(text) {
            Ok(id) => Some(Some(id)),
            Err(_) => {
                push_error(errors, field, format!("\"{text}\" is not a valid UUID."));
                None
            }
        },
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            None
        }
    }
}

/// Optional choice input (`access`, `speaker_type`).
fn check_choice(
    errors: &mut FieldErrors,
    body: &serde_json::Map<String, Value>,
    field: &str,
    choices: &[&str],
) -> Option<String> {
    match body.get(field) {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => {
            if choices.contains(&text.as_str()) {
                Some(text.clone())
            } else {
                push_error(errors, field, format!("\"{text}\" is not a valid choice."));
                None
            }
        }
        _ => {
            push_error(errors, field, "Not a valid string.".to_owned());
            None
        }
    }
}

/// Parse an input datetime the way DRF `DateTimeField` does: offset-aware
/// RFC-3339, or a naive `YYYY-MM-DDThh:mm:ss` read in UTC.
fn parse_input_datetime(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S"))
        .ok()
        .map(|naive| naive.and_utc())
}

/// Optional string-array input (`attachments`, `labels`). Both columns are
/// non-nullable, so explicit null fails like DRF.
fn check_string_list(
    errors: &mut FieldErrors,
    body: &serde_json::Map<String, Value>,
    field: &str,
    max_items: usize,
    item_max_length: Option<usize>,
) -> Option<Vec<String>> {
    match body.get(field) {
        None => None,
        Some(Value::Null) => {
            push_error(errors, field, "This field may not be null.".to_owned());
            None
        }
        Some(Value::Array(items)) => {
            if items.len() > max_items {
                push_error(
                    errors,
                    field,
                    format!("Ensure this field has no more than {max_items} elements."),
                );
                return None;
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(text) => {
                        if let Some(max) = item_max_length {
                            if text.len() > max {
                                push_error(
                                    errors,
                                    field,
                                    format!("Ensure this field has no more than {max} characters."),
                                );
                                return None;
                            }
                        }
                        out.push(text.clone());
                    }
                    _ => {
                        push_error(errors, field, "Not a valid string.".to_owned());
                        return None;
                    }
                }
            }
            Some(out)
        }
        _ => {
            push_error(
                errors,
                field,
                "Expected a list of items but got a different type.".to_owned(),
            );
            None
        }
    }
}

/// Attachment URLs (`ArrayField(URLField())`): Django's `URLValidator`
/// needs a known scheme and a host; anything else fails.
fn check_attachments(
    errors: &mut FieldErrors,
    body: &serde_json::Map<String, Value>,
) -> Option<Vec<String>> {
    let items = check_string_list(errors, body, "attachments", 10, None)?;
    for item in &items {
        if !looks_like_url(item) {
            push_error(errors, "attachments", "Enter a valid URL.".to_owned());
            return None;
        }
    }
    Some(items)
}

fn looks_like_url(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https" | "ftp" | "ftps") {
        return false;
    }
    let host = rest.split('/').next().unwrap_or("");
    !host.is_empty()
}

/// A writable nullable FK input (`actor`, `parent`, `description`): absent
/// leaves the column alone, explicit null clears it, otherwise the value
/// must name a live row, like `PrimaryKeyRelatedField`.
async fn check_nullable_fk(
    pool: &sqlx::PgPool,
    errors: &mut FieldErrors,
    body: &serde_json::Map<String, Value>,
    field: &str,
    table: &str,
) -> Result<Option<Option<uuid::Uuid>>, HandlerError> {
    let Some(inner) = check_nullable_uuid(errors, body, field) else {
        // Absent, or invalid (errors non-empty, so the caller aborts).
        return Ok(None);
    };
    let Some(id) = inner else {
        return Ok(Some(None));
    };
    let sql = format!("SELECT id FROM {table} WHERE id = $1 AND deleted_at IS NULL");
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    if row.is_none() {
        push_error(
            errors,
            field,
            format!("Invalid pk \"{id}\" - object does not exist."),
        );
        return Ok(None);
    }
    Ok(Some(Some(id)))
}

/// Validated comment input shared by create (full) and partial_update
/// (subset: only provided keys validate, like `partial=True`).
/// Validated comment input. Nullable columns are three-state
/// (`Option<Option<T>>`): absent leaves the column alone, explicit null
/// clears it — the `partial=True` semantics for nullable fields.
struct CommentInput {
    comment_html: Option<String>,
    comment_json: Option<Value>,
    comment_stripped: Option<String>,
    attachments: Option<Vec<String>>,
    labels: Option<Vec<String>>,
    access: Option<String>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    speaker_type: Option<String>,
    speaker_label: Option<String>,
    speaker_agent_run_id: Option<Option<uuid::Uuid>>,
    edited_at: Option<Option<String>>,
    parent_id: Option<Option<uuid::Uuid>>,
    description_id: Option<Option<uuid::Uuid>>,
}

/// Validate one comment body. Read-only keys (`workspace`, `project`,
/// `issue`, audit stamps, `id`) are ignored like DRF ignores them.
async fn validate_comment(
    pool: &sqlx::PgPool,
    body: &serde_json::Map<String, Value>,
) -> Result<CommentInput, HandlerError> {
    let mut errors = FieldErrors::new();
    let comment_html = check_text(&mut errors, body, "comment_html", None);
    let comment_json = match body.get("comment_json") {
        None => None,
        Some(Value::Null) => {
            push_error(
                &mut errors,
                "comment_json",
                "This field may not be null.".to_owned(),
            );
            None
        }
        Some(value) => Some(value.clone()),
    };
    let comment_stripped = check_text(&mut errors, body, "comment_stripped", None);
    let attachments = check_attachments(&mut errors, body);
    let labels = check_string_list(&mut errors, body, "labels", 8, Some(32));
    let access = check_choice(&mut errors, body, "access", &["INTERNAL", "EXTERNAL"]);
    let external_source = check_nullable_text(&mut errors, body, "external_source", 255);
    let external_id = check_nullable_text(&mut errors, body, "external_id", 255);
    let speaker_type = check_choice(
        &mut errors,
        body,
        "speaker_type",
        &["human", "agent", "system", "integration"],
    );
    let speaker_label = check_text(&mut errors, body, "speaker_label", Some(128));
    let speaker_agent_run_id = check_nullable_uuid(&mut errors, body, "speaker_agent_run_id");
    let edited_at = check_nullable_datetime(&mut errors, body, "edited_at");
    let parent_id = check_nullable_fk(pool, &mut errors, body, "parent", "issue_comments").await?;
    // `actor` is writable but `save(actor=request.user)` overwrites it:
    // validated for existence, then discarded.
    check_nullable_fk(pool, &mut errors, body, "actor", "users").await?;
    let description_id =
        check_nullable_fk(pool, &mut errors, body, "description", "descriptions").await?;
    field_errors(errors)?;
    Ok(CommentInput {
        comment_html,
        comment_json,
        comment_stripped,
        attachments,
        labels,
        access,
        external_source,
        external_id,
        speaker_type,
        speaker_label,
        speaker_agent_run_id,
        edited_at,
        parent_id,
        description_id,
    })
}

// ---------------------------------------------------------------------------
// Comments: list / retrieve
// ---------------------------------------------------------------------------

/// `IssueCommentPublicViewSet.list` (`views/issue.py:228-255`): `AllowAny`.
/// Board miss or disabled board serves `[]` (the queryset's `.none()`
/// branches — no 404 here). `filterset_fields` (`:218`) scope exact
/// `issue__id` / `workspace__id` matches; anything else is ignored.
async fn comment_list(
    State(state): State<AppState>,
    Path((anchor, issue_id)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<QueryMap>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let issue = match uuid::Uuid::parse_str(&issue_id) {
        Ok(issue) => issue,
        Err(_) => {
            let req = Request::builder()
                .uri(format!(
                    "/api/public/anchor/{anchor}/issues/{issue_id}/comments/"
                ))
                .body(axum::body::Body::empty())
                .expect("proxy request");
            return proxy(&state, req).await;
        }
    };
    match comment_list_inner(&state, &anchor, &issue, &query, extension).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(error) => error.into_response(),
    }
}

async fn comment_list_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &uuid::Uuid,
    query: &QueryMap,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let actor = request_actor(state, extension).await?;
    let Some(board) = fetch_board(pool, anchor).await? else {
        return Ok("[]".to_owned());
    };
    if !board.is_comments_enabled {
        return Ok("[]".to_owned());
    }
    let (sql, params) = comment_list_query(
        &board,
        actor.as_ref().map(|actor| actor.id),
        issue_id,
        query,
    )?;
    let rows = fetch_all_objects(pool, &sql, &params).await?;
    let tz = request_tz(actor.as_ref());
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let rendered = render_comment(pool, &CommentRow::from_value(row)?, &tz, true).await?;
        out.push(serde_json::to_value(&rendered).map_err(|_| HandlerError::ServerError)?);
    }
    serde_json::to_string(&out).map_err(|_| HandlerError::ServerError)
}

/// Build the C1 list query (`queries::comment_list_sql`) with the
/// `filter_queryset` conjunct. `$1` workspace, `$2` project, `$3` the
/// actor (NULL anonymous — QUIRK-anonymous-member), `$4` the issue, then
/// one `$N` per exact filter.
fn comment_list_query<'a>(
    board: &'a BoardRow,
    actor_id: Option<uuid::Uuid>,
    issue_id: &'a uuid::Uuid,
    query: &QueryMap,
) -> Result<(String, Vec<SqlParam<'a>>), HandlerError> {
    let workspace_id =
        uuid::Uuid::parse_str(&board.workspace_id).map_err(|_| HandlerError::ServerError)?;
    let project_id = board
        .project_id
        .as_deref()
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| HandlerError::ServerError)?;
    // Anonymous binds NULL for the member probe (QUIRK-anonymous-member);
    // a NULL `$2` never matches under `=`, like the queries-layer text.
    let mut params = vec![
        SqlParam::Uuid(workspace_id),
        project_id.map(SqlParam::Uuid).unwrap_or(SqlParam::Null),
        actor_id.map(SqlParam::Uuid).unwrap_or(SqlParam::Null),
        SqlParam::Uuid(*issue_id),
    ];
    let mut predicates = Vec::new();
    if let Some(raw) = query_last(query, "issue__id") {
        let id = uuid::Uuid::parse_str(&raw).map_err(|_| {
            HandlerError::Guard(guards::handle_exception(
                guards::ExceptionKind::ValidationError,
            ))
        })?;
        params.push(SqlParam::Uuid(id));
        predicates.push(format!(
            "\"issue_comments\".\"issue_id\" = ${}",
            params.len()
        ));
    }
    if let Some(raw) = query_last(query, "workspace__id") {
        let id = uuid::Uuid::parse_str(&raw).map_err(|_| {
            HandlerError::Guard(guards::handle_exception(
                guards::ExceptionKind::ValidationError,
            ))
        })?;
        params.push(SqlParam::Uuid(id));
        predicates.push(format!(
            "\"issue_comments\".\"workspace_id\" = ${}",
            params.len()
        ));
    }
    let extra = if predicates.is_empty() {
        None
    } else {
        Some(predicates.join(" AND "))
    };
    Ok((queries::social::comment_list_sql(extra.as_deref()), params))
}

/// `IssueCommentPublicViewSet.retrieve`: the same queryset plus the pk
/// lookup; a miss is DRF's `Http404` detail body.
async fn comment_retrieve(
    State(state): State<AppState>,
    Path((anchor, issue_id, pk)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() || uuid::Uuid::parse_str(&pk).is_err() {
        return proxy(&state, req).await;
    }
    match comment_retrieve_inner(&state, &anchor, &issue_id, &pk, extension).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(error) => error.into_response(),
    }
}

async fn comment_retrieve_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &str,
    pk: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let actor = request_actor(state, extension).await?;
    let Some(board) = fetch_board(pool, anchor).await? else {
        return Err(HandlerError::NotFoundDetail);
    };
    if !board.is_comments_enabled {
        return Err(HandlerError::NotFoundDetail);
    }
    let issue = uuid_param(issue_id)?;
    let id = uuid_param(pk)?;
    let workspace = uuid_param(&board.workspace_id)?;
    let project = board
        .project_id
        .as_deref()
        .map(uuid_param)
        .transpose()?
        .unwrap_or(SqlParam::Null);
    let user = actor
        .as_ref()
        .map(|actor| SqlParam::Uuid(actor.id))
        .unwrap_or(SqlParam::Null);
    let extra = "\"issue_comments\".\"id\" = $5".to_owned();
    let rows = fetch_all_objects(
        pool,
        &queries::social::comment_list_sql(Some(&extra)),
        &[workspace, project, user, issue, id],
    )
    .await?;
    let row = rows.first().ok_or(HandlerError::NotFoundDetail)?;
    let tz = request_tz(actor.as_ref());
    let rendered = render_comment(pool, &CommentRow::from_value(row)?, &tz, true).await?;
    serde_json::to_string(&rendered).map_err(|_| HandlerError::ServerError)
}

// ---------------------------------------------------------------------------
// Comments: create / partial_update / destroy
// ---------------------------------------------------------------------------

/// `IssueCommentPublicViewSet.create` (`views/issue.py:257-294`):
/// the board must exist (bare `.get`) and allow comments; the response is
/// the validated output, always 201.
async fn comment_create(
    State(state): State<AppState>,
    Path((anchor, issue_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() {
        return proxy(&state, req).await;
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    match comment_create_inner(&state, &anchor, &issue_id, &body, extension).await {
        Ok(body) => json_response(StatusCode::CREATED, body),
        Err(error) => error.into_response(),
    }
}

async fn comment_create_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &str,
    body: &Value,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let board = fetch_board(pool, anchor)
        .await?
        .ok_or(HandlerError::NotFound)?;
    if !board.is_comments_enabled {
        return Err(HandlerError::Guard(guards::comments_not_enabled()));
    }
    let project_id = board
        .project_id
        .as_deref()
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| HandlerError::ServerError)?
        .ok_or_else(|| {
            HandlerError::Guard(guards::handle_exception(
                guards::ExceptionKind::IntegrityError,
            ))
        })?;
    let input = validate_comment(pool, body.as_object().ok_or(HandlerError::ServerError)?).await?;
    let workspace_id = project_workspace_id(pool, &project_id).await?;
    let now = chrono::Utc::now();
    let comment_id = uuid::Uuid::new_v4();
    // `IssueComment.save`: `comment_stripped` is recomputed from
    // `comment_html` (`db/models/issue.py:601`); absent html defaults to
    // `"<p></p>"` (model default).
    let html = input
        .comment_html
        .clone()
        .unwrap_or_else(|| "<p></p>".to_owned());
    let stripped = if html.is_empty() {
        String::new()
    } else {
        strip_tags(&html)
    };
    let comment_json = input
        .comment_json
        .clone()
        .unwrap_or_else(|| Value::Object(Default::default()));
    let attachments = input
        .attachments
        .clone()
        .map(|items| items.into_iter().map(Value::String).collect::<Vec<_>>())
        .map(Value::Array)
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let labels = input
        .labels
        .clone()
        .map(|items| items.into_iter().map(Value::String).collect::<Vec<_>>())
        .map(Value::Array)
        .unwrap_or_else(|| Value::Array(Vec::new()));
    insert_comment(
        pool,
        &comment_id,
        &now,
        &actor.id,
        &workspace_id,
        &project_id,
        issue_id,
        &stripped,
        &comment_json,
        &html,
        &attachments,
        &labels,
        &input,
    )
    .await?;
    // The `Description` mirror row (`IssueComment.save`, `:604-623`):
    // workspace/project follow the comment, `created_by` is the actor,
    // `updated_by` stays NULL on create.
    let description_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO descriptions (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, description_stripped, description_json, description_html, description_binary) VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, NULL)")
        .bind(description_id)
        .bind(now)
        .bind(actor.id)
        .bind(workspace_id)
        .bind(project_id)
        .bind(&stripped)
        .bind(&comment_json)
        .bind(&html)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    sqlx::query(
        "UPDATE issue_comments SET description_id = $1 WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(description_id)
    .bind(comment_id)
    .execute(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    let row = fetch_comment_by_id(pool, &comment_id)
        .await?
        .ok_or(HandlerError::ServerError)?;
    let tz = request_tz(Some(&actor));
    let rendered = render_comment(pool, &CommentRow::from_value(&row)?, &tz, false).await?;
    let value = serde_json::to_value(&rendered).map_err(|_| HandlerError::ServerError)?;
    // Python order: delay first (`:274-282`), member tracking after
    // (`:283-291`).
    let publish = pidash_jobs::space::comment_created(
        python_dumps(&value),
        actor.id.to_string(),
        issue_id.to_owned(),
        project_id.to_string(),
        now.timestamp(),
    );
    enqueue_message(pool, publish.message()).await;
    ensure_public_member(pool, &project_id, &workspace_id, &actor.id).await?;
    serde_json::to_string(&value).map_err(|_| HandlerError::ServerError)
}

#[allow(clippy::too_many_arguments)]
async fn insert_comment(
    pool: &sqlx::PgPool,
    comment_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
    actor_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    project_id: &uuid::Uuid,
    issue_id: &str,
    stripped: &str,
    comment_json: &Value,
    html: &str,
    attachments: &Value,
    labels: &Value,
    input: &CommentInput,
) -> Result<(), HandlerError> {
    let issue = uuid::Uuid::parse_str(issue_id).map_err(|_| HandlerError::ServerError)?;
    let edited_at: Option<chrono::DateTime<chrono::Utc>> = input
        .edited_at
        .clone()
        .flatten()
        .map(|text| parse_input_datetime(&text).ok_or(HandlerError::ServerError))
        .transpose()?;
    let result = sqlx::query("INSERT INTO issue_comments (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, comment_stripped, comment_json, comment_html, description_id, attachments, labels, issue_id, actor_id, access, external_source, external_id, speaker_type, speaker_label, speaker_agent_run_id, edited_at, parent_id) VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, NULL, $9, $10, $11, $3, 'EXTERNAL', $12, $13, $14, $15, $16, $17, $18)")
        .bind(comment_id)
        .bind(now)
        .bind(actor_id)
        .bind(workspace_id)
        .bind(project_id)
        .bind(stripped)
        .bind(comment_json)
        .bind(html)
        .bind(attachments)
        .bind(labels)
        .bind(issue)
        .bind(input.external_source.clone().flatten().as_deref())
        .bind(input.external_id.clone().flatten().as_deref())
        .bind(input.speaker_type.as_deref().unwrap_or("human"))
        .bind(input.speaker_label.as_deref().unwrap_or(""))
        .bind(input.speaker_agent_run_id.flatten())
        .bind(edited_at)
        .bind(input.parent_id.flatten())
        .execute(pool)
        .await
        .map_err(integrity_error)?;
    if result.rows_affected() != 1 {
        return Err(HandlerError::ServerError);
    }
    Ok(())
}

async fn fetch_comment_by_id(
    pool: &sqlx::PgPool,
    comment_id: &uuid::Uuid,
) -> Result<Option<Value>, HandlerError> {
    fetch_optional_object(
        pool,
        "SELECT \"issue_comments\".* FROM \"issue_comments\" WHERE (\"issue_comments\".\"deleted_at\" IS NULL AND \"issue_comments\".\"id\" = $1)",
        &[SqlParam::Uuid(*comment_id)],
    )
    .await
}

/// `IssueCommentPublicViewSet.partial_update` (`views/issue.py:296-318`):
/// actor-scoped get (a foreign comment is a 404, not a 403); the synced
/// lock rejects changed locked fields; the activity carries the raw input
/// dump plus the pre-save snapshot.
async fn comment_partial_update(
    State(state): State<AppState>,
    Path((anchor, issue_id, pk)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() || uuid::Uuid::parse_str(&pk).is_err() {
        return proxy(&state, req).await;
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    match comment_partial_update_inner(&state, &anchor, &issue_id, &pk, &body, extension).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(error) => error.into_response(),
    }
}

async fn comment_partial_update_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &str,
    pk: &str,
    body: &Value,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let board = fetch_board(pool, anchor)
        .await?
        .ok_or(HandlerError::NotFound)?;
    if !board.is_comments_enabled {
        return Err(HandlerError::Guard(guards::comments_not_enabled()));
    }
    let comment_id = uuid::Uuid::parse_str(pk).map_err(|_| HandlerError::ServerError)?;
    let old_row = fetch_optional_object(
        pool,
        "SELECT \"issue_comments\".* FROM \"issue_comments\" WHERE (\"issue_comments\".\"deleted_at\" IS NULL AND \"issue_comments\".\"id\" = $1 AND \"issue_comments\".\"actor_id\" = $2)",
        &[SqlParam::Uuid(comment_id), SqlParam::Uuid(actor.id)],
    )
    .await?
    .ok_or(HandlerError::NotFound)?;
    let old = CommentRow::from_value(&old_row)?;
    let input = validate_comment(pool, body.as_object().ok_or(HandlerError::ServerError)?).await?;
    // The synced lock (`IssueCommentSerializer.validate`, `:977-989`):
    // changed locked fields fail per field.
    if comment_is_synced(pool, old.external_source.as_deref(), &old.id).await? {
        let mut errors = FieldErrors::new();
        let locked = [
            (
                "comment_html",
                input
                    .comment_html
                    .as_ref()
                    .map(|text| Value::String(text.clone())),
            ),
            ("comment_json", input.comment_json.clone()),
            (
                "comment_stripped",
                input
                    .comment_stripped
                    .as_ref()
                    .map(|text| Value::String(text.clone())),
            ),
        ];
        for (field, new) in locked {
            if let Some(new) = new {
                let current = match field {
                    "comment_html" => old
                        .comment_html
                        .clone()
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                    "comment_json" => old.comment_json.clone(),
                    _ => old
                        .comment_stripped
                        .clone()
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                };
                if new != current {
                    push_error(
                        &mut errors,
                        field,
                        "This comment is synced from a Git provider and is read-only. Unbind the project's repository to edit.".to_owned(),
                    );
                }
            }
        }
        field_errors(errors)?;
    }
    // `save()` recomputes `comment_stripped` from `comment_html`, so an
    // explicit `comment_stripped` without new html is overwritten back.
    let html = input.comment_html.clone().or(old.comment_html.clone());
    let stripped = match &html {
        Some(text) if !text.is_empty() => strip_tags(text),
        _ => String::new(),
    };
    let now = chrono::Utc::now();
    apply_comment_update(
        pool,
        &comment_id,
        &actor.id,
        &now,
        &old,
        &input,
        &html,
        &stripped,
    )
    .await?;
    let row = fetch_comment_by_id(pool, &comment_id)
        .await?
        .ok_or(HandlerError::ServerError)?;
    let tz = request_tz(Some(&actor));
    let rendered = render_comment(pool, &CommentRow::from_value(&row)?, &tz, false).await?;
    let value = serde_json::to_value(&rendered).map_err(|_| HandlerError::ServerError)?;
    // `requested_data` is the raw input dump; `current_instance` is the
    // pre-save snapshot (the row was fetched at `:304`, before `:307`).
    let old_view = render_comment(pool, &old, &tz, false).await?;
    let old_value = serde_json::to_value(&old_view).map_err(|_| HandlerError::ServerError)?;
    let publish = pidash_jobs::space::comment_updated(
        python_dumps(body),
        actor.id.to_string(),
        issue_id.to_owned(),
        board_project_str(&board),
        python_dumps(&old_value),
        now.timestamp(),
    );
    enqueue_message(pool, publish.message()).await;
    serde_json::to_string(&value).map_err(|_| HandlerError::ServerError)
}

/// Write the provided columns plus the audit touch. Django's `save()`
/// writes every column; only provided values can differ, so updating the
/// provided subset plus `updated_at`/`updated_by` is the same row.
#[allow(clippy::too_many_arguments)]
async fn apply_comment_update(
    pool: &sqlx::PgPool,
    comment_id: &uuid::Uuid,
    actor_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
    old: &CommentRow,
    input: &CommentInput,
    html: &Option<String>,
    stripped: &str,
) -> Result<(), HandlerError> {
    // Absent keys keep the old column (Django writes the instance back
    // with only provided attributes changed); explicit nulls clear
    // nullable columns. Merging first makes the UPDATE a plain full-row
    // write like `save()`.
    let edited_at: Option<chrono::DateTime<chrono::Utc>> = match input.edited_at.clone() {
        None => old
            .edited_at
            .clone()
            .map(|text| parse_input_datetime(&text).ok_or(HandlerError::ServerError))
            .transpose()?,
        Some(inner) => inner
            .map(|text| parse_input_datetime(&text).ok_or(HandlerError::ServerError))
            .transpose()?,
    };
    let external_source = input
        .external_source
        .clone()
        .unwrap_or(old.external_source.clone());
    let external_id = input.external_id.clone().unwrap_or(old.external_id.clone());
    let speaker_agent_run_id = input
        .speaker_agent_run_id
        .unwrap_or(parse_old_uuid(&old.speaker_agent_run_id)?);
    let parent_id = input.parent_id.unwrap_or(parse_old_uuid(&old.parent_id)?);
    let description_id = input
        .description_id
        .unwrap_or(parse_old_uuid(&old.description_id)?);
    let access = input.access.clone().or(old.access.clone());
    let speaker_type = input.speaker_type.clone().or(old.speaker_type.clone());
    let speaker_label = input.speaker_label.clone().or(old.speaker_label.clone());
    let comment_json = input
        .comment_json
        .clone()
        .unwrap_or_else(|| old.comment_json.clone());
    let attachments = input
        .attachments
        .clone()
        .map(|items| Value::Array(items.into_iter().map(Value::String).collect()))
        .unwrap_or_else(|| old.attachments.clone());
    let labels = input
        .labels
        .clone()
        .map(|items| Value::Array(items.into_iter().map(Value::String).collect()))
        .unwrap_or_else(|| old.labels.clone());
    let html_changed = input.comment_html.is_some();
    let stripped_value = if html_changed {
        stripped.to_owned()
    } else {
        old.comment_stripped.clone().unwrap_or_default()
    };
    let old_html = old.comment_html.clone().unwrap_or_default();
    let new_html = html.clone().unwrap_or(old_html.clone());
    let html_value = html.clone().or(old.comment_html.clone());
    sqlx::query("UPDATE issue_comments SET updated_at = $1, updated_by_id = $2, comment_stripped = $3, comment_json = $4, comment_html = $5, attachments = $6, labels = $7, access = $8, external_source = $9, external_id = $10, speaker_type = $11, speaker_label = $12, speaker_agent_run_id = $13, edited_at = $14, parent_id = $15, description_id = $16 WHERE id = $17 AND deleted_at IS NULL")
        .bind(now)
        .bind(actor_id)
        .bind(&stripped_value)
        .bind(&comment_json)
        .bind(html_value)
        .bind(&attachments)
        .bind(&labels)
        .bind(access)
        .bind(external_source)
        .bind(external_id)
        .bind(speaker_type)
        .bind(speaker_label)
        .bind(speaker_agent_run_id)
        .bind(edited_at)
        .bind(parent_id)
        .bind(description_id)
        .bind(comment_id)
        .execute(pool)
        .await
        .map_err(integrity_error)?;
    // Mirror tracked-field changes into the description row (`save()`'s
    // change mapping, `:625-641`); untouched when the link is gone.
    let stripped_changed = stripped_value != old.comment_stripped.clone().unwrap_or_default();
    let html_changed = new_html != old_html;
    let json_changed = comment_json != old.comment_json;
    if (stripped_changed || html_changed || json_changed) && old.description_id.is_some() {
        let description_id = uuid::Uuid::parse_str(old.description_id.as_deref().unwrap_or(""))
            .map_err(|_| HandlerError::ServerError)?;
        sqlx::query("UPDATE descriptions SET description_html = $1, description_stripped = $2, description_json = $3, updated_by_id = $4, updated_at = $5 WHERE id = $6")
            .bind(&new_html)
            .bind(&stripped_value)
            .bind(&comment_json)
            .bind(actor_id)
            .bind(now)
            .bind(description_id)
            .execute(pool)
            .await
            .map_err(|_| HandlerError::ServerError)?;
    }
    Ok(())
}

/// `IssueCommentPublicViewSet.destroy` (`views/issue.py:320-339`): the
/// activity delays with the pre-delete snapshot *before* the soft delete.
async fn comment_destroy(
    State(state): State<AppState>,
    Path((anchor, issue_id, pk)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() || uuid::Uuid::parse_str(&pk).is_err() {
        return proxy(&state, req).await;
    }
    match comment_destroy_inner(&state, &anchor, &issue_id, &pk, extension).await {
        Ok(()) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(axum::body::Body::empty())
            .expect("empty 204"),
        Err(error) => error.into_response(),
    }
}

async fn comment_destroy_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &str,
    pk: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(), HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let board = fetch_board(pool, anchor)
        .await?
        .ok_or(HandlerError::NotFound)?;
    if !board.is_comments_enabled {
        return Err(HandlerError::Guard(guards::comments_not_enabled()));
    }
    let comment_id = uuid::Uuid::parse_str(pk).map_err(|_| HandlerError::ServerError)?;
    let old_row = fetch_optional_object(
        pool,
        "SELECT \"issue_comments\".* FROM \"issue_comments\" WHERE (\"issue_comments\".\"deleted_at\" IS NULL AND \"issue_comments\".\"id\" = $1 AND \"issue_comments\".\"actor_id\" = $2)",
        &[SqlParam::Uuid(comment_id), SqlParam::Uuid(actor.id)],
    )
    .await?
    .ok_or(HandlerError::NotFound)?;
    let old = CommentRow::from_value(&old_row)?;
    let tz = request_tz(Some(&actor));
    let old_view = render_comment(pool, &old, &tz, false).await?;
    let old_value = serde_json::to_value(&old_view).map_err(|_| HandlerError::ServerError)?;
    let now = chrono::Utc::now();
    let mut requested = serde_json::Map::new();
    requested.insert("comment_id".to_owned(), Value::String(pk.to_owned()));
    let publish = pidash_jobs::space::comment_deleted(
        python_dumps(&Value::Object(requested)),
        actor.id.to_string(),
        issue_id.to_owned(),
        board_project_str(&board),
        python_dumps(&old_value),
        now.timestamp(),
    );
    enqueue_message(pool, publish.message()).await;
    sqlx::query("UPDATE issue_comments SET deleted_at = $1 WHERE id = $2 AND deleted_at IS NULL")
        .bind(now)
        .bind(comment_id)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    enqueue_soft_delete(pool, "issuecomment", &comment_id).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Issue reactions
// ---------------------------------------------------------------------------

/// App `IssueReactionSerializer` (`app/serializers/issue.py:900-908`):
/// declared `id` + `actor_detail` first, then model definition order.
#[derive(Debug, Serialize)]
struct IssueReactionView {
    id: String,
    actor_detail: ActorDetailView,
    created_at: String,
    updated_at: String,
    created_by: Option<String>,
    updated_by: Option<String>,
    deleted_at: Option<String>,
    project: Option<String>,
    workspace: Option<String>,
    actor: String,
    issue: String,
    reaction: String,
}

async fn render_issue_reaction(
    pool: &sqlx::PgPool,
    row: &Value,
    tz: &chrono_tz::Tz,
) -> Result<IssueReactionView, HandlerError> {
    let o = obj(row)?;
    let actor_id = req_str(o, "actor_id")?;
    Ok(IssueReactionView {
        id: req_str(o, "id")?,
        actor_detail: render_actor_detail(pool, &actor_id).await?,
        created_at: render_dt(&req_str(o, "created_at")?, tz)?,
        updated_at: render_dt(&req_str(o, "updated_at")?, tz)?,
        created_by: opt_str(o, "created_by_id")?
            .or_else(|| opt_str(o, "created_by").unwrap_or(None)),
        updated_by: opt_str(o, "updated_by_id")?
            .or_else(|| opt_str(o, "updated_by").unwrap_or(None)),
        deleted_at: render_dt_opt(opt_str(o, "deleted_at")?, tz)?,
        project: opt_str(o, "project_id")?,
        workspace: opt_str(o, "workspace_id")?,
        actor: actor_id,
        issue: req_str(o, "issue_id")?,
        reaction: req_str(o, "reaction")?,
    })
}

/// `IssueReactionPublicViewSet.list`: the board lookup filters on kwargs
/// the routes never supply (BUG-reaction-list-dead), so the dead board get
/// always misses and the list serves `[]`. Auth still applies first.
async fn issue_reaction_list(
    State(state): State<AppState>,
    Path((anchor, issue_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() {
        return proxy(&state, req).await;
    }
    let _ = anchor;
    match issue_reaction_list_inner(&state, &issue_id, extension).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(error) => error.into_response(),
    }
}

async fn issue_reaction_list_inner(
    state: &AppState,
    issue_id: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let tz = request_tz(Some(&actor));
    let boards = fetch_all_objects(
        pool,
        &queries::social::issue_reaction_dead_board_get_sql(),
        &[],
    )
    .await?;
    let board = match boards.len() {
        0 => return Ok("[]".to_owned()),
        1 => &boards[0],
        _ => return Err(HandlerError::ServerError),
    };
    let _ = board;
    let rows = fetch_all_objects(
        pool,
        &queries::social::issue_reaction_dead_list_sql(),
        &[uuid_param(issue_id)?],
    )
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let rendered = render_issue_reaction(pool, row, &tz).await?;
        out.push(serde_json::to_value(&rendered).map_err(|_| HandlerError::ServerError)?);
    }
    serde_json::to_string(&out).map_err(|_| HandlerError::ServerError)
}

/// `IssueReactionPublicViewSet.create` (`views/issue.py:366-401`).
async fn issue_reaction_create(
    State(state): State<AppState>,
    Path((anchor, issue_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() {
        return proxy(&state, req).await;
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    match issue_reaction_create_inner(&state, &anchor, &issue_id, &body, extension).await {
        Ok(body) => json_response(StatusCode::CREATED, body),
        Err(error) => error.into_response(),
    }
}

async fn issue_reaction_create_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &str,
    body: &Value,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let board = fetch_board(pool, anchor)
        .await?
        .ok_or(HandlerError::NotFound)?;
    if !board.is_reactions_enabled {
        return Err(HandlerError::Guard(guards::issue_reactions_not_enabled()));
    }
    let project_id = board
        .project_id
        .as_deref()
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| HandlerError::ServerError)?
        .ok_or_else(|| {
            HandlerError::Guard(guards::handle_exception(
                guards::ExceptionKind::IntegrityError,
            ))
        })?;
    let mut errors = FieldErrors::new();
    let reaction = check_required_text(
        &mut errors,
        body.as_object().ok_or(HandlerError::ServerError)?,
        "reaction",
    );
    field_errors(errors)?;
    let reaction = reaction.ok_or(HandlerError::ServerError)?;
    let workspace_id = project_workspace_id(pool, &project_id).await?;
    let issue = uuid::Uuid::parse_str(issue_id).map_err(|_| HandlerError::ServerError)?;
    let now = chrono::Utc::now();
    let reaction_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO issue_reactions (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, actor_id, issue_id, reaction) VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $3, $6, $7)")
        .bind(reaction_id)
        .bind(now)
        .bind(actor.id)
        .bind(workspace_id)
        .bind(project_id)
        .bind(issue)
        .bind(&reaction)
        .execute(pool)
        .await
        .map_err(integrity_error)?;
    // Python order: member tracking (`:382-390`), then the delay
    // (`:391-399`).
    ensure_public_member(pool, &project_id, &workspace_id, &actor.id).await?;
    let row = fetch_optional_object(
        pool,
        "SELECT \"issue_reactions\".* FROM \"issue_reactions\" WHERE (\"issue_reactions\".\"deleted_at\" IS NULL AND \"issue_reactions\".\"id\" = $1)",
        &[SqlParam::Uuid(reaction_id)],
    )
    .await?
    .ok_or(HandlerError::ServerError)?;
    let tz = request_tz(Some(&actor));
    let rendered = render_issue_reaction(pool, &row, &tz).await?;
    let value = serde_json::to_value(&rendered).map_err(|_| HandlerError::ServerError)?;
    let publish = pidash_jobs::space::issue_reaction_created(
        python_dumps(body),
        actor.id.to_string(),
        issue_id.to_owned(),
        project_id.to_string(),
        now.timestamp(),
    );
    enqueue_message(pool, publish.message()).await;
    serde_json::to_string(&value).map_err(|_| HandlerError::ServerError)
}

/// `IssueReactionPublicViewSet.destroy` (`views/issue.py:403-427`):
/// scoped by workspace + issue + code + actor; the delay carries no
/// `requested_data`.
async fn issue_reaction_destroy(
    State(state): State<AppState>,
    Path((anchor, issue_id, reaction_code)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() {
        return proxy(&state, req).await;
    }
    match issue_reaction_destroy_inner(&state, &anchor, &issue_id, &reaction_code, extension).await
    {
        Ok(()) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(axum::body::Body::empty())
            .expect("empty 204"),
        Err(error) => error.into_response(),
    }
}

async fn issue_reaction_destroy_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &str,
    reaction_code: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(), HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let board = fetch_board(pool, anchor)
        .await?
        .ok_or(HandlerError::NotFound)?;
    if !board.is_reactions_enabled {
        return Err(HandlerError::Guard(guards::issue_reactions_not_enabled()));
    }
    let workspace_id =
        uuid::Uuid::parse_str(&board.workspace_id).map_err(|_| HandlerError::ServerError)?;
    let issue = uuid::Uuid::parse_str(issue_id).map_err(|_| HandlerError::ServerError)?;
    let row = fetch_optional_object(
        pool,
        "SELECT \"issue_reactions\".* FROM \"issue_reactions\" WHERE (\"issue_reactions\".\"deleted_at\" IS NULL AND \"issue_reactions\".\"workspace_id\" = $1 AND \"issue_reactions\".\"issue_id\" = $2 AND \"issue_reactions\".\"reaction\" = $3 AND \"issue_reactions\".\"actor_id\" = $4)",
        &[
            SqlParam::Uuid(workspace_id),
            SqlParam::Uuid(issue),
            SqlParam::Text(reaction_code),
            SqlParam::Uuid(actor.id),
        ],
    )
    .await?
    .ok_or(HandlerError::NotFound)?;
    let o = obj(&row)?;
    let reaction_id = req_str(o, "id")?;
    let now = chrono::Utc::now();
    let mut current = serde_json::Map::new();
    current.insert(
        "reaction".to_owned(),
        Value::String(reaction_code.to_owned()),
    );
    current.insert("identifier".to_owned(), Value::String(reaction_id.clone()));
    let publish = pidash_jobs::space::issue_reaction_deleted(
        actor.id.to_string(),
        issue_id.to_owned(),
        board_project_str(&board),
        python_dumps(&Value::Object(current)),
        now.timestamp(),
    );
    enqueue_message(pool, publish.message()).await;
    let id = uuid::Uuid::parse_str(&reaction_id).map_err(|_| HandlerError::ServerError)?;
    sqlx::query("UPDATE issue_reactions SET deleted_at = $1 WHERE id = $2 AND deleted_at IS NULL")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    enqueue_soft_delete(pool, "issuereaction", &id).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Comment reactions
// ---------------------------------------------------------------------------

/// `CommentReactionPublicViewSet.list` (`views/issue.py:434-449`): live
/// path — auth first, then board miss or disabled serves `[]`.
async fn comment_reaction_list(
    State(state): State<AppState>,
    Path((anchor, comment_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&comment_id).is_err() {
        return proxy(&state, req).await;
    }
    match comment_reaction_list_inner(&state, &anchor, &comment_id, extension).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(error) => error.into_response(),
    }
}

async fn comment_reaction_list_inner(
    state: &AppState,
    anchor: &str,
    comment_id: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let tz = request_tz(Some(&actor));
    let Some(board) = fetch_board(pool, anchor).await? else {
        return Ok("[]".to_owned());
    };
    if !board.is_reactions_enabled {
        return Ok("[]".to_owned());
    }
    let workspace_id =
        uuid::Uuid::parse_str(&board.workspace_id).map_err(|_| HandlerError::ServerError)?;
    let project_id = board
        .project_id
        .as_deref()
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| HandlerError::ServerError)?
        .ok_or(HandlerError::ServerError)?;
    let comment = uuid::Uuid::parse_str(comment_id).map_err(|_| HandlerError::ServerError)?;
    let rows = fetch_all_objects(
        pool,
        &queries::social::comment_reaction_list_sql(),
        &[
            SqlParam::Uuid(workspace_id),
            SqlParam::Uuid(project_id),
            SqlParam::Uuid(comment),
        ],
    )
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(render_comment_reaction_row(pool, row, &tz).await?);
    }
    serde_json::to_string(&out).map_err(|_| HandlerError::ServerError)
}

/// `CommentReactionPublicViewSet.create` (`views/issue.py:451-486`).
async fn comment_reaction_create(
    State(state): State<AppState>,
    Path((anchor, comment_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&comment_id).is_err() {
        return proxy(&state, req).await;
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    match comment_reaction_create_inner(&state, &anchor, &comment_id, &body, extension).await {
        Ok(body) => json_response(StatusCode::CREATED, body),
        Err(error) => error.into_response(),
    }
}

async fn comment_reaction_create_inner(
    state: &AppState,
    anchor: &str,
    comment_id: &str,
    body: &Value,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let board = fetch_board(pool, anchor)
        .await?
        .ok_or(HandlerError::NotFound)?;
    if !board.is_reactions_enabled {
        return Err(HandlerError::Guard(guards::comment_reactions_not_enabled()));
    }
    let project_id = board
        .project_id
        .as_deref()
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| HandlerError::ServerError)?
        .ok_or_else(|| {
            HandlerError::Guard(guards::handle_exception(
                guards::ExceptionKind::IntegrityError,
            ))
        })?;
    let mut errors = FieldErrors::new();
    let reaction = check_required_text(
        &mut errors,
        body.as_object().ok_or(HandlerError::ServerError)?,
        "reaction",
    );
    field_errors(errors)?;
    let reaction = reaction.ok_or(HandlerError::ServerError)?;
    let workspace_id = project_workspace_id(pool, &project_id).await?;
    let comment = uuid::Uuid::parse_str(comment_id).map_err(|_| HandlerError::ServerError)?;
    let now = chrono::Utc::now();
    let reaction_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO comment_reactions (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, actor_id, comment_id, reaction) VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $3, $6, $7)")
        .bind(reaction_id)
        .bind(now)
        .bind(actor.id)
        .bind(workspace_id)
        .bind(project_id)
        .bind(comment)
        .bind(&reaction)
        .execute(pool)
        .await
        .map_err(integrity_error)?;
    ensure_public_member(pool, &project_id, &workspace_id, &actor.id).await?;
    let row = fetch_optional_object(
        pool,
        "SELECT \"comment_reactions\".* FROM \"comment_reactions\" WHERE (\"comment_reactions\".\"deleted_at\" IS NULL AND \"comment_reactions\".\"id\" = $1)",
        &[SqlParam::Uuid(reaction_id)],
    )
    .await?
    .ok_or(HandlerError::ServerError)?;
    let tz = request_tz(Some(&actor));
    let rendered = render_comment_reaction_row(pool, &row, &tz).await?;
    let value = serde_json::to_value(&rendered).map_err(|_| HandlerError::ServerError)?;
    // BUG-comment-reaction-project: `project_id` is the literal `"None"`.
    let publish = pidash_jobs::space::comment_reaction_created(
        python_dumps(body),
        actor.id.to_string(),
        "None".to_owned(),
        now.timestamp(),
    );
    enqueue_message(pool, publish.message()).await;
    serde_json::to_string(&value).map_err(|_| HandlerError::ServerError)
}

/// `CommentReactionPublicViewSet.destroy` (`views/issue.py:488-519`).
async fn comment_reaction_destroy(
    State(state): State<AppState>,
    Path((anchor, comment_id, reaction_code)): Path<(String, String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&comment_id).is_err() {
        return proxy(&state, req).await;
    }
    match comment_reaction_destroy_inner(&state, &anchor, &comment_id, &reaction_code, extension)
        .await
    {
        Ok(()) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(axum::body::Body::empty())
            .expect("empty 204"),
        Err(error) => error.into_response(),
    }
}

async fn comment_reaction_destroy_inner(
    state: &AppState,
    anchor: &str,
    comment_id: &str,
    reaction_code: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(), HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let board = fetch_board(pool, anchor)
        .await?
        .ok_or(HandlerError::NotFound)?;
    if !board.is_reactions_enabled {
        return Err(HandlerError::Guard(guards::comment_reactions_not_enabled()));
    }
    // `filter(project_id=None)` renders `IS NULL` (project_meta rule).
    let project_id = board
        .project_id
        .as_deref()
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| HandlerError::ServerError)?;
    let workspace_id =
        uuid::Uuid::parse_str(&board.workspace_id).map_err(|_| HandlerError::ServerError)?;
    let comment = uuid::Uuid::parse_str(comment_id).map_err(|_| HandlerError::ServerError)?;
    let (sql, params) = match project_id {
        Some(project) => (
            "SELECT \"comment_reactions\".* FROM \"comment_reactions\" WHERE (\"comment_reactions\".\"deleted_at\" IS NULL AND \"comment_reactions\".\"project_id\" = $1 AND \"comment_reactions\".\"workspace_id\" = $2 AND \"comment_reactions\".\"comment_id\" = $3 AND \"comment_reactions\".\"reaction\" = $4 AND \"comment_reactions\".\"actor_id\" = $5)".to_owned(),
            vec![
                SqlParam::Uuid(project),
                SqlParam::Uuid(workspace_id),
                SqlParam::Uuid(comment),
                SqlParam::Text(reaction_code),
                SqlParam::Uuid(actor.id),
            ],
        ),
        None => (
            "SELECT \"comment_reactions\".* FROM \"comment_reactions\" WHERE (\"comment_reactions\".\"deleted_at\" IS NULL AND \"comment_reactions\".\"project_id\" IS NULL AND \"comment_reactions\".\"workspace_id\" = $1 AND \"comment_reactions\".\"comment_id\" = $2 AND \"comment_reactions\".\"reaction\" = $3 AND \"comment_reactions\".\"actor_id\" = $4)".to_owned(),
            vec![
                SqlParam::Uuid(workspace_id),
                SqlParam::Uuid(comment),
                SqlParam::Text(reaction_code),
                SqlParam::Uuid(actor.id),
            ],
        ),
    };
    let row = fetch_optional_object(pool, &sql, &params)
        .await?
        .ok_or(HandlerError::NotFound)?;
    let o = obj(&row)?;
    let reaction_id = req_str(o, "id")?;
    let now = chrono::Utc::now();
    let mut current = serde_json::Map::new();
    current.insert(
        "reaction".to_owned(),
        Value::String(reaction_code.to_owned()),
    );
    current.insert("identifier".to_owned(), Value::String(reaction_id.clone()));
    current.insert(
        "comment_id".to_owned(),
        Value::String(comment_id.to_owned()),
    );
    let publish = pidash_jobs::space::comment_reaction_deleted(
        actor.id.to_string(),
        board_project_str(&board),
        python_dumps(&Value::Object(current)),
        now.timestamp(),
    );
    enqueue_message(pool, publish.message()).await;
    let id = uuid::Uuid::parse_str(&reaction_id).map_err(|_| HandlerError::ServerError)?;
    sqlx::query(
        "UPDATE comment_reactions SET deleted_at = $1 WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    enqueue_soft_delete(pool, "commentreaction", &id).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Votes
// ---------------------------------------------------------------------------

/// App `IssueVoteSerializer` (`app/serializers/issue.py:939-946`), in
/// `Meta.fields` order.
#[derive(Debug, Serialize)]
struct IssueVoteView {
    issue: String,
    vote: i32,
    workspace: Option<String>,
    project: Option<String>,
    actor: String,
    actor_detail: ActorDetailView,
}

async fn render_issue_vote(
    pool: &sqlx::PgPool,
    issue_id: &str,
    vote: i32,
    workspace_id: Option<&str>,
    project_id: Option<&str>,
    actor_id: &str,
) -> Result<IssueVoteView, HandlerError> {
    Ok(IssueVoteView {
        issue: issue_id.to_owned(),
        vote,
        workspace: workspace_id.map(str::to_owned),
        project: project_id.map(str::to_owned),
        actor: actor_id.to_owned(),
        actor_detail: render_actor_detail(pool, actor_id).await?,
    })
}

/// `request.data.get("vote", 1)`: missing means `1`; numbers must fit an
/// `int4` like the column; anything else fails the way Postgres fails the
/// `save()` (500 envelope); explicit null violates the column (400).
fn parse_vote(body: &serde_json::Map<String, Value>) -> Result<i32, HandlerError> {
    match body.get("vote") {
        None => Ok(1),
        Some(Value::Null) => Err(HandlerError::Guard(guards::handle_exception(
            guards::ExceptionKind::IntegrityError,
        ))),
        Some(Value::Number(n)) => n
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or(HandlerError::ServerError),
        Some(Value::String(text)) => text
            .parse::<i64>()
            .ok()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or(HandlerError::ServerError),
        _ => Err(HandlerError::ServerError),
    }
}

/// `IssueVotePublicViewSet.list`: the board lookup passes the anchor as
/// the workspace slug (BUG-vote-anchor-as-slug). A miss serves `[]`; a
/// (pathological slug-collision) hit runs the would-be filter.
async fn vote_list(
    State(state): State<AppState>,
    Path((anchor, issue_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() {
        return proxy(&state, req).await;
    }
    match vote_list_inner(&state, &anchor, &issue_id, extension).await {
        Ok(body) => json_response(StatusCode::OK, body),
        Err(error) => error.into_response(),
    }
}

async fn vote_list_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    // Auth applies even though the dead list never needs the id.
    let _actor = require_actor(request_actor(state, extension).await?)?;
    let boards = fetch_all_objects(
        pool,
        &queries::social::vote_dead_board_get_sql(),
        &[SqlParam::Text(anchor)],
    )
    .await?;
    let board = match boards.len() {
        0 => return Ok("[]".to_owned()),
        1 => &boards[0],
        _ => return Err(HandlerError::ServerError),
    };
    let o = obj(board)?;
    let workspace_id = req_str(o, "workspace_id")?;
    let project_id = opt_str(o, "project_id")?;
    let issue = uuid_param(issue_id)?;
    let workspace = uuid_param(&workspace_id)?;
    let project = project_id
        .as_deref()
        .map(uuid_param)
        .transpose()?
        .unwrap_or(SqlParam::Null);
    let rows = fetch_all_objects(
        pool,
        &queries::social::vote_list_sql(),
        &[issue, workspace, project],
    )
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let r = obj(row)?;
        let vote_value = match r.get("vote") {
            Some(Value::Number(n)) => n.as_i64().and_then(|v| i32::try_from(v).ok()),
            _ => None,
        }
        .ok_or(HandlerError::ServerError)?;
        let rendered = render_issue_vote(
            pool,
            &req_str(r, "issue_id")?,
            vote_value,
            opt_str(r, "workspace_id")?.as_deref(),
            opt_str(r, "project_id")?.as_deref(),
            &req_str(r, "actor_id")?,
        )
        .await?;
        out.push(serde_json::to_value(&rendered).map_err(|_| HandlerError::ServerError)?);
    }
    serde_json::to_string(&out).map_err(|_| HandlerError::ServerError)
}

/// `IssueVotePublicViewSet.create` (`views/issue.py:543-571`):
/// `get_or_create` on `(actor, project, issue)`, then `vote` is assigned
/// unvalidated and the row answers 201 whether created or updated.
async fn vote_create(
    State(state): State<AppState>,
    Path((anchor, issue_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() {
        return proxy(&state, req).await;
    }
    let body = match read_body(req).await {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    match vote_create_inner(&state, &anchor, &issue_id, &body, extension).await {
        Ok(body) => json_response(StatusCode::CREATED, body),
        Err(error) => error.into_response(),
    }
}

async fn vote_create_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &str,
    body: &Value,
    extension: Option<Extension<SessionHandle>>,
) -> Result<String, HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    // QUIRK-vote-always-201: no `is_votes_enabled` check on this path
    // (`guards::vote_write_gated` pins the absence).
    let board = fetch_board(pool, anchor)
        .await?
        .ok_or(HandlerError::NotFound)?;
    let project_id = board
        .project_id
        .as_deref()
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| HandlerError::ServerError)?
        .ok_or_else(|| {
            HandlerError::Guard(guards::handle_exception(
                guards::ExceptionKind::IntegrityError,
            ))
        })?;
    let vote = parse_vote(body.as_object().ok_or(HandlerError::ServerError)?)?;
    let issue = uuid::Uuid::parse_str(issue_id).map_err(|_| HandlerError::ServerError)?;
    let now = chrono::Utc::now();
    let existing: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT id FROM issue_votes WHERE issue_id = $1 AND actor_id = $2 AND project_id = $3 AND deleted_at IS NULL LIMIT 1",
    )
    .bind(issue)
    .bind(actor.id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| HandlerError::ServerError)?;
    match existing {
        Some((id,)) => {
            sqlx::query("UPDATE issue_votes SET vote = $1, updated_at = $2, updated_by_id = $3 WHERE id = $4 AND deleted_at IS NULL")
                .bind(vote)
                .bind(now)
                .bind(actor.id)
                .bind(id)
                .execute(pool)
                .await
                .map_err(integrity_error)?;
            id
        }
        None => {
            let workspace_id = project_workspace_id(pool, &project_id).await?;
            let id = uuid::Uuid::new_v4();
            // `get_or_create` saves (actor/project/issue), then the
            // assignment saves again with `updated_by` set: one insert
            // with both audit columns is the same row.
            sqlx::query("INSERT INTO issue_votes (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, workspace_id, project_id, issue_id, actor_id, vote) VALUES ($1, $2, $2, $3, $3, NULL, $4, $5, $6, $3, $7)")
                .bind(id)
                .bind(now)
                .bind(actor.id)
                .bind(workspace_id)
                .bind(project_id)
                .bind(issue)
                .bind(vote)
                .execute(pool)
                .await
                .map_err(integrity_error)?;
            id
        }
    };
    // Python order: member tracking (`:551-555`), then assignment+save
    // (`:559-560`), then the delay (`:561-569`). The assignment is folded
    // into the writes above; the observable order is member-then-delay.
    let workspace_id = project_workspace_id(pool, &project_id).await?;
    ensure_public_member(pool, &project_id, &workspace_id, &actor.id).await?;
    let rendered = render_issue_vote(
        pool,
        issue_id,
        vote,
        Some(&workspace_id.to_string()),
        Some(&project_id.to_string()),
        &actor.id.to_string(),
    )
    .await?;
    let value = serde_json::to_value(&rendered).map_err(|_| HandlerError::ServerError)?;
    let publish = pidash_jobs::space::issue_vote_created(
        python_dumps(body),
        actor.id.to_string(),
        issue_id.to_owned(),
        project_id.to_string(),
        now.timestamp(),
    );
    enqueue_message(pool, publish.message()).await;
    serde_json::to_string(&value).map_err(|_| HandlerError::ServerError)
}

/// `IssueVotePublicViewSet.destroy` (`views/issue.py:573-591`).
async fn vote_destroy(
    State(state): State<AppState>,
    Path((anchor, issue_id)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    if uuid::Uuid::parse_str(&issue_id).is_err() {
        return proxy(&state, req).await;
    }
    match vote_destroy_inner(&state, &anchor, &issue_id, extension).await {
        Ok(()) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(axum::body::Body::empty())
            .expect("empty 204"),
        Err(error) => error.into_response(),
    }
}

async fn vote_destroy_inner(
    state: &AppState,
    anchor: &str,
    issue_id: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(), HandlerError> {
    let pool = pool_of(state)?;
    let actor = require_actor(request_actor(state, extension).await?)?;
    let board = fetch_board(pool, anchor)
        .await?
        .ok_or(HandlerError::NotFound)?;
    // `filter(project_id=None)` renders `IS NULL` (project_meta rule).
    let project_id = board
        .project_id
        .as_deref()
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| HandlerError::ServerError)?;
    let workspace_id =
        uuid::Uuid::parse_str(&board.workspace_id).map_err(|_| HandlerError::ServerError)?;
    let issue = uuid::Uuid::parse_str(issue_id).map_err(|_| HandlerError::ServerError)?;
    let (sql, params) = match project_id {
        Some(project) => (
            "SELECT \"issue_votes\".* FROM \"issue_votes\" WHERE (\"issue_votes\".\"deleted_at\" IS NULL AND \"issue_votes\".\"issue_id\" = $1 AND \"issue_votes\".\"actor_id\" = $2 AND \"issue_votes\".\"project_id\" = $3 AND \"issue_votes\".\"workspace_id\" = $4)".to_owned(),
            vec![
                SqlParam::Uuid(issue),
                SqlParam::Uuid(actor.id),
                SqlParam::Uuid(project),
                SqlParam::Uuid(workspace_id),
            ],
        ),
        None => (
            "SELECT \"issue_votes\".* FROM \"issue_votes\" WHERE (\"issue_votes\".\"deleted_at\" IS NULL AND \"issue_votes\".\"issue_id\" = $1 AND \"issue_votes\".\"actor_id\" = $2 AND \"issue_votes\".\"project_id\" IS NULL AND \"issue_votes\".\"workspace_id\" = $3)".to_owned(),
            vec![
                SqlParam::Uuid(issue),
                SqlParam::Uuid(actor.id),
                SqlParam::Uuid(workspace_id),
            ],
        ),
    };
    let row = fetch_optional_object(pool, &sql, &params)
        .await?
        .ok_or(HandlerError::NotFound)?;
    let o = obj(&row)?;
    let vote_id = req_str(o, "id")?;
    let vote_value = match o.get("vote") {
        Some(Value::Number(n)) => n.as_i64().ok_or(HandlerError::ServerError)?,
        _ => return Err(HandlerError::ServerError),
    };
    let now = chrono::Utc::now();
    // `current_instance` stringifies the vote (`str(issue_vote.vote)`).
    let mut current = serde_json::Map::new();
    current.insert("vote".to_owned(), Value::String(vote_value.to_string()));
    current.insert("identifier".to_owned(), Value::String(vote_id.clone()));
    let publish = pidash_jobs::space::issue_vote_deleted(
        actor.id.to_string(),
        issue_id.to_owned(),
        board_project_str(&board),
        python_dumps(&Value::Object(current)),
        now.timestamp(),
    );
    enqueue_message(pool, publish.message()).await;
    let id = uuid::Uuid::parse_str(&vote_id).map_err(|_| HandlerError::ServerError)?;
    sqlx::query("UPDATE issue_votes SET deleted_at = $1 WHERE id = $2 AND deleted_at IS NULL")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| HandlerError::ServerError)?;
    enqueue_soft_delete(pool, "issuevote", &id).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    fn actor_detail_fixture() -> ActorDetailView {
        ActorDetailView {
            id: "a1".to_owned(),
            first_name: "Ada".to_owned(),
            last_name: "L".to_owned(),
            avatar: None,
            avatar_url: None,
            is_bot: false,
            display_name: "Ada".to_owned(),
        }
    }

    fn object_keys(value: &Value) -> Vec<String> {
        value.as_object().expect("object").keys().cloned().collect()
    }

    #[test]
    fn error_bodies_match_python() {
        let (status, body) = HandlerError::Unauthorized.status_and_body();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        let (status, body) = HandlerError::NotFound.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, r#"{"error":"The required object does not exist."}"#);
        let (status, body) = HandlerError::NotFoundDetail.status_and_body();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, r#"{"detail":"Not found."}"#);
        let (status, body) = HandlerError::Guard(guards::comments_not_enabled()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            r#"{"error":"Comments are not enabled for this project"}"#
        );
        let (status, body) =
            HandlerError::Guard(guards::issue_reactions_not_enabled()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            r#"{"error":"Reactions are not enabled for this project board"}"#
        );
        let (status, body) =
            HandlerError::Guard(guards::comment_reactions_not_enabled()).status_and_body();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            r#"{"error":"Reactions are not enabled for this board"}"#
        );
        let (status, _) = HandlerError::ServerError.status_and_body();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn comment_key_order_is_declared_first_then_model_order() {
        let view = CommentView {
            id: "c".to_owned(),
            actor_detail: Some(actor_detail_fixture()),
            issue_detail: IssueFlatView {
                id: "i".to_owned(),
                name: "n".to_owned(),
                description_json: Value::Object(Default::default()),
                description_html: None,
                priority: "none".to_owned(),
                complexity_score: Value::Null,
                start_date: None,
                target_date: None,
                sequence_id: 1,
                sort_order: Value::Number(serde_json::Number::from_f64(65535.0).expect("f64")),
                is_draft: false,
            },
            project_detail: ProjectLiteView {
                id: "p".to_owned(),
                identifier: "CT1".to_owned(),
                name: "n".to_owned(),
                cover_image: None,
                cover_image_url: None,
                logo_props: Value::Object(Default::default()),
                description: None,
                is_default: false,
            },
            workspace_detail: WorkspaceLiteView {
                name: "w".to_owned(),
                slug: "s".to_owned(),
                id: "wid".to_owned(),
                logo_url: None,
            },
            comment_reactions: Vec::new(),
            is_member: Some(false),
            is_synced: false,
            created_at: "2026-09-28T00:00:00Z".to_owned(),
            updated_at: "2026-09-28T00:00:00Z".to_owned(),
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: Some("p".to_owned()),
            workspace: Some("w".to_owned()),
            comment_stripped: Some(String::new()),
            comment_json: Value::Object(Default::default()),
            comment_html: Some("<p></p>".to_owned()),
            description: None,
            attachments: Value::Array(Vec::new()),
            labels: Value::Array(Vec::new()),
            issue: Some("i".to_owned()),
            actor: Some("a1".to_owned()),
            access: Some("EXTERNAL".to_owned()),
            external_source: None,
            external_id: None,
            speaker_type: Some("human".to_owned()),
            speaker_label: Some(String::new()),
            speaker_agent_run_id: None,
            edited_at: None,
            parent: None,
        };
        let value = serde_json::to_value(&view).expect("serializes");
        assert_eq!(
            object_keys(&value),
            [
                "id",
                "actor_detail",
                "issue_detail",
                "project_detail",
                "workspace_detail",
                "comment_reactions",
                "is_member",
                "is_synced",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
                "deleted_at",
                "project",
                "workspace",
                "comment_stripped",
                "comment_json",
                "comment_html",
                "description",
                "attachments",
                "labels",
                "issue",
                "actor",
                "access",
                "external_source",
                "external_id",
                "speaker_type",
                "speaker_label",
                "speaker_agent_run_id",
                "edited_at",
                "parent",
            ]
            .map(str::to_owned)
            .to_vec()
        );
        // Create/update responses skip `is_member` (DRF `SkipField`).
        let mut plain = serde_json::to_value(&CommentView {
            is_member: None,
            ..view_like_without_member()
        })
        .expect("serializes");
        assert!(plain
            .as_object_mut()
            .expect("object")
            .remove("is_member")
            .is_none());
    }

    fn view_like_without_member() -> CommentView {
        CommentView {
            id: "c".to_owned(),
            actor_detail: None,
            issue_detail: IssueFlatView {
                id: "i".to_owned(),
                name: "n".to_owned(),
                description_json: Value::Object(Default::default()),
                description_html: None,
                priority: "none".to_owned(),
                complexity_score: Value::Null,
                start_date: None,
                target_date: None,
                sequence_id: 1,
                sort_order: Value::Number(serde_json::Number::from(1)),
                is_draft: false,
            },
            project_detail: ProjectLiteView {
                id: "p".to_owned(),
                identifier: "CT1".to_owned(),
                name: "n".to_owned(),
                cover_image: None,
                cover_image_url: None,
                logo_props: Value::Object(Default::default()),
                description: None,
                is_default: false,
            },
            workspace_detail: WorkspaceLiteView {
                name: "w".to_owned(),
                slug: "s".to_owned(),
                id: "wid".to_owned(),
                logo_url: None,
            },
            comment_reactions: Vec::new(),
            is_member: None,
            is_synced: false,
            created_at: "2026-09-28T00:00:00Z".to_owned(),
            updated_at: "2026-09-28T00:00:00Z".to_owned(),
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: None,
            workspace: None,
            comment_stripped: None,
            comment_json: Value::Null,
            comment_html: None,
            description: None,
            attachments: Value::Null,
            labels: Value::Null,
            issue: None,
            actor: None,
            access: None,
            external_source: None,
            external_id: None,
            speaker_type: None,
            speaker_label: None,
            speaker_agent_run_id: None,
            edited_at: None,
            parent: None,
        }
    }

    #[test]
    fn reaction_and_vote_key_orders_match_meta_lists() {
        let reaction = IssueReactionView {
            id: "r".to_owned(),
            actor_detail: actor_detail_fixture(),
            created_at: "2026-09-28T00:00:00Z".to_owned(),
            updated_at: "2026-09-28T00:00:00Z".to_owned(),
            created_by: None,
            updated_by: None,
            deleted_at: None,
            project: Some("p".to_owned()),
            workspace: Some("w".to_owned()),
            actor: "a1".to_owned(),
            issue: "i".to_owned(),
            reaction: "rocket".to_owned(),
        };
        assert_eq!(
            object_keys(&serde_json::to_value(&reaction).expect("serializes")),
            [
                "id",
                "actor_detail",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
                "deleted_at",
                "project",
                "workspace",
                "actor",
                "issue",
                "reaction",
            ]
            .map(str::to_owned)
            .to_vec()
        );
        let vote = IssueVoteView {
            issue: "i".to_owned(),
            vote: 1,
            workspace: Some("w".to_owned()),
            project: Some("p".to_owned()),
            actor: "a1".to_owned(),
            actor_detail: actor_detail_fixture(),
        };
        assert_eq!(
            object_keys(&serde_json::to_value(&vote).expect("serializes")),
            [
                "issue",
                "vote",
                "workspace",
                "project",
                "actor",
                "actor_detail"
            ]
            .map(str::to_owned)
            .to_vec()
        );
        let comment_reaction = CommentReactionView {
            id: "r".to_owned(),
            actor: "a1".to_owned(),
            comment: "c".to_owned(),
            reaction: "eyes".to_owned(),
            display_name: "Ada".to_owned(),
            deleted_at: None,
            workspace: Some("w".to_owned()),
            project: Some("p".to_owned()),
            created_at: "2026-09-28T00:00:00Z".to_owned(),
            updated_at: "2026-09-28T00:00:00Z".to_owned(),
            created_by: None,
            updated_by: None,
        };
        assert_eq!(
            object_keys(&serde_json::to_value(&comment_reaction).expect("serializes")),
            [
                "id",
                "actor",
                "comment",
                "reaction",
                "display_name",
                "deleted_at",
                "workspace",
                "project",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
            ]
            .map(str::to_owned)
            .to_vec()
        );
    }

    #[test]
    fn python_dumps_matches_cpython_separators() {
        let value = serde_json::json!({"a": 1, "b": "<p>hi</p>", "c": "héllo"});
        assert_eq!(
            python_dumps(&value),
            r#"{"a": 1, "b": "<p>hi</p>", "c": "h\u00e9llo"}"#
        );
        assert_eq!(
            python_dumps(&serde_json::json!([1, true, None::<()>])),
            "[1, true, null]"
        );
    }

    #[test]
    fn strip_tags_matches_django() {
        assert_eq!(strip_tags("<p>hi</p>"), "hi");
        assert_eq!(strip_tags(""), "");
        assert_eq!(strip_tags("<p></p>"), "");
    }

    #[test]
    fn parse_vote_matches_request_data_get() {
        let empty = serde_json::Map::new();
        assert!(matches!(parse_vote(&empty), Ok(1)));
        let body: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"vote": -1})).expect("map");
        assert!(matches!(parse_vote(&body), Ok(-1)));
        let body: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"vote": null})).expect("map");
        assert!(matches!(parse_vote(&body), Err(HandlerError::Guard(_))));
        let body: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"vote": true})).expect("map");
        assert!(matches!(parse_vote(&body), Err(HandlerError::ServerError)));
        let body: serde_json::Map<String, Value> =
            serde_json::from_value(serde_json::json!({"vote": 2147483648_u64})).expect("map");
        assert!(matches!(parse_vote(&body), Err(HandlerError::ServerError)));
    }

    #[test]
    fn parse_body_matches_drf() {
        assert!(matches!(parse_body(b""), Ok(Value::Object(_))));
        assert!(matches!(parse_body(b"[1]"), Err(HandlerError::ServerError)));
        assert!(matches!(
            parse_body(b"{oops"),
            Err(HandlerError::BadDetail(_))
        ));
    }

    #[test]
    fn comment_list_query_numbers_extra_binds() {
        let board = BoardRow {
            workspace_id: "3d914201-e7b6-449b-b92f-3ab0bd4f330d".to_owned(),
            project_id: Some("62d604a8-db82-44de-9e8c-a3e4a0f28371".to_owned()),
            is_comments_enabled: true,
            is_reactions_enabled: true,
        };
        let issue = uuid::Uuid::parse_str("62d604a8-db82-44de-9e8c-a3e4a0f28371").expect("uuid");
        let query: QueryMap = [
            ("issue__id".to_owned(), OneOrMany::One(issue.to_string())),
            ("unknown".to_owned(), OneOrMany::One("x".to_owned())),
        ]
        .into_iter()
        .collect();
        let (sql, params) = comment_list_query(&board, None, &issue, &query).expect("query");
        assert!(sql.contains("\"issue_comments\".\"issue_id\" = $5"));
        assert!(!sql.contains("unknown"));
        assert_eq!(params.len(), 5);
        assert_eq!(params[2], SqlParam::Null);
    }

    fn test_router() -> Router {
        crate::routes::with_routes(AppState::new("0.1.0"), routes())
    }

    async fn any_status(app: Router, method: &str, path: &str) -> StatusCode {
        let builder = match method {
            "POST" => axum::http::Request::post(path),
            "PATCH" => axum::http::Request::patch(path),
            "DELETE" => axum::http::Request::delete(path),
            _ => axum::http::Request::get(path),
        };
        let response = app
            .oneshot(builder.body(axum::body::Body::empty()).expect("request"))
            .await
            .expect("serve");
        response.status()
    }

    #[tokio::test]
    async fn owned_paths_reach_handlers_without_pools() {
        // No pools → every owned method runs its handler and answers the
        // 500 fallback (proving Rust owns the path); an unowned sibling
        // path proxies and fails closed with 502 (proving the cutover
        // boundary).
        let app = test_router();
        for (method, path) in [
            ("GET", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/comments/"),
            ("POST", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/comments/"),
            ("GET", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/comments/62d604a8-db82-44de-9e8c-a3e4a0f28371/"),
            ("PATCH", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/comments/62d604a8-db82-44de-9e8c-a3e4a0f28371/"),
            ("DELETE", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/comments/62d604a8-db82-44de-9e8c-a3e4a0f28371/"),
            ("GET", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/reactions/"),
            ("POST", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/reactions/"),
            ("DELETE", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/reactions/rocket/"),
            ("GET", "/api/public/anchor/abc/comments/62d604a8-db82-44de-9e8c-a3e4a0f28371/reactions/"),
            ("POST", "/api/public/anchor/abc/comments/62d604a8-db82-44de-9e8c-a3e4a0f28371/reactions/"),
            ("DELETE", "/api/public/anchor/abc/comments/62d604a8-db82-44de-9e8c-a3e4a0f28371/reactions/eyes/"),
            ("GET", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/votes/"),
            ("POST", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/votes/"),
            ("DELETE", "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/votes/"),
        ] {
            let status = any_status(app.clone(), method, path).await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{method} {path}");
        }
        // Sibling retrieve path stays on Django (owned by PIDASHCONV-175).
        let status = any_status(
            app,
            "GET",
            "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn non_owned_methods_proxy() {
        // PUT on an owned path is Django's; without an upstream the proxy
        // fails closed, never a Rust 405.
        let app = test_router();
        let response = app
            .oneshot(
                axum::http::Request::put(
                    "/api/public/anchor/abc/issues/62d604a8-db82-44de-9e8c-a3e4a0f28371/comments/",
                )
                .body(axum::body::Body::empty())
                .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}
