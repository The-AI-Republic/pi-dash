//! Relation + workpad handlers (D-18 handlers D, PIDASHCONV-676).
//!
//! Ports `apps/api/pi_dash/api/views/issue.py` (6 units):
//!
//! * `IssueRelationListCreateAPIEndpoint.get` (`:2965-3008`) — the grouped
//!   id-list read: ten `ArrayAgg` arms over the pair scope, `duplicate` /
//!   `relates_to` merged as direction unions. No `_source` check (a bogus
//!   issue answers 200 with empties), no pagination despite the documented
//!   params, no `project_id` predicate (ported bug REL-2).
//! * `IssueRelationListCreateAPIEndpoint.post` (`:3061-3136`) — create
//!   validation, the wire→stored mapping, `bulk_create` with
//!   `ignore_conflicts`, the activity fan-out, the pairs+type refetch, and
//!   the forward/reverse render split.
//! * `_IssueRelationAgentBase` (`:3139-3196`) — `_source` / `_visible` /
//!   `_write` shared by the three agent endpoints.
//! * `IssueRelationGroupedAPIEndpoint.get` (`:3207-3217`) —
//!   `orchestration.relations.grouped_relations` with the identifier.
//! * `IssueRelationRelateAPIEndpoint.post` (`:3232-3235`) and
//!   `IssueRelationUnrelateAPIEndpoint.post` (`:3250-3253`) — the
//!   idempotent `_write` funnels over `relations.relate` /
//!   `relations.unrelate`.
//! * `IssueWorkpadAPIEndpoint.get` (`:3275-3279`) / `.patch` (`:3281-3321`)
//!   — the agent workpad read plus the locked `body`-gated write that
//!   returns `{updated_at}` only.
//!
//! Registered by [`super::routes`] at the five
//! `apps/api/pi_dash/api/urls/work_item.py:194-216` paths
//! (`relations/`, `relations/grouped/`, `relations/relate/`,
//! `relations/unrelate/`, `workpad/`). These routes have NO deprecated
//! `issues/` twins (unlike links/comments): the twin block
//! (`work_item.py:39-98`) ends at attachments.
//!
//! Layering (all foundation use is read-only): validation and read shapes
//! in `pidash_services::v1_work_items::shape_relations` / `shape_labels`,
//! representative SQL in `queries_sub`, POST/activity kwargs in `tasks`,
//! gates in [`super::perms`] over the F-06 kernel
//! (`pidash_auth::permissions`), request bodies through
//! `crate::v1_cycles_modules::{body, json_cpython}`, the request shell
//! (preamble, project rewrite, membership facts) through
//! `crate::v1_projects::handlers_project` (the `handlers_pr_links`
//! precedent), task fan-out through `pidash_jobs::queue`. This module owns
//! the HTTP shell, the agent-vocabulary port
//! (`orchestration/relations.py`, which no other layer ports), the write
//! statements, and the read-shape rendering.
//!
//! Request order (preserved, not redesigned): UUID-segment shape (proxy
//! when Django's `<uuid:>` converter would not match — before auth, as URL
//! resolving precedes it), API-key authentication (anonymous 401s before
//! any pool or database access), the slug→UUID rewrite
//! (`api/views/base.py:51-98`, skipped for anonymous callers),
//! `check_permissions`, timezone activation (unknown zones 400 —
//! `ZoneInfoNotFoundError` subclasses `KeyError`), then the handler body.
//! Bodies parse only after the gate: the views touch `request.data`
//! inside the handler, never in `initial()`.
//!
//! Ported bugs (also listed in the PR):
//!
//! * REL-1 (`views/issue.py:2965-3008`, `queries_sub` ported bug 3):
//!   `duplicate` / `relates_to` merge with `list(set(...))`, whose order
//!   is CPython hash order, not input order. This port emits first-seen
//!   order via `queries_sub::union_ids` (the merged Q2 decision: only the
//!   membership is contractual).
//! * REL-2 (`views/issue.py:2979-2982`, `queries_sub` ported bug 2): the
//!   grouped read filters `workspace__slug` but NOT `project_id` —
//!   cross-project rows aggregate into the response.
//! * REL-3 (`orchestration/relations.py:82`): `validate_relation_type`
//!   calls `.strip()` on the raw value, so a non-string truthy
//!   `relation_type` (number, bool, list, dict) raises an uncaught
//!   `AttributeError` → 500. Falsy non-strings (`0`, `false`, `""`, `[]`,
//!   `{}`) answer the 400.
//! * REL-4 (`orchestration/relations.py:156-157`): `resolve_refs`
//!   `int(seq)` runs after `seq.isdigit()`, so a numeric-but-not-decimal
//!   sequence (`"²"`, `"½"`) raises an uncaught `ValueError` → 500.
//! * REL-5 (`db/models/base.py:23-45` + `db/models/issue.py:267-348`):
//!   every full `save()` re-stamps actors from the request user, so
//!   `relate()` rows land with `updated_by=NULL` (the adding branch
//!   nulls it, discarding the explicit actor), workpad PATCH stamps
//!   `updated_by` and recomputes `description_stripped` / `completed_at`
//!   (untouched when the issue had no state — the recompute is the `else`
//!   arm), and a workpad PATCH on a stateless issue ASSIGNS a default
//!   state.
//!
//! Deliberate edges (all unpinned — no fixture or contract case sends
//! them):
//!
//! * Multipart/file inputs are ignored (no file field exists here);
//!   Python would 400 them as non-strings. Same accepted edge as the
//!   reviewed sibling (`handlers_social.rs`, PIDASHCONV-674).
//! * `issues[0]`-style indexed form keys assemble via the shared body
//!   kernel; exotic `MultiValueDict` echo shapes are not reproduced.
//! * Non-ASCII numeric-but-not-decimal sequences answer 500 via the
//!   `is_numeric` approximation, which also catches `Nl` characters
//!   (e.g. `一`) that Python's `isdigit` rejects (those would 404 there).
//! * Refetch traversal of a concurrently hard-deleted issue renders the
//!   relation columns with the traversal keys omitted (`SkipField`
//!   parity); the FK makes this unreachable outside a race.
//!
//! Fixture: `F18-11` (`rust-api/fixtures/v1_work_items/handlers/` —
//! `relation_list`, `relation_grouped`, `relation_create`,
//! `relation_create_bad`, `relate`, `relate_unresolvable`, `unrelate`,
//! `workpad_get`, `workpad_patch`, `workpad_patch_no_body`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::extract::{OriginalUri, Path, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::scope::TenantScope;
use pidash_services::v1_work_items::queries_sub::{
    actual_relation, is_reverse_relation, union_ids,
};
use pidash_services::v1_work_items::shape_labels::{
    render_workpad, validate_workpad_write, WorkpadRow, WorkpadWriteInput,
};
use pidash_services::v1_work_items::shape_relations::{
    render_issue_relation, render_related_issue, render_relation_response,
    validate_relation_create, IssueRelationRow, IssueRelationShowInput, RelatedIssueRow,
    RelatedIssueShowInput, RelatedIssueType, RelationResponseGroups,
};
use pidash_services::v1_work_items::tasks as work_tasks;

use crate::state::AppState;
use crate::v1_projects::handlers_project::{preamble, project_base_facts, rewrite_project_id};

use super::perms::{decide, gate_for, V1WorkItemsRoute};

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:154-158`): every `.get()` miss on these endpoints
/// (project lookup, `_source`, workpad read/lock).
pub const RESOURCE_NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception`'s generic branch (`api/views/base.py:166-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `handle_exception`'s `IntegrityError` branch
/// (`api/views/base.py:142-147`): FK violations on the relation create.
pub const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// Workpad PATCH without the wire field (`views/issue.py:3287-3291`).
pub const WORKPAD_MISSING_BODY: &str =
    r#"{"error":"PATCH requires a `body` field in the request payload."}"#;
/// `_write` refs shape denial (`views/issue.py:3170-3174`).
pub const ISSUES_NONEMPTY_BODY: &str =
    r#"{"error":"issues must be a non-empty list of work item identifiers or UUIDs"}"#;
/// The related-objects sweep enqueued by `SoftDeleteModel.delete()`
/// (`db/mixins.py:72-78` over `bgtasks/deletion_task.py:18`).
pub const SOFT_DELETE_TASK: &str = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects";

/// Every relation type an agent may name, in display order
/// (`orchestration/relations.py:55-66`) — also the grouped-response key
/// order.
pub const RELATION_TYPES: &[&str] = &[
    "blocked_by",
    "blocking",
    "relates_to",
    "duplicate",
    "start_before",
    "start_after",
    "finish_before",
    "finish_after",
    "implemented_by",
    "implements",
];

/// Types stored with the ends swapped under their forward name
/// (`orchestration/relations.py:70`). Note the four members — the agent
/// vocabulary reverses `implements` too, unlike the endpoint POST's
/// three-member set (`views/issue.py:3077`).
pub const AGENT_REVERSE_TYPES: &[&str] = &["blocking", "start_after", "finish_after", "implements"];

/// Per-type cap on grouped lists (`orchestration/relations.py:74`).
pub const GROUP_LIMIT: usize = 100;

/// Handler failure with its exact status + body.
#[derive(Debug, PartialEq, Eq)]
pub enum Denial {
    /// 401, DRF `NotAuthenticated` (no `X-Api-Key` header).
    Unauthorized,
    /// 403, invalid/expired/inactive API or machine token.
    InvalidToken,
    /// 403, the DRF-default `PermissionDenied` body (no D-18 guard class
    /// sets `message`).
    Forbidden,
    /// 404, `{"Detail":"Project not found"}` (identifier rewrite miss —
    /// `Project.resolve` raises `Http404`, `db/models/project.py:213-217`).
    ProjectNotFound,
    /// 400, `{"detail": ...}` (DRF `ParseError`: malformed JSON).
    BadDetail(String),
    /// 400, `{"error": ...}` (view-inline: unknown timezones, agent
    /// relation-type errors).
    BadError(String),
    /// 400, serializer `errors` dict (pre-rendered bytes, field order).
    FieldErrors(String),
    /// 415, `{"detail": ...}` (DRF `UnsupportedMediaType`).
    UnsupportedMediaType(String),
    /// 404, view-inline body (unresolvable refs carry `unresolved` too).
    NotFound(String),
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
                r#"{"Detail":"Given API token is not valid"}"#.to_owned(),
            ),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::perms::CLASS_DENIAL_BODY.to_owned(),
            ),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"Detail":"Project not found"}"#.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"Detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::FieldErrors(body) => (StatusCode::BAD_REQUEST, body.clone()),
            Denial::UnsupportedMediaType(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::NotFound(body) => (StatusCode::NOT_FOUND, body.clone()),
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

/// Map the reused D-19 shell's denial onto this module's. Only messages
/// cross the boundary — every body re-renders here, so the D-19
/// capital-`Detail` spellings can never leak onto these routes.
impl From<crate::v1_projects::handlers_project::Denial> for Denial {
    fn from(denial: crate::v1_projects::handlers_project::Denial) -> Self {
        use crate::v1_projects::handlers_project::Denial as D;
        match denial {
            D::Unauthorized => Denial::Unauthorized,
            D::InvalidToken => Denial::InvalidToken,
            D::Forbidden => Denial::Forbidden,
            D::NotFound => Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()),
            D::ProjectNotFound => Denial::ProjectNotFound,
            D::BadDetail(message) => Denial::BadDetail(message),
            D::BadError(message) => Denial::BadError(message),
            // Unreachable from the reused preamble/rewrite/facts calls
            // (pure auth + lookup, no serializer runs); preserved rather
            // than remapped so no byte is ever invented here.
            D::FieldErrors(body) => Denial::FieldErrors(body),
            D::NotFoundError(message) => {
                Denial::NotFound(format!("{{\"error\":{}}}", json_string(&message)))
            }
            // Unreachable from the reused calls (no 409 arm there).
            D::Conflict(_) => Denial::ServerError,
            D::ServerError => Denial::ServerError,
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("json string")
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

/// Render a 201 JSON response with exact bytes.
fn json_created(body: String) -> Response {
    json_response(StatusCode::CREATED, body)
}

/// Map a database/driver failure to the generic 500 while logging the site
/// and error for operators (no secrets: messages never include tokens).
fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Denial {
    tracing::warn!(%error, site, "v1_work_items relations database failure");
    Denial::ServerError
}

/// `timezone.now()` truncated to microseconds: Python datetimes carry no
/// nanos, and `timestamptz` stores micros — an untruncated `Utc::now()`
/// would render nanos in the response while the DB row reads back micros.
fn now_utc() -> DateTime<Utc> {
    trunc_micros(Utc::now())
}

/// Truncate an instant to microsecond precision (see [`now_utc`]).
fn trunc_micros(dt: DateTime<Utc>) -> DateTime<Utc> {
    let nanos = dt.timestamp_subsec_nanos();
    dt - chrono::Duration::nanoseconds(i64::from(nanos % 1000))
}

/// Best-effort post-write task fan-out (the `.delay()` calls): without a
/// queue table the response still stands (the `v1_projects`
/// `enqueue_best_effort` precedent).
async fn enqueue_best_effort(
    pool: &PgPool,
    task: &str,
    args: Vec<Value>,
    kwargs: Map<String, Value>,
) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, args, kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task, "task enqueue failed; response stands");
    }
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

/// The relations path owns GET + POST (`urls/work_item.py:194-196`,
/// `as_view(http_method_names=["get", "post"])`).
pub fn owned_relation_list(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "POST"])
}

/// The grouped path owns GET (`urls/work_item.py:199-200`).
pub fn owned_relation_grouped(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET"])
}

