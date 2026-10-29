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
//! `crate::v1_cycles_modules::{body, json}`, the request shell
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

use std::sync::Arc;

use axum::extract::{OriginalUri, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_auth::scope::TenantScope;
use pidash_services::domain::activity::TaskEnqueueHook;
use pidash_services::domain::relations as relation_domain;
use pidash_services::domain::transition::IssueTransitionHook;
use pidash_services::domain::{WriteActor, WriteCtx, WriteScope};
use pidash_services::operations;
use pidash_services::v1_work_items::queries_sub::{
    actual_relation, is_reverse_relation, union_ids,
};
use pidash_services::v1_work_items::shape_labels::{render_workpad, WorkpadRow};
use pidash_services::v1_work_items::shape_relations::{
    render_issue_relation, render_related_issue, render_relation_response,
    validate_relation_create, IssueRelationRow, IssueRelationShowInput, RelatedIssueRow,
    RelatedIssueShowInput, RelatedIssueType, RelationResponseGroups,
};
use pidash_services::v1_work_items::tasks as work_tasks;

use crate::domain_hooks::AppTaskEnqueueHook;
use crate::issue_transition::AppTransitionHook;
use crate::state::AppState;
use crate::v1_projects::handlers_project::{
    preamble, project_base_facts, rewrite_project_id, Preamble,
};

use super::perms::{decide, gate_for, V1WorkItemsRoute};
pub use pidash_compat::response::SERVER_ERROR_BODY;

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// `handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:154-158`): every `.get()` miss on these endpoints
/// (project lookup, `_source`, workpad read/lock).
pub const RESOURCE_NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception`'s `IntegrityError` branch
/// (`api/views/base.py:142-147`): FK violations on the relation create.
pub const PAYLOAD_NOT_VALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `_write` refs shape denial (`views/issue.py:3170-3174`).
pub const ISSUES_NONEMPTY_BODY: &str =
    r#"{"error":"issues must be a non-empty list of work item identifiers or UUIDs"}"#;
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
    /// 404, `{"detail":"Project not found"}` (identifier rewrite miss —
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
                r#"{"detail":"Given API token is not valid"}"#.to_owned(),
            ),
            Denial::Forbidden => (
                StatusCode::FORBIDDEN,
                super::perms::CLASS_DENIAL_BODY.to_owned(),
            ),
            Denial::ProjectNotFound => (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned(),
            ),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
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
/// cross the boundary — every body re-renders here, so this module
/// owns the exact `detail`-key bytes on every denial it emits.
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
            D::UnsupportedMediaType(message) => Denial::UnsupportedMediaType(message),
            D::BadError(message) => Denial::BadError(message),
            // Unreachable from the reused preamble/rewrite/facts calls
            // (pure auth + lookup, no serializer runs); preserved rather
            // than remapped so no byte is ever invented here.
            D::FieldErrors(body) => Denial::FieldErrors(body),
            D::NotFoundError(message) => {
                Denial::NotFound(format!("{{\"error\":{}}}", json_string(&message)))
            }
            // Unreachable from the reused calls (no 409 arm there).
            // Unreachable from the reused calls (patch-only lock).
            D::ForbiddenError(_) => Denial::ServerError,
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
    pidash_compat::response::json_response(status, pidash_compat::response::escape_u2028(&body))
}

/// Render a 201 JSON response with exact bytes.
fn json_created(body: String) -> Response {
    json_response(StatusCode::CREATED, body)
}

/// Map an operation failure onto the `Denial` arms the moved handlers
/// used (OVL-14, PIDASHCONV-1649). The relation operations never return
/// the issues-only/move-only variants (`Conflict`, `ProjectNotFound`,
/// `Rejection`), so those arms 500 instead of rendering a body this
/// module cannot spell.
impl From<operations::Error> for Denial {
    fn from(error: operations::Error) -> Self {
        match error {
            operations::Error::BadError(message) => Denial::BadError(message),
            operations::Error::FieldErrors(body) => Denial::FieldErrors(body),
            operations::Error::NotFound => Denial::NotFound(RESOURCE_NOT_FOUND_BODY.to_owned()),
            operations::Error::NotFoundBody(body) => Denial::NotFound(body),
            operations::Error::ServerError => Denial::ServerError,
            operations::Error::Conflict(_)
            | operations::Error::ProjectNotFound
            | operations::Error::Rejection { .. } => Denial::ServerError,
        }
    }
}

/// Wrap an operation outcome in the v1 response.
fn op_response(outcome: operations::Outcome) -> Result<Response, Denial> {
    let status = StatusCode::from_u16(outcome.status).map_err(|_| Denial::ServerError)?;
    Ok(json_response(status, outcome.body))
}

/// The actor/scope context the relation operations run with.
fn domain_write_ctx(pre: &Preamble, project_id: &Uuid) -> WriteCtx {
    WriteCtx::new(WriteActor {
        user_id: pre.actor.id,
        timezone: pre.actor.timezone.clone(),
    })
    .with_scope(WriteScope {
        workspace_id: pre.workspace_id,
        project_id: Some(*project_id),
    })
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
    let message = pidash_jobs::TaskMessage::new(task, args, kwargs);
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
    use crate::v1_cycles_modules::json::{
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
/// miss answers the `ObjectDoesNotExist` 404 — including soft-deleted
/// projects, whose live memberships still pass the gate but which the
/// `SoftDeletionManager` excludes here.
async fn fetch_project_workspace(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
) -> Result<Uuid, Denial> {
    let workspace_id: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT "projects"."workspace_id" FROM "projects"
           INNER JOIN "workspaces" ON ("projects"."workspace_id" = "workspaces"."id")
           WHERE "projects"."id" = $1 AND "projects"."deleted_at" IS NULL
             AND "workspaces"."slug" = $2"#,
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

// ---------------------------------------------------------------------------
// Agent vocabulary (`orchestration/relations.py`, PDASHOSS01-199)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn read_body(body: axum::body::Body) -> Result<Vec<u8>, Denial> {
    pidash_compat::http::read_body(body)
        .await
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
        // Domain batch twin: one chunk-atomic `INSERT ... ON CONFLICT
        // DO NOTHING` per 10-row batch, each autocommitted — an FK
        // failure in a later batch keeps the earlier rows and still
        // answers 400. Two `now()` calls per row like the two
        // `auto_now`/`auto_now_add` pre-saves on each constructed
        // instance; `bulk_create` calls no `save()`, so the explicit
        // actor audit survives verbatim.
        let mut rows = Vec::with_capacity(batch.len());
        for target_id in batch {
            let (row_issue, row_related) = if reverse {
                (*target_id, *issue_id)
            } else {
                (*issue_id, *target_id)
            };
            rows.push(relation_domain::RelationRow {
                id: Uuid::new_v4(),
                created_at: now_utc(),
                updated_at: now_utc(),
                created_by: Some(pre.actor.id),
                updated_by: Some(pre.actor.id),
                project_id,
                workspace_id: link_workspace,
                issue_id: row_issue,
                related_issue_id: row_related,
                relation_type: &stored,
            });
        }
        relation_domain::relate_rows_or_nothing(&pre.pool, &rows)
            .await
            .map_err(|error| {
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
    let source =
        operations::relations::fetch_source(&pre.pool, slug, &project_id, issue_id).await?;
    let relations =
        operations::relations::grouped_relations(&pre.pool, slug, &pre.actor.id, &source).await?;
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

/// `_write` (`views/issue.py:3163-3194`) over `relate` / `unrelate`.
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
    // `TimezoneMixin.initial` precedes the view body: an unknown stored
    // zone 400s here, before the source fetch 404s below.
    activate_timezone(pre.actor.timezone.as_deref())?;
    // `_source` BEFORE the body parse (`:3165-3167`): a bogus issue 404s
    // even when the body is malformed (Python touches `request.data`
    // only after the source lookup).
    let source =
        operations::relations::fetch_source(&pre.pool, slug, &project_id, issue_id).await?;
    let parsed_body = parse_agent_body(headers, raw_body)?;
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = domain_write_ctx(&pre, &project_id);
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(state, &pre.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pre.pool));
    let deps = operations::Deps {
        pool: pre.pool.clone(),
        orm: orm.clone(),
        transition,
        enqueue,
    };
    let write = operations::relations::RelationWrite {
        slug,
        source: &source,
        body: &parsed_body.value,
    };
    let outcome = if is_relate {
        operations::relations::relate(
            &deps,
            &ctx,
            &write,
            operations::relations::RelateOptions {
                emit_activity: true,
                // The v1 agent endpoint dedupes across orientations
                // (`orchestration/relations.py:193-218`); pair-exact
                // creation is the MCP tool's semantic.
                pair_exact: false,
                // The v1 agent endpoint refuses self-relations and
                // dedupes duplicate ids
                // (`orchestration/relations.py:166-177`); the raw
                // per-iteration loop is the MCP tool's semantic.
                raw_targets: false,
            },
        )
        .await?
    } else {
        operations::relations::unrelate(
            &deps,
            &ctx,
            &write,
            operations::relations::UnrelateOptions {
                emit_activity: true,
                // The v1 agent endpoint deletes every matching row
                // (`orchestration/relations.py:251-252`); newest-only
                // is the MCP tool's semantic.
                newest_only: false,
                // The v1 agent endpoint refuses self-relations and
                // dedupes duplicate ids
                // (`orchestration/relations.py:166-177`); the raw
                // per-iteration loop is the MCP tool's semantic.
                raw_targets: false,
            },
        )
        .await?
    };
    op_response(outcome)
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
    // `TimezoneMixin.initial` precedes the view body: an unknown stored
    // zone 400s here, before body parsing below.
    activate_timezone(pre.actor.timezone.as_deref())?;
    let parsed_body = parse_agent_body(headers, raw_body)?;
    let orm = state.orm().ok_or(Denial::ServerError)?;
    let ctx = domain_write_ctx(&pre, &project_id);
    let transition: Arc<dyn IssueTransitionHook> =
        Arc::new(AppTransitionHook::new(state, &pre.pool));
    let enqueue: Arc<dyn TaskEnqueueHook> = Arc::new(AppTaskEnqueueHook::new(&pre.pool));
    let deps = operations::Deps {
        pool: pre.pool.clone(),
        orm: orm.clone(),
        transition,
        enqueue,
    };
    let write = operations::workpad::WorkpadWrite {
        slug,
        project_id,
        issue_id: *issue_id,
        body: &parsed_body.value,
    };
    let outcome = operations::workpad::update(&deps, &ctx, &write).await?;
    op_response(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::v1_work_items::shape_labels::{validate_workpad_write, WorkpadWriteInput};

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
        let item = operations::relations::GroupedItem {
            id: str_field(first, "id").parse().expect("uuid"),
            project_identifier: "CT00003".to_owned(),
            sequence_id: 2,
            name: str_field(first, "name"),
            state_name: opt_str_field(first, "state"),
            state_group: opt_str_field(first, "state_group"),
        };
        assert_eq!(
            serde_json::to_string(&operations::relations::render_grouped_item(&item))
                .expect("render"),
            serde_json::to_string(&first).expect("fixture"),
        );
        // Every type key present, in RELATION_TYPES order.
        let relations = body
            .get("relations")
            .and_then(Value::as_object)
            .expect("map");
        let keys: Vec<&str> = relations.keys().map(String::as_str).collect();
        assert_eq!(keys, operations::relations::RELATION_TYPES);
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
        assert!(!operations::workpad::workpad_has_body(&request).expect("bool"));
        assert_eq!(
            operations::workpad::WORKPAD_MISSING_BODY,
            serde_json::to_string(&body).expect("fixture")
        );
    }

    // --- Agent vocabulary units ---

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
                r#"{"detail":"Given API token is not valid"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::ProjectNotFound.status_and_body(),
            (
                StatusCode::NOT_FOUND,
                r#"{"detail":"Project not found"}"#.to_owned()
            )
        );
        assert_eq!(
            Denial::BadDetail("x".to_owned()).status_and_body(),
            (StatusCode::BAD_REQUEST, r#"{"detail":"x"}"#.to_owned())
        );
        assert_eq!(
            Denial::BadError("x".to_owned()).status_and_body(),
            (StatusCode::BAD_REQUEST, r#"{"error":"x"}"#.to_owned())
        );
        // DRF `exception_handler` renders every APIException (including
        // `UnsupportedMediaType`) under lowercase-`detail`.
        assert_eq!(
            Denial::UnsupportedMediaType("Unsupported media type".to_owned()).status_and_body(),
            (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                r#"{"detail":"Unsupported media type"}"#.to_owned()
            )
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