/// The relate path owns POST (`urls/work_item.py:204-205`).
pub fn owned_relation_relate(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["POST"])
}

/// The unrelate path owns POST (`urls/work_item.py:209-210`).
pub fn owned_relation_unrelate(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["POST"])
}

/// The workpad path owns GET + PATCH (`urls/work_item.py:214-215`).
pub fn owned_workpad(
    router: axum::routing::MethodRouter<AppState>,
) -> axum::routing::MethodRouter<AppState> {
    owned(router, &["GET", "PATCH"])
}

// ---------------------------------------------------------------------------
// Gate + timezone
// ---------------------------------------------------------------------------

/// Activate the actor's rendering timezone (`TimezoneMixin.initial` runs
/// after `super().initial()`). A missing zone defaults to UTC; an unknown
/// zone name 400s: `zoneinfo.ZoneInfo` raises `ZoneInfoNotFoundError`,
/// which subclasses `KeyError`, so `handle_exception` answers the
/// `KeyError` branch (`api/views/base.py:160-164`). An EMPTY zone 500s:
/// `ZoneInfo('')` raises `ValueError` (not `KeyError`), which falls
/// through to the generic 500 (`api/views/base.py:166-171`).
fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Denial> {
    match timezone {
        None => Ok(chrono_tz::UTC),
        Some("") => Err(Denial::ServerError),
        Some(zone) => zone
            .parse()
            .map_err(|_| Denial::BadError("The required key does not exist.".to_owned())),
    }
}

/// Run the route's gate; deny 403 on failure. All five routes carry
/// `ProjectEntityPermission` (see [`gate_for`]).
async fn require_entity_gate(
    pool: &PgPool,
    workspace_id: &Uuid,
    workspace_slug: &str,
    user_id: &Uuid,
    project_id: &Uuid,
    route: V1WorkItemsRoute,
    method: &str,
) -> Result<(), Denial> {
    let facts = project_base_facts(pool, workspace_id, workspace_slug, user_id, project_id).await?;
    let scope = TenantScope::new(pidash_types::WorkspaceId::from(workspace_slug.to_owned()));
    let gate = gate_for(route, method);
    if decide(gate, method, &scope, &facts) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

/// The write body spec: `issues` arrives as an array (DRF
/// `ListField.get_value` → `getlist` on the create path; the `_write`
/// paths read `.get`, i.e. the last element — hence the scalar spec
/// below). No blank-skipping: a present-empty form value stays `""` and
/// each endpoint's validation applies the `get_value` rule
/// (`fields.py:407-429`) for its own fields.
const WRITE_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &["issues"],
        skip_blank_fields: &[],
    };

/// The agent/workpad body spec: NO list fields — `_write` reads
/// `request.data.get` (last-wins, not `getlist`) and the workpad takes a
/// scalar, so repeated form keys collapse to their last value exactly
/// like Python.
const AGENT_BODY_SPEC: crate::v1_cycles_modules::body::BodySpec =
    crate::v1_cycles_modules::body::BodySpec {
        list_fields: &[],
        skip_blank_fields: &[],
    };

/// A parsed write body: the JSON value plus whether it arrived as an HTML
/// form (JSON-string fields and blank arms differ per
/// `JSONField.get_value` / `Field.get_value`).
struct WriteBody {
    value: Value,
    from_form: bool,
}

/// Parse a relation-create body: content-type dispatch (415 for the
/// rest), empty bodies to `{}`, JSON through the CPython parser, forms
/// through the HTML-input kernel. Uploads are ignored (no file field
/// exists here — an unpinned edge; Python would 400 them as non-strings).
fn parse_write_body(headers: &HeaderMap, body: &[u8]) -> Result<WriteBody, Denial> {
    parse_body_with_spec(headers, body, &WRITE_BODY_SPEC)
}

/// Parse an agent/workpad body (scalar spec — see [`AGENT_BODY_SPEC`]).
fn parse_agent_body(headers: &HeaderMap, body: &[u8]) -> Result<WriteBody, Denial> {
    parse_body_with_spec(headers, body, &AGENT_BODY_SPEC)
}

fn parse_body_with_spec(
    headers: &HeaderMap,
    body: &[u8],
    spec: &crate::v1_cycles_modules::body::BodySpec,
) -> Result<WriteBody, Denial> {
    use crate::v1_cycles_modules::body::{negotiate_body, BodyError, NegotiatedBody};
    use crate::v1_cycles_modules::json_cpython::{
        parse_json_text_spans, to_serde_publish, to_serde_publish_map, JsonFail,
    };
    match negotiate_body(headers, body, spec) {
        Err(BodyError::UnsupportedMediaType(detail)) => Err(Denial::UnsupportedMediaType(detail)),
        Err(BodyError::ParseDetail(detail)) => Err(Denial::BadDetail(detail)),
        Err(BodyError::ServerError) => Err(Denial::ServerError),
        Ok(NegotiatedBody::Empty) => Ok(WriteBody {
            value: Value::Object(Map::new()),
            from_form: false,
        }),
        Ok(NegotiatedBody::Form { map, .. }) => Ok(WriteBody {
            value: Value::Object(map),
            from_form: true,
        }),
        Ok(NegotiatedBody::JsonText { text, surr }) => match parse_json_text_spans(&text, &surr) {
            Err(JsonFail::Message(detail)) => {
                Err(Denial::BadDetail(format!("JSON parse error - {detail}")))
            }
            Err(JsonFail::Recursion) => Err(Denial::ServerError),
            Ok(value) => {
                if value.is_object() {
                    let object = value.into_object().expect("checked object");
                    Ok(WriteBody {
                        value: Value::Object(to_serde_publish_map(&object)),
                        from_form: false,
                    })
                } else {
                    Ok(WriteBody {
                        value: to_serde_publish(&value),
                        from_form: false,
                    })
                }
            }
        },
    }
}

/// The activity `requested_data` text for a parsed write body
/// (PIDASHCONV-763): form bodies dump list fields as `QueryDict`
/// last-wins scalars, while validation keeps the `getlist` arrays. JSON
/// bodies dump as-is (arrays are real data there) without a copy.
fn requested_data_text(parsed: &WriteBody) -> String {
    if parsed.from_form {
        let mut projected = parsed.value.clone();
        if let Value::Object(map) = &mut projected {
            crate::v1_cycles_modules::body::project_list_scalars(map, WRITE_BODY_SPEC.list_fields);
        }
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&projected)
    } else {
        pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps(&parsed.value)
    }
}

/// `base_host(request, is_app=True)` (`utils/host.py:17-60`): the app base
/// URL when set, else the web origin; a missing pair raises
/// `ImproperlyConfigured` into the 500.
fn app_origin(urls: &pidash_db::config::UrlSettings) -> Result<String, Denial> {
    if let Some(url) = urls.app_base_url.as_deref().filter(|s| !s.is_empty()) {
        return Ok(url.to_owned());
    }
    if let Some(url) = urls.web_url.as_deref().filter(|s| !s.is_empty()) {
        return Ok(url.to_owned());
    }
    Err(Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

fn row_uuid(row: &sqlx::postgres::PgRow, column: &str) -> Result<Uuid, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_uuid_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<Uuid>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_string(row: &sqlx::postgres::PgRow, column: &str) -> Result<String, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_string_opt(row: &sqlx::postgres::PgRow, column: &str) -> Result<Option<String>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

fn row_datetime(row: &sqlx::postgres::PgRow, column: &str) -> Result<DateTime<Utc>, Denial> {
    row.try_get(column).map_err(|_| Denial::ServerError)
}

/// The ten grouped-read arms (`views/issue.py:2984-2995`), in source order.
struct RelationAggregate {
    blocking: Vec<Uuid>,
    blocked_by: Vec<Uuid>,
    duplicate: Vec<Uuid>,
    duplicate_related: Vec<Uuid>,
    relates_to: Vec<Uuid>,
    relates_to_related: Vec<Uuid>,
    start_after: Vec<Uuid>,
    start_before: Vec<Uuid>,
    finish_after: Vec<Uuid>,
    finish_before: Vec<Uuid>,
}

/// The grouped aggregate (`:2971-2995`, fixture `relation_grouped_aggregation`):
/// one `SELECT` with the ten `_agg_ids` arms over the pair scope (live
/// rows mentioning the issue on either side, in this workspace — NO
/// `project_id` predicate, REL-2). Array order is Postgres's, preserved
/// as-is like Python (only the two unions reorder — first-seen).
async fn fetch_relation_aggregate(
    pool: &PgPool,
    slug: &str,
    issue_id: &Uuid,
) -> Result<RelationAggregate, Denial> {
    let row = sqlx::query(
        r#"SELECT
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."issue_id") FILTER (WHERE "issue_relations"."related_issue_id" = $1 AND "issue_relations"."relation_type" = 'blocked_by'), '{}'::uuid[]) AS "blocking_ids",
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."related_issue_id") FILTER (WHERE "issue_relations"."issue_id" = $1 AND "issue_relations"."relation_type" = 'blocked_by'), '{}'::uuid[]) AS "blocked_by_ids",
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."related_issue_id") FILTER (WHERE "issue_relations"."issue_id" = $1 AND "issue_relations"."relation_type" = 'duplicate'), '{}'::uuid[]) AS "duplicate_ids",
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."issue_id") FILTER (WHERE "issue_relations"."related_issue_id" = $1 AND "issue_relations"."relation_type" = 'duplicate'), '{}'::uuid[]) AS "duplicate_ids_related",
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."related_issue_id") FILTER (WHERE "issue_relations"."issue_id" = $1 AND "issue_relations"."relation_type" = 'relates_to'), '{}'::uuid[]) AS "relates_to_ids",
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."issue_id") FILTER (WHERE "issue_relations"."related_issue_id" = $1 AND "issue_relations"."relation_type" = 'relates_to'), '{}'::uuid[]) AS "relates_to_ids_related",
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."issue_id") FILTER (WHERE "issue_relations"."related_issue_id" = $1 AND "issue_relations"."relation_type" = 'start_before'), '{}'::uuid[]) AS "start_after_ids",
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."related_issue_id") FILTER (WHERE "issue_relations"."issue_id" = $1 AND "issue_relations"."relation_type" = 'start_before'), '{}'::uuid[]) AS "start_before_ids",
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."issue_id") FILTER (WHERE "issue_relations"."related_issue_id" = $1 AND "issue_relations"."relation_type" = 'finish_before'), '{}'::uuid[]) AS "finish_after_ids",
             COALESCE(ARRAY_AGG(DISTINCT "issue_relations"."related_issue_id") FILTER (WHERE "issue_relations"."issue_id" = $1 AND "issue_relations"."relation_type" = 'finish_before'), '{}'::uuid[]) AS "finish_before_ids"
           FROM "issue_relations" INNER JOIN "workspaces" ON ("issue_relations"."workspace_id" = "workspaces"."id")
           WHERE ("issue_relations"."deleted_at" IS NULL
             AND ("issue_relations"."issue_id" = $1 OR "issue_relations"."related_issue_id" = $1)
             AND "workspaces"."slug" = $2)"#,
    )
    .bind(issue_id)
    .bind(slug)
    .fetch_one(pool)
    .await
    .map_err(|error| db_error(error, "relation-aggregate"))?;
    let arm = |column: &str| -> Result<Vec<Uuid>, Denial> {
        row.try_get(column).map_err(|_| Denial::ServerError)
    };
    Ok(RelationAggregate {
        blocking: arm("blocking_ids")?,
        blocked_by: arm("blocked_by_ids")?,
        duplicate: arm("duplicate_ids")?,
        duplicate_related: arm("duplicate_ids_related")?,
        relates_to: arm("relates_to_ids")?,
        relates_to_related: arm("relates_to_ids_related")?,
        start_after: arm("start_after_ids")?,
        start_before: arm("start_before_ids")?,
        finish_after: arm("finish_after_ids")?,
        finish_before: arm("finish_before_ids")?,
    })
}

/// `Project.objects.get(pk=project_id, workspace__slug=slug)`
/// (`views/issue.py:3074`): the relation POST's workspace id source. A
/// miss answers the `ObjectDoesNotExist` 404 (unreachable past the gate —
/// membership implies the project — but kept for faithfulness).
async fn fetch_project_workspace(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
) -> Result<Uuid, Denial> {
    let workspace_id: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT "projects"."workspace_id" FROM "projects"
           INNER JOIN "workspaces" ON ("projects"."workspace_id" = "workspaces"."id")
           WHERE "projects"."id" = $1 AND "workspaces"."slug" = $2"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "relation-post-project"))?
    .flatten();
    workspace_id.ok_or_else(|| Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()))
}

/// Whether a sqlx failure is a foreign-key violation (`IntegrityError` →
/// `{"error": "The payload is not valid"}`).
fn is_fk_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.code().as_deref() == Some("23503"),
        _ => false,
    }
}

/// Whether a sqlx failure is a unique violation (the relate race:
/// `IntegrityError` → re-read the pair).
fn is_unique_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db) => db.code().as_deref() == Some("23505"),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Agent vocabulary (`orchestration/relations.py`, PDASHOSS01-199)
// ---------------------------------------------------------------------------

/// `validate_relation_type` (`relations.py:81-85`): strip + lowercase, then
/// the ten names. A non-string truthy value raises `AttributeError`
/// (REL-3, uncaught → 500); falsy values (`None`, `0`, `false`, `""`,
/// `[]`, `{}`) take the `""` arm → the 400. `None` (missing) also 400s.
fn validate_relation_type(raw: Option<&Value>) -> Result<String, Denial> {
    let text = match raw {
        None => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) if number.as_f64() == Some(0.0) => String::new(),
        Some(Value::Bool(false)) | Some(Value::Null) => String::new(),
        Some(Value::Array(items)) if items.is_empty() => String::new(),
        Some(Value::Object(map)) if map.is_empty() => String::new(),
        // REL-3: `(relation_type or "")` keeps truthy non-strings, and
        // `.strip()` on them raises `AttributeError` → generic 500.
        Some(_) => return Err(Denial::ServerError),
    };
    // Python `str.strip()` trims ASCII whitespace plus Unicode spaces;
    // `char::is_whitespace` matches it for this lowercase alpha vocabulary
    // (unpinned either way).
    let normalized: String = text
        .trim_matches(|c: char| c.is_whitespace())
        .chars()
        .flat_map(|c| c.to_lowercase())
        .collect();
    if RELATION_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(Denial::BadError(format!(
            "relation_type must be one of: {}",
            RELATION_TYPES.join(", ")
        )))
    }
}

/// CPython `uuid.UUID(hex-string)` acceptance (`resolve_refs` tries it
/// before the identifier form): optional case-sensitive `urn:` + `uuid:`
/// prefixes, any number of surrounding braces, hyphens in ANY positions,
/// then exactly 32 hex digits where `int(hex, 16)` also tolerates
/// single underscores between digits. Returns the normalized lowercase
/// hyphenated id. Anything else is `None` (→ the `PROJ-123` attempt).
fn parse_python_uuid(text: &str) -> Option<Uuid> {
    let mut hex = text;
    hex = hex.strip_prefix("urn:").unwrap_or(hex);
    hex = hex.strip_prefix("uuid:").unwrap_or(hex);
    hex = hex.trim_matches(|c| c == '{' || c == '}');
    if hex.is_empty() {
        return None;
    }
    let compact: String = hex.chars().filter(|c| *c != '-').collect();
    // `int(hex, 16)` grammar: hex digits with single `_` separators
    // (never leading, trailing, or doubled), 32 digits total.
    if compact.is_empty() || compact.starts_with('_') || compact.ends_with('_') {
        return None;
    }
    let mut digits = String::with_capacity(32);
    let mut previous_underscore = false;
    for c in compact.chars() {
        if c == '_' {
            if previous_underscore {
                return None;
            }
            previous_underscore = true;
            continue;
        }
        previous_underscore = false;
        if !c.is_ascii_hexdigit() {
            return None;
        }
        digits.push(c);
    }
    if digits.len() != 32 {
        return None;
    }
    Uuid::parse_str(&digits).ok()
}

/// One resolved reference: the visible-pool row plus its identifier facts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedIssue {
    id: Uuid,
    workspace_id: Uuid,
    project_identifier: String,
    sequence_id: i32,
}

impl ResolvedIssue {
    /// `identifier(issue)` (`relations.py:88-89`).
    fn identifier(&self) -> String {
        format!("{}-{}", self.project_identifier, self.sequence_id)
    }
}

/// The shared visible-pool predicates: `member_project_issues(user, slug)`
/// (`core/querysets.py:19-30`) — the `IssueManager` scope plus workspace
/// slug plus an active project membership of the caller. The membership
/// join carries NO `deleted_at` guard (fixture-verbatim
/// `member_guard_sql`).
const VISIBLE_POOL_JOINS: &str = r#"FROM "issues"
           LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
           INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
           INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id")"#;

const VISIBLE_POOL_WHERE: &str = r#""issues"."deleted_at" IS NULL
             AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)
             AND NOT ("issues"."archived_at" IS NOT NULL)
             AND NOT ("projects"."archived_at" IS NOT NULL)
             AND NOT ("issues"."is_draft")
             AND "workspaces"."slug" = $1
             AND EXISTS(SELECT 1 FROM "project_members"
                        WHERE "project_members"."project_id" = "issues"."project_id"
                          AND "project_members"."member_id" = $2
                          AND "project_members"."is_active")"#;

/// `resolve_refs(refs, visible)` (`relations.py:138-163`): each raw ref is
/// `str(raw or "").strip()`, tried as a UUID first (flexible parse),
/// then as `PROJ-123` (`project__identifier__iexact` + `sequence_id`),
/// each against the caller's visible pool (invisible ≡ nonexistent).
/// Returns `(found, unresolved)` in request order; duplicates resolve
/// repeatedly (deduped later by `_check_targets`).
async fn resolve_refs(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    refs: &[Value],
) -> Result<(Vec<ResolvedIssue>, Vec<String>), Denial> {
    let mut found = Vec::with_capacity(refs.len());
    let mut unresolved = Vec::new();
    for raw in refs {
        let text = python_ref_text(raw);
        let stripped = text.trim_matches(|c: char| c.is_whitespace()).to_owned();
        let mut matched: Option<ResolvedIssue> = None;
        if !stripped.is_empty() {
            if let Some(id) = parse_python_uuid(&stripped) {
                matched = resolve_by_id(pool, slug, user_id, &id).await?;
            } else if let Some((ident, sequence)) = split_identifier_ref(&stripped)? {
                matched = resolve_by_identifier(pool, slug, user_id, &ident, sequence).await?;
            }
        }
        match matched {
            Some(issue) => found.push(issue),
            // `ref or str(raw)`: an empty strip echoes the ORIGINAL
            // rendering (`None`, `0`, `{}`, ...), never "".
            None => unresolved.push(if stripped.is_empty() {
                python_str(raw)
            } else {
                stripped
            }),
        }
    }
    Ok((found, unresolved))
}

/// `str(raw or "")` for a ref: falsy JSON (`null`, `false`, `0`/`0.0`,
/// `""`, `[]`, `{}`) renders `""` here (the echo arm re-renders the
/// original below); everything else renders Python `str()`.
fn python_ref_text(raw: &Value) -> String {
    let falsy = match raw {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
    };
    if falsy {
        String::new()
    } else {
        python_str(raw)
    }
}

/// Python `str()` of a JSON-decoded value: strings bare, `None`/`True`/
/// `False`, ints plain, floats shortest-repr, containers `repr`.
fn python_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else {
                python_float_repr(number.as_f64().expect("f64"))
            }
        }
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", python_str_repr(key), python_repr(item)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` of a JSON-decoded value (for container echoes).
fn python_repr(value: &Value) -> String {
    match value {
        Value::String(text) => python_str_repr(text),
        other => python_str(other),
    }
}

/// Python `repr()` of a string.
fn python_str_repr(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        if c == quote {
            out.push('\\');
            out.push(quote);
        } else {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c < ' ' || c == '\u{7f}' => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c if c.is_control() => {
                    let code = c as u32;
                    if code < 0x100 {
                        out.push_str(&format!("\\x{:02x}", code));
                    } else if code < 0x10000 {
                        out.push_str(&format!("\\u{:04x}", code));
                    } else {
                        out.push_str(&format!("\\U{:08x}", code));
                    }
                }
                c => out.push(c),
            }
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a float: shortest round-trip digits laid out by
/// Python's rules — fixed notation for decimal exponents `-3..=16` with a
/// mandatory `.0` on integral values, else `d[.ddd]e±XX` with a signed,
/// ≥2-digit exponent.
fn python_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let negative = value.is_sign_negative();
    let abs = value.abs();
    if abs == 0.0 {
        return if negative { "-0.0" } else { "0.0" }.to_string();
    }
    let ryu = serde_json::Number::from_f64(abs)
        .expect("finite")
        .to_string();
    let (mantissa, exp): (&str, i32) = match ryu.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse().expect("ryu exponent")),
        None => (ryu.as_str(), 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len());
    let mut digits: Vec<char> = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    while digits.len() > 1 && digits[0] == '0' {
        digits.remove(0);
    }
    let after_point = mantissa.len() - point - usize::from(point < mantissa.len());
    let dec_exp = exp - after_point as i32 + digits.len() as i32;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-3..=16).contains(&dec_exp) {
        if dec_exp <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-dec_exp) as usize));
            out.extend(digits);
        } else if dec_exp as usize >= digits.len() {
            let pad = dec_exp as usize - digits.len();
            out.extend(digits);
            out.push_str(&"0".repeat(pad));
            out.push_str(".0");
        } else {
            let split = dec_exp as usize;
            out.extend(digits[..split].iter());
            out.push('.');
            out.extend(digits[split..].iter());
        }
    } else {
        out.push(digits[0]);
        if digits.len() > 1 {
            out.push('.');
            out.extend(digits[1..].iter());
        }
        let exp10 = dec_exp - 1;
        out.push('e');
        out.push(if exp10 < 0 { '-' } else { '+' });
        let mag = exp10.unsigned_abs().to_string();
        if mag.len() < 2 {
            out.push('0');
        }
        out.push_str(&mag);
    }
    out
}

/// `ref.rpartition("-")` + `seq.isdigit()` + `int(seq)`
/// (`relations.py:155-157`): the identifier attempt. Returns `None` when
/// the shape misses; a numeric-but-not-decimal tail raises REL-4's 500;
/// an overflowing tail matches nothing (a PG int column never holds it).
fn split_identifier_ref(text: &str) -> Result<Option<(String, i64)>, Denial> {
    let Some((ident, seq)) = text.rsplit_once('-') else {
        return Ok(None);
    };
    if ident.is_empty() || seq.is_empty() {
        return Ok(None);
    }
    if seq.chars().all(|c| c.is_ascii_digit()) {
        return match seq.parse::<i64>() {
            Ok(sequence) => Ok(Some((ident.to_owned(), sequence))),
            // Unbounded Python ints filter to nothing on an int column.
            Err(_) => Ok(None),
        };
    }
    // REL-4: `seq.isdigit()` true but `int(seq)` raises → uncaught
    // `ValueError` → 500. Approximated with `is_numeric` (see the module
    // docs for the `Nl` edge).
    if seq.chars().all(|c| c.is_numeric()) {
        return Err(Denial::ServerError);
    }
    Ok(None)
}

/// One `match...first()` (`relations.py:158`): the visible pool filtered
/// to the id, `Meta.ordering` (`-created_at`) + `LIMIT 1`.
async fn resolve_by_id(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    id: &Uuid,
) -> Result<Option<ResolvedIssue>, Denial> {
    let row = sqlx::query(&format!(
        r#"SELECT "issues"."id", "issues"."workspace_id", "projects"."identifier", "issues"."sequence_id"
           {VISIBLE_POOL_JOINS}
           WHERE ({VISIBLE_POOL_WHERE}) AND "issues"."id" = $3
           ORDER BY "issues"."created_at" DESC LIMIT 1"#
    ))
    .bind(slug)
    .bind(user_id)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "resolve-ref-id"))?;
    row.map(decode_resolved).transpose()
}

/// The identifier attempt (`relations.py:157-158`): `UPPER(identifier) =
/// UPPER($)` (`__iexact`) + `sequence_id`, same `.first()` tail.
async fn resolve_by_identifier(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    ident: &str,
    sequence: i64,
) -> Result<Option<ResolvedIssue>, Denial> {
    // Unbounded Python ints filter to nothing on the int column.
    if sequence > i64::from(i32::MAX) || sequence < i64::from(i32::MIN) {
        return Ok(None);
    }
    let row = sqlx::query(&format!(
        r#"SELECT "issues"."id", "issues"."workspace_id", "projects"."identifier", "issues"."sequence_id"
           {VISIBLE_POOL_JOINS}
           WHERE ({VISIBLE_POOL_WHERE}) AND UPPER("projects"."identifier") = UPPER($3)
             AND "issues"."sequence_id" = $4
           ORDER BY "issues"."created_at" DESC LIMIT 1"#
    ))
    .bind(slug)
    .bind(user_id)
    .bind(ident)
    .bind(sequence as i32)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "resolve-ref-identifier"))?;
    row.map(decode_resolved).transpose()
}

fn decode_resolved(row: sqlx::postgres::PgRow) -> Result<ResolvedIssue, Denial> {
    Ok(ResolvedIssue {
        id: row_uuid(&row, "id")?,
        workspace_id: row_uuid(&row, "workspace_id")?,
        project_identifier: row_string(&row, "identifier")?,
        sequence_id: row
            .try_get("sequence_id")
            .map_err(|_| Denial::ServerError)?,
    })
}

/// `_source` (`views/issue.py:3153-3156`): the manager-scoped issue plus
/// its project identifier (for `identifier()`), state, and workspace.
/// A miss answers the `ObjectDoesNotExist` 404.
struct SourceIssue {
    id: Uuid,
    project_id: Uuid,
    workspace_id: Uuid,
    project_identifier: String,
    sequence_id: i32,
}

impl SourceIssue {
    fn identifier(&self) -> String {
        format!("{}-{}", self.project_identifier, self.sequence_id)
    }
}

async fn fetch_source(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    issue_id: &Uuid,
) -> Result<SourceIssue, Denial> {
    let row = sqlx::query(
        r#"SELECT "issues"."id", "issues"."project_id", "issues"."workspace_id",
                  "projects"."identifier", "issues"."sequence_id"
           FROM "issues"
           LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
           INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
           INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id")
           WHERE ("issues"."deleted_at" IS NULL
             AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)
             AND NOT ("issues"."archived_at" IS NOT NULL)
             AND NOT ("projects"."archived_at" IS NOT NULL)
             AND NOT ("issues"."is_draft")
             AND "workspaces"."slug" = $1
             AND "issues"."project_id" = $2
             AND "issues"."id" = $3)"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "relation-source"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    Ok(SourceIssue {
        id: row_uuid(&row, "id")?,
        project_id: row_uuid(&row, "project_id")?,
        workspace_id: row_uuid(&row, "workspace_id")?,
        project_identifier: row_string(&row, "identifier")?,
        sequence_id: row
            .try_get("sequence_id")
            .map_err(|_| Denial::ServerError)?,
    })
}

/// `_check_targets` (`relations.py:166-177`): self-relation and
/// cross-workspace refusals, then first-seen dedupe. The refusals fire
/// before the dedupe check, so a duplicate self-ref still 400s.
fn check_targets(
    source: &SourceIssue,
    targets: Vec<ResolvedIssue>,
) -> Result<Vec<ResolvedIssue>, Denial> {
    let mut unique = Vec::with_capacity(targets.len());
    let mut seen = std::collections::HashSet::new();
    for target in targets {
        if target.id == source.id {
            return Err(Denial::BadError(format!(
                "{} cannot be related to itself",
                source.identifier()
            )));
        }
        if target.workspace_id != source.workspace_id {
            return Err(Denial::BadError(format!(
                "{} is in a different workspace",
                target.identifier()
            )));
        }
        if seen.insert(target.id) {
            unique.push(target);
        }
    }
    Ok(unique)
}

/// `get_inverse_relation` (`utils/issue_relation_mapper.py:5-16`):
/// unknown wires fall through unchanged.
fn inverse_relation(relation_type: &str) -> &str {
    match relation_type {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocked_by" => "blocking",
        "blocking" => "blocked_by",
        "start_before" => "start_after",
        "finish_before" => "finish_after",
        "implemented_by" => "implements",
        "implements" => "implemented_by",
        other => other,
    }
}

/// `_stored_edge` (`relations.py:92-97`): `(issue_id, related_issue_id,
/// stored_type)` for "source \<type\> target".
fn stored_edge(source_id: Uuid, relation_type: &str, target_id: Uuid) -> (Uuid, Uuid, String) {
    let stored = actual_relation(relation_type).to_owned();
    if AGENT_REVERSE_TYPES.contains(&relation_type) {
        (target_id, source_id, stored)
    } else {
        (source_id, target_id, stored)
    }
}

/// One live pair row for viewpoint classification.
struct PairRow {
    id: Uuid,
    issue_id: Uuid,
    related_issue_id: Uuid,
    relation_type: String,
}

/// `_type_from` (`relations.py:100-114`): the relation a row expresses,
/// named from the viewpoint's side. Rows stored under a reverse name
/// (which the UI never writes but older data may hold) normalize first.
fn type_from(row: &PairRow, viewpoint_id: &Uuid) -> String {
    let mut stored = row.relation_type.as_str();
    let (mut issue_id, mut related_id) = (row.issue_id, row.related_issue_id);
    if AGENT_REVERSE_TYPES.contains(&stored) {
        stored = actual_relation(stored);
        std::mem::swap(&mut issue_id, &mut related_id);
    }
    if issue_id == *viewpoint_id {
        stored.to_owned()
    } else {
        inverse_relation(stored).to_owned()
    }
}

/// `_pair_rows` (`relations.py:117-120`): live rows between the two ids in
/// either direction, `Meta.ordering` (`-created_at`).
async fn fetch_pair_rows(pool: &PgPool, a_id: &Uuid, b_id: &Uuid) -> Result<Vec<PairRow>, Denial> {
    let rows = sqlx::query(
        r#"SELECT "id", "issue_id", "related_issue_id", "relation_type" FROM "issue_relations"
           WHERE "deleted_at" IS NULL
             AND (("issue_id" = $1 AND "related_issue_id" = $2)
               OR ("issue_id" = $2 AND "related_issue_id" = $1))
           ORDER BY "created_at" DESC"#,
    )
    .bind(a_id)
    .bind(b_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "relation-pair-rows"))?;
    rows.iter()
        .map(|row| {
            Ok(PairRow {
                id: row_uuid(row, "id")?,
                issue_id: row_uuid(row, "issue_id")?,
                related_issue_id: row_uuid(row, "related_issue_id")?,
                relation_type: row_string(row, "relation_type")?,
            })
        })
        .collect()
}

/// The related-objects sweep payload (`SoftDeleteModel.delete()` calls
/// `.delay(app_label, model_name, pk, using=None)`): positionals
/// `("db", model, pk)`, kwargs `{"using": null}`.
fn soft_delete_sweep(model_name: &str, pk: &str) -> (Vec<Value>, Map<String, Value>) {
    let mut kwargs = Map::with_capacity(1);
    kwargs.insert("using".to_owned(), Value::Null);
    (
        vec![
            Value::String("db".to_owned()),
            Value::String(model_name.to_owned()),
            Value::String(pk.to_owned()),
        ],
        kwargs,
    )
}

/// One grouped item (`relations.py:269-277`).
struct GroupedItem {
    id: Uuid,
    project_identifier: String,
    sequence_id: i32,
    name: String,
    state_name: Option<String>,
    state_group: Option<String>,
}

fn render_grouped_item(item: &GroupedItem) -> Value {
    let mut out = Map::with_capacity(5);
    out.insert("id".to_owned(), Value::String(item.id.to_string()));
    out.insert(
        "identifier".to_owned(),
        Value::String(format!("{}-{}", item.project_identifier, item.sequence_id)),
    );
    out.insert("name".to_owned(), Value::String(item.name.clone()));
    out.insert(
        "state".to_owned(),
        item.state_name
            .as_deref()
            .map_or(Value::Null, |name| Value::String(name.to_owned())),
    );
    out.insert(
        "state_group".to_owned(),
        item.state_group
            .as_deref()
            .map_or(Value::Null, |group| Value::String(group.to_owned())),
    );
    Value::Object(out)
}

/// `grouped_relations` (`relations.py:280-312`): every live relation of
/// the source, grouped by type from its side, narrowed to the visible
/// pool, sorted by `(project.identifier, sequence_id)`, capped at
/// [`GROUP_LIMIT`] per type. Every type key is always present, in
/// [`RELATION_TYPES`] order.
async fn grouped_relations(
    pool: &PgPool,
    slug: &str,
    user_id: &Uuid,
    source: &SourceIssue,
) -> Result<Map<String, Value>, Denial> {
    let rows = sqlx::query(
        r#"SELECT "issue_id", "related_issue_id", "relation_type" FROM "issue_relations"
           WHERE "deleted_at" IS NULL
             AND ("issue_id" = $1 OR "related_issue_id" = $1)
             AND "workspace_id" = $2
             AND NOT ("issue_id" = $1 AND "related_issue_id" = $1)"#,
    )
    .bind(source.id)
    .bind(source.workspace_id)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "grouped-rows"))?;
    let mut other_by_type: HashMap<&str, std::collections::HashSet<Uuid>> = HashMap::new();
    for relation in RELATION_TYPES {
        other_by_type.insert(*relation, std::collections::HashSet::new());
    }
    for row in &rows {
        let issue_id = row_uuid(row, "issue_id")?;
        let related_id = row_uuid(row, "related_issue_id")?;
        let relation_type = row_string(row, "relation_type")?;
        let other = if issue_id == source.id {
            related_id
        } else {
            issue_id
        };
        // The viewpoint probe has no id (unneeded for classification).
        let probe = PairRow {
            id: Uuid::nil(),
            issue_id,
            related_issue_id: related_id,
            relation_type,
        };
        let relation = type_from(&probe, &source.id);
        if let Some(bucket) = other_by_type.get_mut(relation.as_str()) {
            bucket.insert(other);
        }
    }
    let wanted: Vec<Uuid> = other_by_type
        .values()
        .flat_map(|bucket| bucket.iter().copied())
        .collect();
    let mut others: HashMap<Uuid, GroupedItem> = HashMap::new();
    if !wanted.is_empty() {
        let pool_rows = sqlx::query(&format!(
            r#"SELECT "issues"."id", "issues"."name", "issues"."sequence_id",
                      "projects"."identifier" AS "project_identifier",
                      "states"."name" AS "state_name", "states"."group" AS "state_group"
               {VISIBLE_POOL_JOINS}
               WHERE ({VISIBLE_POOL_WHERE})
                 AND "issues"."id" = ANY($3)
                 AND "issues"."workspace_id" = $4"#
        ))
        .bind(slug)
        .bind(user_id)
        .bind(&wanted)
        .bind(source.workspace_id)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "grouped-pool"))?;
        for row in &pool_rows {
            let item = GroupedItem {
                id: row_uuid(row, "id")?,
                project_identifier: row_string(row, "project_identifier")?,
                sequence_id: row
                    .try_get("sequence_id")
                    .map_err(|_| Denial::ServerError)?,
                name: row_string(row, "name")?,
                state_name: row_string_opt(row, "state_name")?,
                state_group: row_string_opt(row, "state_group")?,
            };
            others.insert(item.id, item);
        }
    }
    let mut out = Map::with_capacity(RELATION_TYPES.len());
    for relation in RELATION_TYPES {
        let bucket = other_by_type.get(*relation).expect("seeded types");
        let mut items: Vec<&GroupedItem> = bucket.iter().filter_map(|id| others.get(id)).collect();
        items.sort_by(|a, b| {
            (&a.project_identifier, a.sequence_id).cmp(&(&b.project_identifier, b.sequence_id))
        });
        items.truncate(GROUP_LIMIT);
        out.insert(
            (*relation).to_owned(),
            Value::Array(items.iter().map(|item| render_grouped_item(item)).collect()),
        );
    }
    Ok(out)
}

/// The `_write` refs read (`views/issue.py:3167-3174`): `request.data.get`
/// (last-wins for forms — hence the scalar body spec, NOT `getlist`),
/// a bare string wraps to one element, anything else must be a non-empty
/// list. A non-object JSON body 500s: `.get` on it raises
/// `AttributeError` (same wart class as REL-3).
fn write_refs(parsed: &WriteBody) -> Result<Vec<Value>, Denial> {
    let Value::Object(map) = &parsed.value else {
        return Err(Denial::ServerError);
    };
    match map.get("issues") {
        Some(Value::String(text)) => Ok(vec![Value::String(text.clone())]),
        Some(Value::Array(items)) if !items.is_empty() => Ok(items.clone()),
        _ => Err(Denial::BadError(
            "issues must be a non-empty list of work item identifiers or UUIDs".to_owned(),
        )),
    }
}

/// The relate/unrelate activity kwargs (`relations.py:123-135`): the
/// 7-key [`work_tasks::issue_activity_kwargs`] order plus trailing
/// `notification` — and NO `origin` (unlike the endpoint POST).
fn agent_activity_kwargs(
    activity_type: &str,
    requested_data: &str,
    actor_id: &str,
    issue_id: &str,
    project_id: &str,
    current_instance: Option<&str>,
    epoch: i64,
) -> Map<String, Value> {
    let mut kwargs = work_tasks::issue_activity_kwargs(
        activity_type,
        Some(requested_data),
        actor_id,
        issue_id,
        project_id,
        current_instance,
        epoch,
    );
    kwargs.insert("notification".to_owned(), Value::Bool(true));
    kwargs
}

/// `relate` (`relations.py:180-232`): record "source \<type\> each
/// target", idempotent. Returns the response map minus `relations` (the
/// caller appends the grouped read).
async fn relate_op(
    pool: &PgPool,
    source: &SourceIssue,
    relation_type: &str,
    targets: Vec<ResolvedIssue>,
    actor_id: &Uuid,
) -> Result<Map<String, Value>, Denial> {
    let targets = check_targets(source, targets)?;
    let mut created: Vec<&ResolvedIssue> = Vec::new();
    let mut unchanged: Vec<String> = Vec::new();
    let mut conflicts: Vec<Value> = Vec::new();
    for target in &targets {
        let (issue_id, related_id, stored) = stored_edge(source.id, relation_type, target.id);
        let existing = fetch_pair_rows(pool, &source.id, &target.id).await?;
        if existing.is_empty() {
            // `objects.create()` in `transaction.atomic()` (a single
            // statement is atomic anyway): the explicit
            // `created_by`/`updated_by` pass through `BaseModel.save`,
            // whose adding branch keeps `created_by` but NULLS
            // `updated_by` (REL-5). A concurrent writer's unique
            // violation re-reads below instead of 500ing.
            let inserted = sqlx::query(
                r#"INSERT INTO "issue_relations" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
                     "project_id", "workspace_id", "issue_id", "related_issue_id", "relation_type")
                   VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9) ON CONFLICT DO NOTHING"#,
            )
            .bind(Uuid::new_v4())
            .bind(now_utc())
            .bind(now_utc())
            .bind(actor_id)
            .bind(source.project_id)
            .bind(source.workspace_id)
            .bind(issue_id)
            .bind(related_id)
            .bind(&stored)
            .execute(pool)
            .await;
            match inserted {
                Ok(done) if done.rows_affected() == 1 => {
                    created.push(target);
                    continue;
                }
                Ok(_) => {}
                Err(error) if is_unique_violation(&error) => {}
                Err(error) => return Err(db_error(error, "relate-insert")),
            }
        }
        let existing = if existing.is_empty() {
            fetch_pair_rows(pool, &source.id, &target.id).await?
        } else {
            existing
        };
        let current: std::collections::BTreeSet<String> = existing
            .iter()
            .map(|row| type_from(row, &source.id))
            .collect();
        if current.len() == 1 && current.contains(relation_type) {
            unchanged.push(target.identifier());
        } else {
            let first = current.iter().next().cloned().unwrap_or_default();
            let mut conflict = Map::with_capacity(2);
            conflict.insert("identifier".to_owned(), Value::String(target.identifier()));
            conflict.insert("existing_relation".to_owned(), Value::String(first));
            conflicts.push(Value::Object(conflict));
        }
    }
    if !created.is_empty() {
        let mut requested = Map::with_capacity(2);
        requested.insert(
            "relation_type".to_owned(),
            Value::String(relation_type.to_owned()),
        );
        requested.insert(
            "issues".to_owned(),
            Value::Array(
                created
                    .iter()
                    .map(|target| Value::String(target.id.to_string()))
                    .collect(),
            ),
        );
        let dumps = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps;
        let kwargs = agent_activity_kwargs(
            work_tasks::ACTIVITY_RELATION_CREATED,
            &dumps(&Value::Object(requested)),
            &actor_id.to_string(),
            &source.id.to_string(),
            &source.project_id.to_string(),
            None,
            Utc::now().timestamp(),
        );
        enqueue_best_effort(pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    }
    let mut out = Map::with_capacity(5);
    out.insert("issue".to_owned(), Value::String(source.identifier()));
    out.insert(
        "relation_type".to_owned(),
        Value::String(relation_type.to_owned()),
    );
    out.insert(
        "created".to_owned(),
        Value::Array(
            created
                .iter()
                .map(|target| Value::String(target.identifier()))
                .collect(),
        ),
    );
    out.insert(
        "unchanged".to_owned(),
        Value::Array(
            unchanged
                .iter()
                .map(|ident| Value::String(ident.clone()))
                .collect(),
        ),
    );
    out.insert("conflicts".to_owned(), Value::Array(conflicts));
    Ok(out)
}

/// `unrelate` (`relations.py:235-266`): remove "source \<type\> each
/// target", idempotent — only exactly that type is removed. Returns the
/// response map minus `relations`.
async fn unrelate_op(
    pool: &PgPool,
    source: &SourceIssue,
    relation_type: &str,
    targets: Vec<ResolvedIssue>,
    actor_id: &Uuid,
) -> Result<Map<String, Value>, Denial> {
    let targets = check_targets(source, targets)?;
    let mut removed: Vec<String> = Vec::new();
    let mut not_related: Vec<String> = Vec::new();
    for target in &targets {
        let rows = fetch_pair_rows(pool, &source.id, &target.id).await?;
        let matching: Vec<&PairRow> = rows
            .iter()
            .filter(|row| type_from(row, &source.id) == relation_type)
            .collect();
        if matching.is_empty() {
            not_related.push(target.identifier());
            continue;
        }
        // `row.delete()`: `deleted_at = now`, full `save()` (`updated_at`
        // re-stamped, `updated_by` = actor per REL-5), then the sweep.
        for row in matching {
            sqlx::query(
                r#"UPDATE "issue_relations" SET "deleted_at" = $1, "updated_at" = $2, "updated_by_id" = $3
                   WHERE "id" = $4"#,
            )
            .bind(now_utc())
            .bind(now_utc())
            .bind(actor_id)
            .bind(row.id)
            .execute(pool)
            .await
            .map_err(|error| db_error(error, "unrelate-delete"))?;
            let (sweep_args, sweep_kwargs) =
                soft_delete_sweep("issuerelation", &row.id.to_string());
            enqueue_best_effort(pool, SOFT_DELETE_TASK, sweep_args, sweep_kwargs).await;
        }
        removed.push(target.identifier());
        // One activity per removed target (inside the loop).
        let mut requested = Map::with_capacity(2);
        requested.insert(
            "relation_type".to_owned(),
            Value::String(relation_type.to_owned()),
        );
        requested.insert(
            "related_issue".to_owned(),
            Value::String(target.id.to_string()),
        );
        let mut current = Map::with_capacity(1);
        current.insert(
            "relation_type".to_owned(),
            Value::String(relation_type.to_owned()),
        );
        let dumps = pidash_jobs::tasks_webhooks::activity_dispatch::django_dumps;
        let kwargs = agent_activity_kwargs(
            "issue_relation.activity.deleted",
            &dumps(&Value::Object(requested)),
            &actor_id.to_string(),
            &source.id.to_string(),
            &source.project_id.to_string(),
            Some(&dumps(&Value::Object(current))),
            Utc::now().timestamp(),
        );
        enqueue_best_effort(pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    }
    let mut out = Map::with_capacity(4);
    out.insert("issue".to_owned(), Value::String(source.identifier()));
    out.insert(
        "relation_type".to_owned(),
        Value::String(relation_type.to_owned()),
    );
    out.insert(
        "removed".to_owned(),
        Value::Array(
            removed
                .iter()
                .map(|ident| Value::String(ident.clone()))
                .collect(),
        ),
    );
    out.insert(
        "not_related".to_owned(),
        Value::Array(
            not_related
                .iter()
                .map(|ident| Value::String(ident.clone()))
                .collect(),
        ),
    );
    Ok(out)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn read_body(body: axum::body::Body) -> Result<Vec<u8>, Denial> {
    axum::body::to_bytes(body, usize::MAX)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| db_error(error, "read-body"))
}

/// Rebuild the request against the ORIGINAL path for the proxy
/// (Django's `<uuid:>` converter would not match — before auth runs, as URL
/// resolving precedes it). The URI must be the request's own: Django's 404
/// page echoes the path.
fn proxy_request<'a>(
    state: &'a AppState,
    method: &'a str,
    uri: String,
) -> impl std::future::Future<Output = Response> + 'a {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::empty())
        .expect("proxy request");
    crate::edge::proxy(State(state.clone()), req)
}

/// `GET .../relations/` (`views/issue.py:2965-3008`).
pub async fn get_relation_list(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    match relation_list_inner(&state, &headers, &slug, &project_id, &issue_id).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn relation_list_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::RelationList,
        "GET",
    )
    .await?;
    // No datetime renders here, but `TimezoneMixin.initial` still runs —
    // an unknown zone 400s on this path too.
    let _tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let aggregate = fetch_relation_aggregate(&pre.pool, slug, issue_id).await?;
    // `:3000-3001` — direction unions, first-seen order (REL-1).
    let duplicate = union_ids(&aggregate.duplicate, &aggregate.duplicate_related);
    let relates_to = union_ids(&aggregate.relates_to, &aggregate.relates_to_related);
    let strings = |ids: &[Uuid]| -> Vec<String> { ids.iter().map(Uuid::to_string).collect() };
    let blocking = strings(&aggregate.blocking);
    let blocked_by = strings(&aggregate.blocked_by);
    let duplicate = strings(&duplicate);
    let relates_to = strings(&relates_to);
    let start_after = strings(&aggregate.start_after);
    let start_before = strings(&aggregate.start_before);
    let finish_after = strings(&aggregate.finish_after);
    let finish_before = strings(&aggregate.finish_before);
    let blocking: Vec<&str> = blocking.iter().map(String::as_str).collect();
    let blocked_by: Vec<&str> = blocked_by.iter().map(String::as_str).collect();
    let duplicate: Vec<&str> = duplicate.iter().map(String::as_str).collect();
    let relates_to: Vec<&str> = relates_to.iter().map(String::as_str).collect();
    let start_after: Vec<&str> = start_after.iter().map(String::as_str).collect();
    let start_before: Vec<&str> = start_before.iter().map(String::as_str).collect();
    let finish_after: Vec<&str> = finish_after.iter().map(String::as_str).collect();
    let finish_before: Vec<&str> = finish_before.iter().map(String::as_str).collect();
    let body = render_relation_response(&RelationResponseGroups {
        blocking: &blocking,
        blocked_by: &blocked_by,
        duplicate: &duplicate,
        relates_to: &relates_to,
        start_after: &start_after,
        start_before: &start_before,
        finish_after: &finish_after,
        finish_before: &finish_before,
    });
    let body = serde_json::to_string(&body).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `POST .../relations/` (`views/issue.py:3061-3136`).
pub async fn post_relation(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "POST", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match relation_post_inner(&state, &headers, &slug, &project_id, &issue_id, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn relation_post_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::RelationList,
        "POST",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let parsed = parse_write_body(headers, raw_body)?;
    let validated = validate_relation_create(&parsed.value)
        .map_err(|error| Denial::FieldErrors(error.body().to_owned()))?;
    let link_workspace = fetch_project_workspace(&pre.pool, slug, &project_id).await?;
    let stored = actual_relation(&validated.relation_type).to_owned();
    let reverse = is_reverse_relation(&validated.relation_type);
    let issue_ids: Vec<Uuid> = validated
        .issues
        .iter()
        .map(|text| text.parse::<Uuid>())
        .collect::<Result<_, _>>()
        .map_err(|_| Denial::ServerError)?;
    // `bulk_create(..., batch_size=10, ignore_conflicts=True)` (`:3079-3094`):
    // one `INSERT ... ON CONFLICT DO NOTHING` per 10-row batch, each
    // autocommitted — an FK failure in a later batch keeps the earlier
    // rows and still answers 400. The explicit `created_by`/`updated_by`
    // survive verbatim (`bulk_create` calls no `save()`).
    for batch in issue_ids.chunks(10) {
        let mut sql = String::from(
            r#"INSERT INTO "issue_relations" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
                 "project_id", "workspace_id", "issue_id", "related_issue_id", "relation_type") VALUES "#,
        );
        for (index, _) in batch.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            let base = index * 10;
            sql.push_str(&format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
                base + 7,
                base + 8,
                base + 9,
                base + 10,
            ));
        }
        sql.push_str(" ON CONFLICT DO NOTHING");
        let mut query = sqlx::query(&sql);
        for target_id in batch {
            let (row_issue, row_related) = if reverse {
                (*target_id, *issue_id)
            } else {
                (*issue_id, *target_id)
            };
            // Two `now()` calls like the two `auto_now`/`auto_now_add`
            // pre-saves on each constructed instance.
            query = query
                .bind(Uuid::new_v4())
                .bind(now_utc())
                .bind(now_utc())
                .bind(pre.actor.id)
                .bind(pre.actor.id)
                .bind(project_id)
                .bind(link_workspace)
                .bind(row_issue)
                .bind(row_related)
                .bind(&stored);
        }
        query.execute(&pre.pool).await.map_err(|error| {
            if is_fk_violation(&error) {
                Denial::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
            } else {
                db_error(error, "relation-post-insert")
            }
        })?;
    }
    // The activity fan-out (`:3096-3106`): `requested_data` dumps the
    // parsed request body, `origin` resolves after the insert (a missing
    // pair 500s with the rows already committed, like Python).
    let requested = requested_data_text(&parsed);
    let origin = app_origin(&state.settings().urls)?;
    let kwargs = work_tasks::issue_activity_notify_kwargs(
        work_tasks::ACTIVITY_RELATION_CREATED,
        Some(&requested),
        &pre.actor.id.to_string(),
        &issue_id.to_string(),
        &project_id.to_string(),
        None,
        Utc::now().timestamp(),
        true,
        &origin,
    );
    enqueue_best_effort(&pre.pool, work_tasks::ISSUE_ACTIVITY_TASK, vec![], kwargs).await;
    // The refetch (`:3111-3136`): written pairs + stored type + slug,
    // `Meta.ordering` (`-created_at`); reverse wires render
    // `RelatedIssueSerializer`, forward `IssueRelationSerializer` — both
    // constructed WITHOUT `fields=`/`expand=`.
    let rendered =
        fetch_refetch_rows(&pre.pool, slug, issue_id, &issue_ids, &stored, reverse, &tz).await?;
    let body = serde_json::to_string(&rendered).map_err(|_| Denial::ServerError)?;
    Ok(json_created(body))
}

/// The post-create refetch (`:3111-3130`) with its traversal facts, in
/// `Meta.ordering`, rendered per the forward/reverse split (`:3132`).
async fn fetch_refetch_rows(
    pool: &PgPool,
    slug: &str,
    issue_id: &Uuid,
    issues: &[Uuid],
    stored: &str,
    reverse: bool,
    tz: &Tz,
) -> Result<Value, Denial> {
    // The traversed side: forward renders `related_issue`, reverse `issue`.
    let side = if reverse {
        "issue_id"
    } else {
        "related_issue_id"
    };
    let (pair_left, pair_right) = if reverse {
        (
            r#""issue_relations"."issue_id" = ANY($3)"#,
            r#""issue_relations"."related_issue_id" = $2"#,
        )
    } else {
        (
            r#""issue_relations"."issue_id" = $2"#,
            r#""issue_relations"."related_issue_id" = ANY($3)"#,
        )
    };
    let rows = sqlx::query(&format!(
        r#"SELECT "issue_relations"."relation_type",
                  "issue_relations"."created_by_id", "issue_relations"."created_at",
                  "issue_relations"."updated_at", "issue_relations"."updated_by_id",
                  "t"."id" AS "t_id", "t"."project_id" AS "t_project",
                  "t"."sequence_id" AS "t_seq", "t"."name" AS "t_name",
                  "t"."priority" AS "t_priority", "s"."id" AS "s_id",
                  "ty"."id" AS "ty_id", "ty"."is_epic" AS "ty_epic"
           FROM "issue_relations"
           LEFT OUTER JOIN "issues" AS "t" ON ("t"."id" = "issue_relations"."{side}")
           LEFT OUTER JOIN "states" AS "s" ON ("s"."id" = "t"."state_id")
           LEFT OUTER JOIN "issue_types" AS "ty" ON ("ty"."id" = "t"."type_id")
           INNER JOIN "workspaces" ON ("issue_relations"."workspace_id" = "workspaces"."id")
           WHERE ("issue_relations"."deleted_at" IS NULL
             AND {pair_left} AND {pair_right}
             AND "issue_relations"."relation_type" = $4
             AND "workspaces"."slug" = $1)
           ORDER BY "issue_relations"."created_at" DESC"#
    ))
    .bind(slug)
    .bind(issue_id)
    .bind(issues)
    .bind(stored)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "relation-refetch"))?;
    let mut rendered = Vec::with_capacity(rows.len());
    for row in &rows {
        rendered.push(render_refetch_row(row, reverse, tz)?);
    }
    Ok(Value::Array(rendered))
}

fn render_refetch_row(
    row: &sqlx::postgres::PgRow,
    reverse: bool,
    tz: &Tz,
) -> Result<Value, Denial> {
    let relation_type = row_string(row, "relation_type")?;
    let created_by: Option<Uuid> = row_uuid_opt(row, "created_by_id")?;
    let created_at: DateTime<Utc> = row_datetime(row, "created_at")?;
    let updated_at: DateTime<Utc> = row_datetime(row, "updated_at")?;
    let updated_by: Option<Uuid> = row_uuid_opt(row, "updated_by_id")?;
    let created_by = created_by.map(|id| id.to_string());
    let updated_by = updated_by.map(|id| id.to_string());
    let created_at = crate::serializer::render_datetime_in(&created_at, tz);
    let updated_at = crate::serializer::render_datetime_in(&updated_at, tz);
    let traversed: Option<Uuid> = row_uuid_opt(row, "t_id")?;
    if traversed.is_none() {
        // A concurrently hard-deleted traversal (the FK forbids it
        // otherwise): DRF `SkipField`s every traversed key and renders
        // the relation columns in field order.
        return Ok(Value::Object(missing_traversal_body(
            &relation_type,
            created_by.as_deref(),
            &created_at,
            &updated_at,
            updated_by.as_deref(),
            reverse,
        )));
    }
    let project_id = row_uuid(row, "t_project")?;
    let sequence_id: i32 = row.try_get("t_seq").map_err(|_| Denial::ServerError)?;
    let name = row_string(row, "t_name")?;
    let priority = row_string(row, "t_priority")?;
    let state_id: Option<Uuid> = row_uuid_opt(row, "s_id")?;
    let project_id = project_id.to_string();
    let state_id = state_id.map(|id| id.to_string());
    let traversed = traversed.expect("checked some").to_string();
    if reverse {
        let type_id: Option<Uuid> = row_uuid_opt(row, "ty_id")?;
        let is_epic: Option<bool> = row.try_get("ty_epic").map_err(|_| Denial::ServerError)?;
        let type_id = type_id.map(|id| id.to_string());
        let issue_type = match (&type_id, is_epic) {
            (Some(id), Some(is_epic)) => Some(RelatedIssueType { id, is_epic }),
            _ => None,
        };
        let row = RelatedIssueRow {
            id: &traversed,
            project_id: &project_id,
            sequence_id: i64::from(sequence_id),
            relation_type: &relation_type,
            name: &name,
            issue_type,
            state_id: state_id.as_deref(),
            priority: &priority,
            created_by: created_by.as_deref(),
            created_at: &created_at,
            updated_by: updated_by.as_deref(),
            updated_at: &updated_at,
        };
        let map = render_related_issue(&RelatedIssueShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .map_err(|_| Denial::ServerError)?;
        Ok(Value::Object(map))
    } else {
        let row = IssueRelationRow {
            id: &traversed,
            project_id: &project_id,
            sequence_id: i64::from(sequence_id),
            relation_type: &relation_type,
            name: &name,
            state_id: state_id.as_deref(),
            priority: &priority,
            created_by: created_by.as_deref(),
            created_at: &created_at,
            updated_at: &updated_at,
            updated_by: updated_by.as_deref(),
        };
        let map = render_issue_relation(&IssueRelationShowInput {
            row: &row,
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .map_err(|_| Denial::ServerError)?;
        Ok(Value::Object(map))
    }
}

/// The hard-deleted-traversal render: relation columns only, in the
/// serializer's field order (`SkipField` parity).
fn missing_traversal_body(
    relation_type: &str,
    created_by: Option<&str>,
    created_at: &str,
    updated_at: &str,
    updated_by: Option<&str>,
    reverse: bool,
) -> Map<String, Value> {
    let mut out = Map::with_capacity(5);
    out.insert(
        "relation_type".to_owned(),
        Value::String(relation_type.to_owned()),
    );
    out.insert(
        "created_by".to_owned(),
        created_by.map_or(Value::Null, |id| Value::String(id.to_owned())),
    );
    out.insert(
        "created_at".to_owned(),
        Value::String(created_at.to_owned()),
    );
    if reverse {
        out.insert(
            "updated_by".to_owned(),
            updated_by.map_or(Value::Null, |id| Value::String(id.to_owned())),
        );
        out.insert(
            "updated_at".to_owned(),
            Value::String(updated_at.to_owned()),
        );
    } else {
        out.insert(
            "updated_at".to_owned(),
            Value::String(updated_at.to_owned()),
        );
        out.insert(
            "updated_by".to_owned(),
            updated_by.map_or(Value::Null, |id| Value::String(id.to_owned())),
        );
    }
    out
}

/// `GET .../relations/grouped/` (`views/issue.py:3207-3217`).
pub async fn get_relation_grouped(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    match relation_grouped_inner(&state, &headers, &slug, &project_id, &issue_id).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn relation_grouped_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::RelationGrouped,
        "GET",
    )
    .await?;
    let _tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let source = fetch_source(&pre.pool, slug, &project_id, issue_id).await?;
    let relations = grouped_relations(&pre.pool, slug, &pre.actor.id, &source).await?;
    let mut out = Map::with_capacity(2);
    out.insert("issue".to_owned(), Value::String(source.identifier()));
    out.insert("relations".to_owned(), Value::Object(relations));
    let body = serde_json::to_string(&out).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `POST .../relations/relate/` (`views/issue.py:3232-3235`).
pub async fn post_relate(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "POST", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match relate_inner(&state, &headers, &slug, &project_id, &issue_id, &raw, true).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `POST .../relations/unrelate/` (`views/issue.py:3250-3253`).
pub async fn post_unrelate(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "POST", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match relate_inner(&state, &headers, &slug, &project_id, &issue_id, &raw, false).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

/// `_write` (`views/issue.py:3163-3194`) over `relate` / `unrelate`:
/// source first (404 before any body read), then refs shape, relation
/// type, resolve, the operation, and the trailing grouped read.
async fn relate_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    raw_body: &[u8],
    is_relate: bool,
) -> Result<Response, Denial> {
    let route = if is_relate {
        V1WorkItemsRoute::RelationRelate
    } else {
        V1WorkItemsRoute::RelationUnrelate
    };
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        route,
        "POST",
    )
    .await?;
    let _tz = activate_timezone(pre.actor.timezone.as_deref())?;
    // `_source` BEFORE the body parse (`:3165-3167`): a bogus issue 404s
    // even when the body is malformed (Python touches `request.data`
    // only after the source lookup).
    let source = fetch_source(&pre.pool, slug, &project_id, issue_id).await?;
    let parsed = parse_agent_body(headers, raw_body)?;
    let refs = write_refs(&parsed)?;
    let relation_type = validate_relation_type(
        parsed
            .value
            .as_object()
            .and_then(|map| map.get("relation_type")),
    )?;
    let (targets, unresolved) = resolve_refs(&pre.pool, slug, &pre.actor.id, &refs).await?;
    if !unresolved.is_empty() {
        let mut out = Map::with_capacity(2);
        out.insert(
            "error".to_owned(),
            Value::String(format!(
                "work items not found or not accessible: {}",
                unresolved.join(", ")
            )),
        );
        out.insert(
            "unresolved".to_owned(),
            Value::Array(
                unresolved
                    .iter()
                    .map(|text| Value::String(text.clone()))
                    .collect(),
            ),
        );
        let body = serde_json::to_string(&out).map_err(|_| Denial::ServerError)?;
        return Err(Denial::NotFound(body));
    }
    let mut out = if is_relate {
        relate_op(&pre.pool, &source, &relation_type, targets, &pre.actor.id).await?
    } else {
        unrelate_op(&pre.pool, &source, &relation_type, targets, &pre.actor.id).await?
    };
    let relations = grouped_relations(&pre.pool, slug, &pre.actor.id, &source).await?;
    out.insert("relations".to_owned(), Value::Object(relations));
    let body = serde_json::to_string(&out).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `GET .../workpad/` (`views/issue.py:3275-3279`).
pub async fn get_workpad(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "GET", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    match workpad_get_inner(&state, &headers, &slug, &project_id, &issue_id).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn workpad_get_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::Workpad,
        "GET",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let row = sqlx::query(
        r#"SELECT "issues"."workpad", "issues"."updated_at"
           FROM "issues"
           LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
           INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
           INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id")
           WHERE ("issues"."deleted_at" IS NULL
             AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)
             AND NOT ("issues"."archived_at" IS NOT NULL)
             AND NOT ("projects"."archived_at" IS NOT NULL)
             AND NOT ("issues"."is_draft")
             AND "workspaces"."slug" = $1
             AND "issues"."project_id" = $2
             AND "issues"."id" = $3)"#,
    )
    .bind(slug)
    .bind(project_id)
    .bind(issue_id)
    .fetch_optional(&pre.pool)
    .await
    .map_err(|error| db_error(error, "workpad-get"))?;
    let Some(row) = row else {
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let workpad = row_string(&row, "workpad")?;
    let updated_at: DateTime<Utc> = row_datetime(&row, "updated_at")?;
    let updated_at = crate::serializer::render_datetime_in(&updated_at, &tz);
    let body = render_workpad(&WorkpadRow {
        body: &workpad,
        updated_at: &updated_at,
    });
    let body = serde_json::to_string(&body).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `PATCH .../workpad/` (`views/issue.py:3281-3321`).
pub async fn patch_workpad(
    State(state): State<AppState>,
    OriginalUri(original): OriginalUri,
    Path((slug, project_id, issue_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    if !crate::runner_runs::is_uuid_path_segment(&issue_id) {
        return proxy_request(&state, "PATCH", original.to_string()).await;
    }
    let issue_id = issue_id.parse::<Uuid>().expect("checked segment");
    let raw = match read_body(body).await {
        Ok(raw) => raw,
        Err(denial) => return denial.into_response(),
    };
    match workpad_patch_inner(&state, &headers, &slug, &project_id, &issue_id, &raw).await {
        Ok(response) => response,
        Err(denial) => denial.into_response(),
    }
}

async fn workpad_patch_inner(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    project_id_raw: &str,
    issue_id: &Uuid,
    raw_body: &[u8],
) -> Result<Response, Denial> {
    let pre = preamble(state, headers, slug).await?;
    let project_id = rewrite_project_id(&pre.pool, slug, project_id_raw).await?;
    let workspace_id = pre.workspace_id.ok_or(Denial::Forbidden)?;
    require_entity_gate(
        &pre.pool,
        &workspace_id,
        slug,
        &pre.actor.id,
        &project_id,
        V1WorkItemsRoute::Workpad,
        "PATCH",
    )
    .await?;
    let tz = activate_timezone(pre.actor.timezone.as_deref())?;
    let parsed = parse_agent_body(headers, raw_body)?;
    // The wire-field gate (`:3287-3291`): `"body" not in request.data`.
    // On a JSON string it is a SUBSTRING test; on numbers/bools/null it
    // raises `TypeError` → 500.
    if !workpad_has_body(&parsed.value)? {
        return Err(Denial::FieldErrors(WORKPAD_MISSING_BODY.to_owned()));
    }
    let mut txn = pre
        .pool
        .begin()
        .await
        .map_err(|error| db_error(error, "workpad-txn"))?;
    // The locked read (`:3307-3311`, fixture `workpad_patch_lock`):
    // `LIMIT 21` (executed `.get()`) + `FOR UPDATE OF issues`
    // (`of=("self",)` — a bare lock would 500 on the nullable state
    // join). A miss 404s BEFORE validation runs.
    let row = sqlx::query(
        r#"SELECT "issues"."description_html", "issues"."state_id",
                  "issues"."completed_at", "states"."group" AS "state_group"
           FROM "issues"
           LEFT OUTER JOIN "states" ON ("issues"."state_id" = "states"."id")
           INNER JOIN "projects" ON ("issues"."project_id" = "projects"."id")
           INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id")
           WHERE ("issues"."deleted_at" IS NULL
             AND NOT ("states"."group" = 'triage' AND "states"."group" IS NOT NULL)
             AND NOT ("issues"."archived_at" IS NOT NULL)
             AND NOT ("projects"."archived_at" IS NOT NULL)
             AND NOT ("issues"."is_draft")
             AND "issues"."id" = $1
             AND "issues"."project_id" = $2
             AND "workspaces"."slug" = $3)
           LIMIT 21 FOR UPDATE OF "issues""#,
    )
    .bind(issue_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(&mut *txn)
    .await
    .map_err(|error| db_error(error, "workpad-lock"))?;
    let Some(row) = row else {
        txn.rollback()
            .await
            .map_err(|error| db_error(error, "workpad-rollback"))?;
        return Err(Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()));
    };
    let validated = validate_workpad_write(&WorkpadWriteInput {
        body: &parsed.value,
        partial: true,
    })
    .map_err(|error| Denial::FieldErrors(error.body().to_owned()));
    let validated = match validated {
        Ok(validated) => validated,
        Err(denial) => {
            txn.rollback()
                .await
                .map_err(|error| db_error(error, "workpad-rollback"))?;
            return Err(denial);
        }
    };
    let Some(workpad) = validated.workpad else {
        // Unreachable: the wire-field gate above guarantees a `body` key,
        // and validation only omits it when absent.
        txn.rollback()
            .await
            .map_err(|error| db_error(error, "workpad-rollback"))?;
        return Err(Denial::ServerError);
    };
    let description_html = row_string(&row, "description_html")?;
    let state_id: Option<Uuid> = row_uuid_opt(&row, "state_id")?;
    let completed_at: Option<DateTime<Utc>> = row
        .try_get("completed_at")
        .map_err(|_| Denial::ServerError)?;
    let state_group: Option<String> = row_string_opt(&row, "state_group")?;
    // `Issue.save` (REL-5): a stateless issue is ASSIGNED the default
    // non-triage state (else the first by `sequence`), but its
    // `completed_at` is untouched (the recompute needs a prior state);
    // `description_stripped` recomputes; `updated_by` stamps the actor.
    let (state_id, completed_at) = resolve_workpad_state(
        &mut txn,
        &project_id,
        state_id,
        state_group.as_deref(),
        completed_at,
    )
    .await?;
    let stripped = if description_html.is_empty() {
        None
    } else {
        Some(crate::space::sanitize::strip_tags(&description_html))
    };
    let updated_at = now_utc();
    sqlx::query(
        r#"UPDATE "issues" SET "workpad" = $1, "description_stripped" = $2, "completed_at" = $3,
                  "state_id" = $4, "updated_by_id" = $5, "updated_at" = $6
           WHERE "id" = $7"#,
    )
    .bind(&workpad)
    .bind(stripped.as_deref())
    .bind(completed_at)
    .bind(state_id)
    .bind(pre.actor.id)
    .bind(updated_at)
    .bind(issue_id)
    .execute(&mut *txn)
    .await
    .map_err(|error| db_error(error, "workpad-update"))?;
    txn.commit()
        .await
        .map_err(|error| db_error(error, "workpad-commit"))?;
    // The timestamp only — the body just came from the caller (`:3315-3320`).
    let mut out = Map::with_capacity(1);
    out.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(&updated_at, &tz)),
    );
    let body = serde_json::to_string(&out).map_err(|_| Denial::ServerError)?;
    Ok(json_response(StatusCode::OK, body))
}

/// `"body" in request.data` (`:3287`): key membership on objects, the
/// always-false membership on arrays, substring on strings — and
/// `TypeError` (500) on numbers/bools/null.
fn workpad_has_body(value: &Value) -> Result<bool, Denial> {
    match value {
        Value::Object(map) => Ok(map.contains_key("body")),
        Value::Array(_) => Ok(false),
        Value::String(text) => Ok(text.contains("body")),
        // `"body" in 5` / `in True` / `in None` raises `TypeError` →
        // generic 500.
        _ => Err(Denial::ServerError),
    }
}

/// The NULL-state arm of `Issue.save` (`db/models/issue.py:288-308`):
/// default non-triage state first, else the first non-triage state, both
/// by `Meta.ordering` (`sequence`); with no states at all the issue keeps
/// its NULL state. `completed_at` is untouched in every NULL-state case —
/// the recompute runs only when the issue already had a state (the `else`
/// arm, `issue.py:300-308`).
async fn resolve_workpad_state(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: &Uuid,
    state_id: Option<Uuid>,
    state_group: Option<&str>,
    completed_at: Option<DateTime<Utc>>,
) -> Result<(Option<Uuid>, Option<DateTime<Utc>>), Denial> {
    if let Some(state_id) = state_id {
        let group = state_group.unwrap_or_default();
        if group == "completed" {
            return Ok((Some(state_id), Some(now_utc())));
        }
        return Ok((Some(state_id), None));
    }
    let found: Option<(Uuid, String)> = sqlx::query_as(
        r#"SELECT "id", "group" FROM "states"
           WHERE "deleted_at" IS NULL AND "group" != 'triage' AND NOT ("is_triage")
             AND "project_id" = $1 AND "default"
           ORDER BY "sequence" LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(&mut **txn)
    .await
    .map_err(|error| db_error(error, "workpad-default-state"))?;
    let found = match found {
        Some(found) => Some(found),
        None => sqlx::query_as(
            r#"SELECT "id", "group" FROM "states"
                   WHERE "deleted_at" IS NULL AND "group" != 'triage' AND NOT ("is_triage")
                     AND "project_id" = $1
                   ORDER BY "sequence" LIMIT 1"#,
        )
        .bind(project_id)
        .fetch_optional(&mut **txn)
        .await
        .map_err(|error| db_error(error, "workpad-random-state"))?,
    };
    // The `completed_at` recompute lives in the `else` arm of the
    // `self.state is None` branch (`issue.py:300-308`): a stateless issue
    // keeps its `completed_at` untouched whether or not a state is found.
    match found {
        Some((id, _group)) => Ok((Some(id), completed_at)),
        // No states at all: `self.state` stays `None`, `completed_at`
        // untouched (the `else` arm never runs).
        None => Ok((None, completed_at)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F18_11: &str =
        include_str!("../../../../fixtures/v1_work_items/handlers/F18-11.work_items.json");

    fn calls() -> Map<String, Value> {
        let fixture: Value = serde_json::from_str(F18_11).expect("fixture parses");
        fixture
            .get("calls")
            .and_then(Value::as_object)
            .cloned()
            .expect("calls map")
    }

    fn call(name: &str) -> Value {
        calls().get(name).cloned().unwrap_or(Value::Null)
    }

    fn str_field(body: &Value, key: &str) -> String {
        body.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    }

    fn opt_str_field(body: &Value, key: &str) -> Option<String> {
        body.get(key).and_then(|v| match v {
            Value::Null => None,
            Value::String(text) => Some(text.clone()),
            _ => None,
        })
    }

    fn str_list(body: &Value, key: &str) -> Vec<String> {
        body.get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|v| v.as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// This issue's F18-11 subset exists with the recorded statuses.
    #[test]
    fn fixture_subset_present() {
        let calls = calls();
        for (name, status) in [
            ("relation_list", 200),
            ("relation_grouped", 200),
            ("relation_create", 201),
            ("relation_create_bad", 400),
            ("relate", 200),
            ("relate_unresolvable", 404),
            ("unrelate", 200),
            ("workpad_get", 200),
            ("workpad_patch", 200),
            ("workpad_patch_no_body", 400),
        ] {
            let call = calls.get(name).expect(name);
            assert_eq!(
                call.get("status").and_then(Value::as_u64),
                Some(status),
                "{name}"
            );
        }
    }

    // --- F18-11 replays: status + body byte-identical per route ---

    #[test]
    fn replay_relation_list() {
        let body = call("relation_list").get("body").cloned().expect("body");
        let blocking = str_list(&body, "blocking");
        let blocked_by = str_list(&body, "blocked_by");
        let duplicate = str_list(&body, "duplicate");
        let relates_to = str_list(&body, "relates_to");
        let start_after = str_list(&body, "start_after");
        let start_before = str_list(&body, "start_before");
        let finish_after = str_list(&body, "finish_after");
        let finish_before = str_list(&body, "finish_before");
        let blocking: Vec<&str> = blocking.iter().map(String::as_str).collect();
        let blocked_by: Vec<&str> = blocked_by.iter().map(String::as_str).collect();
        let duplicate: Vec<&str> = duplicate.iter().map(String::as_str).collect();
        let relates_to: Vec<&str> = relates_to.iter().map(String::as_str).collect();
        let start_after: Vec<&str> = start_after.iter().map(String::as_str).collect();
        let start_before: Vec<&str> = start_before.iter().map(String::as_str).collect();
        let finish_after: Vec<&str> = finish_after.iter().map(String::as_str).collect();
        let finish_before: Vec<&str> = finish_before.iter().map(String::as_str).collect();
        let rendered = render_relation_response(&RelationResponseGroups {
            blocking: &blocking,
            blocked_by: &blocked_by,
            duplicate: &duplicate,
            relates_to: &relates_to,
            start_after: &start_after,
            start_before: &start_before,
            finish_after: &finish_after,
            finish_before: &finish_before,
        });
        assert_eq!(
            serde_json::to_string(&rendered).expect("render"),
            serde_json::to_string(&body).expect("fixture"),
        );
    }

    #[test]
    fn replay_relation_create_bad() {
        let call = call("relation_create_bad");
        let request = call.get("request").cloned().expect("request");
        let body = call.get("body").cloned().expect("body");
        let error = validate_relation_create(&request).expect_err("invalid choice");
        assert_eq!(error.body(), serde_json::to_string(&body).expect("fixture"));
    }

    #[test]
    fn replay_relation_create_row() {
        let body = call("relation_create").get("body").cloned().expect("body");
        let row = body.as_array().expect("list").first().expect("row");
        let id = str_field(row, "id");
        let project_id = str_field(row, "project_id");
        let relation_type = str_field(row, "relation_type");
        let name = str_field(row, "name");
        let state_id = opt_str_field(row, "state_id");
        let priority = str_field(row, "priority");
        let created_by = opt_str_field(row, "created_by");
        let created_at = str_field(row, "created_at");
        let updated_at = str_field(row, "updated_at");
        let updated_by = opt_str_field(row, "updated_by");
        let sequence_id = row.get("sequence_id").and_then(Value::as_i64).expect("seq");
        let rendered = render_issue_relation(&IssueRelationShowInput {
            row: &IssueRelationRow {
                id: &id,
                project_id: &project_id,
                sequence_id,
                relation_type: &relation_type,
                name: &name,
                state_id: state_id.as_deref(),
                priority: &priority,
                created_by: created_by.as_deref(),
                created_at: &created_at,
                updated_at: &updated_at,
                updated_by: updated_by.as_deref(),
            },
            fields: None,
            expand: &[],
            expansions: &[],
        })
        .expect("render");
        assert_eq!(
            serde_json::to_string(&rendered).expect("render"),
            serde_json::to_string(&row).expect("fixture"),
        );
    }

    #[test]
    fn replay_relate_unresolvable_body() {
        let body = call("relate_unresolvable")
            .get("body")
            .cloned()
            .expect("body");
        // The 404 builder: same shape the handler emits for `unresolved`.
        let unresolved = ["ZZZ-999".to_owned()];
        let mut out = Map::with_capacity(2);
        out.insert(
            "error".to_owned(),
            Value::String(format!(
                "work items not found or not accessible: {}",
                unresolved.join(", ")
            )),
        );
        out.insert(
            "unresolved".to_owned(),
            Value::Array(
                unresolved
                    .iter()
                    .map(|text| Value::String(text.clone()))
                    .collect(),
            ),
        );
        assert_eq!(
            serde_json::to_string(&out).expect("render"),
            serde_json::to_string(&body).expect("fixture"),
        );
    }

    #[test]
    fn replay_grouped_item() {
        let body = call("relation_grouped").get("body").cloned().expect("body");
        let blocking = body
            .get("relations")
            .and_then(|r| r.get("blocking"))
            .and_then(Value::as_array)
            .expect("blocking");
        let first = blocking.first().expect("item");
        let item = GroupedItem {
            id: str_field(first, "id").parse().expect("uuid"),
            project_identifier: "CT00003".to_owned(),
            sequence_id: 2,
            name: str_field(first, "name"),
            state_name: opt_str_field(first, "state"),
            state_group: opt_str_field(first, "state_group"),
        };
        assert_eq!(
            serde_json::to_string(&render_grouped_item(&item)).expect("render"),
            serde_json::to_string(&first).expect("fixture"),
        );
        // Every type key present, in RELATION_TYPES order.
        let relations = body
            .get("relations")
            .and_then(Value::as_object)
            .expect("map");
        let keys: Vec<&str> = relations.keys().map(String::as_str).collect();
        assert_eq!(keys, RELATION_TYPES);
    }

    #[test]
    fn replay_workpad_get() {
        let body = call("workpad_get").get("body").cloned().expect("body");
        let text = str_field(&body, "body");
        let updated_at = str_field(&body, "updated_at");
        let rendered = render_workpad(&WorkpadRow {
            body: &text,
            updated_at: &updated_at,
        });
        assert_eq!(
            serde_json::to_string(&rendered).expect("render"),
            serde_json::to_string(&body).expect("fixture"),
        );
    }

    #[test]
    fn replay_workpad_patch_validates() {
        let call = call("workpad_patch");
        let request = call.get("request").cloned().expect("request");
        let validated = validate_workpad_write(&WorkpadWriteInput {
            body: &request,
            partial: true,
        })
        .expect("valid");
        assert_eq!(validated.workpad.as_deref(), Some("# notes\n\n- a"));
    }

    #[test]
    fn replay_workpad_patch_no_body() {
        let call = call("workpad_patch_no_body");
        let request = call.get("request").cloned().expect("request");
        let body = call.get("body").cloned().expect("body");
        assert!(!workpad_has_body(&request).expect("bool"));
        assert_eq!(
            WORKPAD_MISSING_BODY,
            serde_json::to_string(&body).expect("fixture")
        );
    }

    // --- Agent vocabulary units ---

    #[test]
    fn relation_type_validation_matrix() {
        assert_eq!(
            validate_relation_type(Some(&Value::String("blocking".to_owned()))).expect("valid"),
            "blocking"
        );
        assert_eq!(
            validate_relation_type(Some(&Value::String("  BLOCKING  ".to_owned())))
                .expect("strip+lower"),
            "blocking"
        );
        assert_eq!(
            validate_relation_type(None).expect_err("missing"),
            Denial::BadError(format!(
                "relation_type must be one of: {}",
                RELATION_TYPES.join(", ")
            )),
        );
        assert_eq!(
            validate_relation_type(Some(&Value::String("nope".to_owned()))).expect_err("unknown"),
            Denial::BadError(format!(
                "relation_type must be one of: {}",
                RELATION_TYPES.join(", ")
            )),
        );
        // Falsy non-strings take the "" arm → 400 ...
        for raw in [
            Value::Null,
            Value::Bool(false),
            Value::Number(0.into()),
            Value::String(String::new()),
            Value::Array(vec![]),
            Value::Object(Map::new()),
        ] {
            assert!(
                matches!(validate_relation_type(Some(&raw)), Err(Denial::BadError(_))),
                "{raw:?} 400s"
            );
        }
        // ... while truthy non-strings 500 (REL-3).
        for raw in [
            Value::Bool(true),
            Value::Number(5.into()),
            Value::Number(serde_json::Number::from_f64(1.5).expect("f64")),
            Value::Array(vec![Value::String("blocking".to_owned())]),
            Value::Object({
                let mut map = Map::new();
                map.insert("a".to_owned(), Value::Null);
                map
            }),
        ] {
            assert_eq!(
                validate_relation_type(Some(&raw)),
                Err(Denial::ServerError),
                "{raw:?} 500s"
            );
        }
    }

    #[test]
    fn python_uuid_acceptance() {
        let hyphenated = "25ace52e-c64d-4043-a700-911b1e42bffc";
        let expected: Uuid = hyphenated.parse().expect("uuid");
        // Canonical forms.
        assert_eq!(parse_python_uuid(hyphenated), Some(expected));
        assert_eq!(
            parse_python_uuid("25ACE52E-C64D-4043-A700-911B1E42BFFC"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("25ace52ec64d4043a700911b1e42bffc"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("{25ace52e-c64d-4043-a700-911b1e42bffc}"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("urn:uuid:25ace52e-c64d-4043-a700-911b1e42bffc"),
            Some(expected)
        );
        // Pathological spellings CPython accepts (misplaced hyphens,
        // unbalanced braces, underscores).
        assert_eq!(
            parse_python_uuid("25ace52ec-64d-4043a700-911b1e42bffc"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("{{25ace52e-c64d-4043-a700-911b1e42bffc}"),
            Some(expected)
        );
        assert_eq!(
            parse_python_uuid("25ace52e_c64d4043a700911b1e42bffc"),
            Some(expected)
        );
        // Rejects: wrong length, bad hex, doubled/edge underscores,
        // uppercase URN prefix (case-sensitive strip), identifiers.
        for bad in [
            "25ace52e-c64d-4043-a700-911b1e42bff",
            "zzace52e-c64d-4043-a700-911b1e42bffc",
            "25ace52e__c64d4043a700911b1e42bffc",
            "_5ace52ec64d4043a700911b1e42bffc1",
            "URN:UUID:25ace52e-c64d-4043-a700-911b1e42bffc",
            "CT00003-1",
            "",
        ] {
            assert_eq!(parse_python_uuid(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn identifier_ref_splitting() {
        assert_eq!(
            split_identifier_ref("CT00003-1").expect("ok"),
            Some(("CT00003".to_owned(), 1))
        );
        assert_eq!(
            split_identifier_ref("A-B-12").expect("ok"),
            Some(("A-B".to_owned(), 12))
        );
        assert_eq!(split_identifier_ref("CT00003").expect("ok"), None);
        assert_eq!(split_identifier_ref("-12").expect("ok"), None);
        assert_eq!(split_identifier_ref("CT-").expect("ok"), None);
        assert_eq!(split_identifier_ref("CT-1x").expect("ok"), None);
        // Overflowing tails match nothing.
        assert_eq!(
            split_identifier_ref("CT-99999999999999999999999").expect("ok"),
            None
        );
        // Numeric-but-not-decimal tails 500 (REL-4).
        assert_eq!(split_identifier_ref("CT-²"), Err(Denial::ServerError));
    }

    #[test]
    fn python_str_matrix() {
        assert_eq!(python_str(&Value::Null), "None");
        assert_eq!(python_str(&Value::Bool(true)), "True");
        assert_eq!(python_str(&Value::Bool(false)), "False");
        assert_eq!(python_str(&Value::Number(5.into())), "5");
        assert_eq!(
            python_str(&Value::Number(
                serde_json::Number::from_f64(1.5).expect("f64")
            )),
            "1.5"
        );
        assert_eq!(python_str(&Value::String("x".to_owned())), "x");
        assert_eq!(python_str(&Value::Array(vec![])), "[]");
        assert_eq!(python_str(&Value::Object(Map::new())), "{}");
        assert_eq!(python_str(&serde_json::json!({"a": 1})), "{'a': 1}");
        assert_eq!(python_ref_text(&Value::Null), "");
        assert_eq!(python_ref_text(&Value::Number(0.into())), "");
        assert_eq!(python_ref_text(&Value::Number(7.into())), "7");
    }

    #[test]
    fn write_refs_matrix() {
        let parsed = |value: Value| WriteBody {
            value,
            from_form: false,
        };
        // Bare strings wrap.
        assert_eq!(
            write_refs(&parsed(serde_json::json!({"issues": "CT-1"}))).expect("wrap"),
            vec![Value::String("CT-1".to_owned())]
        );
        // Non-empty lists pass through.
        assert_eq!(
            write_refs(&parsed(serde_json::json!({"issues": ["a", "b"]}))).expect("list"),
            vec![Value::String("a".to_owned()), Value::String("b".to_owned())]
        );
        // Missing / wrong shapes 400 ...
        for body in [
            serde_json::json!({}),
            serde_json::json!({"issues": []}),
            serde_json::json!({"issues": 5}),
            serde_json::json!({"issues": {"a": 1}}),
            serde_json::json!({"issues": null}),
        ] {
            assert_eq!(
                write_refs(&parsed(body)),
                Err(Denial::BadError(
                    "issues must be a non-empty list of work item identifiers or UUIDs".to_owned()
                )),
            );
        }
        // ... while non-object bodies 500 (`.get` wart).
        for body in [
            Value::Array(vec![]),
            Value::String("x".to_owned()),
            Value::Number(5.into()),
            Value::Null,
        ] {
            assert_eq!(write_refs(&parsed(body)), Err(Denial::ServerError));
        }
    }

    #[test]
    fn workpad_body_membership() {
        assert!(workpad_has_body(&serde_json::json!({"body": "x"})).expect("bool"));
        assert!(!workpad_has_body(&serde_json::json!({"workpad": "x"})).expect("bool"));
        assert!(!workpad_has_body(&serde_json::json!(["body"])).expect("bool"));
        assert!(workpad_has_body(&Value::String("has body inside".to_owned())).expect("bool"));
        assert!(!workpad_has_body(&Value::String("nope".to_owned())).expect("bool"));
        // `TypeError` wart → 500.
        for body in [Value::Number(5.into()), Value::Bool(true), Value::Null] {
            assert_eq!(workpad_has_body(&body), Err(Denial::ServerError));
        }
    }

    #[test]
    fn stored_edge_and_inverse_matrix() {
        let source = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let target = Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid");
        // Forward wires keep sides; reverse wires swap.
        assert_eq!(
            stored_edge(source, "blocked_by", target),
            (source, target, "blocked_by".to_owned())
        );
        assert_eq!(
            stored_edge(source, "blocking", target),
            (target, source, "blocked_by".to_owned())
        );
        assert_eq!(
            stored_edge(source, "implements", target),
            (target, source, "implemented_by".to_owned())
        );
        assert_eq!(
            stored_edge(source, "relates_to", target),
            (source, target, "relates_to".to_owned())
        );
        assert_eq!(inverse_relation("blocked_by"), "blocking");
        assert_eq!(inverse_relation("blocking"), "blocked_by");
        assert_eq!(inverse_relation("implements"), "implemented_by");
        assert_eq!(inverse_relation("duplicate"), "duplicate");
        assert_eq!(inverse_relation("bogus"), "bogus");
    }

    #[test]
    fn type_from_matrix() {
        let viewpoint = Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid");
        let other = Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("uuid");
        let row = |issue: Uuid, related: Uuid, stored: &str| PairRow {
            id: Uuid::nil(),
            issue_id: issue,
            related_issue_id: related,
            relation_type: stored.to_owned(),
        };
        // Stored forward, viewed from the issue side.
        assert_eq!(
            type_from(&row(viewpoint, other, "blocked_by"), &viewpoint),
            "blocked_by"
        );
        // Same row from the other side inverts.
        assert_eq!(
            type_from(&row(viewpoint, other, "blocked_by"), &other),
            "blocking"
        );
        // Symmetric types read the same both ways.
        assert_eq!(
            type_from(&row(viewpoint, other, "relates_to"), &other),
            "relates_to"
        );
        // Legacy reverse-stored rows normalize (swap + actual).
        assert_eq!(
            type_from(&row(viewpoint, other, "blocking"), &other),
            "blocked_by"
        );
        assert_eq!(
            type_from(&row(viewpoint, other, "blocking"), &viewpoint),
            "blocking"
        );
    }

    #[test]
    fn check_targets_matrix() {
        let workspace = Uuid::parse_str("33333333-3333-3333-3333-333333333333").expect("uuid");
        let source = SourceIssue {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").expect("uuid"),
            project_id: Uuid::nil(),
            workspace_id: workspace,
            project_identifier: "CT".to_owned(),
            sequence_id: 1,
        };
        let target = |id: &str, ws: Uuid| ResolvedIssue {
            id: id.parse().expect("uuid"),
            workspace_id: ws,
            project_identifier: "CT".to_owned(),
            sequence_id: 2,
        };
        // Dedupe preserves first-seen order.
        let kept = check_targets(
            &source,
            vec![
                target("22222222-2222-2222-2222-222222222222", workspace),
                target("22222222-2222-2222-2222-222222222222", workspace),
            ],
        )
        .expect("dedupe");
        assert_eq!(kept.len(), 1);
        // Self-relation 400s, even duplicated.
        assert_eq!(
            check_targets(
                &source,
                vec![target("11111111-1111-1111-1111-111111111111", workspace)],
            ),
            Err(Denial::BadError(
                "CT-1 cannot be related to itself".to_owned()
            )),
        );
        // Cross-workspace 400s.
        let elsewhere = Uuid::parse_str("44444444-4444-4444-4444-444444444444").expect("uuid");
        assert!(matches!(
            check_targets(&source, vec![target(
                "22222222-2222-2222-2222-222222222222",
                elsewhere
            )]),
            Err(Denial::BadError(message)) if message.ends_with("is in a different workspace"),
        ));
    }

    // --- Denials, sweep, kwargs, misc ---

    #[test]
    fn denial_status_and_bodies() {
        assert_eq!(
            Denial::Unauthorized.status_and_body().0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            Denial::InvalidToken.status_and_body(),
            (
                StatusCode::FORBIDDEN,
                r#"{"Detail":"Given API token is not valid"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::ProjectNotFound.status_and_body(),
            (
                StatusCode::NOT_FOUND,
                r#"{"Detail":"Project not found"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::BadDetail("x".to_owned()).status_and_body(),
            (StatusCode::BAD_REQUEST, r#"{"Detail":"x"}"#.to_owned())
        );
        assert_eq!(
            Denial::BadError("x".to_owned()).status_and_body(),
            (StatusCode::BAD_REQUEST, r#"{"error":"x"}"#.to_owned())
        );
        assert_eq!(
            Denial::ServerError.status_and_body(),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                SERVER_ERROR_BODY.to_owned()
            )
        );
    }

    #[test]
    fn timezone_activation_matrix() {
        assert_eq!(activate_timezone(None).expect("utc").to_string(), "UTC");
        assert!(matches!(
            activate_timezone(Some("Nope/Nowhere")),
            Err(Denial::BadError(_))
        ));
    }

    #[test]
    fn activate_timezone_empty_zone_500s() {
        // `ZoneInfo('')` raises `ValueError` (not `KeyError`), so an
        // empty stored zone is the generic 500 while an unknown zone is
        // the `KeyError`-branch 400 (PIDASHCONV-747, live-probed; same
        // arm as PIDASHCONV-786 ported to social/pr_links).
        assert!(matches!(
            activate_timezone(Some("")),
            Err(Denial::ServerError)
        ));
        assert!(matches!(
            activate_timezone(Some("Not/AZone")),
            Err(Denial::BadError(_))
        ));
        assert_eq!(activate_timezone(None).expect("none"), chrono_tz::UTC);
        assert_eq!(activate_timezone(Some("UTC")).expect("utc"), chrono_tz::UTC);
    }

    #[test]
    fn sweep_and_agent_kwargs_shapes() {
        let (args, kwargs) = soft_delete_sweep("issuerelation", "abc");
        assert_eq!(
            args,
            vec![
                Value::String("db".to_owned()),
                Value::String("issuerelation".to_owned()),
                Value::String("abc".to_owned()),
            ]
        );
        assert_eq!(kwargs.get("using"), Some(&Value::Null));
        let kwargs = agent_activity_kwargs("t", "r", "a", "i", "p", None, 7);
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "type",
                "requested_data",
                "actor_id",
                "issue_id",
                "project_id",
                "current_instance",
                "epoch",
                "notification",
            ]
        );
    }

    #[test]
    fn missing_traversal_orders() {
        let forward = missing_traversal_body("blocked_by", None, "c", "u", None, false);
        let keys: Vec<&str> = forward.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "relation_type",
                "created_by",
                "created_at",
                "updated_at",
                "updated_by"
            ]
        );
        let reverse = missing_traversal_body("blocked_by", None, "c", "u", None, true);
        let keys: Vec<&str> = reverse.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "relation_type",
                "created_by",
                "created_at",
                "updated_by",
                "updated_at"
            ]
        );
    }
}
