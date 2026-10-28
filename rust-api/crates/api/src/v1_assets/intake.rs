//! api-v1 intake issues: list / create / retrieve / update / delete (D-21, stage 5).
//!
//! Ports `IntakeIssueListCreateAPIEndpoint` (`get` + `post`,
//! `apps/api/pi_dash/api/views/intake.py:60-219`) and
//! `IntakeIssueDetailAPIEndpoint` (`get` + `patch` + `delete`, `:225-494`),
//! both carrying `ProjectLitePermission` (`:60,:225`). Routes:
//! `workspaces/<slug>/projects/<project_id>/intake-issues/` and
//! `.../intake-issues/<uuid:issue_id>/` (`api/urls/intake.py:14-23`).
//!
//! Every action also runs the `BaseAPIView` preamble (`api/views/base.py`):
//! `X-Api-Key` authentication (401/403), the slug-or-UUID `project_id`
//! rewrite (404 `Project not found` on an unresolvable slug), the
//! `ProjectLitePermission` membership gate (403), then the request
//! timezone. Throttle *enforcement* stays edge-owned (the sticky-handler
//! precedent, `sticky.rs`): no merged v1 handler answers 429.
//!
//! # Layering
//!
//! * queries: [`pidash_db::v1_assets::intake_queries`] builders plus the
//!   `v1_projects::queries_projmem` slug resolver (read-only);
//! * validation kernels: [`pidash_types::v1_assets::intake`] (accept guard,
//!   triage transition) and `super::permissions` (gates, role matrix);
//! * tasks: [`pidash_jobs::v1_assets::tasks`] payload builders, enqueued
//!   best-effort through `pidash_jobs::queue` (the sticky precedent);
//! * pagination envelope: [`pidash_services::app_issues::envelope`];
//! * the full `IssueSerializer` read shape, the Lite expansions, the
//!   relations block and the soft-delete fan-out are the merged
//!   `v1_cycles_modules::cycle` helpers — the same Django serializers, so
//!   the same bytes;
//! * `description_html` sanitization is
//!   [`crate::space::sanitize::sanitize_html`] (`nh3.clean` port) and tag
//!   stripping is `pidash_db`'s `strip_tags_ml` (`html_processor`
//!   port). The lxml round-trip is the shared
//!   [`pidash_services::html_normalize::lxml_roundtrip`]; the markdown-it
//!   converter below it has no foundation home, so it lives here,
//!   differentially tested against live Python (see `markdown`).
//!
//! # Ported bugs (translate, don't redesign)
//!
//! * create/patch/delete guard is AND (`intake is None and not
//!   `project.intake_view`): intake-missing + view-enabled falls through
//!   and 500s on `None.id` (`views/intake.py:156,315,463`).
//! * patch pops `issue` out of `request.data`, so the status-change
//!   activity never carries it (`:344`).
//! * patch answers 200 with the row even when neither serializer ran
//!   (`:433-434`), and a `{status: 1}` accept clobbers the issue-half
//!   writes (the transition saves the pre-save issue cache, `:147-155`).
//! * delete skips the creator/admin guard for accepted rows (`:475`).
//! * priority allowlist is lowercase-exact (`:163-169`).
//! * the issue-half `IssueSerializer` runs with an EMPTY context, so
//!   `assignees`/`labels` always validate to `[]` and any `state`,
//!   `parent` or `estimate_point` always 400s (`serializers/issue.py`
//!   context reads with `context={}`).
//! * `cycle`/`module` never render on `IssueExpandSerializer`
//!   (reverse-FK managers have no `.cycle`/`.module` attribute, so DRF
//!   `SkipField`s both keys).
//!
//! Fixture: `rust-api/fixtures/v1_assets/fx-h-intake.json` (`fx-h-intake`).
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};
use sqlx::Row;
use uuid::Uuid;

use pidash_auth::permissions::project::ProjectFacts;
use pidash_auth::scope::TenantScope;
use pidash_auth::token as token_kernel;
use pidash_db::v1_assets::intake_queries;
use pidash_db::v1_projects::queries_projmem::{self, TenantScope as QueryScope};
use pidash_jobs::v1_assets::tasks as intake_tasks;
use pidash_services::app_issues::envelope;
use pidash_services::html_normalize::lxml_roundtrip;
use pidash_types::v1_assets::intake as intake_types;
use pidash_types::{ProjectId, WorkspaceId};

use super::permissions as v1_perm;
use crate::assistant::events::py_dumps;
use crate::license::form_body::{self, BodyFail, BodyKind};
use crate::paginator::{
    apply_offset_window, max_hits, next_cursor, offset_window, prev_cursor, Cursor,
};
use crate::space::sanitize::{sanitize_html, Sanitize};
use crate::state::AppState;
use crate::uuid_path::{classify_uuid_segment, UuidSegment};
use crate::v1_cycles_modules::cycle as cycle_mod;
use pidash_compat::db::{is_integrity_error, is_unique_violation};
pub(crate) use pidash_compat::response::json_response;
pub use pidash_compat::response::SERVER_ERROR_BODY;
use pidash_db::v1_assets::model::sticky::strip_tags_ml;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Collection path (`api/urls/intake.py:14-18`).
pub const INTAKE_ISSUES_PATH: &str =
    "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/";
/// Detail path (`api/urls/intake.py:19-23`; `<uuid:issue_id>`).
pub const INTAKE_ISSUE_PATH: &str =
    "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{issue_id}/";

/// Intake routes: the five ported actions serve from Rust; every other
/// method on those paths proxies to Django so its bytes (405s, `OPTIONS`
/// metadata) stay Django's.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            INTAKE_ISSUES_PATH,
            axum::routing::post(intake_create)
                .get(intake_list)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            INTAKE_ISSUE_PATH,
            axum::routing::get(intake_retrieve)
                .patch(intake_update)
                .delete(intake_destroy)
                .post(crate::edge::proxy)
                .put(crate::edge::proxy)
                .head(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

// ---------------------------------------------------------------------------
// Exact response bodies (byte parity with DRF / `BaseAPIView`)
// ---------------------------------------------------------------------------

/// DRF `NotAuthenticated` (anonymous on a guarded endpoint).
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `AuthenticationFailed("Given API token is not valid")`, coerced to 403
/// (the class defines no `authenticate_header`; probed live).
pub const INVALID_TOKEN_BODY: &str = r#"{"detail":"Given API token is not valid"}"#;
/// `Project.resolve` miss (`db/models/project.py:218`): DRF `Http404`
/// with the message (NOT the `handle_exception` branch — `Http404` is an
/// `APIException`, so `super().handle_exception` renders it first).
pub const PROJECT_NOT_FOUND_BODY: &str = r#"{"detail":"Project not found"}"#;
/// `BaseAPIView.handle_exception`'s `ObjectDoesNotExist` branch
/// (`api/views/base.py:145-149`): the `.get()` miss body. NOTE this is
/// the `BaseAPIView` spelling — `BaseViewSet` (sticky) says "The required
/// object does not exist.", which is wrong here.
pub const RESOURCE_MISSING_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// Global `handler404` body (`pi_dash/urls.py:15` ->
/// `app/views/error_404.py:10`): a non-UUID `issue_id` never reaches the
/// view, because Django's `<uuid:>` converter 404s at URL resolve. The
/// view is a plain Django `JsonResponse`, so the bytes are `json.dumps`
/// defaults — a space after the colon (the `asset_user.rs` /
/// `asset_generic.rs` `PAGE_NOT_FOUND_BODY`).
pub const PAGE_NOT_FOUND_BODY: &str = r#"{"error": "Page not found."}"#;
/// `handle_exception`'s `IntegrityError` branch (`base.py:136-140`): the
/// triage-creation race (unique `name`+`project`) 400s here.
pub const PAYLOAD_INVALID_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// Create pre-check (`views/intake.py:148-149`).
pub const NAME_REQUIRED_BODY: &str = r#"{"error":"Name is required"}"#;
/// Create priority allowlist (`views/intake.py:163-169`).
pub const INVALID_PRIORITY_BODY: &str = r#"{"error":"Invalid priority"}"#;
/// Intake-view guard (`views/intake.py:156-161,315-320,463-468`).
pub const INTAKE_DISABLED_BODY: &str =
    r#"{"error":"Intake is not enabled for this project enable it through the project's api"}"#;

/// Handler failure with its exact status + body.
#[derive(Debug)]
pub enum Denial {
    /// 401, anonymous (no usable credential).
    Unauthorized,
    /// 403, bad/expired/inactive token.
    InvalidToken,
    /// 403, `ProjectLitePermission` denial (DRF-default body).
    Forbidden,
    /// 404, slug `project_id` resolves to no project.
    ProjectNotFound,
    /// 404, `.get()` miss (`ObjectDoesNotExist` branch).
    ResourceMissing,
    /// 404, non-UUID `issue_id` (the `<uuid:>` resolver-404 body).
    PageNotFound,
    /// 400, `{"Detail": ...}` (`ParseError`: per_page / cursor).
    BadDetail(String),
    /// 400, `{"error":<message>}` (`KeyError` branch: unknown zone name).
    BadError(String),
    /// 400, serializer `errors` dict.
    FieldErrors(Value),
    /// 400, `{"error": "Name is required"}`.
    NameRequired,
    /// 400, `{"error": "Invalid priority"}`.
    InvalidPriority,
    /// 400, intake-view guard.
    IntakeDisabled,
    /// 400, patch low-role non-author (`EDIT_DENIED_BODY`).
    EditDenied,
    /// 403, delete creator/admin guard (`DELETE_DENIED_BODY`).
    DeleteDenied,
    /// 400, triage-creation race (`IntegrityError` branch).
    PayloadInvalid,
    /// 415, unhandled content type with a non-empty body.
    UnsupportedMediaType(String),
    /// 500, generic branch.
    ServerError,
}

impl Denial {
    fn status_and_body(&self) -> (StatusCode, String) {
        match self {
            Denial::Unauthorized => (StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY.to_owned()),
            Denial::InvalidToken => (StatusCode::FORBIDDEN, INVALID_TOKEN_BODY.to_owned()),
            Denial::Forbidden => (StatusCode::FORBIDDEN, v1_perm::CLASS_DENIAL_BODY.to_owned()),
            Denial::ProjectNotFound => (StatusCode::NOT_FOUND, PROJECT_NOT_FOUND_BODY.to_owned()),
            Denial::ResourceMissing => (StatusCode::NOT_FOUND, RESOURCE_MISSING_BODY.to_owned()),
            Denial::PageNotFound => (StatusCode::NOT_FOUND, PAGE_NOT_FOUND_BODY.to_owned()),
            Denial::BadDetail(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"detail\":{}}}", json_string(message)),
            ),
            Denial::BadError(message) => (
                StatusCode::BAD_REQUEST,
                format!("{{\"error\":{}}}", json_string(message)),
            ),
            Denial::FieldErrors(body) => (
                StatusCode::BAD_REQUEST,
                serde_json::to_string(body).expect("error body serializes"),
            ),
            Denial::NameRequired => (StatusCode::BAD_REQUEST, NAME_REQUIRED_BODY.to_owned()),
            Denial::InvalidPriority => (StatusCode::BAD_REQUEST, INVALID_PRIORITY_BODY.to_owned()),
            Denial::IntakeDisabled => (StatusCode::BAD_REQUEST, INTAKE_DISABLED_BODY.to_owned()),
            Denial::EditDenied => (
                StatusCode::BAD_REQUEST,
                v1_perm::EDIT_DENIED_BODY.to_owned(),
            ),
            Denial::DeleteDenied => (
                StatusCode::FORBIDDEN,
                v1_perm::DELETE_DENIED_BODY.to_owned(),
            ),
            Denial::PayloadInvalid => (StatusCode::BAD_REQUEST, PAYLOAD_INVALID_BODY.to_owned()),
            Denial::UnsupportedMediaType(raw) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                format!(
                    "{{\"detail\":{}}}",
                    json_string(&format!("Unsupported media type \"{raw}\" in request."))
                ),
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

pub(crate) type HandlerResult = Result<Response, Denial>;

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serializes")
}

/// Map a paginator-kernel error to its HTTP fate: input errors are
/// `ParseError` 400s (`{"Detail": ...}`); arithmetic errors are the 500s
/// Python's `ZeroDivisionError` / lazy-queryset `ValueError` become.
fn page_denial(error: crate::paginator::PageError) -> Denial {
    use crate::paginator::PageError;
    match error {
        PageError::InvalidPerPage
        | PageError::PerPageTooLarge(_)
        | PageError::InvalidCursor
        | PageError::OffsetTooLarge
        | PageError::NegativeOffset => Denial::BadDetail(error.detail()),
        PageError::ZeroLimit
        | PageError::NegativeSlice
        | PageError::NonFiniteCursor
        | PageError::MissingOrderKey => Denial::ServerError,
    }
}

/// Map a sibling-cycle-module failure to its HTTP fate. The reused
/// render/expand/relations helpers only fail on database errors, which
/// Python surfaces as the generic 500 (a `DatabaseError` matches no
/// `handle_exception` branch).
fn cycle_denial(_error: cycle_mod::Denial) -> Denial {
    Denial::ServerError
}

// ---------------------------------------------------------------------------
// Shared request plumbing
// ---------------------------------------------------------------------------

/// Parse the request body the way DRF `_parse` selects a parser
/// (`PIDASHCONV-1158`). An empty body is `{}` whatever the content type
/// claims; otherwise the default parser order (`JSONParser`, `FormParser`,
/// `MultiPartParser` — `select_body_kind`) picks the parser by media type.
/// The JSON branch serves the exact `application/json` in any case plus the
/// `*/*`, `application/*` and `*/json` wildcards (`_MediaType.match`);
/// form/multipart bodies become a last-wins field map (`QueryDict`) carrying
/// the file parts, so the update view can replay the immutable-`QueryDict`
/// `.pop` 500 (`PIDASHCONV-1402`); anything else is 415. Malformed JSON keeps
/// the existing `ParseError` 400.
fn parse_json_body(
    body: &[u8],
    headers: &HeaderMap,
    memory_limit: i64,
) -> Result<form_body::ParsedBody, Denial> {
    // DRF's `_load_stream` leaves the stream `None` at content-length
    // zero and `_parse` returns the empty mapping without touching a
    // parser (`request.py`, DRF 3.15.2; same shape as the merged
    // precedents) — so emptiness is checked before the media type.
    if body.is_empty() {
        return Ok(form_body::ParsedBody::json(Value::Object(Map::new())));
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    match form_body::select_body_kind(content_type) {
        Some(BodyKind::Json) => serde_json::from_slice(body)
            .map(form_body::ParsedBody::json)
            .map_err(|error| Denial::BadDetail(format!("JSON parse error - {error}"))),
        // QueryDict semantics: last value wins (`parse_form_body`).
        Some(BodyKind::Form) => match form_body::parse_form_body(body) {
            Ok(pairs) => Ok(form_body::ParsedBody::form(pairs)),
            Err(_) => Err(Denial::ServerError),
        },
        Some(BodyKind::Multipart) => {
            match form_body::parse_multipart_body(content_type, body, memory_limit) {
                Ok(pairs) => Ok(form_body::ParsedBody::form(pairs)),
                Err(BodyFail::Parse(message)) => Err(Denial::BadDetail(message)),
                Err(BodyFail::Fatal) | Err(BodyFail::TooMany) => Err(Denial::ServerError),
            }
        }
        None => {
            let raw = if content_type.is_empty() {
                "text/plain"
            } else {
                content_type
            };
            Err(Denial::UnsupportedMediaType(raw.to_owned()))
        }
    }
}

/// True when the update view's `request.data.pop("issue", False)`
/// (`views/intake.py:344`) raises `AttributeError` on an immutable
/// `QueryDict` (`PIDASHCONV-1402`): DRF only copies the data (making it
/// mutable) when files are present (`request.py`, `_load_data_and_files`),
/// so a form/multipart body with no file parts pops immutable — and an
/// empty body with a form content type is `QueryDict('')` (`_parse`),
/// immutable too. Both are the generic 500. `intake_create` is unaffected
/// because `QueryDict.get` never mutates.
fn pops_immutable_querydict(
    parsed: &form_body::ParsedBody,
    body: &[u8],
    headers: &HeaderMap,
) -> bool {
    if parsed.is_form() {
        // File parts are the `_files` that trigger the mutable copy;
        // without one the `QueryDict` stays immutable.
        let has_files = parsed
            .keys()
            .iter()
            .any(|key| matches!(parsed.field(key), form_body::BodyField::File(_)));
        return !has_files;
    }
    if body.is_empty() {
        // `_parse`'s empty branch checks the exact form media types
        // (`is_form_media_type`), not the parser wildcards.
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let base = content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        return base == "application/x-www-form-urlencoded" || base == "multipart/form-data";
    }
    false
}

/// Django `QueryDict.get`: the last value, or `None`. Axum's
/// `Query<HashMap<String, String>>` keeps the last duplicate the same way,
/// and unlike the `Vec<String>` form it accepts a single `?key=value`.
fn query_last(params: &HashMap<String, String>, key: &str) -> Option<String> {
    params.get(key).cloned()
}

/// `?fields=` / `?expand=` (`api/views/base.py:199-208`): comma-split,
/// empties dropped, `None` when nothing survives.
fn fields_param(params: &HashMap<String, String>, key: &str) -> Option<Vec<String>> {
    let raw = query_last(params, key).unwrap_or_default();
    let kept: Vec<String> = raw
        .split(',')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    if kept.is_empty() {
        None
    } else {
        Some(kept)
    }
}

/// Rebuild the inbound request and proxy it to Django (non-canonical UUID
/// spellings: routing precedes auth, so Django's resolver 404 answers
/// before any 401 — the `v1_cli_auth` PIDASHCONV-1095 precedent).
async fn proxy_through(
    state: AppState,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .body(axum::body::Body::from(body))
        .expect("rebuild proxy request");
    *req.headers_mut() = headers;
    crate::edge::proxy(State(state), req).await
}

// ---------------------------------------------------------------------------
// Authentication (`api/middleware/api_authentication.py:20-84`)
// ---------------------------------------------------------------------------

/// The authenticated caller. Mirrors the sticky precedent: the
/// `X-Api-Key` header carries an `APIToken` or, when it starts with
/// `mt_`, a `MachineToken`. No usable credential is 401; a bad one is
/// 403 (see [`INVALID_TOKEN_BODY`]).
async fn authenticate(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    headers: &HeaderMap,
) -> Result<Uuid, Denial> {
    let Some(raw) = headers.get(token_kernel::API_KEY_HEADER) else {
        return Err(Denial::Unauthorized);
    };
    // Django decodes headers lossily to `str`: undecodable bytes are a
    // lookup miss (403), never a missing credential (401).
    let presented = raw.to_str().unwrap_or("\0");
    if presented.is_empty() {
        return Err(Denial::Unauthorized);
    }
    if presented.starts_with(token_kernel::MACHINE_TOKEN_PREFIX) {
        authenticate_machine(pool, secret_key, presented).await
    } else {
        authenticate_api(pool, presented).await
    }
}

/// `validate_api_token` (`api_authentication.py:30-43`): exact token match,
/// `is_active`, unexpired — then stamp `last_used`. The `deleted_at IS
/// NULL` conjunct is the `SoftDeletionManager` scope
/// (`db/mixins.py:56-66`): a soft-deleted token 403s.
async fn authenticate_api(pool: &sqlx::PgPool, presented: &str) -> Result<Uuid, Denial> {
    let now = Utc::now();
    let row: Option<(Uuid, Uuid, bool, Option<DateTime<Utc>>)> = sqlx::query_as(
        r#"SELECT id, user_id, is_active, expired_at FROM api_tokens WHERE token = $1 AND deleted_at IS NULL"#,
    )
    .bind(presented)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, user_id, is_active, expired_at)) = row else {
        return Err(Denial::InvalidToken);
    };
    if !is_active {
        return Err(Denial::InvalidToken);
    }
    if let Some(expires) = expired_at {
        if expires <= now {
            return Err(Denial::InvalidToken);
        }
    }
    pidash_services::domain::tokens::touch_api_token_last_used_by_id(pool, id, now)
        .await
        .map_err(|_| Denial::ServerError)?;
    // The token loggers stamp this id on the `process_logs` payload
    // (Django reads `request.user` post-view; PIDASHCONV-937).
    crate::middleware::publish_logger_user_id(&user_id.to_string());
    Ok(user_id)
}

/// `is_workspace_member` for machine tokens
/// (`core/permissions.py:28-34`): active row, any role, through the
/// default manager (soft-deleted rows do not count — the
/// `handlers_members` precedent carries the same conjunct).
/// Binds: `$1` workspace id, `$2` user id.
const MACHINE_MEMBER_SQL: &str = r#"SELECT EXISTS(SELECT 1 FROM workspace_members
           WHERE workspace_id = $1 AND member_id = $2 AND is_active AND deleted_at IS NULL)"#;

/// One `machine_token` + `dev_machine` lookup row: token id, user id,
/// workspace id, token revocation, dev-machine id, dev-machine
/// revocation, dev-machine id again (the join-hit proof).
type MachineTokenRow = (
    Uuid,
    Uuid,
    Uuid,
    Option<DateTime<Utc>>,
    Option<Uuid>,
    Option<DateTime<Utc>>,
    Option<Uuid>,
);

/// `validate_machine_token` (`api_authentication.py:45-63`): match on
/// `token_hash`, unrevoked, dev-machine unrevoked, workspace member (a
/// non-member's token is revoked, then rejected) — then stamp
/// `last_used_at`.
async fn authenticate_machine(
    pool: &sqlx::PgPool,
    secret_key: &[u8],
    presented: &str,
) -> Result<Uuid, Denial> {
    let now = Utc::now();
    let token_hash = token_kernel::hash_token(presented, secret_key);
    let row: Option<MachineTokenRow> = sqlx::query_as(
        r#"SELECT mt.id, mt.user_id, mt.workspace_id, mt.revoked_at,
                  mt.dev_machine_id, dm.revoked_at, dm.id
           FROM machine_token mt
           LEFT JOIN dev_machine dm ON dm.id = mt.dev_machine_id
           WHERE mt.token_hash = $1"#,
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id, user_id, workspace_id, revoked_at, dev_machine_id, dm_revoked, dm_found)) = row
    else {
        return Err(Denial::InvalidToken);
    };
    if revoked_at.is_some() {
        return Err(Denial::InvalidToken);
    }
    if let Some(machine_id) = dev_machine_id {
        if dm_found != Some(machine_id) {
            return Err(Denial::ServerError);
        }
        if dm_revoked.is_some() {
            return Err(Denial::InvalidToken);
        }
    }
    let member: Option<bool> = sqlx::query_scalar(MACHINE_MEMBER_SQL)
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    if !member.unwrap_or(false) {
        pidash_services::domain::tokens::revoke_machine_token_by_id(pool, id, now)
            .await
            .map_err(|_| Denial::ServerError)?;
        return Err(Denial::InvalidToken);
    }
    pidash_services::domain::tokens::touch_machine_token_last_used_by_id(pool, id, now)
        .await
        .map_err(|_| Denial::ServerError)?;
    // The token loggers stamp this id on the `process_logs` payload
    // (Django reads `request.user` post-view; PIDASHCONV-937).
    crate::middleware::publish_logger_user_id(&user_id.to_string());
    Ok(user_id)
}

// ---------------------------------------------------------------------------
// Permission gate (`ProjectLitePermission`, `intake.py:60,225`)
// ---------------------------------------------------------------------------

/// Resolve `project_id` exactly like `_rewrite_project_kwarg`
/// (`base.py:51-98`): UUID-looking input passes through unchecked (no
/// existence check — the view's own `Project.objects.get` 404s later);
/// other input resolves via `Project.resolve` and 404s `Project not
/// found` on a miss. Anonymous callers never reach here (auth runs
/// first).
async fn rewrite_project_id(
    pool: &sqlx::PgPool,
    slug: &str,
    actor_id: Uuid,
    raw: &str,
) -> Result<Uuid, Denial> {
    if let Ok(id) = raw.parse::<Uuid>() {
        return Ok(id);
    }
    let scope = QueryScope::new(slug, actor_id);
    queries_projmem::fetch_project_id(pool, &scope, raw)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::ProjectNotFound)
}

/// Enforce `ProjectLitePermission` for one `(user, slug, project_id)`:
/// anonymous was already 401'd, so any denial here is the DRF-default
/// 403. The membership row is fetched with the exact
/// `(workspace__slug, member, project_id, is_active)` filters Python
/// checks (see [`v1_perm::decide_intake_gate`]).
async fn require_project_member(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<(), Denial> {
    let role: Option<i16> = sqlx::query_scalar(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.member_id = $2 AND pm.project_id = $3
           AND pm.is_active AND pm.deleted_at IS NULL LIMIT 1"#,
    )
    .bind(slug)
    .bind(user_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let facts = ProjectFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        project_id: ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: false,
        has_workspace_admin_or_member: false,
        is_workspace_admin: false,
        is_project_member: role.is_some(),
        is_project_admin: false,
        has_project_admin_or_member: false,
        has_identifier_membership: false,
        has_project_identifier: false,
    };
    let scope = TenantScope::new(WorkspaceId::from(slug.to_owned()));
    if v1_perm::decide_intake_gate(&scope, &facts) {
        Ok(())
    } else {
        Err(Denial::Forbidden)
    }
}

/// The request's render zone (`TimezoneMixin.initial` runs after
/// authentication and permissions and calls
/// `zoneinfo.ZoneInfo(request.user.user_timezone)`). A missing user row
/// or a NULL zone is the 500 Python's `DoesNotExist`/`TypeError` path
/// becomes (NULL already 500s at decode); an unknown zone name 400s:
/// `zoneinfo.ZoneInfo` raises `ZoneInfoNotFoundError`, which subclasses
/// `KeyError` (`api/views/base.py:160-164`). An EMPTY zone 500s:
/// `ZoneInfo('')` raises `ValueError` (not `KeyError`), which falls
/// through to the generic 500 (`api/views/base.py:166-171`).
async fn request_timezone(pool: &sqlx::PgPool, user_id: &Uuid) -> Result<Tz, Denial> {
    let zone: Option<String> =
        sqlx::query_scalar(r#"SELECT user_timezone FROM users WHERE id = $1"#)
            .bind(user_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    match zone.as_deref() {
        None => Err(Denial::ServerError),
        Some("") => Err(Denial::ServerError),
        Some(zone) => zone
            .parse::<Tz>()
            .map_err(|_| Denial::BadError("The required key does not exist.".to_owned())),
    }
}

fn pool_of(state: &AppState) -> Result<sqlx::PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary().clone())
        .ok_or(Denial::ServerError)
}

/// Request preamble shared by all five actions: pool, acting user
/// (401/403), rewritten project id (404), `ProjectLitePermission`
/// (403), render zone.
struct Context {
    pool: sqlx::PgPool,
    user_id: Uuid,
    project_id: Uuid,
    timezone: Tz,
}

async fn context(
    state: &AppState,
    headers: &HeaderMap,
    slug: &str,
    raw_project_id: &str,
) -> Result<Context, Denial> {
    let pool = pool_of(state)?;
    let secret = state.settings().secret_key.as_bytes();
    let user_id = authenticate(&pool, secret, headers).await?;
    let project_id = rewrite_project_id(&pool, slug, user_id, raw_project_id).await?;
    require_project_member(&pool, slug, &project_id, &user_id).await?;
    let timezone = request_timezone(&pool, &user_id).await?;
    Ok(Context {
        pool,
        user_id,
        project_id,
        timezone,
    })
}

// ---------------------------------------------------------------------------
// Row reads
// ---------------------------------------------------------------------------

/// Physical positions inside `SELECT "intake_issues".*` (from
/// `information_schema`, `pidash_426`): `*` expands in physical
/// (migration) order, NOT `_meta` order. Existing positions are stable
/// under migrations (`ADD COLUMN` appends; `DROP COLUMN` leaves its
/// `attnum` gap), and any skew fails loudly in the contract suite.
const II_CREATED_AT: usize = 0;
const II_UPDATED_AT: usize = 1;
const II_ID: usize = 2;
const II_STATUS: usize = 3;
const II_SNOOZED_TILL: usize = 4;
const II_SOURCE: usize = 5;
const II_CREATED_BY: usize = 6;
const II_DUPLICATE_TO: usize = 7;
const II_INTAKE: usize = 8;
const II_ISSUE: usize = 9;
const II_PROJECT: usize = 10;
const II_UPDATED_BY: usize = 11;
const II_WORKSPACE: usize = 12;
const II_EXTERNAL_ID: usize = 13;
const II_EXTERNAL_SOURCE: usize = 14;
const II_DELETED_AT: usize = 15;
const II_EXTRA: usize = 16;
const II_SOURCE_EMAIL: usize = 17;
/// Column count of `intake_issues.*`: the `issues.*` block starts here in
/// the joined list/detail rows.
const II_WIDTH: usize = 18;

/// Physical positions inside `SELECT "issues".*` (same source).
const IS_CREATED_AT: usize = 0;
const IS_UPDATED_AT: usize = 1;
const IS_ID: usize = 2;
const IS_NAME: usize = 3;
const IS_DESCRIPTION_JSON: usize = 4;
const IS_PRIORITY: usize = 5;
const IS_START_DATE: usize = 6;
const IS_TARGET_DATE: usize = 7;
const IS_SEQUENCE_ID: usize = 8;
const IS_CREATED_BY: usize = 9;
const IS_PARENT: usize = 10;
const IS_PROJECT: usize = 11;
const IS_STATE: usize = 12;
const IS_UPDATED_BY: usize = 13;
const IS_WORKSPACE: usize = 14;
const IS_DESCRIPTION_HTML: usize = 15;
const IS_DESCRIPTION_STRIPPED: usize = 16;
const IS_COMPLETED_AT: usize = 17;
const IS_SORT_ORDER: usize = 18;
const IS_POINT: usize = 19;
const IS_ARCHIVED_AT: usize = 20;
const IS_IS_DRAFT: usize = 21;
const IS_EXTERNAL_ID: usize = 22;
const IS_EXTERNAL_SOURCE: usize = 23;
const IS_DESCRIPTION_BINARY: usize = 24;
const IS_ESTIMATE_POINT: usize = 25;
const IS_TYPE: usize = 26;
const IS_DELETED_AT: usize = 27;
const IS_GIT_WORK_BRANCH: usize = 28;
const IS_ASSIGNED_POD: usize = 29;
const IS_CREATED_VIA: usize = 31;
const IS_AGENT_EXECUTOR: usize = 32;
const IS_COMPLEXITY_SCORE: usize = 33;

/// One `intake_issues` row.
#[derive(Debug, Clone)]
struct IntakeIssueRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    project_id: Uuid,
    workspace_id: Uuid,
    intake_id: Uuid,
    issue_id: Uuid,
    status: i32,
    snoozed_till: Option<DateTime<Utc>>,
    duplicate_to_id: Option<Uuid>,
    source: Option<String>,
    source_email: Option<String>,
    external_source: Option<String>,
    external_id: Option<String>,
    extra: Value,
}

/// One `issues` row (the subset the expand/full shapes read).
#[derive(Debug, Clone)]
struct IssueRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    point: Option<i32>,
    name: String,
    description_json: Value,
    description_html: String,
    description_stripped: Option<String>,
    description_binary: Option<Vec<u8>>,
    priority: Option<String>,
    complexity_score: Option<i32>,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i32,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    archived_at: Option<NaiveDate>,
    is_draft: bool,
    external_source: Option<String>,
    external_id: Option<String>,
    git_work_branch: String,
    created_via: Option<String>,
    agent_executor: Option<String>,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    project_id: Uuid,
    workspace_id: Uuid,
    parent_id: Option<Uuid>,
    state_id: Option<Uuid>,
    estimate_point_id: Option<Uuid>,
    type_id: Option<Uuid>,
    assigned_pod_id: Option<Uuid>,
}

fn cell<T>(row: &sqlx::postgres::PgRow, index: usize) -> Result<T, Denial>
where
    T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(index).map_err(|_| Denial::ServerError)
}

impl IntakeIssueRow {
    /// Decode the leading `intake_issues.*` block of a joined row.
    /// Name-based decoding is impossible here: sqlx resolves duplicate
    /// column names to the LAST table, so `"id"` would read
    /// `projects.id` (verified in sqlx-postgres 0.8.6
    /// `connection/describe.rs`: later `insert`s overwrite).
    fn from_joined(row: &sqlx::postgres::PgRow) -> Result<Self, Denial> {
        Ok(Self {
            created_at: cell(row, II_CREATED_AT)?,
            updated_at: cell(row, II_UPDATED_AT)?,
            id: cell(row, II_ID)?,
            status: cell(row, II_STATUS)?,
            snoozed_till: cell(row, II_SNOOZED_TILL)?,
            source: cell(row, II_SOURCE)?,
            created_by_id: cell(row, II_CREATED_BY)?,
            duplicate_to_id: cell(row, II_DUPLICATE_TO)?,
            intake_id: cell(row, II_INTAKE)?,
            issue_id: cell(row, II_ISSUE)?,
            project_id: cell(row, II_PROJECT)?,
            updated_by_id: cell(row, II_UPDATED_BY)?,
            workspace_id: cell(row, II_WORKSPACE)?,
            external_id: cell(row, II_EXTERNAL_ID)?,
            external_source: cell(row, II_EXTERNAL_SOURCE)?,
            deleted_at: cell(row, II_DELETED_AT)?,
            extra: cell(row, II_EXTRA)?,
            source_email: cell(row, II_SOURCE_EMAIL)?,
        })
    }

    /// Decode a standalone `intake_issues` row selected with the
    /// explicit [`INTAKE_ISSUE_COLS`] list (same field order as the
    /// physical block above, so one decoder serves both).
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, Denial> {
        Self::from_joined(row)
    }
}

/// Explicit column list in physical-block order for standalone
/// `intake_issues` reads (avoids `SELECT *` while keeping one decoder).
const INTAKE_ISSUE_COLS: &str = "ii.created_at, ii.updated_at, ii.id, ii.status, ii.snoozed_till, ii.source, \
     ii.created_by_id, ii.duplicate_to_id, ii.intake_id, ii.issue_id, ii.project_id, ii.updated_by_id, \
     ii.workspace_id, ii.external_id, ii.external_source, ii.deleted_at, ii.extra, ii.source_email";

impl IssueRow {
    /// Decode the `issues.*` block of a joined row (offset [`II_WIDTH`]).
    fn from_joined(row: &sqlx::postgres::PgRow) -> Result<Self, Denial> {
        Self::from_block(row, II_WIDTH)
    }

    /// Decode a standalone `issues` row selected with [`ISSUE_COLS`].
    fn from_row(row: &sqlx::postgres::PgRow) -> Result<Self, Denial> {
        Self::from_block(row, 0)
    }

    fn from_block(row: &sqlx::postgres::PgRow, base: usize) -> Result<Self, Denial> {
        Ok(Self {
            created_at: cell(row, base + IS_CREATED_AT)?,
            updated_at: cell(row, base + IS_UPDATED_AT)?,
            id: cell(row, base + IS_ID)?,
            name: cell(row, base + IS_NAME)?,
            description_json: cell(row, base + IS_DESCRIPTION_JSON)?,
            priority: cell(row, base + IS_PRIORITY)?,
            start_date: cell(row, base + IS_START_DATE)?,
            target_date: cell(row, base + IS_TARGET_DATE)?,
            sequence_id: cell(row, base + IS_SEQUENCE_ID)?,
            created_by_id: cell(row, base + IS_CREATED_BY)?,
            parent_id: cell(row, base + IS_PARENT)?,
            project_id: cell(row, base + IS_PROJECT)?,
            state_id: cell(row, base + IS_STATE)?,
            updated_by_id: cell(row, base + IS_UPDATED_BY)?,
            workspace_id: cell(row, base + IS_WORKSPACE)?,
            description_html: cell(row, base + IS_DESCRIPTION_HTML)?,
            description_stripped: cell(row, base + IS_DESCRIPTION_STRIPPED)?,
            completed_at: cell(row, base + IS_COMPLETED_AT)?,
            sort_order: cell(row, base + IS_SORT_ORDER)?,
            point: cell(row, base + IS_POINT)?,
            archived_at: cell(row, base + IS_ARCHIVED_AT)?,
            is_draft: cell(row, base + IS_IS_DRAFT)?,
            external_id: cell(row, base + IS_EXTERNAL_ID)?,
            external_source: cell(row, base + IS_EXTERNAL_SOURCE)?,
            description_binary: cell(row, base + IS_DESCRIPTION_BINARY)?,
            estimate_point_id: cell(row, base + IS_ESTIMATE_POINT)?,
            type_id: cell(row, base + IS_TYPE)?,
            deleted_at: cell(row, base + IS_DELETED_AT)?,
            git_work_branch: cell(row, base + IS_GIT_WORK_BRANCH)?,
            assigned_pod_id: cell(row, base + IS_ASSIGNED_POD)?,
            created_via: cell(row, base + IS_CREATED_VIA)?,
            agent_executor: cell(row, base + IS_AGENT_EXECUTOR)?,
            complexity_score: cell(row, base + IS_COMPLEXITY_SCORE)?,
        })
    }

    /// Adapt to the merged full-`IssueSerializer` renderer input.
    fn to_cycle_detail(&self) -> cycle_mod::IssueDetail {
        cycle_mod::IssueDetail {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            point: self.point,
            name: self.name.clone(),
            description_html: self.description_html.clone(),
            description_binary: self.description_binary.clone(),
            priority: self.priority.clone(),
            complexity_score: self.complexity_score,
            start_date: self.start_date,
            target_date: self.target_date,
            sequence_id: self.sequence_id,
            sort_order: self.sort_order,
            completed_at: self.completed_at,
            archived_at: self.archived_at,
            is_draft: self.is_draft,
            external_source: self.external_source.clone(),
            external_id: self.external_id.clone(),
            git_work_branch: self.git_work_branch.clone(),
            created_via: self.created_via.clone(),
            agent_executor: self.agent_executor.clone(),
            created_by: self.created_by_id,
            updated_by: self.updated_by_id,
            project_id: self.project_id,
            workspace_id: self.workspace_id,
            parent_id: self.parent_id,
            state_id: self.state_id,
            estimate_point_id: self.estimate_point_id,
            type_id: self.type_id,
            assigned_pod_id: self.assigned_pod_id,
        }
    }
}

/// Explicit column list in physical-block order for standalone `issues`
/// reads (the render subset; `workpad` excluded — never rendered here).
const ISSUE_COLS: &str = "created_at, updated_at, id, name, description_json, priority, \
     start_date, target_date, sequence_id, created_by_id, parent_id, project_id, state_id, \
     updated_by_id, workspace_id, description_html, description_stripped, completed_at, \
     sort_order, point, archived_at, is_draft, external_id, external_source, description_binary, \
     estimate_point_id, type_id, deleted_at, git_work_branch, assigned_pod_id, workpad, \
     created_via, agent_executor, complexity_score";

/// [`ISSUE_COLS`] with every column qualified by the `issues` table — for
/// reads that join another table (e.g. `workspaces`), where the bare column
/// names are ambiguous.
fn issue_cols_qualified() -> String {
    ISSUE_COLS
        .split(", ")
        .map(|col| format!("issues.{col}"))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Renders (`IntakeIssueSerializer` + `IssueExpandSerializer` read shapes)
// ---------------------------------------------------------------------------

fn opt_uuid(value: &Option<Uuid>) -> Value {
    value.map_or(Value::Null, |id| Value::String(id.to_string()))
}

fn opt_string(value: &Option<String>) -> Value {
    value.clone().map_or(Value::Null, Value::String)
}

fn render_datetime_opt(value: &Option<DateTime<Utc>>, timezone: &Tz) -> Value {
    value.map_or(Value::Null, |dt| {
        Value::String(crate::serializer::render_datetime_in(&dt, timezone))
    })
}

fn render_date_opt(value: &Option<NaiveDate>) -> Value {
    value.map_or(Value::Null, |date| Value::String(date.to_string()))
}

/// `description_binary` renders through DRF's `ModelField`
/// (`BinaryField.value_to_string`): standard base64 ASCII — verified
/// live against Django (200 with `AQID...`, never a 500). `None`
/// renders null.
fn render_binary(value: &Option<Vec<u8>>) -> Value {
    value.as_ref().map_or(Value::Null, |bytes| {
        use base64::Engine as _;
        Value::String(base64::engine::general_purpose::STANDARD.encode(bytes))
    })
}

/// `sort_order` as a JSON number. Non-finite values (only storable via
/// an explicit `nan`/`inf` patch — `float()` accepts them) render as
/// DRF `json.dumps` emits them: bare `NaN`/`Infinity`/`-Infinity`
/// tokens, which `serde_json` cannot produce — so the final row
/// strings pass through [`fix_nonfinite_sort`] (the key appears only
/// for the row's own issue, so the replacement is exact).
fn render_sort(value: f64) -> Value {
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

/// Replace the `"sort_order":null` placeholder with the DRF token when
/// the row's own sort is non-finite. Both nested issue objects (if any)
/// render the same row, so a global replacement stays exact.
fn fix_nonfinite_sort(rendered: &str, sort_order: f64) -> String {
    if sort_order.is_finite() {
        return rendered.to_owned();
    }
    let token = if sort_order.is_nan() {
        "NaN"
    } else if sort_order.is_sign_positive() {
        "Infinity"
    } else {
        "-Infinity"
    };
    rendered.replace("\"sort_order\":null", &format!("\"sort_order\":{token}"))
}

/// `label_issue` / `issue_assignee` id lists (`Meta.ordering =
/// ("-created_at",)` on both through tables; soft-deleted links
/// excluded by the default managers).
async fn issue_label_ids(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<Vec<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT label_id FROM issue_labels WHERE issue_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

async fn issue_assignee_ids(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<Vec<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT assignee_id FROM issue_assignees WHERE issue_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// Nested `StateLiteSerializer` (`id`, `name`, `color`, `group`): a null
/// FK — or a dangling one (DRF's `get_attribute` swallows
/// `ObjectDoesNotExist`) — renders null.
async fn render_state_lite(pool: &sqlx::PgPool, state_id: &Option<Uuid>) -> Result<Value, Denial> {
    let Some(id) = state_id else {
        return Ok(Value::Null);
    };
    let row: Option<sqlx::postgres::PgRow> =
        sqlx::query(r#"SELECT "id", "name", "color", "group" FROM "states" WHERE "id" = $1"#)
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(Value::Null);
    };
    let id: Uuid = row.try_get("id").map_err(|_| Denial::ServerError)?;
    let name: String = row.try_get("name").map_err(|_| Denial::ServerError)?;
    let color: String = row.try_get("color").map_err(|_| Denial::ServerError)?;
    let group: String = row.try_get("group").map_err(|_| Denial::ServerError)?;
    let mut map = Map::with_capacity(4);
    map.insert("id".to_owned(), Value::String(id.to_string()));
    map.insert("name".to_owned(), Value::String(name));
    map.insert("color".to_owned(), Value::String(color));
    map.insert("group".to_owned(), Value::String(group));
    Ok(Value::Object(map))
}

/// `IssueExpandSerializer` read shape
/// (`api/serializers/issue.py:1074-1116`): the 36 keys in live DRF field
/// order. `cycle`/`module` are structurally absent (reverse-FK
/// managers raise `AttributeError` on `.cycle`/`.module`, so DRF
/// `SkipField`s both keys — always, even when bridge rows exist).
/// `labels`/`assignees` are id lists (the nested context carries no
/// `expand`, so the method fields take the id-list branch).
async fn render_issue_expand(
    pool: &sqlx::PgPool,
    issue: &IssueRow,
    timezone: &Tz,
) -> Result<Value, Denial> {
    let labels = issue_label_ids(pool, &issue.id).await?;
    let assignees = issue_assignee_ids(pool, &issue.id).await?;
    let mut map = Map::with_capacity(36);
    map.insert("id".to_owned(), Value::String(issue.id.to_string()));
    map.insert(
        "labels".to_owned(),
        Value::Array(
            labels
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        ),
    );
    map.insert(
        "assignees".to_owned(),
        Value::Array(
            assignees
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        ),
    );
    map.insert(
        "state".to_owned(),
        render_state_lite(pool, &issue.state_id).await?,
    );
    map.insert("description".to_owned(), issue.description_json.clone());
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &issue.created_at,
            timezone,
        )),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &issue.updated_at,
            timezone,
        )),
    );
    map.insert(
        "deleted_at".to_owned(),
        render_datetime_opt(&issue.deleted_at, timezone),
    );
    map.insert(
        "point".to_owned(),
        issue.point.map_or(Value::Null, Value::from),
    );
    map.insert("name".to_owned(), Value::String(issue.name.clone()));
    map.insert(
        "description_json".to_owned(),
        issue.description_json.clone(),
    );
    map.insert(
        "description_html".to_owned(),
        Value::String(issue.description_html.clone()),
    );
    map.insert(
        "description_stripped".to_owned(),
        opt_string(&issue.description_stripped),
    );
    map.insert(
        "description_binary".to_owned(),
        render_binary(&issue.description_binary),
    );
    map.insert("priority".to_owned(), opt_string(&issue.priority));
    map.insert(
        "complexity_score".to_owned(),
        issue.complexity_score.map_or(Value::Null, Value::from),
    );
    map.insert("start_date".to_owned(), render_date_opt(&issue.start_date));
    map.insert(
        "target_date".to_owned(),
        render_date_opt(&issue.target_date),
    );
    map.insert("sequence_id".to_owned(), Value::from(issue.sequence_id));
    map.insert("sort_order".to_owned(), render_sort(issue.sort_order));
    map.insert(
        "completed_at".to_owned(),
        render_datetime_opt(&issue.completed_at, timezone),
    );
    map.insert(
        "archived_at".to_owned(),
        render_date_opt(&issue.archived_at),
    );
    map.insert("is_draft".to_owned(), Value::Bool(issue.is_draft));
    map.insert(
        "external_source".to_owned(),
        opt_string(&issue.external_source),
    );
    map.insert("external_id".to_owned(), opt_string(&issue.external_id));
    map.insert(
        "git_work_branch".to_owned(),
        Value::String(issue.git_work_branch.clone()),
    );
    map.insert("created_via".to_owned(), opt_string(&issue.created_via));
    map.insert(
        "agent_executor".to_owned(),
        opt_string(&issue.agent_executor),
    );
    map.insert("created_by".to_owned(), opt_uuid(&issue.created_by_id));
    map.insert("updated_by".to_owned(), opt_uuid(&issue.updated_by_id));
    map.insert(
        "project".to_owned(),
        Value::String(issue.project_id.to_string()),
    );
    map.insert(
        "workspace".to_owned(),
        Value::String(issue.workspace_id.to_string()),
    );
    map.insert("parent".to_owned(), opt_uuid(&issue.parent_id));
    map.insert(
        "estimate_point".to_owned(),
        opt_uuid(&issue.estimate_point_id),
    );
    map.insert("type".to_owned(), opt_uuid(&issue.type_id));
    map.insert("assigned_pod".to_owned(), opt_uuid(&issue.assigned_pod_id));
    Ok(Value::Object(map))
}

/// Full single `IssueSerializer` read shape: the merged renderer plus
/// the blocker keys `to_representation` appends for single-item
/// payloads (`serializers/issue.py:460-481`; skipped only under
/// `many=True`). No requested-fields gating and no relations viewer on
/// this endpoint, so `relations` stays omitted.
async fn render_issue_full(
    state: &AppState,
    pool: &sqlx::PgPool,
    slug: &str,
    issue: &IssueRow,
    timezone: &Tz,
) -> Result<Value, Denial> {
    let detail = issue.to_cycle_detail();
    let labels = issue_label_ids(pool, &issue.id)
        .await
        .map_err(|_| Denial::ServerError)?;
    let assignees = issue_assignee_ids(pool, &issue.id)
        .await
        .map_err(|_| Denial::ServerError)?;
    let identifier: Option<String> =
        sqlx::query_scalar(r#"SELECT "identifier" FROM "projects" WHERE "id" = $1"#)
            .bind(issue.project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    // `get_url` dereferences `obj.project` unguarded: a dangling
    // `project_id` is the 500 Python's `DoesNotExist`-outside-`getattr`
    // shape becomes. (`workspace_id`/`sequence_id` cannot dangle the
    // same way: the slug comes from the URL and the sequence is a
    // non-null column.)
    let identifier = identifier.ok_or(Denial::ServerError)?;
    let url = cycle_mod::issue_url(state, slug, &identifier, issue.sequence_id);
    let mut rendered = cycle_mod::render_issue(
        pool, &detail, &assignees, &labels, url, timezone, None, None,
    )
    .await
    .map_err(cycle_denial)?;
    if let Value::Object(ref mut map) = rendered {
        map.insert(
            "relations_summary".to_owned(),
            cycle_mod::fetch_relations_summary(pool, &issue.id)
                .await
                .map_err(cycle_denial)?,
        );
        map.insert(
            "has_open_blockers".to_owned(),
            Value::Bool(
                cycle_mod::has_open_blockers(pool, &issue.id)
                    .await
                    .map_err(cycle_denial)?,
            ),
        );
    }
    Ok(rendered)
}

/// `IntakeIssueSerializer` read shape
/// (`api/serializers/intake.py:57-82`, `fields = "__all__"`): declared
/// `id`/`issue_detail`/`inbox` first, then the concrete columns in
/// `_meta` order, then the forward FKs in `_meta` order — the live DRF
/// order, probed (it differs from the `types` layer's
/// `IntakeIssueView`, which this renderer deliberately does not use).
/// `inbox` is the parent `Intake` row id. `fields`/`expand` narrow and
/// expand exactly like `BaseSerializer` (`serializers/base.py:19-118`).
#[allow(clippy::too_many_arguments)]
async fn render_intake_issue(
    state: &AppState,
    pool: &sqlx::PgPool,
    slug: &str,
    row: &IntakeIssueRow,
    issue: &IssueRow,
    timezone: &Tz,
    fields: Option<&[String]>,
    expand: Option<&[String]>,
) -> Result<Value, Denial> {
    let mut map = Map::with_capacity(19);
    map.insert("id".to_owned(), Value::String(row.id.to_string()));
    map.insert(
        "issue_detail".to_owned(),
        render_issue_expand(pool, issue, timezone).await?,
    );
    map.insert("inbox".to_owned(), Value::String(row.intake_id.to_string()));
    map.insert(
        "created_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &row.created_at,
            timezone,
        )),
    );
    map.insert(
        "updated_at".to_owned(),
        Value::String(crate::serializer::render_datetime_in(
            &row.updated_at,
            timezone,
        )),
    );
    map.insert(
        "deleted_at".to_owned(),
        render_datetime_opt(&row.deleted_at, timezone),
    );
    map.insert("status".to_owned(), Value::from(row.status));
    map.insert(
        "snoozed_till".to_owned(),
        render_datetime_opt(&row.snoozed_till, timezone),
    );
    map.insert("source".to_owned(), opt_string(&row.source));
    map.insert("source_email".to_owned(), opt_string(&row.source_email));
    map.insert(
        "external_source".to_owned(),
        opt_string(&row.external_source),
    );
    map.insert("external_id".to_owned(), opt_string(&row.external_id));
    map.insert("extra".to_owned(), row.extra.clone());
    map.insert("created_by".to_owned(), opt_uuid(&row.created_by_id));
    map.insert("updated_by".to_owned(), opt_uuid(&row.updated_by_id));
    map.insert(
        "project".to_owned(),
        Value::String(row.project_id.to_string()),
    );
    map.insert(
        "workspace".to_owned(),
        Value::String(row.workspace_id.to_string()),
    );
    map.insert(
        "intake".to_owned(),
        Value::String(row.intake_id.to_string()),
    );
    map.insert("issue".to_owned(), Value::String(row.issue_id.to_string()));
    map.insert("duplicate_to".to_owned(), opt_uuid(&row.duplicate_to_id));
    if let Some(fields) = fields {
        // `_filter_fields`: unknown names match nothing and every
        // existing-but-unasked key is popped — `?fields=bogus` yields
        // `{}`.
        map.retain(|key, _| fields.iter().any(|name| name == key));
    }
    if let Some(expand) = expand {
        apply_intake_expand(state, pool, slug, row, issue, timezone, &mut map, expand).await?;
    }
    Ok(Value::Object(map))
}

/// `BaseSerializer.to_representation`'s expand loop for the intake
/// shape: only names surviving the fields filter fire; map hits render
/// the Lite (or full `IssueSerializer`) shapes; every other name falls
/// through to `getattr(instance, f"{name}_id", None)` — `intake` and
/// `duplicate_to` re-render their unchanged id, everything else nulls
/// the key (including `id`, `status` and `issue_detail`).
#[allow(clippy::too_many_arguments)]
async fn apply_intake_expand(
    state: &AppState,
    pool: &sqlx::PgPool,
    slug: &str,
    row: &IntakeIssueRow,
    issue: &IssueRow,
    timezone: &Tz,
    map: &mut Map<String, Value>,
    expand: &[String],
) -> Result<(), Denial> {
    for name in expand {
        if !map.contains_key(name.as_str()) {
            continue;
        }
        match name.as_str() {
            "issue" => {
                let full = render_issue_full(state, pool, slug, issue, timezone).await?;
                map.insert("issue".to_owned(), full);
            }
            "workspace" => {
                let lite = cycle_mod::expand_workspace(pool, &row.workspace_id)
                    .await
                    .map_err(cycle_denial)?;
                map.insert("workspace".to_owned(), lite);
            }
            "project" => {
                let lite = cycle_mod::expand_project(pool, &row.project_id)
                    .await
                    .map_err(cycle_denial)?;
                map.insert("project".to_owned(), lite);
            }
            "created_by" => {
                let lite = cycle_mod::expand_user_opt(pool, &row.created_by_id)
                    .await
                    .map_err(cycle_denial)?;
                map.insert("created_by".to_owned(), lite);
            }
            "updated_by" => {
                let lite = cycle_mod::expand_user_opt(pool, &row.updated_by_id)
                    .await
                    .map_err(cycle_denial)?;
                map.insert("updated_by".to_owned(), lite);
            }
            "intake" | "duplicate_to" => {
                // Fallthrough `getattr(instance, "{name}_id")`: the
                // column exists, so the id re-renders unchanged.
            }
            _ => {
                map.insert(name.clone(), Value::Null);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// DRF write primitives (probed against DRF 3.15.2 + Django 4.2)
// ---------------------------------------------------------------------------

/// A field-level failure: a 400 message for the errors dict, or a 500
/// (values that pass field validation but cannot be stored — the
/// `DataError`-outside-`handle_exception` shape).
#[derive(Debug, Clone, PartialEq)]
enum FieldFail {
    Msg(String),
    Server,
}

type FieldResult<T> = Result<T, FieldFail>;

fn fail_msg(message: impl Into<String>) -> FieldFail {
    FieldFail::Msg(message.into())
}

/// Python `str()` over a JSON value (choice/UUID failure echoes,
/// `int()`/`float()` inputs). Dicts keep insertion order (`preserve_order`
/// is on, matching `json.loads`).
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => {
            if let Some(i) = number.as_i64() {
                i.to_string()
            } else if let Some(u) = number.as_u64() {
                u.to_string()
            } else {
                py_float_str(number.as_f64().unwrap_or(f64::NAN))
            }
        }
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(key, val)| format!("{}: {}", py_repr_str(key), py_repr(val)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// Python `repr()` over a JSON value (container element echoes).
fn py_repr(value: &Value) -> String {
    match value {
        Value::String(text) => py_repr_str(text),
        _ => py_str(value),
    }
}

/// Python `repr()` of a string: single quotes unless the text contains
/// a single quote but no double quote; backslashes, the quote char and
/// controls escaped.
fn py_repr_str(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            _ if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\x{:02x}", ch as u32));
            }
            _ if (ch as u32) == 0x7f => out.push_str("\\x7f"),
            _ => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a float: shortest round-trip with `.0` for
/// integral values and a signed, 2-digit-minimum exponent.
fn py_float_str(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    let debug = format!("{value:?}");
    let Some(pos) = debug.find('e') else {
        return debug;
    };
    let (mantissa, exp) = debug.split_at(pos);
    let exp = &exp[1..];
    let (sign, digits) = match exp.strip_prefix(['+', '-']) {
        Some(rest) if exp.starts_with('-') => ("-", rest),
        Some(rest) => ("+", rest),
        None => ("+", exp),
    };
    let mut digits = digits.to_owned();
    while digits.len() < 2 {
        digits.insert(0, '0');
    }
    format!("{mantissa}e{sign}{digits}")
}

/// `CharField.to_internal_value` (numbers coerce via `str()`; bools and
/// containers fail), plus blank/max-length. `trim_ws` is DRF's
/// `trim_whitespace` (default true; the stored value is trimmed).
fn check_char(
    value: &Value,
    allow_blank: bool,
    max_length: Option<usize>,
    trim_ws: bool,
) -> FieldResult<String> {
    let mut text = match value {
        Value::Bool(_) | Value::Array(_) | Value::Object(_) | Value::Null => {
            return Err(fail_msg("Not a valid string."));
        }
        Value::String(text) => text.clone(),
        Value::Number(_) => py_str(value),
    };
    if trim_ws {
        text = text.trim().to_owned();
    }
    if !allow_blank && text.is_empty() {
        return Err(fail_msg("This field may not be blank."));
    }
    if let Some(max) = max_length {
        if text.chars().count() > max {
            return Err(fail_msg(format!(
                "Ensure this field has no more than {max} characters."
            )));
        }
    }
    Ok(text)
}

/// `int(re.sub(r'\.0*\s*$', '', str(data)))` with the 1000-char guard.
/// Python ints are unbounded, so the value parses to `i128`; the `i32`
/// column range check is the caller's 500 (`DataError` matches no
/// `handle_exception` branch).
fn parse_drf_int(value: &Value) -> FieldResult<i128> {
    if let Value::String(text) = value {
        if text.len() > 1000 {
            return Err(fail_msg("String value too large."));
        }
    }
    // `re.sub(r'\.0*\s*$', '', s)`: strip trailing whitespace, then
    // trailing zeros, then one trailing dot — only when the dot is
    // there (otherwise no substitution at all).
    let raw = py_str(value);
    let stripped = raw.trim_end().trim_end_matches('0');
    let candidate = stripped.strip_suffix('.').unwrap_or(raw.as_str());
    parse_python_int(candidate).ok_or_else(|| fail_msg("A valid integer is required."))
}

/// Python `int(s)`: surrounding whitespace, optional sign, digits with
/// single underscores between digits only (ASCII fast path; other
/// decimal digits via `to_digit`).
fn parse_python_int(text: &str) -> Option<i128> {
    let text = text.trim();
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() {
        return None;
    }
    let mut value: i128 = 0;
    let mut prev_underscore = true; // leading '_' forbidden
    let mut any = false;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        let digit = ch.to_digit(10)?;
        prev_underscore = false;
        any = true;
        value = value.checked_mul(10)?.checked_add(digit as i128)?;
    }
    if prev_underscore || !any {
        // Trailing '_' (or empty) is forbidden.
        return None;
    }
    if text.starts_with('-') {
        value = value.checked_neg()?;
    }
    Some(value)
}

/// Range-narrow a validated DRF int to the `integer` column: outside
/// `i32` the INSERT dies with `DataError` → generic 500.
fn int_column(value: i128) -> Result<i32, Denial> {
    i32::try_from(value).map_err(|_| Denial::ServerError)
}

/// `FloatField.to_internal_value` with the 1000-char guard. `float()`
/// accepts `nan`/`inf`/`infinity` (any case, signed), trims whitespace
/// and allows single underscores between digits; huge exponents saturate
/// to infinity rather than failing.
fn parse_drf_float(value: &Value) -> FieldResult<f64> {
    if let Value::String(text) = value {
        if text.len() > 1000 {
            return Err(fail_msg("String value too large."));
        }
    }
    match value {
        Value::Null | Value::Array(_) | Value::Object(_) => {
            Err(fail_msg("A valid number is required."))
        }
        Value::Bool(true) => Ok(1.0),
        Value::Bool(false) => Ok(0.0),
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| fail_msg("A valid number is required.")),
        Value::String(text) => {
            parse_python_float(text).ok_or_else(|| fail_msg("A valid number is required."))
        }
    }
}

fn parse_python_float(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lowered = text.to_ascii_lowercase();
    let (sign, rest) = match lowered.strip_prefix(['+', '-']) {
        Some(rest) if lowered.starts_with('-') => (-1.0f64, rest),
        Some(rest) => (1.0f64, rest),
        None => (1.0f64, lowered.as_str()),
    };
    if rest == "nan" {
        return Some(f64::NAN.copysign(sign));
    }
    if rest == "inf" || rest == "infinity" {
        return Some(f64::INFINITY.copysign(sign));
    }
    // Validate the float grammar (underscores only between digits),
    // then let Rust parse the cleaned text.
    let mut cleaned = String::with_capacity(text.len());
    let mut prev_underscore = true;
    let mut any_digit = false;
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    if chars[index] == '+' || chars[index] == '-' {
        cleaned.push(chars[index]);
        index += 1;
    }
    let mut seen_dot = false;
    let mut seen_exp = false;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            index += 1;
            continue;
        }
        if ch.is_ascii_digit() {
            prev_underscore = false;
            any_digit = true;
            cleaned.push(ch);
            index += 1;
            continue;
        }
        if (ch == '.' && !seen_dot && !seen_exp)
            || ((ch == 'e' || ch == 'E') && !seen_exp && any_digit)
        {
            if ch == '.' {
                seen_dot = true;
            } else {
                seen_exp = true;
            }
            prev_underscore = true; // '.'/'e' need a digit after them...
                                    // ...except the exponent sign, handled below.
            cleaned.push(ch);
            index += 1;
            if (ch == 'e' || ch == 'E')
                && index < chars.len()
                && (chars[index] == '+' || chars[index] == '-')
            {
                cleaned.push(chars[index]);
                index += 1;
            }
            continue;
        }
        return None;
    }
    if prev_underscore || !any_digit {
        return None;
    }
    cleaned.parse::<f64>().ok()
}

/// `BooleanField.to_internal_value`: case-insensitive `TRUE_VALUES` /
/// `FALSE_VALUES` membership; `1`/`1.0` and `0`/`0.0` coerce by numeric
/// equality; anything else fails.
fn parse_drf_bool(value: &Value) -> FieldResult<bool> {
    const TRUE_STRINGS: [&str; 6] = ["t", "y", "yes", "true", "on", "1"];
    const FALSE_STRINGS: [&str; 6] = ["f", "n", "no", "false", "off", "0"];
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::String(text) => {
            let lowered = text.to_ascii_lowercase();
            if TRUE_STRINGS.contains(&lowered.as_str()) {
                Ok(true)
            } else if FALSE_STRINGS.contains(&lowered.as_str()) {
                Ok(false)
            } else {
                Err(fail_msg("Must be a valid boolean."))
            }
        }
        Value::Number(number) => {
            if let Some(i) = number.as_i64() {
                if i == 1 {
                    return Ok(true);
                }
                if i == 0 {
                    return Ok(false);
                }
            }
            if let Some(u) = number.as_u64() {
                if u == 1 {
                    return Ok(true);
                }
                if u == 0 {
                    return Ok(false);
                }
            }
            if let Some(f) = number.as_f64() {
                if f == 1.0 {
                    return Ok(true);
                }
                if f == 0.0 {
                    return Ok(false);
                }
            }
            Err(fail_msg("Must be a valid boolean."))
        }
        _ => Err(fail_msg("Must be a valid boolean.")),
    }
}

/// `ChoiceField.to_internal_value` over string choices: `""` passes
/// only with `allow_blank`, else `str(data)` must be a choice key. The
/// echo is `str(data)` — verbatim for strings, `True`/`None`-style for
/// scalars, `repr`-style for containers.
fn check_choice_str(value: &Value, choices: &[&str], allow_blank: bool) -> FieldResult<String> {
    if *value == Value::String(String::new()) && allow_blank {
        return Ok(String::new());
    }
    let key = py_str(value);
    if choices.contains(&key.as_str()) {
        Ok(key)
    } else {
        Err(fail_msg(format!("\"{key}\" is not a valid choice.")))
    }
}

/// `ChoiceField.to_internal_value` over the intake status ints: the
/// same `str(data)` lookup (`"1"` coerces to `1`; `1.0`/`True` fail
/// with their echo).
fn check_status(value: &Value) -> FieldResult<i32> {
    const KEYS: [&str; 5] = ["-2", "-1", "0", "1", "2"];
    let key = py_str(value);
    if let Some(pos) = KEYS.iter().position(|known| *known == key) {
        Ok([-2, -1, 0, 1, 2][pos])
    } else {
        Err(fail_msg(format!("\"{key}\" is not a valid choice.")))
    }
}

/// `ListField`: the container check. Element errors key by index
/// (`{"0": [...]}`); the type echo is Python's (`str`/`int`/`float`/
/// `bool`/`dict`).
fn check_list(value: &Value) -> FieldResult<&Vec<Value>> {
    match value {
        Value::Array(items) => Ok(items),
        Value::Bool(_) => Err(fail_msg("Expected a list of items but got type \"bool\".")),
        Value::Number(number) => {
            let kind = if number.is_i64() || number.is_u64() {
                "int"
            } else {
                "float"
            };
            Err(fail_msg(format!(
                "Expected a list of items but got type \"{kind}\"."
            )))
        }
        Value::String(_) => Err(fail_msg("Expected a list of items but got type \"str\".")),
        Value::Object(_) => Err(fail_msg("Expected a list of items but got type \"dict\".")),
        Value::Null => Err(fail_msg(
            "Expected a list of items but got type \"NoneType\".",
        )),
    }
}

/// `JSONField` (non-binary): any parsed-JSON value passes through
/// verbatim (`json.dumps` cannot fail on it). `None` is handled by the
/// null check before this runs.
fn check_json(value: &Value) -> Value {
    value.clone()
}

/// UUID-typed primary keys (`parent`, `state`, `type`, ...): bools take
/// the `incorrect_type` branch; ints go through `UUID(int=...)` (out of
/// `u128` range fails); strings accept every `uuid.UUID` spelling
/// (canonical, simple, braced, URN); anything else fails with the
/// Django curly-quote message echoing `str(data)`. Returns the parsed
/// id plus the verbatim echo for a later `does_not_exist`.
fn parse_uuid_pk(value: &Value) -> FieldResult<(Uuid, String)> {
    if let Value::Bool(_) = value {
        return Err(fail_msg(
            "Incorrect type. Expected pk value, received bool.",
        ));
    }
    if let Some(echo) = pk_int_echo(value) {
        let (raw, echo) = echo;
        let Some(int) = u128_from_json(&raw) else {
            return Err(fail_msg(format!("“{echo}” is not a valid UUID.")));
        };
        let id = Uuid::from_u128(int);
        Ok((id, echo))
    } else if let Value::String(text) = value {
        text.parse::<Uuid>()
            .map(|id| (id, text.clone()))
            .map_err(|_| fail_msg(format!("“{text}” is not a valid UUID.")))
    } else {
        let echo = py_str(value);
        Err(fail_msg(format!("“{echo}” is not a valid UUID.")))
    }
}

/// The `UUID(int=...)` path only applies to JSON integers (floats go
/// through `UUID(hex=...)` and always fail the same way).
fn pk_int_echo(value: &Value) -> Option<(Value, String)> {
    match value {
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            Some((value.clone(), py_str(value)))
        }
        _ => None,
    }
}

fn u128_from_json(value: &Value) -> Option<u128> {
    match value {
        Value::Number(number) => {
            if let Some(i) = number.as_i64() {
                u128::try_from(i).ok()
            } else {
                number.as_u64().map(u128::from)
            }
        }
        _ => None,
    }
}

/// The `does_not_exist` echo renders the ORIGINAL input (`pk_value`
/// is the pre-parse value): non-canonical UUID spellings and plain
/// ints echo verbatim.
fn pk_missing(echo: &str) -> FieldFail {
    fail_msg(format!("Invalid pk \"{echo}\" - object does not exist."))
}

/// `DateField` failure (DRF renders `iso-8601` as `YYYY-MM-DD`).
const DATE_MSG: &str = "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
/// `DateTimeField` failure.
const DATETIME_MSG: &str = "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";

/// `DateField.to_internal_value`: Django `parse_date` — strict
/// `fromisoformat` shapes first (extended, basic, week), then the
/// `date_re` fallback (`\d{4}-\d{1,2}-\d{1,2}`). Well-formed but
/// impossible dates fail the same way (the `date()` constructor raises
/// into DRF's suppress).
fn check_date(value: &Value) -> FieldResult<NaiveDate> {
    let Value::String(text) = value else {
        return Err(fail_msg(DATE_MSG));
    };
    parse_django_date(text).ok_or_else(|| fail_msg(DATE_MSG))
}

fn parse_django_date(text: &str) -> Option<NaiveDate> {
    if let Some(date) = parse_iso_date(text) {
        return Some(date);
    }
    // `date_re` fallback: 1-2 digit month/day, then calendar check.
    let mut parts = text.split('-');
    let (year, month, day) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(d), None) => (y, m, d),
        _ => return None,
    };
    if year.len() != 4 || !(1..=2).contains(&month.len()) || !(1..=2).contains(&day.len()) {
        return None;
    }
    let (year, month, day) = (year.parse().ok()?, month.parse().ok()?, day.parse().ok()?);
    NaiveDate::from_ymd_opt(year, month, day)
}

/// The `datetime.date.fromisoformat` shapes Django tries first:
/// strict extended, basic (`YYYYMMDD`) and week (`YYYY-Www-D`).
fn parse_iso_date(text: &str) -> Option<NaiveDate> {
    if text.len() == 8 && text.bytes().all(|b| b.is_ascii_digit()) {
        let (y, m, d) = (
            text[0..4].parse().ok()?,
            text[4..6].parse().ok()?,
            text[6..8].parse().ok()?,
        );
        return NaiveDate::from_ymd_opt(y, m, d);
    }
    if text.len() == 10 && text.as_bytes()[4] == b'-' && text.as_bytes()[7] == b'-' {
        let (y, m, d) = (
            text[0..4].parse().ok()?,
            text[5..7].parse().ok()?,
            text[8..10].parse().ok()?,
        );
        return NaiveDate::from_ymd_opt(y, m, d);
    }
    parse_iso_week_date(text)
}

/// `YYYY-Www-D` (ISO week date): Monday of week 1 is the Monday of the
/// week containing Jan 4th; week 1..53, day 1..7, calendar-checked.
fn parse_iso_week_date(text: &str) -> Option<NaiveDate> {
    if text.len() != 10 || &text[4..5] != "-" || &text[5..6] != "W" || &text[8..9] != "-" {
        return None;
    }
    let year: i32 = text[0..4].parse().ok()?;
    let week: u32 = text[6..8].parse().ok()?;
    let day: u32 = text[9..10].parse().ok()?;
    if !(1..=53).contains(&week) || !(1..=7).contains(&day) {
        return None;
    }
    NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::try_from(day as u8 - 1).ok()?)
}

/// `DateTimeField.to_internal_value` + `enforce_timezone` (`USE_TZ` is
/// on, so the field timezone is always the current/request zone):
/// Django `parse_datetime`, then naive values resolve in the request
/// zone and aware values convert through it (overflow → the `overflow`
/// message when the zoned result leaves years 1..=9999).
fn check_datetime(value: &Value, timezone: &Tz) -> FieldResult<DateTime<Utc>> {
    let Value::String(text) = value else {
        return Err(fail_msg(DATETIME_MSG));
    };
    let parsed = parse_django_datetime(text).ok_or_else(|| fail_msg(DATETIME_MSG))?;
    match parsed {
        ParsedDateTime::Naive(naive) => match timezone.from_local_datetime(&naive) {
            chrono::LocalResult::Single(aware) => Ok(aware.with_timezone(&Utc)),
            chrono::LocalResult::Ambiguous(first, second) => {
                // `fold=0`: the earlier (pre-transition) instant, matching
                // `zoneinfo`'s default for ambiguous wall times.
                Ok(first.min(second).with_timezone(&Utc))
            }
            chrono::LocalResult::None => Err(fail_msg(format!(
                "Invalid datetime for the timezone \"{}\".",
                timezone.name()
            ))),
        },
        ParsedDateTime::Aware(instant) => {
            // `astimezone(field_timezone)`: the conversion target is the
            // REQUEST zone, not UTC — the overflow boundary moves with it.
            let zoned = instant.with_timezone(timezone);
            if !(1..=9999).contains(&zoned.year()) {
                return Err(fail_msg("Datetime value out of range."));
            }
            Ok(instant)
        }
    }
}

enum ParsedDateTime {
    Naive(chrono::NaiveDateTime),
    Aware(DateTime<Utc>),
}

/// Django `parse_datetime`: `datetime.fromisoformat` first, then the
/// `datetime_re` fallback. Both layers accept date-only (midnight),
/// `T`/space separators and `Z`/numeric offsets; the regex layer
/// additionally allows 1-2 digit fields, comma fractions and whitespace
/// before the offset.
fn parse_django_datetime(text: &str) -> Option<ParsedDateTime> {
    if text.is_empty() {
        return None;
    }
    if let Some(parsed) = parse_iso_datetime(text) {
        return Some(parsed);
    }
    parse_regex_datetime(text)
}

/// The strict `fromisoformat` shapes: extended/basic/week dates with an
/// optional `T`/space time (`HH:MM[:SS[.ffffff]]`, 2-digit fields) and
/// an optional `Z`/`±HH:MM[:SS[.ffffff]]`/`±HHMM`/`±HH` offset.
/// Fractions truncate (never round) to microseconds.
fn parse_iso_datetime(text: &str) -> Option<ParsedDateTime> {
    // No separator: strict date-only is midnight (`fromisoformat`
    // rejects `2030-1-1`, so only strict shapes pass here).
    let Some(split) = text.find(['T', ' ']) else {
        let date = parse_iso_date(text)?;
        return Some(ParsedDateTime::Naive(date.and_hms_opt(0, 0, 0)?));
    };
    let (date_part, rest) = text.split_at(split);
    let date = parse_iso_date(date_part)?;
    // Rest starts with the separator; an empty time fails.
    let time_part = rest.get(1..)?;
    if time_part.is_empty() {
        return None;
    }
    // fromisoformat rejects inner spaces (the regex layer allows
    // spaces only directly before the offset).
    if time_part.contains(' ') {
        return None;
    }
    let (clock, offset) = split_iso_offset(time_part)?;
    let time = parse_iso_clock(clock)?;
    let naive = date.and_time(time);
    match offset {
        None => Some(ParsedDateTime::Naive(naive)),
        Some(IsoOffset::Minutes(minutes)) => {
            if minutes.abs() >= 24 * 60 {
                return None;
            }
            let instant = naive.and_utc() - chrono::Duration::minutes(minutes as i64);
            Some(ParsedDateTime::Aware(instant))
        }
        Some(IsoOffset::WithSeconds(minutes, seconds)) => {
            let total = minutes as i64 * 60 + seconds as i64;
            if total.abs() >= 24 * 3600 {
                return None;
            }
            let instant = naive.and_utc() - chrono::Duration::seconds(total);
            Some(ParsedDateTime::Aware(instant))
        }
    }
}

/// Split `HH:MM[:SS[.ffffff]][offset]` into clock and offset-minutes.
/// `Z` is 0; numeric offsets accept `±HH:MM[:SS[.ffffff]]`, `±HHMM` and
/// `±HH` (offset seconds are dropped — `fromisoformat` keeps them, but
/// Django's `get_fixed_timezone` path never sees this layer; offsets
/// with seconds only arise here, and `timezone(timedelta)` keeps the
/// seconds — handled by carrying them into the instant below).
fn split_iso_offset(time_part: &str) -> Option<(&str, Option<IsoOffset>)> {
    if let Some(clock) = time_part.strip_suffix(['Z']) {
        return Some((clock, Some(IsoOffset::Minutes(0))));
    }
    // A numeric offset starts at the LAST '+'/'-' after position 0
    // (the date was already split off, so no date `-` can match).
    let bytes = time_part.as_bytes();
    let mut sign_pos = None;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        if *byte == b'+' || *byte == b'-' {
            sign_pos = Some(index);
        }
    }
    let Some(pos) = sign_pos else {
        return Some((time_part, None));
    };
    let (clock, offset) = time_part.split_at(pos);
    // The clock must contain its minute separator before the offset.
    if !clock.contains(':') {
        return None;
    }
    let minutes = parse_iso_offset_minutes(offset)?;
    Some((clock, Some(minutes)))
}

#[derive(Clone, Copy)]
enum IsoOffset {
    Minutes(i32),
    WithSeconds(i32, i32),
}

/// `±HH:MM[:SS[.ffffff]]` / `±HHMM` / `±HH`, strictly 2-digit fields.
fn parse_iso_offset_minutes(offset: &str) -> Option<IsoOffset> {
    let (sign, rest) = match offset.strip_prefix(['+', '-']) {
        Some(rest) if offset.starts_with('-') => (-1i32, rest),
        Some(rest) => (1i32, rest),
        None => return None,
    };
    if rest.len() < 2 || !rest.bytes().take(2).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = rest[0..2].parse().ok()?;
    let tail = &rest[2..];
    if tail.is_empty() {
        return Some(IsoOffset::Minutes(sign * hours * 60));
    }
    if let Some(colon) = tail.strip_prefix(':') {
        if colon.len() < 2 || !colon.bytes().take(2).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mins: i32 = colon[0..2].parse().ok()?;
        let rest = &colon[2..];
        if rest.is_empty() {
            return Some(IsoOffset::Minutes(sign * (hours * 60 + mins)));
        }
        let secs = rest.strip_prefix(':')?;
        if secs.len() < 2 || !secs.bytes().take(2).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let seconds: i32 = secs[0..2].parse().ok()?;
        let frac = &secs[2..];
        if !frac.is_empty() {
            let digits = frac.strip_prefix('.')?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            // Sub-minute offset precision is below the storage
            // resolution AND outside the `get_fixed_timezone` minute
            // model — but fromisoformat keeps it. Fold seconds into
            // the minute total only when they divide evenly... no:
            // carry them exactly (see below).
        }
        return Some(IsoOffset::WithSeconds(
            sign * (hours * 60 + mins),
            sign * seconds,
        ));
    }
    if tail.len() == 2 && tail.bytes().all(|b| b.is_ascii_digit()) {
        let mins: i32 = tail.parse().ok()?;
        return Some(IsoOffset::Minutes(sign * (hours * 60 + mins)));
    }
    None
}

/// Strict clock: `HH:MM[:SS[.ffffff]]`, 2-digit fields, dot fractions
/// of any length (truncated to 6).
fn parse_iso_clock(clock: &str) -> Option<chrono::NaiveTime> {
    let mut parts = clock.splitn(3, ':');
    let (hour, minute, secs) = match (parts.next(), parts.next(), parts.next()) {
        (Some(hour), Some(minute), rest) => (hour, minute, rest),
        _ => return None,
    };
    if hour.len() != 2 || minute.len() != 2 {
        return None;
    }
    if !hour.bytes().all(|b| b.is_ascii_digit()) || !minute.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hour, minute): (u32, u32) = (hour.parse().ok()?, minute.parse().ok()?);
    let (second, micros) = match secs {
        None => (0, 0),
        Some(rest) => {
            let (sec, frac) = match rest.split_once('.') {
                Some((sec, frac)) => (sec, Some(frac)),
                None => (rest, None),
            };
            if sec.len() != 2 {
                return None;
            }
            let second: u32 = sec.parse().ok()?;
            let micros = match frac {
                None => 0,
                Some(digits) => {
                    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                        return None;
                    }
                    let mut padded = digits.to_owned();
                    padded.truncate(6);
                    while padded.len() < 6 {
                        padded.push('0');
                    }
                    padded.parse().ok()?
                }
            };
            (second, micros)
        }
    };
    chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micros)
}

/// The `datetime_re` fallback: 1-2 digit date/time fields, optional
/// seconds with dot-or-comma fractions, optional whitespace before a
/// `Z`/`±HH(:?MM)?` offset. `get_fixed_timezone` rejects offsets at or
/// beyond 24h.
fn parse_regex_datetime(text: &str) -> Option<ParsedDateTime> {
    let split = text.find(['T', ' '])?;
    let (date_part, rest) = text.split_at(split);
    let date = parse_regex_date(date_part)?;
    let time_part = rest.get(1..)?;
    if time_part.is_empty() {
        return None;
    }
    // `datetime_re` ends `\s*(tzinfo)?$` with `$` matching at the end
    // or before ONE trailing `\n`: a lone trailing newline is dead
    // weight, but trailing spaces after a `Z`/offset kill the match
    // (probed: `"00Z "` → None, `"00Z\n"` → aware, `"00  "` → naive).
    let body = time_part.strip_suffix('\n').unwrap_or(time_part);
    let (clock, offset) = split_regex_offset(body)?;
    // The clock itself must not contain inner spaces (the parser
    // rejects them structurally; this mirrors the shape).
    if clock.contains(' ') {
        return None;
    }
    let time = parse_regex_clock(clock)?;
    let naive = date.and_time(time);
    match offset {
        None => Some(ParsedDateTime::Naive(naive)),
        Some(minutes) => {
            if minutes.abs() >= 24 * 60 {
                return None;
            }
            let instant = naive.and_utc() - chrono::Duration::minutes(minutes as i64);
            Some(ParsedDateTime::Aware(instant))
        }
    }
}

fn parse_regex_date(text: &str) -> Option<NaiveDate> {
    let mut parts = text.split('-');
    let (year, month, day) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(d), None) => (y, m, d),
        _ => return None,
    };
    if year.len() != 4 || !(1..=2).contains(&month.len()) || !(1..=2).contains(&day.len()) {
        return None;
    }
    if !year.bytes().all(|b| b.is_ascii_digit())
        || !month.bytes().all(|b| b.is_ascii_digit())
        || !day.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
}

/// Split a trailing `Z`/`±HH(:?MM)?` offset: the `\s*` gap (Python
/// `re-\s`, so `\x1c` counts and Rust's `trim_end` is wrong) sits
/// between clock and tz, and nothing may follow the tz (the caller
/// already dropped the `$`-quirk newline). No tz → trailing `\s*` is
/// simply eaten (naive). Returns the clock and the offset in minutes
/// (`+00:60` normalizes to +60 — never range-checked here).
fn split_regex_offset(body: &str) -> Option<(&str, Option<i32>)> {
    if let Some(clock) = body.strip_suffix('Z') {
        let clock = clock.trim_end_matches(is_py_strip_char);
        return Some((clock, Some(0)));
    }
    let bytes = body.as_bytes();
    let mut sign_pos = None;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        if *byte == b'+' || *byte == b'-' {
            sign_pos = Some(index);
        }
    }
    let Some(pos) = sign_pos else {
        return Some((body.trim_end_matches(is_py_strip_char), None));
    };
    let (clock, offset) = body.split_at(pos);
    if !clock.contains(':') {
        return None;
    }
    let clock = clock.trim_end_matches(is_py_strip_char);
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let rest = &offset[1..];
    let minutes = if rest.len() == 2 && rest.bytes().all(|b| b.is_ascii_digit()) {
        sign * rest.parse::<i32>().ok()? * 60
    } else if rest.len() == 4 && rest.bytes().all(|b| b.is_ascii_digit()) {
        let hours: i32 = rest[0..2].parse().ok()?;
        let mins: i32 = rest[2..4].parse().ok()?;
        sign * (hours * 60 + mins)
    } else if rest.len() == 5 && rest.as_bytes()[2] == b':' {
        let hours: i32 = rest[0..2].parse().ok()?;
        let mins: i32 = rest[3..5].parse().ok()?;
        sign * (hours * 60 + mins)
    } else {
        return None;
    };
    Some((clock, Some(minutes)))
}

/// `H{1,2}:M{1,2}[:S{1,2}[.,]fraction]`; fractions truncate to 6.
fn parse_regex_clock(clock: &str) -> Option<chrono::NaiveTime> {
    let mut parts = clock.splitn(3, ':');
    let (hour, minute, secs) = match (parts.next(), parts.next(), parts.next()) {
        (Some(hour), Some(minute), rest) => (hour, minute, rest),
        _ => return None,
    };
    if !(1..=2).contains(&hour.len()) || !(1..=2).contains(&minute.len()) {
        return None;
    }
    if !hour.bytes().all(|b| b.is_ascii_digit()) || !minute.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hour, minute): (u32, u32) = (hour.parse().ok()?, minute.parse().ok()?);
    let (second, micros) = match secs {
        None => (0, 0),
        Some(rest) => {
            let (sec, frac) = match rest.find(['.', ',']) {
                Some(pos) => (&rest[..pos], Some(&rest[pos + 1..])),
                None => (rest, None),
            };
            if !(1..=2).contains(&sec.len()) || !sec.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let second: u32 = sec.parse().ok()?;
            let micros = match frac {
                None => 0,
                Some(digits) => {
                    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                        return None;
                    }
                    let mut padded = digits.to_owned();
                    padded.truncate(6);
                    while padded.len() < 6 {
                        padded.push('0');
                    }
                    padded.parse().ok()?
                }
            };
            (second, micros)
        }
    };
    chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micros)
}

// ---------------------------------------------------------------------------
// Descriptions (`normalize_description_input` + lxml gate)
// ---------------------------------------------------------------------------

/// `normalize_description_input` failures. These raise out of
/// `to_internal_value` (past the per-field `try`), so the dict detail
/// keeps STRING values on the wire (`{"description_markdown": "..."}`,
/// `{"error": "..."}`) — probed.
#[derive(Debug, Clone, PartialEq)]
enum NormalizeFail {
    NotString,
    MarkdownInvalid(String),
    HtmlInvalid,
}

impl NormalizeFail {
    fn body(&self) -> Value {
        let mut map = Map::with_capacity(1);
        match self {
            NormalizeFail::NotString => {
                map.insert(
                    "description_markdown".to_owned(),
                    Value::String("Must be a string.".to_owned()),
                );
            }
            NormalizeFail::MarkdownInvalid(message) => {
                map.insert(
                    "description_markdown".to_owned(),
                    Value::String(message.clone()),
                );
            }
            NormalizeFail::HtmlInvalid => {
                map.insert(
                    "error".to_owned(),
                    Value::String("html content is not valid".to_owned()),
                );
            }
        }
        Value::Object(map)
    }
}

struct NormalizedInput {
    data: Map<String, Value>,
    from_markdown: bool,
}

/// `normalize_description_input`
/// (`api/serializers/issue.py:48-105`): both markdown keys must be
/// strings when present; `description_markdown` converts (and wins);
/// the legacy `description` converts only when no `description_html`
/// was sent; everything is sanitized through `validate_html_content`
/// (`Sanitize::Invalid` → the `error` body).
fn normalize_description_input(
    data: &Map<String, Value>,
) -> Result<NormalizedInput, NormalizeFail> {
    for key in ["description_markdown", "description"] {
        if let Some(value) = data.get(key) {
            if !value.is_string() {
                return Err(NormalizeFail::NotString);
            }
        }
    }
    let mut data = data.clone();
    let markdown = data.remove("description_markdown");
    let legacy = data.remove("description");
    if markdown.is_none() && !data.contains_key("description_html") {
        if let Some(Value::String(legacy)) = legacy {
            let html = markdown::to_html(&legacy).map_err(NormalizeFail::MarkdownInvalid)?;
            let cleaned = sanitize_or_fail(&html)?;
            data.insert("description_html".to_owned(), Value::String(cleaned));
            return Ok(NormalizedInput {
                data,
                from_markdown: true,
            });
        }
        return Ok(NormalizedInput {
            data,
            from_markdown: false,
        });
    }
    if let Some(Value::String(markdown)) = markdown {
        let html = markdown::to_html(&markdown).map_err(NormalizeFail::MarkdownInvalid)?;
        let cleaned = sanitize_or_fail(&html)?;
        data.insert("description_html".to_owned(), Value::String(cleaned));
        return Ok(NormalizedInput {
            data,
            from_markdown: true,
        });
    }
    Ok(NormalizedInput {
        data,
        from_markdown: false,
    })
}

fn sanitize_or_fail(html: &str) -> Result<String, NormalizeFail> {
    match sanitize_html(html) {
        Sanitize::Clean(cleaned) => Ok(cleaned),
        Sanitize::Invalid => Err(NormalizeFail::HtmlInvalid),
    }
}

/// The `validate()` description gate
/// (`api/serializers/issue.py:216-251`): markdown-born html skips the
/// lxml round-trip (it is already well-formed); anything else parses
/// through [`lxml_roundtrip`] (`None` → `Invalid HTML passed`) and is
/// sanitized (`Invalid` → the `error` body — a STRING-valued
/// `non_field_errors` entry, since `validate()` raises a bare string).
#[derive(Debug, Clone, PartialEq)]
enum DescriptionFail {
    InvalidHtml,
    SanitizeInvalid,
}

fn check_description_html(html: &str, from_markdown: bool) -> Result<String, DescriptionFail> {
    let round_tripped = if from_markdown {
        html.to_owned()
    } else {
        lxml_roundtrip(html).ok_or(DescriptionFail::InvalidHtml)?
    };
    match sanitize_html(&round_tripped) {
        Sanitize::Clean(cleaned) => Ok(cleaned),
        Sanitize::Invalid => Err(DescriptionFail::SanitizeInvalid),
    }
}

// ---------------------------------------------------------------------------
// Writes: `IssueSerializer` / `IntakeIssueUpdateSerializer` validation
// ---------------------------------------------------------------------------

/// Field errors in field order (`{"field": [...]}`), plus an optional
/// `non_field_errors` tail. DRF collects every field error before
/// `validate()` runs, so field errors and `validate()` errors never mix.
#[derive(Default)]
struct ErrorBag {
    fields: Vec<(String, Value)>,
}

impl ErrorBag {
    fn field(&mut self, name: &str, message: String) {
        match self.fields.iter_mut().find(|(key, _)| key == name) {
            Some((_, Value::Array(list))) => list.push(Value::String(message)),
            _ => {
                self.fields
                    .push((name.to_owned(), Value::Array(vec![Value::String(message)])));
            }
        }
    }

    fn nested(&mut self, name: &str, body: Value) {
        self.fields.push((name.to_owned(), body));
    }

    fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    fn body(&self) -> Value {
        Value::Object(self.fields.iter().cloned().collect())
    }
}

/// `users`: Django's stock manager — bare row existence.
async fn user_exists(pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar(r#"SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)"#)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)
}

/// Soft-deletion managers (`labels`, `estimate_points`, `issue_types`):
/// live rows only.
async fn live_exists(pool: &sqlx::PgPool, table: &str, id: &Uuid) -> Result<bool, Denial> {
    let sql =
        format!(r#"SELECT EXISTS(SELECT 1 FROM "{table}" WHERE id = $1 AND deleted_at IS NULL)"#);
    sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `StateManager`: live rows outside the triage group.
async fn state_choice_exists(pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM states WHERE id = $1 AND deleted_at IS NULL AND "group" != 'triage')"#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// `IssueManager` (`db/models/issue.py:30-41`): live, non-archived,
/// non-draft rows in a non-archived project whose state is missing or
/// outside triage — the queryset behind the `parent` / `duplicate_to`
/// primary keys. (The `NULL`-group shape mirrors Django's
/// `NOT (group = 'triage' AND group IS NOT NULL)` exactly.)
async fn issue_choice_exists(pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM issues i
           JOIN projects p ON p.id = i.project_id
           LEFT JOIN states s ON s.id = i.state_id
           WHERE i.id = $1 AND i.deleted_at IS NULL AND i.archived_at IS NULL
           AND i.is_draft = FALSE AND p.archived_at IS NULL
           AND (s."group" IS NULL OR s."group" != 'triage'))"#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// `PodManager`: live pods.
async fn pod_exists(pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar(r#"SELECT EXISTS(SELECT 1 FROM pod WHERE id = $1 AND deleted_at IS NULL)"#)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `type_id`'s explicit `IssueType.objects.all()`: soft-deleted rows
/// count (unlike the auto `type` field, which uses the manager).
async fn issue_type_any_exists(pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar(r#"SELECT EXISTS(SELECT 1 FROM issue_types WHERE id = $1)"#)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `Issue.has_active_run` (`db/models/issue.py:275-282`): an `agent_run`
/// on this issue in a non-terminal status.
async fn issue_has_active_run(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<bool, Denial> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM agent_run WHERE work_item_id = $1 AND status IN
           ('queued', 'assigned', 'waiting_for_worktree', 'running', 'cancel_requested',
            'awaiting_approval', 'awaiting_reauth', 'paused_awaiting_input'))"#,
    )
    .bind(issue_id)
    .fetch_one(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// Validate one primary-key input: format first (curly/incorrect
/// messages), then the manager-scoped existence check (verbatim echo).
async fn check_pk(
    pool: &sqlx::PgPool,
    value: &Value,
    exists: impl AsyncExists,
) -> Result<Uuid, FieldFail> {
    let (id, echo) = parse_uuid_pk(value)?;
    if exists
        .exists(pool, &id)
        .await
        .map_err(|_| FieldFail::Server)?
    {
        Ok(id)
    } else {
        Err(pk_missing(&echo))
    }
}

trait AsyncExists {
    async fn exists(&self, pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial>;
}

#[derive(Clone, Copy)]
struct ExistsUser;
#[derive(Clone, Copy)]
struct ExistsStateChoice;
#[derive(Clone, Copy)]
struct ExistsIssueChoice;
#[derive(Clone, Copy)]
struct ExistsPod;
#[derive(Clone, Copy)]
struct ExistsIssueTypeAny;
#[derive(Clone, Copy)]
struct ExistsLive(&'static str);

impl AsyncExists for ExistsUser {
    async fn exists(&self, pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
        user_exists(pool, id).await
    }
}
impl AsyncExists for ExistsStateChoice {
    async fn exists(&self, pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
        state_choice_exists(pool, id).await
    }
}
impl AsyncExists for ExistsIssueChoice {
    async fn exists(&self, pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
        issue_choice_exists(pool, id).await
    }
}
impl AsyncExists for ExistsPod {
    async fn exists(&self, pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
        pod_exists(pool, id).await
    }
}
impl AsyncExists for ExistsIssueTypeAny {
    async fn exists(&self, pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
        issue_type_any_exists(pool, id).await
    }
}
impl AsyncExists for ExistsLive {
    async fn exists(&self, pool: &sqlx::PgPool, id: &Uuid) -> Result<bool, Denial> {
        live_exists(pool, self.0, id).await
    }
}

/// `assignees` / `labels`: the `ListField` container plus per-element
/// primary keys, errors keyed by index (`{"0": [...]}`). The outer
/// `Denial` is a database failure (500); the inner `Value` is the
/// field-error body (400).
async fn check_id_list(
    pool: &sqlx::PgPool,
    value: &Value,
    exists: impl AsyncExists + Copy,
) -> Result<Result<Vec<Uuid>, Value>, Denial> {
    let items = match check_list(value) {
        Ok(items) => items,
        Err(FieldFail::Msg(message)) => {
            return Ok(Err(Value::Array(vec![Value::String(message)])));
        }
        Err(FieldFail::Server) => return Err(Denial::ServerError),
    };
    let mut ids = Vec::with_capacity(items.len());
    let mut errors = Map::new();
    for (index, item) in items.iter().enumerate() {
        match check_pk(pool, item, exists).await {
            Ok(id) => ids.push(id),
            Err(FieldFail::Msg(message)) => {
                errors.insert(
                    index.to_string(),
                    Value::Array(vec![Value::String(message)]),
                );
            }
            Err(FieldFail::Server) => return Err(Denial::ServerError),
        }
    }
    if errors.is_empty() {
        Ok(Ok(ids))
    } else {
        Ok(Err(Value::Object(errors)))
    }
}

/// `git_work_branch` model validator
/// (`RegexValidator(r"^[A-Za-z0-9._/-]*$")`).
fn check_branch_name(branch: &str) -> Result<(), FieldFail> {
    let ok = branch
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(fail_msg(
            "Branch name may contain only letters, numbers, and . _ / -",
        ))
    }
}

/// Django's `ProhibitNullCharactersValidator` (on every `CharField` /
/// `TextField`): `\x00` fails after the blank/max checks.
fn check_no_null_chars(text: &str) -> Result<(), FieldFail> {
    if text.contains('\x00') {
        Err(fail_msg("Null characters are not allowed."))
    } else {
        Ok(())
    }
}

/// Validated issue-half patch (`IssueSerializer(issue, data,
/// partial=True)`): each `Some` is a present key (inner `None` clears
/// a nullable column).
#[derive(Default)]
struct IssuePatch {
    assignees: Option<Vec<Uuid>>,
    labels: Option<Vec<Uuid>>,
    issue_type: Option<Option<Uuid>>,
    point: Option<Option<i32>>,
    name: Option<String>,
    description_html: Option<String>,
    priority: Option<String>,
    complexity_score: Option<Option<i32>>,
    start_date: Option<Option<NaiveDate>>,
    target_date: Option<Option<NaiveDate>>,
    sequence_id: Option<i32>,
    sort_order: Option<f64>,
    completed_at: Option<Option<DateTime<Utc>>>,
    archived_at: Option<Option<NaiveDate>>,
    is_draft: Option<bool>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    git_work_branch: Option<String>,
    created_via: Option<Option<String>>,
    agent_executor: Option<Option<String>>,
    created_by: Option<Option<Uuid>>,
    parent: Option<Option<Uuid>>,
    state: Option<Option<Uuid>>,
    estimate_point: Option<Option<Uuid>>,
    assigned_pod: Option<Option<Uuid>>,
    deleted_at: Option<Option<DateTime<Utc>>>,
}

/// `IssueSerializer` field validation in field order (`partial=True`:
/// absent keys skip, even `required` ones; read-only and unknown keys
/// are ignored), then `validate()` (`serializers/issue.py:168-302`).
/// The serializer runs with an EMPTY context on this endpoint, so
/// `assignees`/`labels` always validate to `[]` and any non-null
/// `state`/`parent`/`estimate_point` always fails.
async fn validate_issue_input(
    pool: &sqlx::PgPool,
    data: &Map<String, Value>,
    from_markdown: bool,
    timezone: &Tz,
    instance: &IssueRow,
) -> Result<IssuePatch, Denial> {
    let mut errors = ErrorBag::default();
    let mut patch = IssuePatch::default();

    if let Some(value) = data.get("assignees") {
        if value.is_null() {
            errors.field("assignees", "This field may not be null.".to_owned());
        } else {
            match check_id_list(pool, value, ExistsUser).await? {
                Ok(_) => patch.assignees = Some(Vec::new()),
                Err(body) => errors.nested("assignees", body),
            }
        }
    }
    if let Some(value) = data.get("labels") {
        if value.is_null() {
            errors.field("labels", "This field may not be null.".to_owned());
        } else {
            match check_id_list(pool, value, ExistsLive("labels")).await? {
                Ok(_) => patch.labels = Some(Vec::new()),
                Err(body) => errors.nested("labels", body),
            }
        }
    }
    if let Some(value) = data.get("type_id") {
        if value.is_null() {
            patch.issue_type = Some(None);
        } else {
            match check_pk(pool, value, ExistsIssueTypeAny).await {
                Ok(id) => patch.issue_type = Some(Some(id)),
                Err(FieldFail::Msg(message)) => errors.field("type_id", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("point") {
        if value.is_null() {
            patch.point = Some(None);
        } else {
            match parse_drf_int(value) {
                Ok(raw) => {
                    if raw < 0 {
                        errors.field(
                            "point",
                            "Ensure this value is greater than or equal to 0.".to_owned(),
                        );
                    } else if raw > 12 {
                        errors.field(
                            "point",
                            "Ensure this value is less than or equal to 12.".to_owned(),
                        );
                    } else {
                        patch.point = Some(Some(int_column(raw)?));
                    }
                }
                Err(FieldFail::Msg(message)) => errors.field("point", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("name") {
        if value.is_null() {
            errors.field("name", "This field may not be null.".to_owned());
        } else {
            match check_char(value, false, Some(255), true) {
                Ok(text) => match check_no_null_chars(&text) {
                    Ok(()) => patch.name = Some(text),
                    Err(FieldFail::Msg(message)) => errors.field("name", message),
                    Err(FieldFail::Server) => return Err(Denial::ServerError),
                },
                Err(FieldFail::Msg(message)) => errors.field("name", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("description_html") {
        if value.is_null() {
            errors.field("description_html", "This field may not be null.".to_owned());
        } else {
            match check_char(value, true, None, true) {
                Ok(text) => match check_no_null_chars(&text) {
                    Ok(()) => patch.description_html = Some(text),
                    Err(FieldFail::Msg(message)) => errors.field("description_html", message),
                    Err(FieldFail::Server) => return Err(Denial::ServerError),
                },
                Err(FieldFail::Msg(message)) => errors.field("description_html", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("priority") {
        if value.is_null() {
            errors.field("priority", "This field may not be null.".to_owned());
        } else {
            match check_choice_str(value, &["low", "medium", "high", "urgent", "none"], false) {
                Ok(choice) => patch.priority = Some(choice),
                Err(FieldFail::Msg(message)) => errors.field("priority", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("complexity_score") {
        if value.is_null() {
            errors.field("complexity_score", "This field may not be null.".to_owned());
        } else {
            match parse_drf_int(value) {
                Ok(raw) => {
                    if raw < 0 {
                        errors.field(
                            "complexity_score",
                            "Ensure this value is greater than or equal to 0.".to_owned(),
                        );
                    } else if raw > 10 {
                        errors.field(
                            "complexity_score",
                            "Ensure this value is less than or equal to 10.".to_owned(),
                        );
                    } else {
                        patch.complexity_score = Some(Some(int_column(raw)?));
                    }
                }
                Err(FieldFail::Msg(message)) => errors.field("complexity_score", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("start_date") {
        if value.is_null() {
            patch.start_date = Some(None);
        } else {
            match check_date(value) {
                Ok(date) => patch.start_date = Some(Some(date)),
                Err(FieldFail::Msg(message)) => errors.field("start_date", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("target_date") {
        if value.is_null() {
            patch.target_date = Some(None);
        } else {
            match check_date(value) {
                Ok(date) => patch.target_date = Some(Some(date)),
                Err(FieldFail::Msg(message)) => errors.field("target_date", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("sequence_id") {
        if value.is_null() {
            errors.field("sequence_id", "This field may not be null.".to_owned());
        } else {
            match parse_drf_int(value) {
                Ok(raw) => patch.sequence_id = Some(int_column(raw)?),
                Err(FieldFail::Msg(message)) => errors.field("sequence_id", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("sort_order") {
        if value.is_null() {
            errors.field("sort_order", "This field may not be null.".to_owned());
        } else {
            match parse_drf_float(value) {
                Ok(number) => patch.sort_order = Some(number),
                Err(FieldFail::Msg(message)) => errors.field("sort_order", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("completed_at") {
        if value.is_null() {
            patch.completed_at = Some(None);
        } else {
            match check_datetime(value, timezone) {
                Ok(instant) => patch.completed_at = Some(Some(instant)),
                Err(FieldFail::Msg(message)) => errors.field("completed_at", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("archived_at") {
        if value.is_null() {
            patch.archived_at = Some(None);
        } else {
            match check_date(value) {
                Ok(date) => patch.archived_at = Some(Some(date)),
                Err(FieldFail::Msg(message)) => errors.field("archived_at", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("is_draft") {
        if value.is_null() {
            errors.field("is_draft", "This field may not be null.".to_owned());
        } else {
            match parse_drf_bool(value) {
                Ok(flag) => patch.is_draft = Some(flag),
                Err(FieldFail::Msg(message)) => errors.field("is_draft", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("external_source") {
        if value.is_null() {
            patch.external_source = Some(None);
        } else {
            match check_char(value, true, Some(255), true) {
                Ok(text) => match check_no_null_chars(&text) {
                    Ok(()) => patch.external_source = Some(Some(text)),
                    Err(FieldFail::Msg(message)) => errors.field("external_source", message),
                    Err(FieldFail::Server) => return Err(Denial::ServerError),
                },
                Err(FieldFail::Msg(message)) => errors.field("external_source", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("external_id") {
        if value.is_null() {
            patch.external_id = Some(None);
        } else {
            match check_char(value, true, Some(255), true) {
                Ok(text) => match check_no_null_chars(&text) {
                    Ok(()) => patch.external_id = Some(Some(text)),
                    Err(FieldFail::Msg(message)) => errors.field("external_id", message),
                    Err(FieldFail::Server) => return Err(Denial::ServerError),
                },
                Err(FieldFail::Msg(message)) => errors.field("external_id", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("git_work_branch") {
        if value.is_null() {
            errors.field("git_work_branch", "This field may not be null.".to_owned());
        } else {
            match check_char(value, true, Some(128), true) {
                Ok(text) => match check_branch_name(&text) {
                    Ok(()) => match check_no_null_chars(&text) {
                        Ok(()) => patch.git_work_branch = Some(text),
                        Err(FieldFail::Msg(message)) => errors.field("git_work_branch", message),
                        Err(FieldFail::Server) => return Err(Denial::ServerError),
                    },
                    Err(FieldFail::Msg(message)) => errors.field("git_work_branch", message),
                    Err(FieldFail::Server) => return Err(Denial::ServerError),
                },
                Err(FieldFail::Msg(message)) => errors.field("git_work_branch", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("created_via") {
        if value.is_null() {
            patch.created_via = Some(None);
        } else {
            match check_char(value, true, Some(32), true) {
                Ok(text) => match check_no_null_chars(&text) {
                    Ok(()) => patch.created_via = Some(Some(text)),
                    Err(FieldFail::Msg(message)) => errors.field("created_via", message),
                    Err(FieldFail::Server) => return Err(Denial::ServerError),
                },
                Err(FieldFail::Msg(message)) => errors.field("created_via", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("agent_executor") {
        if value.is_null() {
            patch.agent_executor = Some(None);
        } else {
            match check_choice_str(
                value,
                &["local_runner", "cloud_agent", "managed_runner"],
                true,
            ) {
                Ok(choice) => patch.agent_executor = Some(Some(choice)),
                Err(FieldFail::Msg(message)) => errors.field("agent_executor", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("created_by") {
        if value.is_null() {
            patch.created_by = Some(None);
        } else {
            match check_pk(pool, value, ExistsUser).await {
                Ok(id) => patch.created_by = Some(Some(id)),
                Err(FieldFail::Msg(message)) => errors.field("created_by", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("parent") {
        if value.is_null() {
            patch.parent = Some(None);
        } else {
            match check_pk(pool, value, ExistsIssueChoice).await {
                Ok(id) => patch.parent = Some(Some(id)),
                Err(FieldFail::Msg(message)) => errors.field("parent", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("state") {
        if value.is_null() {
            patch.state = Some(None);
        } else {
            match check_pk(pool, value, ExistsStateChoice).await {
                Ok(id) => patch.state = Some(Some(id)),
                Err(FieldFail::Msg(message)) => errors.field("state", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("estimate_point") {
        if value.is_null() {
            patch.estimate_point = Some(None);
        } else {
            match check_pk(pool, value, ExistsLive("estimate_points")).await {
                Ok(id) => patch.estimate_point = Some(Some(id)),
                Err(FieldFail::Msg(message)) => errors.field("estimate_point", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("type") {
        if value.is_null() {
            patch.issue_type = Some(None);
        } else {
            match check_pk(pool, value, ExistsLive("issue_types")).await {
                // `type` sorts after `type_id`, so it wins the shared
                // `validated_data["type"]` slot when both are sent.
                Ok(id) => patch.issue_type = Some(Some(id)),
                Err(FieldFail::Msg(message)) => errors.field("type", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("assigned_pod") {
        if value.is_null() {
            patch.assigned_pod = Some(None);
        } else {
            match check_pk(pool, value, ExistsPod).await {
                Ok(id) => patch.assigned_pod = Some(Some(id)),
                Err(FieldFail::Msg(message)) => errors.field("assigned_pod", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("deleted_at") {
        if value.is_null() {
            patch.deleted_at = Some(None);
        } else {
            match check_datetime(value, timezone) {
                Ok(instant) => patch.deleted_at = Some(Some(instant)),
                Err(FieldFail::Msg(message)) => errors.field("deleted_at", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if !errors.is_empty() {
        return Err(Denial::FieldErrors(errors.body()));
    }

    // `validate()` extras, in source order — first failure wins.
    if let (Some(Some(start)), Some(Some(target))) = (patch.start_date, patch.target_date) {
        if start > target {
            return Err(Denial::FieldErrors(single_non_field(
                "Start date cannot exceed target date",
            )));
        }
    }
    // `assigned_pod` gate (`serializers/issue.py:193-216`): runs right after
    // the start/target date check, keyed on the field itself. The project
    // check reads the INSTANCE's project (the context fallback, `:200-201`),
    // so it is live on this endpoint; the active-run gate fires on any real
    // change — assign, reassign or clear — with null counted as a value.
    if let Some(new_pod) = patch.assigned_pod {
        if let Some(pod_id) = new_pod {
            let pod_project: Option<Uuid> = sqlx::query_scalar(
                r#"SELECT project_id FROM pod WHERE id = $1 AND deleted_at IS NULL"#,
            )
            .bind(pod_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
            let pod_project = pod_project.ok_or(Denial::ServerError)?;
            if pod_project != instance.project_id {
                return Err(Denial::FieldErrors(single_field(
                    "assigned_pod",
                    "pod is in a different project",
                )));
            }
        }
        if new_pod != instance.assigned_pod_id && issue_has_active_run(pool, &instance.id).await? {
            return Err(Denial::FieldErrors(single_field(
                "assigned_pod",
                "cannot reassign pod while the issue has an active run",
            )));
        }
    }
    if let Some(html) = patch.description_html.clone() {
        match check_description_html(&html, from_markdown) {
            Ok(cleaned) => patch.description_html = Some(cleaned),
            Err(DescriptionFail::InvalidHtml) => {
                return Err(Denial::FieldErrors(single_non_field("Invalid HTML passed")));
            }
            Err(DescriptionFail::SanitizeInvalid) => {
                // A dict detail survives `as_serializer_error`
                // unwrapped (STRING value, not a list).
                let mut map = Map::with_capacity(1);
                map.insert(
                    "error".to_owned(),
                    Value::String("html content is not valid".to_owned()),
                );
                return Err(Denial::FieldErrors(Value::Object(map)));
            }
        }
    }
    // `assignees` / `labels` narrow against `context["project_id"]`,
    // which is empty on this endpoint — both always validate to `[]`
    // (the field phase above still rejects malformed ids first).
    if patch.assignees.is_some() {
        patch.assignees = Some(Vec::new());
    }
    if patch.labels.is_some() {
        patch.labels = Some(Vec::new());
    }
    // `state` / `parent` / `estimate_point` validate against the empty
    // context's `(None, None)` scope, which matches no row — any
    // non-null value fails here (verified live).
    if matches!(patch.state, Some(Some(_))) {
        return Err(Denial::FieldErrors(single_non_field(
            "State is not valid please pass a valid state_id",
        )));
    }
    if matches!(patch.parent, Some(Some(_))) {
        return Err(Denial::FieldErrors(single_non_field(
            "Parent is not valid issue_id please pass a valid issue_id",
        )));
    }
    if matches!(patch.estimate_point, Some(Some(_))) {
        return Err(Denial::FieldErrors(single_non_field(
            "Estimate point is not valid please pass a valid estimate_point_id",
        )));
    }
    Ok(patch)
}

fn single_field(name: &str, message: &str) -> Value {
    let mut map = Map::with_capacity(1);
    map.insert(
        name.to_owned(),
        Value::Array(vec![Value::String(message.to_owned())]),
    );
    Value::Object(map)
}

fn single_non_field(message: &str) -> Value {
    let mut map = Map::with_capacity(1);
    map.insert(
        "non_field_errors".to_owned(),
        Value::Array(vec![Value::String(message.to_owned())]),
    );
    Value::Object(map)
}

/// Validated intake-half patch
/// (`IntakeIssueUpdateSerializer(intake_issue, data, partial=True)`).
#[derive(Default)]
struct IntakePatch {
    status: Option<i32>,
    snoozed_till: Option<Option<DateTime<Utc>>>,
    duplicate_to: Option<Option<Uuid>>,
    source: Option<Option<String>>,
    source_email: Option<Option<String>>,
}

/// The update serializer in field order, then the accept guard
/// (`serializers/intake.py:138-169`) through the types kernel. The
/// nested `issue` key never survives the view's `pop`, but it still
/// validates when present (defensive — unreachable over HTTP).
async fn validate_intake_input(
    pool: &sqlx::PgPool,
    data: &Map<String, Value>,
    timezone: &Tz,
    intake_issue: &IntakeIssueRow,
    issue: &IssueRow,
) -> Result<IntakePatch, Denial> {
    let mut errors = ErrorBag::default();
    let mut patch = IntakePatch::default();

    if let Some(value) = data.get("status") {
        if value.is_null() {
            errors.field("status", "This field may not be null.".to_owned());
        } else {
            match check_status(value) {
                Ok(status) => patch.status = Some(status),
                Err(FieldFail::Msg(message)) => errors.field("status", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("snoozed_till") {
        if value.is_null() {
            patch.snoozed_till = Some(None);
        } else {
            match check_datetime(value, timezone) {
                Ok(instant) => patch.snoozed_till = Some(Some(instant)),
                Err(FieldFail::Msg(message)) => errors.field("snoozed_till", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("duplicate_to") {
        if value.is_null() {
            patch.duplicate_to = Some(None);
        } else {
            match check_pk(pool, value, ExistsIssueChoice).await {
                Ok(id) => patch.duplicate_to = Some(Some(id)),
                Err(FieldFail::Msg(message)) => errors.field("duplicate_to", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("source") {
        if value.is_null() {
            patch.source = Some(None);
        } else {
            match check_char(value, true, Some(255), true) {
                Ok(text) => match check_no_null_chars(&text) {
                    Ok(()) => patch.source = Some(Some(text)),
                    Err(FieldFail::Msg(message)) => errors.field("source", message),
                    Err(FieldFail::Server) => return Err(Denial::ServerError),
                },
                Err(FieldFail::Msg(message)) => errors.field("source", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("source_email") {
        if value.is_null() {
            patch.source_email = Some(None);
        } else {
            match check_char(value, true, None, true) {
                Ok(text) => match check_no_null_chars(&text) {
                    Ok(()) => patch.source_email = Some(Some(text)),
                    Err(FieldFail::Msg(message)) => errors.field("source_email", message),
                    Err(FieldFail::Server) => return Err(Denial::ServerError),
                },
                Err(FieldFail::Msg(message)) => errors.field("source_email", message),
                Err(FieldFail::Server) => return Err(Denial::ServerError),
            }
        }
    }
    if let Some(value) = data.get("issue") {
        if value.is_null() {
            errors.field("issue", "This field may not be null.".to_owned());
        } else if let Value::Object(nested) = value {
            match validate_for_intake_nested(nested) {
                Ok(()) => {}
                Err(body) => errors.nested("issue", body),
            }
        } else {
            let mut nested = Map::with_capacity(1);
            nested.insert(
                "non_field_errors".to_owned(),
                Value::Array(vec![Value::String(format!(
                    "Invalid data. Expected a dictionary, but got {}.",
                    json_type_name(value)
                ))]),
            );
            errors.nested("issue", Value::Object(nested));
        }
    }
    if !errors.is_empty() {
        return Err(Denial::FieldErrors(errors.body()));
    }

    // `validate()` accept guard through the types kernel: needs the
    // PRE-SAVE issue state group (a dangling `state_id` raises
    // `DoesNotExist` out of `issue.state` → the 404 branch, not a 400)
    // plus the default-state lookup.
    if patch.status == Some(1) {
        let group = match issue.state_id {
            None => None,
            Some(state_id) => {
                let group: Option<String> =
                    sqlx::query_scalar(r#"SELECT "group" FROM states WHERE id = $1"#)
                        .bind(state_id)
                        .fetch_optional(pool)
                        .await
                        .map_err(|_| Denial::ServerError)?;
                Some(group.ok_or(Denial::ResourceMissing)?)
            }
        };
        let default_exists: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM states
               WHERE workspace_id = $1 AND project_id = $2 AND "default" = TRUE
               AND deleted_at IS NULL AND "group" != 'triage')"#,
        )
        .bind(intake_issue.workspace_id)
        .bind(intake_issue.project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
        .unwrap_or(false);
        if let Err(message) =
            intake_types::validate_accept(Some(1), group.as_deref(), default_exists)
        {
            let mut map = Map::with_capacity(1);
            map.insert(
                "status".to_owned(),
                Value::Array(vec![Value::String(message.to_owned())]),
            );
            return Err(Denial::FieldErrors(Value::Object(map)));
        }
    }
    Ok(patch)
}

/// Python `type(data).__name__` over JSON values (serializer
/// non-dict errors).
fn json_type_name(value: &Value) -> &'static str {
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

/// `IssueForIntakeSerializer` (nested under `issue`): name (required
/// at root level, but the root runs partial), legacy `description` /
/// `description_json` (both plain JSON), `description_html`,
/// `priority`. Unreachable over HTTP (the view pops `issue`), ported
/// for shape completeness.
fn validate_for_intake_nested(data: &Map<String, Value>) -> Result<(), Value> {
    let mut errors = ErrorBag::default();
    if let Some(value) = data.get("name") {
        if value.is_null() {
            errors.field("name", "This field may not be null.".to_owned());
        } else {
            match check_char(value, false, Some(255), true) {
                Ok(text) => {
                    if let Err(FieldFail::Msg(message)) = check_no_null_chars(&text) {
                        errors.field("name", message);
                    }
                }
                Err(FieldFail::Msg(message)) => errors.field("name", message),
                Err(FieldFail::Server) => {
                    errors.field("name", "A valid string is required.".to_owned());
                }
            }
        }
    }
    if let Some(value) = data.get("description") {
        if value.is_null() {
            // `JSONField(allow_null=True)`: explicit null validates.
        } else {
            let _ = check_json(value);
        }
    }
    if let Some(value) = data.get("description_json") {
        if value.is_null() {
            errors.field("description_json", "This field may not be null.".to_owned());
        } else {
            let _ = check_json(value);
        }
    }
    if let Some(value) = data.get("description_html") {
        if value.is_null() {
            errors.field("description_html", "This field may not be null.".to_owned());
        } else {
            match check_char(value, true, None, true) {
                Ok(text) => {
                    if let Err(FieldFail::Msg(message)) = check_no_null_chars(&text) {
                        errors.field("description_html", message);
                    }
                }
                Err(FieldFail::Msg(message)) => errors.field("description_html", message),
                Err(FieldFail::Server) => {
                    errors.field("description_html", "A valid string is required.".to_owned());
                }
            }
        }
    }
    if let Some(value) = data.get("priority") {
        if value.is_null() {
            errors.field("priority", "This field may not be null.".to_owned());
        } else {
            match check_choice_str(value, &["low", "medium", "high", "urgent", "none"], false) {
                Ok(_) => {}
                Err(FieldFail::Msg(message)) => errors.field("priority", message),
                Err(FieldFail::Server) => {
                    errors.field("priority", "A valid choice is required.".to_owned());
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.body())
    }
}

// ---------------------------------------------------------------------------
// Shared lookups (`.get()` / `.first()` semantics)
// ---------------------------------------------------------------------------

struct ProjectRow {
    id: Uuid,
    workspace_id: Uuid,
    intake_view: bool,
}

/// `Project.objects.get(pk=...)` (`views/intake.py:81,153,...`): a miss
/// is the `ObjectDoesNotExist` 404 (raised outside `get_object`, so the
/// `handle_exception` branch renders it — NOT the DRF `Http404` body).
async fn fetch_project(pool: &sqlx::PgPool, id: &Uuid) -> Result<ProjectRow, Denial> {
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT "id", "workspace_id", "intake_view" FROM "projects" WHERE "id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let row = row.ok_or(Denial::ResourceMissing)?;
    Ok(ProjectRow {
        id: row.try_get("id").map_err(|_| Denial::ServerError)?,
        workspace_id: row
            .try_get("workspace_id")
            .map_err(|_| Denial::ServerError)?,
        intake_view: row
            .try_get("intake_view")
            .map_err(|_| Denial::ServerError)?,
    })
}

/// `Intake.objects.filter(...).first()`: the intake id, if any.
async fn fetch_intake_id(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    // The pinned lookup selects `*` (Django `.first()` returns the full
    // object), so decode the row and read `id` by name — `query_scalar`
    // cannot decode a multi-column row.
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&intake_queries::intake_lookup_sql())
        .bind(slug)
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    row.map(|row| row.try_get("id").map_err(|_| Denial::ServerError))
        .transpose()
}

/// `IntakeIssue.objects.get(issue_id, workspace__slug, project_id,
/// intake_id)` (patch/delete preamble — NO snooze filter): 0 rows is
/// the 404 branch, 2+ is the generic 500 (`MultipleObjectsReturned`
/// matches no `handle_exception` branch).
async fn get_intake_issue(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
    slug: &str,
    project_id: &Uuid,
    intake_id: &Uuid,
) -> Result<IntakeIssueRow, Denial> {
    let sql = format!(
        "SELECT {INTAKE_ISSUE_COLS} FROM intake_issues ii JOIN workspaces w ON w.id = ii.workspace_id \
         WHERE ii.issue_id = $1 AND w.slug = $2 AND ii.project_id = $3 AND ii.intake_id = $4 \
         AND ii.deleted_at IS NULL"
    );
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .bind(slug)
        .bind(project_id)
        .bind(intake_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match rows.len() {
        0 => Err(Denial::ResourceMissing),
        1 => IntakeIssueRow::from_row(&rows[0]),
        _ => Err(Denial::ServerError),
    }
}

/// `ProjectMember.objects.get(workspace__slug, project_id, member,
/// is_active)` (patch preamble): same 0/1/2+ mapping.
async fn get_member_role(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &Uuid,
    user_id: &Uuid,
) -> Result<i16, Denial> {
    let rows: Vec<i16> = sqlx::query_scalar(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE w.slug = $1 AND pm.member_id = $2 AND pm.project_id = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(slug)
    .bind(user_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    match rows.len() {
        0 => Err(Denial::ResourceMissing),
        1 => Ok(rows[0]),
        _ => Err(Denial::ServerError),
    }
}

/// The patch issue fetch (`views/intake.py:366-372`): the annotated
/// live-issue `.get()` (the label/assignee annotations ride along for
/// SQL parity but no read path consumes them).
async fn get_patch_issue(
    pool: &sqlx::PgPool,
    issue_id: &Uuid,
    slug: &str,
    project_id: &Uuid,
) -> Result<IssueRow, Denial> {
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&intake_queries::patch_issue_lookup_sql())
        .bind(issue_id)
        .bind(slug)
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    match rows.len() {
        0 => Err(Denial::ResourceMissing),
        1 => IssueRow::from_block(&rows[0], 0),
        _ => Err(Denial::ServerError),
    }
}

/// `intake_issue.issue` / response re-reads: the `_base_manager`
/// (plain, unfiltered) fetch — soft-deleted rows resolve, only a truly
/// missing row 404s.
async fn fetch_companion_issue(pool: &sqlx::PgPool, issue_id: &Uuid) -> Result<IssueRow, Denial> {
    let sql = format!("SELECT {ISSUE_COLS} FROM issues WHERE id = $1");
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let row = row.ok_or(Denial::ResourceMissing)?;
    IssueRow::from_row(&row)
}

/// `issue.state` / `State` group reads: `_base_manager` (unfiltered) —
/// the triage group resolves (the accept flow depends on it); a
/// dangling id is the 404 branch.
async fn fetch_state_group(pool: &sqlx::PgPool, state_id: &Uuid) -> Result<String, Denial> {
    let group: Option<String> = sqlx::query_scalar(r#"SELECT "group" FROM states WHERE id = $1"#)
        .bind(state_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    group.ok_or(Denial::ResourceMissing)
}

/// The accept default (`State.objects.filter(workspace, project,
/// default=True).first()`, `StateManager` = live + non-triage,
/// `Meta.ordering = ("sequence",)`).
async fn fetch_default_state(
    pool: &sqlx::PgPool,
    workspace_id: &Uuid,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT "id" FROM "states" WHERE "workspace_id" = $1 AND "project_id" = $2
           AND "default" = TRUE AND "deleted_at" IS NULL AND "group" != 'triage'
           ORDER BY "sequence" LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// `Issue.save()`'s `state is None` resolution (`db/models/issue.py`):
/// `State.objects` (whose manager excludes `group = 'triage'`) filtered
/// by `~Q(is_triage=True)` — the auto-created Triage row carries
/// `is_triage = FALSE`, so both conditions are needed — with NO
/// workspace scope: the default first, else the first row, both in
/// sequence order.
async fn resolve_save_state(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    let default: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT "id" FROM "states" WHERE "project_id" = $1
           AND "deleted_at" IS NULL AND "group" <> 'triage' AND "is_triage" = FALSE AND "default" = TRUE
           ORDER BY "sequence" LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if default.is_some() {
        return Ok(default);
    }
    sqlx::query_scalar(
        r#"SELECT "id" FROM "states" WHERE "project_id" = $1
           AND "deleted_at" IS NULL AND "group" <> 'triage' AND "is_triage" = FALSE
           ORDER BY "sequence" LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Saves (`Issue.save` / `ModelSerializer.update` / soft delete)
// ---------------------------------------------------------------------------

/// Replace one link set (`IssueAssignee` / `IssueLabel`): soft-delete
/// the live links, then `bulk_create(..., ignore_conflicts=True)` the
/// new ids with the PRE-SAVE issue's audit columns. `IntegrityError`
/// (unique + FK races alike) is swallowed; anything else is the 500.
async fn replace_links(
    pool: &sqlx::PgPool,
    table: &str,
    id_column: &str,
    issue: &IssueRow,
    ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<(), Denial> {
    let delete_sql = format!(
        "UPDATE \"{table}\" SET deleted_at = $1 WHERE issue_id = $2 AND deleted_at IS NULL"
    );
    sqlx::query(&delete_sql)
        .bind(now)
        .bind(issue.id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    for id in ids {
        let insert_sql = format!(
            "INSERT INTO \"{table}\" (id, created_at, updated_at, created_by_id, updated_by_id, \
             deleted_at, issue_id, \"{id_column}\", project_id, workspace_id) \
             VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9) ON CONFLICT DO NOTHING"
        );
        let result = sqlx::query(&insert_sql)
            .bind(Uuid::new_v4())
            .bind(now)
            .bind(now)
            .bind(issue.created_by_id)
            .bind(issue.updated_by_id)
            .bind(issue.id)
            .bind(id)
            .bind(issue.project_id)
            .bind(issue.workspace_id)
            .execute(pool)
            .await;
        match result {
            Ok(_) => {}
            Err(error) if is_integrity_error(&error) => {}
            Err(_) => return Err(Denial::ServerError),
        }
    }
    Ok(())
}

/// Apply an issue-half patch: link replacement, `updated_at` stamp,
/// column writes, then `Issue.save()` (not adding): `state is None`
/// resolution (which leaves `completed_at` alone — an explicitly
/// supplied value sticks, an absent one keeps the stored value),
/// otherwise `completed_at` recompute (overwriting any validated value
/// whenever a state is set), `description_stripped` recompute (`""` →
/// `None` on this path), audit stamp. Returns the post-save row.
async fn save_issue_patch(
    pool: &sqlx::PgPool,
    issue: &IssueRow,
    patch: &IssuePatch,
    user_id: &Uuid,
) -> Result<IssueRow, Denial> {
    let now_links = cycle_mod::micros_now();
    if let Some(assignees) = &patch.assignees {
        replace_links(
            pool,
            "issue_assignees",
            "assignee_id",
            issue,
            assignees,
            now_links,
        )
        .await?;
    }
    if let Some(labels) = &patch.labels {
        replace_links(pool, "issue_labels", "label_id", issue, labels, now_links).await?;
    }
    // `update()` stamps `updated_at`, then `save()`'s `auto_now`
    // overwrites it — two clock reads, either of which can straddle.
    let _now_update = cycle_mod::micros_now();
    let now_save = cycle_mod::micros_now();

    let mut state_id = patch.state.unwrap_or(issue.state_id);
    // `Issue.save`'s `state is None` branch resolves the state but never
    // recomputes `completed_at` (that is only the `else` branch).
    let state_was_null = patch.state == Some(None);
    if state_was_null {
        state_id = resolve_save_state(pool, &issue.project_id).await?;
    }
    let completed_at = match state_id {
        Some(id) if !state_was_null => {
            let group = fetch_state_group(pool, &id).await?;
            if group == "completed" {
                Some(now_save)
            } else {
                None
            }
        }
        _ => patch.completed_at.unwrap_or(issue.completed_at),
    };
    let html = patch
        .description_html
        .clone()
        .unwrap_or_else(|| issue.description_html.clone());
    // `Issue.save()` non-adding branch: falsy html → NULL, else the
    // strip result as-is (even `""` — `strip_tags("<p></p>")`).
    let stripped: Option<String> = if html.is_empty() {
        None
    } else {
        Some(strip_tags_ml(&html))
    };

    sqlx::query(
        r#"UPDATE "issues" SET "name" = $1, "description_html" = $2, "priority" = $3,
           "point" = $4, "complexity_score" = $5, "start_date" = $6, "target_date" = $7,
           "sequence_id" = $8, "sort_order" = $9, "completed_at" = $10, "archived_at" = $11,
           "is_draft" = $12, "external_source" = $13, "external_id" = $14,
           "git_work_branch" = $15, "created_via" = $16, "agent_executor" = $17,
           "created_by_id" = $18, "parent_id" = $19, "state_id" = $20,
           "estimate_point_id" = $21, "type_id" = $22, "assigned_pod_id" = $23,
           "deleted_at" = $24, "description_stripped" = $25,
           "updated_at" = $26, "updated_by_id" = $27 WHERE "id" = $28"#,
    )
    .bind(patch.name.clone().unwrap_or_else(|| issue.name.clone()))
    .bind(html.clone())
    .bind(
        patch
            .priority
            .clone()
            .unwrap_or_else(|| issue.priority.clone().unwrap_or_default()),
    )
    .bind(patch.point.unwrap_or(issue.point))
    .bind(patch.complexity_score.unwrap_or(issue.complexity_score))
    .bind(patch.start_date.unwrap_or(issue.start_date))
    .bind(patch.target_date.unwrap_or(issue.target_date))
    .bind(patch.sequence_id.unwrap_or(issue.sequence_id))
    .bind(patch.sort_order.unwrap_or(issue.sort_order))
    .bind(completed_at)
    .bind(patch.archived_at.unwrap_or(issue.archived_at))
    .bind(patch.is_draft.unwrap_or(issue.is_draft))
    .bind(
        patch
            .external_source
            .clone()
            .unwrap_or_else(|| issue.external_source.clone()),
    )
    .bind(
        patch
            .external_id
            .clone()
            .unwrap_or_else(|| issue.external_id.clone()),
    )
    .bind(
        patch
            .git_work_branch
            .clone()
            .unwrap_or_else(|| issue.git_work_branch.clone()),
    )
    .bind(
        patch
            .created_via
            .clone()
            .unwrap_or_else(|| issue.created_via.clone()),
    )
    .bind(
        patch
            .agent_executor
            .clone()
            .unwrap_or_else(|| issue.agent_executor.clone()),
    )
    .bind(patch.created_by.unwrap_or(issue.created_by_id))
    .bind(patch.parent.unwrap_or(issue.parent_id))
    .bind(state_id)
    .bind(patch.estimate_point.unwrap_or(issue.estimate_point_id))
    .bind(patch.issue_type.unwrap_or(issue.type_id))
    .bind(patch.assigned_pod.unwrap_or(issue.assigned_pod_id))
    .bind(patch.deleted_at.unwrap_or(issue.deleted_at))
    .bind(stripped.clone())
    .bind(now_save)
    .bind(user_id)
    .bind(issue.id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    let mut saved = issue.clone();
    if let Some(name) = &patch.name {
        saved.name = name.clone();
    }
    saved.description_html = html;
    if let Some(priority) = &patch.priority {
        saved.priority = Some(priority.clone());
    }
    if let Some(point) = patch.point {
        saved.point = point;
    }
    if let Some(score) = patch.complexity_score {
        saved.complexity_score = score;
    }
    if let Some(start) = patch.start_date {
        saved.start_date = start;
    }
    if let Some(target) = patch.target_date {
        saved.target_date = target;
    }
    if let Some(sequence) = patch.sequence_id {
        saved.sequence_id = sequence;
    }
    if let Some(sort) = patch.sort_order {
        saved.sort_order = sort;
    }
    saved.completed_at = completed_at;
    if let Some(archived) = patch.archived_at {
        saved.archived_at = archived;
    }
    if let Some(draft) = patch.is_draft {
        saved.is_draft = draft;
    }
    if let Some(source) = &patch.external_source {
        saved.external_source = source.clone();
    }
    if let Some(external) = &patch.external_id {
        saved.external_id = external.clone();
    }
    if let Some(branch) = &patch.git_work_branch {
        saved.git_work_branch = branch.clone();
    }
    if let Some(via) = &patch.created_via {
        saved.created_via = via.clone();
    }
    if let Some(executor) = &patch.agent_executor {
        saved.agent_executor = executor.clone();
    }
    if let Some(created_by) = patch.created_by {
        saved.created_by_id = created_by;
    }
    if let Some(parent) = patch.parent {
        saved.parent_id = parent;
    }
    saved.state_id = state_id;
    if let Some(estimate) = patch.estimate_point {
        saved.estimate_point_id = estimate;
    }
    if let Some(issue_type) = patch.issue_type {
        saved.type_id = issue_type;
    }
    if let Some(pod) = patch.assigned_pod {
        saved.assigned_pod_id = pod;
    }
    if let Some(deleted) = patch.deleted_at {
        saved.deleted_at = deleted;
    }
    saved.description_stripped = stripped;
    saved.updated_at = now_save;
    saved.updated_by_id = Some(*user_id);
    Ok(saved)
}

/// Apply an intake-half patch (`super().update` + `save()`): column
/// writes plus the audit stamp. Returns the post-save row.
async fn save_intake_patch(
    pool: &sqlx::PgPool,
    row: &IntakeIssueRow,
    patch: &IntakePatch,
    user_id: &Uuid,
) -> Result<IntakeIssueRow, Denial> {
    let now = cycle_mod::micros_now();
    sqlx::query(
        r#"UPDATE "intake_issues" SET "status" = $1, "snoozed_till" = $2, "duplicate_to_id" = $3,
           "source" = $4, "source_email" = $5, "updated_at" = $6, "updated_by_id" = $7
           WHERE "id" = $8"#,
    )
    .bind(patch.status.unwrap_or(row.status))
    .bind(patch.snoozed_till.unwrap_or(row.snoozed_till))
    .bind(patch.duplicate_to.unwrap_or(row.duplicate_to_id))
    .bind(patch.source.clone().unwrap_or_else(|| row.source.clone()))
    .bind(
        patch
            .source_email
            .clone()
            .unwrap_or_else(|| row.source_email.clone()),
    )
    .bind(now)
    .bind(user_id)
    .bind(row.id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    let mut saved = row.clone();
    if let Some(status) = patch.status {
        saved.status = status;
    }
    if let Some(snoozed) = patch.snoozed_till {
        saved.snoozed_till = snoozed;
    }
    if let Some(duplicate) = patch.duplicate_to {
        saved.duplicate_to_id = duplicate;
    }
    if let Some(source) = &patch.source {
        saved.source = source.clone();
    }
    if let Some(email) = &patch.source_email {
        saved.source_email = email.clone();
    }
    saved.updated_at = now;
    saved.updated_by_id = Some(*user_id);
    Ok(saved)
}

/// The accept transition (`IntakeIssueUpdateSerializer.update`,
/// `:147-155`): the PRE-SAVE issue cache gets the default state and is
/// saved whole — clobbering any issue-half writes from the same
/// request — with `completed_at` / `description_stripped` recomputed
/// and the audit stamp moved. Returns the post-save (clobbered) row.
async fn apply_triage_transition(
    pool: &sqlx::PgPool,
    pre_save: &IssueRow,
    default_id: &Uuid,
    user_id: &Uuid,
) -> Result<IssueRow, Denial> {
    let now = cycle_mod::micros_now();
    let group = fetch_state_group(pool, default_id).await?;
    let completed_at = if group == "completed" {
        Some(now)
    } else {
        None
    };
    // Same non-adding rule (`issue.save()` rewrites all fields):
    // falsy html → NULL, else the strip result as-is.
    let stripped: Option<String> = if pre_save.description_html.is_empty() {
        None
    } else {
        Some(strip_tags_ml(&pre_save.description_html))
    };
    // The full-row rewrite IS the clobber: every pre-save column is
    // written back, so issue-half writes from this request vanish
    // (`point` included — it is writable via the issue-half).
    sqlx::query(
        r#"UPDATE "issues" SET "name" = $1, "description_json" = $2, "description_html" = $3,
           "description_stripped" = $4, "priority" = $5, "complexity_score" = $6,
           "start_date" = $7, "target_date" = $8, "sequence_id" = $9, "sort_order" = $10,
           "completed_at" = $11, "archived_at" = $12, "is_draft" = $13,
           "external_source" = $14, "external_id" = $15, "git_work_branch" = $16,
           "created_via" = $17, "agent_executor" = $18, "created_by_id" = $19,
           "parent_id" = $20, "state_id" = $21, "estimate_point_id" = $22, "type_id" = $23,
           "assigned_pod_id" = $24, "deleted_at" = $25, "point" = $26,
           "updated_at" = $27, "updated_by_id" = $28 WHERE "id" = $29"#,
    )
    .bind(&pre_save.name)
    .bind(&pre_save.description_json)
    .bind(&pre_save.description_html)
    .bind(&stripped)
    .bind(&pre_save.priority)
    .bind(pre_save.complexity_score)
    .bind(pre_save.start_date)
    .bind(pre_save.target_date)
    .bind(pre_save.sequence_id)
    .bind(pre_save.sort_order)
    .bind(completed_at)
    .bind(pre_save.archived_at)
    .bind(pre_save.is_draft)
    .bind(&pre_save.external_source)
    .bind(&pre_save.external_id)
    .bind(&pre_save.git_work_branch)
    .bind(&pre_save.created_via)
    .bind(&pre_save.agent_executor)
    .bind(pre_save.created_by_id)
    .bind(pre_save.parent_id)
    .bind(default_id)
    .bind(pre_save.estimate_point_id)
    .bind(pre_save.type_id)
    .bind(pre_save.assigned_pod_id)
    .bind(pre_save.deleted_at)
    .bind(pre_save.point)
    .bind(now)
    .bind(user_id)
    .bind(pre_save.id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    let mut saved = pre_save.clone();
    saved.state_id = Some(*default_id);
    saved.completed_at = completed_at;
    saved.description_stripped = stripped;
    saved.updated_at = now;
    saved.updated_by_id = Some(*user_id);
    Ok(saved)
}

/// Instance soft delete (`db/mixins.py:72-78`): `deleted_at` stamp,
/// `save()` (audit moves), then the sweep fan-out (best-effort, like
/// every `.delay()` on these endpoints).
async fn soft_delete_row(
    pool: &sqlx::PgPool,
    table: &str,
    model: &str,
    id: &Uuid,
    user_id: &Uuid,
) -> Result<(), Denial> {
    // `delete()` stamps `deleted_at`, then `save()`'s `auto_now`
    // moves `updated_at` — two clock reads.
    let deleted_at = cycle_mod::micros_now();
    let updated_at = cycle_mod::micros_now();
    let sql = format!(
        "UPDATE \"{table}\" SET deleted_at = $1, updated_at = $2, updated_by_id = $3 WHERE id = $4"
    );
    sqlx::query(&sql)
        .bind(deleted_at)
        .bind(updated_at)
        .bind(user_id)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    cycle_mod::enqueue_soft_delete(pool, model, id).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Create path (`views/intake.py:142-219`)
// ---------------------------------------------------------------------------

/// JSON falsiness (`name` / `description` or-chains): null, false, 0,
/// `""`, `[]`, `{}` — everything else (including `NaN`) is truthy.
fn is_falsy_json(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => {
            number.as_i64().is_some_and(|i| i == 0)
                || number.as_u64().is_some_and(|u| u == 0)
                || number.as_f64().is_some_and(|f| f == 0.0)
        }
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
    }
}

/// Create-path `name`: truthiness was checked by the caller.
/// `CharField.to_python` maps every non-str non-None through `str()`
/// — bools (`True`/`False`), ints, floats (Python float repr, `5.0`
/// stays `"5.0"`), containers (single-quote repr). Over-`varchar(255)`
/// and NUL bytes are the 500 `DataError` at INSERT.
fn coerce_create_name(value: &Value) -> Result<String, Denial> {
    // Null is unreachable (falsy → the 400 above); `py_str` maps it
    // to `"None"` rather than crashing.
    let text = py_str(value);
    if text.contains('\x00') || text.chars().count() > 255 {
        return Err(Denial::ServerError);
    }
    Ok(text)
}

/// Create-path `description_html`: missing defaults to `"<p></p>"`;
/// explicit null violates the non-null column (the `IntegrityError`
/// 400); strings store (NUL bytes are the 500; `text` has no length
/// cap). Every other JSON type 500s — NOT via `to_python`'s `str()`:
/// `Issue.save()` runs `strip_tags(html)` first, and
/// `HTMLParser.feed` raises `TypeError` on non-str (verified live:
/// bool/int/float/list/dict all 500).
fn coerce_create_html(value: Option<&Value>) -> Result<String, Denial> {
    let Some(value) = value else {
        return Ok("<p></p>".to_owned());
    };
    if value.is_null() {
        return Err(Denial::PayloadInvalid);
    }
    let Value::String(text) = value else {
        return Err(Denial::ServerError);
    };
    if text.contains('\x00') {
        return Err(Denial::ServerError);
    }
    Ok(text.clone())
}

/// `jsonb` rejects non-finite floats (`invalid input syntax` →
/// `DataError` → 500), at any nesting depth.
fn json_has_nonfinite(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_f64().is_some_and(|float| !float.is_finite()),
        Value::Array(items) => items.iter().any(json_has_nonfinite),
        Value::Object(map) => map.values().any(json_has_nonfinite),
        _ => false,
    }
}

/// Triage get-or-create (`views/intake.py:173-184`): the lookup by
/// `(project, slug)` first; the insert on a miss (a lost race hits the
/// unique `name`+`project` row → the `IntegrityError` 400). The insert
/// carries the `State.save` resequence (`max + 15000` over the live
/// non-triage project states, `65000` when there are none) and the
/// crum `created_by` (the request user).
async fn ensure_triage(
    pool: &sqlx::PgPool,
    slug: &str,
    project: &ProjectRow,
    user_id: &Uuid,
) -> Result<Uuid, Denial> {
    // The pinned lookup selects `*`; read `id` by name off the row
    // (`query_scalar` cannot decode a multi-column row).
    let existing: Option<Uuid> = sqlx::query(&intake_queries::triage_lookup_sql())
        .bind(project.id)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?
        .map(|row: sqlx::postgres::PgRow| row.try_get("id"))
        .transpose()
        .map_err(|_| Denial::ServerError)?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let max_sequence: Option<f64> = sqlx::query_scalar(&intake_queries::triage_max_sequence_sql())
        .bind(project.id)
        .fetch_one(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let sequence = max_sequence
        .map(|max| max + 15000.0)
        .unwrap_or(intake_queries::TRIAGE_SEQUENCE_FALLBACK);
    let created_at = cycle_mod::micros_now();
    let updated_at = cycle_mod::micros_now();
    let result = pidash_services::domain::projects::states::add_triage(
        pool,
        project.id,
        project.workspace_id,
        sequence,
        *user_id,
        created_at,
        updated_at,
        pidash_services::domain::projects::states::TriageText::Full,
    )
    .await;
    match result {
        Ok(id) => Ok(id),
        Err(error) if is_unique_violation(&error) => Err(Denial::PayloadInvalid),
        Err(_) => Err(Denial::ServerError),
    }
}

/// `convert_uuid_to_integer` (`utils/uuid.py`): the advisory-lock key —
/// signed big-endian `int64` of the `sha256(str(uuid))` prefix.
fn advisory_key(project_id: &Uuid) -> i64 {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(project_id.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

/// `Pod.default_for_project_id` (`runner/models.py`): the live default
/// pod, earliest first — or `None` (raw-SQL-seeded projects have none:
/// the `post_save` signal only fires on ORM creates).
async fn default_pod_for_project(
    pool: &sqlx::PgPool,
    project_id: &Uuid,
) -> Result<Option<Uuid>, Denial> {
    sqlx::query_scalar(
        r#"SELECT "id" FROM "pod" WHERE "project_id" = $1 AND "is_default" = TRUE
           AND "deleted_at" IS NULL ORDER BY "created_at" ASC LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)
}

/// `Issue.objects.create(...)` (`views/intake.py:186-194` +
/// `Issue.save()` adding branch): the transaction-scoped advisory lock
/// is taken first, then `sequence_id` from the `issue_sequences`
/// counter (`max+1`, falsy → 1) with its new `IssueSequence` row and
/// `sort_order = max(project, state) + 10000` when siblings exist — all
/// on one transaction (`db/models/issue.py:311-336`), so concurrent
/// creates serialize per project. The lock must live on the
/// transaction: taken on the pool it would drop as soon as its own
/// autocommit statement ends (PIDASHCONV-1406). Pod defaulting stays
/// outside: Django resolves `assigned_pod` before `atomic()`
/// (`issue.py:277-284`). Tag stripping as-is (no `or None` on this
/// path), workspace from the project, crum audit.
#[allow(clippy::too_many_arguments)]
async fn insert_issue(
    pool: &sqlx::PgPool,
    name: &str,
    description_json: &Value,
    description_html: &str,
    priority: &str,
    project: &ProjectRow,
    triage_id: &Uuid,
    user_id: &Uuid,
) -> Result<IssueRow, Denial> {
    let pod_id = default_pod_for_project(pool, &project.id).await?;
    let mut tx = pool.begin().await.map_err(|_| Denial::ServerError)?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(advisory_key(&project.id))
        .execute(&mut *tx)
        .await
        .map_err(|_| Denial::ServerError)?;
    let last_sequence: Option<Option<i64>> = sqlx::query_scalar(
        r#"SELECT MAX("sequence") FROM "issue_sequences" WHERE "project_id" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(project.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| Denial::ServerError)?;
    // `last_sequence + 1 if last_sequence else 1`: `0` and `None` both
    // restart at 1 (falsy either way).
    let sequence_id = i32::try_from(
        last_sequence
            .flatten()
            .filter(|last| *last != 0)
            .map(|last| last + 1)
            .unwrap_or(1),
    )
    .map_err(|_| Denial::ServerError)?;
    let max_sort: Option<f64> = sqlx::query_scalar(
        r#"SELECT MAX("sort_order") FROM "issues"
           WHERE "project_id" = $1 AND "state_id" = $2 AND "deleted_at" IS NULL"#,
    )
    .bind(project.id)
    .bind(triage_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| Denial::ServerError)?
    .flatten();
    let sort_order = max_sort.map_or(65535.0, |max| max + 10000.0);
    // `Issue.save()` adding branch: falsy html → NULL, else the strip
    // result as-is (even `""` — no `or None` on this path).
    let stripped: Option<String> = if description_html.is_empty() {
        None
    } else {
        Some(strip_tags_ml(description_html))
    };

    let id = Uuid::new_v4();
    let created_at = cycle_mod::micros_now();
    let updated_at = cycle_mod::micros_now();
    sqlx::query(
        r#"INSERT INTO "issues" ("id", "created_at", "updated_at", "created_by_id", "updated_by_id",
           "deleted_at", "project_id", "workspace_id", "parent_id", "state_id", "point",
           "estimate_point_id", "name", "description_json", "description_html",
           "description_stripped", "description_binary", "priority", "complexity_score",
           "start_date", "target_date", "sequence_id", "sort_order", "completed_at",
           "archived_at", "is_draft", "external_source", "external_id", "type_id",
           "git_work_branch", "workpad", "created_via", "assigned_pod_id", "agent_executor")
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, NULL, $7, NULL, NULL, $8, $9, $10,
           $11, NULL, $12, 0, NULL, NULL, $13, $14, NULL, NULL, FALSE, NULL, NULL, NULL,
           '', '', NULL, $15, NULL)"#,
    )
    .bind(id)
    .bind(created_at)
    .bind(updated_at)
    .bind(user_id)
    .bind(project.id)
    .bind(project.workspace_id)
    .bind(triage_id)
    .bind(name)
    .bind(description_json)
    .bind(description_html)
    .bind(&stripped)
    .bind(priority)
    .bind(sequence_id)
    .bind(sort_order)
    .bind(pod_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| Denial::ServerError)?;

    let sequence_created = cycle_mod::micros_now();
    let sequence_updated = cycle_mod::micros_now();
    sqlx::query(
        r#"INSERT INTO "issue_sequences" ("id", "created_at", "updated_at", "created_by_id",
           "updated_by_id", "deleted_at", "issue_id", "sequence", "project_id", "workspace_id",
           "deleted") VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, FALSE)"#,
    )
    .bind(Uuid::new_v4())
    .bind(sequence_created)
    .bind(sequence_updated)
    .bind(user_id)
    .bind(id)
    .bind(i64::from(sequence_id))
    .bind(project.id)
    .bind(project.workspace_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| Denial::ServerError)?;
    // The intake-bridge insert and the activity enqueue in the caller
    // run after this commit: Django's view is not atomic beyond
    // `Issue.save`, so they stay outside this transaction
    // (`views/intake.py:200-207`).
    tx.commit().await.map_err(|_| Denial::ServerError)?;

    Ok(IssueRow {
        id,
        created_at,
        updated_at,
        deleted_at: None,
        point: None,
        name: name.to_owned(),
        description_json: description_json.clone(),
        description_html: description_html.to_owned(),
        description_stripped: stripped,
        description_binary: None,
        priority: Some(priority.to_owned()),
        complexity_score: Some(0),
        start_date: None,
        target_date: None,
        sequence_id,
        sort_order,
        completed_at: None,
        archived_at: None,
        is_draft: false,
        external_source: None,
        external_id: None,
        git_work_branch: String::new(),
        created_via: None,
        agent_executor: None,
        created_by_id: Some(*user_id),
        updated_by_id: None,
        project_id: project.id,
        workspace_id: project.workspace_id,
        parent_id: None,
        state_id: Some(*triage_id),
        estimate_point_id: None,
        type_id: None,
        assigned_pod_id: pod_id,
    })
}

/// `IntakeIssue.objects.create(intake_id, project_id, issue,
/// source=IN_APP)` (`:196-201`): `status -2`, `extra {}`, workspace
/// from the project, crum audit.
async fn insert_intake_issue(
    pool: &sqlx::PgPool,
    intake_id: &Uuid,
    project: &ProjectRow,
    issue_id: &Uuid,
    user_id: &Uuid,
) -> Result<IntakeIssueRow, Denial> {
    let id = Uuid::new_v4();
    let created_at = cycle_mod::micros_now();
    let updated_at = cycle_mod::micros_now();
    sqlx::query(
        r#"INSERT INTO "intake_issues" ("id", "created_at", "updated_at", "created_by_id",
           "updated_by_id", "deleted_at", "project_id", "workspace_id", "intake_id", "issue_id",
           "status", "snoozed_till", "duplicate_to_id", "source", "source_email",
           "external_source", "external_id", "extra")
           VALUES ($1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, -2, NULL, NULL, 'IN_APP',
           NULL, NULL, NULL, '{}')"#,
    )
    .bind(id)
    .bind(created_at)
    .bind(updated_at)
    .bind(user_id)
    .bind(project.id)
    .bind(project.workspace_id)
    .bind(intake_id)
    .bind(issue_id)
    .execute(pool)
    .await
    .map_err(|_| Denial::ServerError)?;

    Ok(IntakeIssueRow {
        id,
        created_at,
        updated_at,
        created_by_id: Some(*user_id),
        updated_by_id: None,
        deleted_at: None,
        project_id: project.id,
        workspace_id: project.workspace_id,
        intake_id: *intake_id,
        issue_id: *issue_id,
        status: -2,
        snoozed_till: None,
        duplicate_to_id: None,
        source: Some("IN_APP".to_owned()),
        source_email: None,
        external_source: None,
        external_id: None,
        extra: Value::Object(Map::new()),
    })
}

/// `base_host(request, is_app=True)` (`utils/host.py`): `APP_BASE_URL`
/// else `WEB_URL`; both empty is the 500 `ImproperlyConfigured`.
fn app_origin(state: &AppState) -> Result<String, Denial> {
    let urls = &state.settings().urls;
    for url in [&urls.app_base_url, &urls.web_url].into_iter().flatten() {
        if !url.is_empty() {
            return Ok(url.clone());
        }
    }
    Err(Denial::ServerError)
}

/// The list `COUNT(*)` over the fixture scope: same joins + predicates
/// as [`intake_issue_list_sql`], order stripped (Django's `.count()` —
/// plain `COUNT(*)`, no `DISTINCT`).
fn intake_issue_count_sql(list_sql: &str) -> String {
    let from = list_sql
        .find(" FROM \"intake_issues\"")
        .expect("list sql carries the intake_issues from clause");
    let mut count = format!("SELECT COUNT(*){}", &list_sql[from..]);
    if let Some(order) = count.find(" ORDER BY ") {
        count.truncate(order);
    }
    count
}

// ---------------------------------------------------------------------------
// Unit 1 — list (`views/intake.py:106-119`)
// ---------------------------------------------------------------------------

/// `get`: project 404 → intake lookup → OR-`.none()` → `paginate` with
/// `fields`/`expand` (per-page/cursor `ParseError`s fire even on the
/// empty path — `get_queryset` runs before `paginate`).
async fn intake_list(
    State(state): State<AppState>,
    Path((slug, raw_project_id)): Path<(String, String)>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> HandlerResult {
    let ctx = context(&state, &headers, &slug, &raw_project_id).await?;
    let project = fetch_project(&ctx.pool, &ctx.project_id).await?;
    let intake_id = fetch_intake_id(&ctx.pool, &slug, &ctx.project_id).await?;
    let fields = fields_param(&params, "fields");
    let expand = fields_param(&params, "expand");
    let per_page =
        crate::paginator::parse_per_page(query_last(&params, "per_page").as_deref(), 1000, 1000)
            .map_err(page_denial)?;
    let cursor_raw = query_last(&params, "cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
    let cursor =
        Cursor::from_string(&cursor_raw).map_err(|error| Denial::BadDetail(error.detail()))?;
    let limit = per_page.min(1000);
    let window = offset_window(limit, cursor.offset, cursor.value, cursor.is_prev, None)
        .map_err(page_denial)?;

    // The OR-`.none()` (`:72-74`): intake missing OR view disabled.
    let empty = intake_id.is_none() || !project.intake_view;
    let mut list_sql = intake_queries::intake_issue_list_sql(None);
    list_sql.push_str(&format!(
        " LIMIT {} OFFSET {}",
        window.stop - window.offset,
        window.offset
    ));
    let count_sql = intake_issue_count_sql(&intake_queries::intake_issue_list_sql(None));
    let now = cycle_mod::micros_now();
    let (rows, total_count) = if empty {
        (Vec::new(), 0)
    } else {
        let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(&list_sql)
            .bind(now)
            .bind(intake_id)
            .bind(ctx.project_id)
            .bind(&slug)
            .fetch_all(&ctx.pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        let total_count: i64 = sqlx::query_scalar(&count_sql)
            .bind(now)
            .bind(intake_id)
            .bind(ctx.project_id)
            .bind(&slug)
            .fetch_optional(&ctx.pool)
            .await
            .map_err(|_| Denial::ServerError)?
            .unwrap_or(0);
        (rows, total_count)
    };
    let has_more = rows.len() as i64 > limit;
    let mut decoded = Vec::with_capacity(rows.len());
    for row in &rows {
        decoded.push((
            IntakeIssueRow::from_joined(row)?,
            IssueRow::from_joined(row)?,
        ));
    }
    let page = apply_offset_window(&decoded, limit).map_err(page_denial)?;
    let next = next_cursor(limit, window.page, has_more);
    let prev = prev_cursor(limit, window.page);
    let mut rendered = Vec::with_capacity(page.len());
    for (intake_row, issue) in &page {
        let value = render_intake_issue(
            &state,
            &ctx.pool,
            &slug,
            intake_row,
            issue,
            &ctx.timezone,
            fields.as_deref(),
            expand.as_deref(),
        )
        .await?;
        let text = serde_json::to_string(&value).expect("row serializes");
        rendered.push(fix_nonfinite_sort(&text, issue.sort_order));
    }
    let body = envelope(
        None,
        None,
        total_count,
        &next.to_string(),
        &prev.to_string(),
        next.has_results_or_false(),
        prev.has_results_or_false(),
        rendered.len(),
        max_hits(total_count, limit).map_err(page_denial)?,
        total_count,
        &format!("[{}]", rendered.join(",")),
    );
    Ok(json_response(StatusCode::OK, body))
}

// ---------------------------------------------------------------------------
// Unit 2 — create (`views/intake.py:142-219`)
// ---------------------------------------------------------------------------

/// `post`: body-dict 500 → name 400 → intake lookup → project 404 → the
/// AND guard 400 → priority 400 → triage get-or-create → `Issue.create`
/// → `IntakeIssue.create` (the `intake.id` 500 lands between the two
/// inserts) → activity → 201 (full shape, NO `fields`/`expand`).
async fn intake_create(
    State(state): State<AppState>,
    Path((slug, raw_project_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> HandlerResult {
    let ctx = context(&state, &headers, &slug, &raw_project_id).await?;
    let parsed = parse_json_body(&body, &headers, state.settings().file_size_limit)?;
    let Value::Object(data) = parsed.value().clone() else {
        // `.get` on a non-dict (`None`, list, scalar) is the 500
        // `AttributeError`.
        return Err(Denial::ServerError);
    };
    let empty_issue = Value::Object(Map::new());
    let issue_data = data.get("issue").unwrap_or(&empty_issue);
    let Value::Object(issue_data) = issue_data else {
        return Err(Denial::ServerError);
    };
    let false_value = Value::Bool(false);
    let name_value = issue_data.get("name").unwrap_or(&false_value);
    if is_falsy_json(name_value) {
        return Err(Denial::NameRequired);
    }

    let intake_id = fetch_intake_id(&ctx.pool, &slug, &ctx.project_id).await?;
    let project = fetch_project(&ctx.pool, &ctx.project_id).await?;
    // The AND guard (`:156`): intake-missing + view-enabled falls
    // through to the `None.id` 500 below.
    if intake_id.is_none() && !project.intake_view {
        return Err(Denial::IntakeDisabled);
    }
    // The priority allowlist is verbatim and lowercase-exact; missing
    // defaults to `"none"`, explicit null 400s.
    let priority = match issue_data.get("priority") {
        None => "none".to_owned(),
        Some(Value::String(priority))
            if matches!(
                priority.as_str(),
                "low" | "medium" | "high" | "urgent" | "none"
            ) =>
        {
            priority.clone()
        }
        _ => return Err(Denial::InvalidPriority),
    };

    let triage_id = ensure_triage(&ctx.pool, &slug, &project, &ctx.user_id).await?;
    let description_json = issue_data
        .get("description")
        .filter(|value| !is_falsy_json(value))
        .or_else(|| {
            issue_data
                .get("description_json")
                .filter(|value| !is_falsy_json(value))
        })
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    if json_has_nonfinite(&description_json) {
        return Err(Denial::ServerError);
    }
    // Both length/NUL `DataError`s fire at INSERT (`:190`) — after
    // the guard, the priority check and the triage get-or-create.
    let name = coerce_create_name(name_value)?;
    let description_html = coerce_create_html(issue_data.get("description_html"))?;
    let issue = insert_issue(
        &ctx.pool,
        &name,
        &description_json,
        &description_html,
        &priority,
        &project,
        &triage_id,
        &ctx.user_id,
    )
    .await?;
    // `intake_id=intake.id` (`:197`): the AND-guard fallthrough 500s
    // here, AFTER the issue row persists.
    let intake_id = intake_id.ok_or(Denial::ServerError)?;
    let intake_row =
        insert_intake_issue(&ctx.pool, &intake_id, &project, &issue.id, &ctx.user_id).await?;

    let ids = intake_tasks::IntakeActivityIds::new(
        &ctx.user_id.to_string(),
        &issue.id.to_string(),
        &ctx.project_id.to_string(),
        &intake_row.id.to_string(),
    );
    let job = intake_tasks::intake_issue_created_job(
        &py_dumps(&Value::Object(data)),
        &ids,
        Utc::now().timestamp(),
    );
    cycle_mod::enqueue_best_effort(&ctx.pool, &job).await;

    let value = render_intake_issue(
        &state,
        &ctx.pool,
        &slug,
        &intake_row,
        &issue,
        &ctx.timezone,
        None,
        None,
    )
    .await?;
    let text = serde_json::to_string(&value).expect("row serializes");
    Ok(json_response(StatusCode::CREATED, text))
}

// ---------------------------------------------------------------------------
// Unit 3 — retrieve (`views/intake.py:272-279`)
// ---------------------------------------------------------------------------

/// `get`: the scoped `.get(issue_id)` (0 → 404, 2+ → 500) with
/// `fields`/`expand`. No AND guard — a disabled view 404s through the
/// OR-`.none()`.
async fn intake_retrieve(
    State(state): State<AppState>,
    Path((slug, raw_project_id, raw_issue_id)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> HandlerResult {
    // URL-resolve precedence: Django's `<uuid:issue_id>` converter runs
    // before auth. Canonical serves; parseable-but-non-canonical proxies so
    // Django's resolver 404 answers; garbage keeps the prod resolver body.
    let issue_id = match classify_uuid_segment(&raw_issue_id) {
        UuidSegment::Canonical(id) => id,
        UuidSegment::NonCanonical => {
            return Ok(proxy_through(state, method, uri, headers, body).await);
        }
        UuidSegment::Garbage => return Err(Denial::PageNotFound),
    };
    let ctx = context(&state, &headers, &slug, &raw_project_id).await?;
    let project = fetch_project(&ctx.pool, &ctx.project_id).await?;
    let intake_id = fetch_intake_id(&ctx.pool, &slug, &ctx.project_id).await?;
    if intake_id.is_none() || !project.intake_view {
        return Err(Denial::ResourceMissing);
    }
    let fields = fields_param(&params, "fields");
    let expand = fields_param(&params, "expand");
    // `.get()` fetches unbounded (the fixture's `LIMIT 1` is the
    // `.first()` shape, not `.get()`'s) — strip it so duplicates 500.
    let detail_sql = intake_queries::intake_issue_detail_sql(None);
    let get_sql = detail_sql
        .strip_suffix(" LIMIT 1")
        .expect("detail sql ends with limit 1");
    let now = cycle_mod::micros_now();
    let rows: Vec<sqlx::postgres::PgRow> = sqlx::query(get_sql)
        .bind(now)
        .bind(intake_id)
        .bind(ctx.project_id)
        .bind(&slug)
        .bind(issue_id)
        .fetch_all(&ctx.pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let row = match rows.len() {
        0 => return Err(Denial::ResourceMissing),
        1 => &rows[0],
        _ => return Err(Denial::ServerError),
    };
    let intake_row = IntakeIssueRow::from_joined(row)?;
    let issue = IssueRow::from_joined(row)?;
    let value = render_intake_issue(
        &state,
        &ctx.pool,
        &slug,
        &intake_row,
        &issue,
        &ctx.timezone,
        fields.as_deref(),
        expand.as_deref(),
    )
    .await?;
    let text = serde_json::to_string(&value).expect("row serializes");
    Ok(json_response(
        StatusCode::OK,
        fix_nonfinite_sort(&text, issue.sort_order),
    ))
}

// ---------------------------------------------------------------------------
// Unit 4 — update (`views/intake.py:303-434`)
// ---------------------------------------------------------------------------

/// `patch`: preamble → author 400 → `issue` pop → issue-half
/// (annotated fetch, guest whitelist, partial `IssueSerializer`) →
/// intake-half (`role > 15`, accept guard) → issue activity → issue
/// save → intake snapshot → intake save (+ triage transition, which
/// clobbers the issue-half writes) → status activity (origin 500s
/// AFTER the save) → 200 re-serialized (stale-cache rules apply).
async fn intake_update(
    State(state): State<AppState>,
    Path((slug, raw_project_id, raw_issue_id)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> HandlerResult {
    let issue_id = match classify_uuid_segment(&raw_issue_id) {
        UuidSegment::Canonical(id) => id,
        UuidSegment::NonCanonical => {
            return Ok(proxy_through(state, method, uri, headers, body).await);
        }
        UuidSegment::Garbage => return Err(Denial::PageNotFound),
    };
    let ctx = context(&state, &headers, &slug, &raw_project_id).await?;
    let intake_id = fetch_intake_id(&ctx.pool, &slug, &ctx.project_id).await?;
    let project = fetch_project(&ctx.pool, &ctx.project_id).await?;
    if intake_id.is_none() && !project.intake_view {
        return Err(Denial::IntakeDisabled);
    }
    // `intake_id=intake.id` inside the `.get` kwargs: the AND-guard
    // fallthrough 500s before the query runs.
    let intake_id = intake_id.ok_or(Denial::ServerError)?;
    let intake_row =
        get_intake_issue(&ctx.pool, &issue_id, &slug, &ctx.project_id, &intake_id).await?;
    let role = get_member_role(&ctx.pool, &slug, &ctx.project_id, &ctx.user_id).await?;
    let is_author = intake_row.created_by_id == Some(ctx.user_id);
    if v1_perm::patch_edit_gate(i32::from(role), is_author).is_err() {
        return Err(Denial::EditDenied);
    }

    // `request.data` is lazy — the first touch is the `pop` at `:344`,
    // AFTER every guard above, so malformed bodies lose to them.
    let parsed = parse_json_body(&body, &headers, state.settings().file_size_limit)?;
    if pops_immutable_querydict(&parsed, &body, &headers) {
        // `.pop` on the immutable `QueryDict` is the `AttributeError` 500.
        return Err(Denial::ServerError);
    }
    let Value::Object(mut data) = parsed.value().clone() else {
        // `.pop` on a non-dict (list → `TypeError`, else
        // `AttributeError`) is the 500.
        return Err(Denial::ServerError);
    };
    // `request.data.pop("issue", False)` — the pop MUTATES the data
    // the status-half activity dumps later.
    let issue_data = data.remove("issue");
    let mut issue_patch: Option<IssuePatch> = None;
    let mut issue_dump: Option<Map<String, Value>> = None;
    let mut pre_save: Option<IssueRow> = None;
    if let Some(issue_data) = issue_data {
        if !is_falsy_json(&issue_data) {
            let fetched = get_patch_issue(&ctx.pool, &issue_id, &slug, &ctx.project_id).await?;
            let narrowed;
            let issue_map = if v1_perm::guest_issue_narrowed(i32::from(role)) {
                // Guests whitelist to name + description; a non-dict
                // crashes on `.get` (500).
                let Value::Object(raw) = &issue_data else {
                    return Err(Denial::ServerError);
                };
                let description_json = raw
                    .get("description")
                    .filter(|value| !is_falsy_json(value))
                    .or_else(|| {
                        raw.get("description_json")
                            .filter(|value| !is_falsy_json(value))
                    })
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Map::new()));
                let mut map = Map::with_capacity(3);
                map.insert(
                    "name".to_owned(),
                    raw.get("name")
                        .cloned()
                        .unwrap_or_else(|| Value::String(fetched.name.clone())),
                );
                map.insert(
                    "description_html".to_owned(),
                    raw.get("description_html")
                        .cloned()
                        .unwrap_or_else(|| Value::String(fetched.description_html.clone())),
                );
                map.insert("description_json".to_owned(), description_json);
                narrowed = map;
                &narrowed
            } else if let Value::Object(raw) = &issue_data {
                raw
            } else {
                // A non-dict reaches `IssueSerializer(data=...)` →
                // the non-dict `non_field_errors`.
                return Err(Denial::FieldErrors(single_non_field(&format!(
                    "Invalid data. Expected a dictionary, but got {}.",
                    json_type_name(&issue_data)
                ))));
            };
            let normalized = match normalize_description_input(issue_map) {
                Ok(normalized) => normalized,
                Err(failure) => return Err(Denial::FieldErrors(failure.body())),
            };
            let patch = validate_issue_input(
                &ctx.pool,
                &normalized.data,
                normalized.from_markdown,
                &ctx.timezone,
                &fetched,
            )
            .await?;
            issue_dump = Some(issue_map.clone());
            pre_save = Some(fetched);
            issue_patch = Some(patch);
        }
    }

    let mut intake_patch: Option<IntakePatch> = None;
    // The companion is the `_base_manager` (unfiltered) issue: the
    // guard, the transition and the snapshots all read through it.
    let companion = fetch_companion_issue(&ctx.pool, &issue_id).await?;
    if v1_perm::intake_fields_writable(i32::from(role)) {
        let patch =
            validate_intake_input(&ctx.pool, &data, &ctx.timezone, &intake_row, &companion).await?;
        intake_patch = Some(patch);
    }

    let ids = intake_tasks::IntakeActivityIds::new(
        &ctx.user_id.to_string(),
        &issue_id.to_string(),
        &ctx.project_id.to_string(),
        &intake_row.id.to_string(),
    );
    if let (Some(patch), Some(pre_save_issue)) = (&issue_patch, &pre_save) {
        let current =
            render_issue_full(&state, &ctx.pool, &slug, pre_save_issue, &ctx.timezone).await?;
        let current_text = fix_nonfinite_sort(
            &serde_json::to_string(&current).expect("snapshot serializes"),
            pre_save_issue.sort_order,
        );
        let requested = py_dumps(&Value::Object(issue_dump.clone().unwrap_or_default()));
        let job = intake_tasks::intake_issue_updated_job(
            &requested,
            &ids,
            &current_text,
            Utc::now().timestamp(),
        );
        cycle_mod::enqueue_best_effort(&ctx.pool, &job).await;
        save_issue_patch(&ctx.pool, pre_save_issue, patch, &ctx.user_id).await?;
    }

    // `views/intake.py:416` serializes `IntakeIssueSerializer(intake_issue)`
    // AFTER `issue_serializer.save()` (`:412`), so the nested `.issue`
    // relation resolves to the post-issue-save companion — UNLESS intake
    // validation already cached it (`validate()` touches
    // `intake_issue.issue` when `status == 1` validates;
    // `serializers/intake.py:120-122`), in which case the cached PRE-save
    // issue renders. The response `stale_cache` below models the same
    // split; the job serializes before the transition, so it takes no
    // `transitioned` leg.
    let validate_cached_issue = intake_patch
        .as_ref()
        .is_some_and(|patch| patch.status == Some(1));
    let job_issue = if issue_patch.is_some() && !validate_cached_issue {
        fetch_companion_issue(&ctx.pool, &issue_id).await?
    } else {
        companion.clone()
    };
    let pre_snapshot = render_intake_issue(
        &state,
        &ctx.pool,
        &slug,
        &intake_row,
        &job_issue,
        &ctx.timezone,
        None,
        None,
    )
    .await?;
    let pre_text = fix_nonfinite_sort(
        &serde_json::to_string(&pre_snapshot).expect("snapshot serializes"),
        job_issue.sort_order,
    );
    let mut saved_row = intake_row.clone();
    let mut transitioned = false;
    if let Some(patch) = &intake_patch {
        saved_row = save_intake_patch(&ctx.pool, &intake_row, patch, &ctx.user_id).await?;
        if patch.status == Some(1) {
            let group = fetch_state_group_optional(&ctx.pool, &companion.state_id).await?;
            let default_id =
                fetch_default_state(&ctx.pool, &companion.workspace_id, &ctx.project_id).await?;
            let default_text = default_id.map(|id| id.to_string());
            if intake_types::resolve_triage_transition(
                patch.status,
                group.as_deref(),
                default_text.as_deref(),
            )
            .is_some()
            {
                if let Some(default_id) = default_id {
                    apply_triage_transition(&ctx.pool, &companion, &default_id, &ctx.user_id)
                        .await?;
                    transitioned = true;
                }
            }
        }
        // The origin resolves AFTER the intake save: unconfigured
        // hosts 500 with the row already written.
        let origin = app_origin(&state)?;
        let job = intake_tasks::intake_status_changed_job(
            &py_dumps(&Value::Object(data)),
            &ids,
            &pre_text,
            Utc::now().timestamp(),
            &origin,
        );
        cycle_mod::enqueue_best_effort(&ctx.pool, &job).await;
    }

    // The response re-serializes post-save state — except the stale
    // cache: with `status == 1` validated and the issue-half run, the
    // guard already cached the PRE-SAVE issue, so `issue_detail`
    // renders pre-save fields (plus the transitioned state, which the
    // clobber wrote back — a re-fetch coincides there).
    let stale_cache = intake_patch
        .as_ref()
        .is_some_and(|patch| patch.status == Some(1))
        && issue_patch.is_some()
        && !transitioned;
    let response_issue = if stale_cache {
        pre_save.unwrap_or(companion)
    } else {
        fetch_companion_issue(&ctx.pool, &issue_id).await?
    };
    let value = render_intake_issue(
        &state,
        &ctx.pool,
        &slug,
        &saved_row,
        &response_issue,
        &ctx.timezone,
        None,
        None,
    )
    .await?;
    let text = serde_json::to_string(&value).expect("row serializes");
    Ok(json_response(
        StatusCode::OK,
        fix_nonfinite_sort(&text, response_issue.sort_order),
    ))
}

/// Nullable-`state_id` group read for the transition check (the guard
/// already 404'd a dangling id during validation; a race resolves to
/// the 500, since `update()` runs outside the guard's `try`).
async fn fetch_state_group_optional(
    pool: &sqlx::PgPool,
    state_id: &Option<Uuid>,
) -> Result<Option<String>, Denial> {
    let Some(id) = state_id else {
        return Ok(None);
    };
    let group: Option<String> = sqlx::query_scalar(r#"SELECT "group" FROM states WHERE id = $1"#)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(group)
}

// ---------------------------------------------------------------------------
// Unit 5 — delete (`views/intake.py:449-494`)
// ---------------------------------------------------------------------------

/// `delete`: preamble → status-set cascade (issue `first()` 500s on a
/// miss; creator-or-admin 403) → issue soft delete + sweep → intake
/// soft delete + sweep → 204. Accepted rows skip the cascade entirely
/// (no issue fetch, no guard).
async fn intake_destroy(
    State(state): State<AppState>,
    Path((slug, raw_project_id, raw_issue_id)): Path<(String, String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> HandlerResult {
    let issue_id = match classify_uuid_segment(&raw_issue_id) {
        UuidSegment::Canonical(id) => id,
        UuidSegment::NonCanonical => {
            return Ok(proxy_through(state, method, uri, headers, Bytes::new()).await);
        }
        UuidSegment::Garbage => return Err(Denial::PageNotFound),
    };
    let ctx = context(&state, &headers, &slug, &raw_project_id).await?;
    let intake_id = fetch_intake_id(&ctx.pool, &slug, &ctx.project_id).await?;
    let project = fetch_project(&ctx.pool, &ctx.project_id).await?;
    if intake_id.is_none() && !project.intake_view {
        return Err(Denial::IntakeDisabled);
    }
    let intake_id = intake_id.ok_or(Denial::ServerError)?;
    let intake_row =
        get_intake_issue(&ctx.pool, &issue_id, &slug, &ctx.project_id, &intake_id).await?;
    if v1_perm::destroy_cascades_to_issue(intake_row.status) {
        // `views/intake.py:477`: `Issue.objects.filter(workspace__slug=slug,
        // project_id=project_id, pk=issue_id).first()` — scoped to the
        // workspace and project, not id alone; an issue moved to another
        // project is a miss. `.first()` -> `None`, and the caller then reads
        // `issue.created_by_id` (`:478`) -> AttributeError -> 500, no writes.
        let sql = format!(
            "SELECT {cols} FROM issues \
             INNER JOIN workspaces w ON w.id = issues.workspace_id \
             WHERE issues.id = $1 AND w.slug = $2 AND issues.project_id = $3 \
             AND issues.deleted_at IS NULL",
            cols = issue_cols_qualified(),
        );
        let row: Option<sqlx::postgres::PgRow> = sqlx::query(&sql)
            .bind(issue_id)
            .bind(&slug)
            .bind(ctx.project_id)
            .fetch_optional(&ctx.pool)
            .await
            .map_err(|_| Denial::ServerError)?;
        let row = row.ok_or(Denial::ServerError)?;
        let issue = IssueRow::from_row(&row)?;
        let is_creator = issue.created_by_id == Some(ctx.user_id);
        let is_admin: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM project_members pm
               JOIN workspaces w ON w.id = pm.workspace_id
               WHERE w.slug = $1 AND pm.member_id = $2 AND pm.project_id = $3
               AND pm.role = 20 AND pm.is_active AND pm.deleted_at IS NULL)"#,
        )
        .bind(&slug)
        .bind(ctx.user_id)
        .bind(ctx.project_id)
        .fetch_optional(&ctx.pool)
        .await
        .map_err(|_| Denial::ServerError)?
        .unwrap_or(false);
        if v1_perm::delete_guard(is_creator, is_admin).is_err() {
            return Err(Denial::DeleteDenied);
        }
        soft_delete_row(&ctx.pool, "issues", "issue", &issue.id, &ctx.user_id).await?;
    }
    soft_delete_row(
        &ctx.pool,
        "intake_issues",
        "intakeissue",
        &intake_row.id,
        &ctx.user_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Python `str.strip` / `re-\s` ranges (29 chars; verified identical
/// sets on this repo's Python 3.12 — `/tmp/md_gen_tables.py`). Shared
/// by the markdown layer and the datetime `\s*` gap. Differs from
/// Rust's `trim` only on `\x1c`-`\x1f`.
const STRIP_RANGES: &[(u32, u32)] = &[
    (0x9, 0xD),
    (0x1C, 0x20),
    (0x85, 0x85),
    (0xA0, 0xA0),
    (0x1680, 0x1680),
    (0x2000, 0x200A),
    (0x2028, 0x2029),
    (0x202F, 0x202F),
    (0x205F, 0x205F),
    (0x3000, 0x3000),
];

/// Python strip-whitespace membership.
fn is_py_strip_char(ch: char) -> bool {
    let code = ch as u32;
    STRIP_RANGES
        .binary_search_by(|&(lo, hi)| {
            if code < lo {
                std::cmp::Ordering::Greater
            } else if code > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

// ---------------------------------------------------------------------------
// Markdown → Tiptap HTML (`pi_dash/utils/markdown_converter.py:192-469`)
// ---------------------------------------------------------------------------

/// markdown-it 4.2.0 (`commonmark` preset + `table` + `strikethrough` +
/// `tasklists` plugin, `html=False`) transliterated to emit exactly what
/// `markdown_to_html` returns, byte for byte.
///
/// Positions are char indices throughout (Python strings index by char;
/// sources are decoded to `Vec<char>` at each parse boundary). Options
/// the converter never sets (`alerts`, `tasklists`, `inline_definitions`,
/// `store_labels`, `strikethrough_single_tilde`, `linkify`, `typographer`,
/// `breaks`, `highlight`) are hardcoded to their defaults; rules the
/// preset leaves disabled (`linkify`, `replacements`, `smartquotes`) and
/// the `html=True` halves of `html_block`/`html_inline` are absent — the
/// option checks that reject them are kept as early `false` returns.
///
/// The lookup tables below are machine-generated from this repo's venv
/// (`/tmp/md_gen_tables.py`, `/tmp/md_gen_rust.py`; markdown-it-py 4.2.0,
/// Python 3.12): `ENTITIES` is `html.entities.html5` with `;` rstripped
/// (deduped, order-free); `PUNCT_RANGES` is `unicodedata.category` P/S;
/// `LOWER_EXC`/`UPPER_EXC` are the full `str.lower`/`str.upper` mappings
/// (`normalizeReference` is per-char composition, so the tables are exact).
/// (`STRIP_RANGES`, the `str.strip`/`re-\s` set, lives at module top
/// level — the datetime layer shares it.)
mod markdown {
    pub const ENTITIES: &[(&str, &str)] = &[
        ("AElig", "Æ"),
        ("AMP", "&"),
        ("Aacute", "Á"),
        ("Abreve", "Ă"),
        ("Acirc", "Â"),
        ("Acy", "А"),
        ("Afr", "𝔄"),
        ("Agrave", "À"),
        ("Alpha", "Α"),
        ("Amacr", "Ā"),
        ("And", "⩓"),
        ("Aogon", "Ą"),
        ("Aopf", "𝔸"),
        ("ApplyFunction", "\u{2061}"),
        ("Aring", "Å"),
        ("Ascr", "𝒜"),
        ("Assign", "≔"),
        ("Atilde", "Ã"),
        ("Auml", "Ä"),
        ("Backslash", "∖"),
        ("Barv", "⫧"),
        ("Barwed", "⌆"),
        ("Bcy", "Б"),
        ("Because", "∵"),
        ("Bernoullis", "ℬ"),
        ("Beta", "Β"),
        ("Bfr", "𝔅"),
        ("Bopf", "𝔹"),
        ("Breve", "˘"),
        ("Bscr", "ℬ"),
        ("Bumpeq", "≎"),
        ("CHcy", "Ч"),
        ("COPY", "©"),
        ("Cacute", "Ć"),
        ("Cap", "⋒"),
        ("CapitalDifferentialD", "ⅅ"),
        ("Cayleys", "ℭ"),
        ("Ccaron", "Č"),
        ("Ccedil", "Ç"),
        ("Ccirc", "Ĉ"),
        ("Cconint", "∰"),
        ("Cdot", "Ċ"),
        ("Cedilla", "¸"),
        ("CenterDot", "·"),
        ("Cfr", "ℭ"),
        ("Chi", "Χ"),
        ("CircleDot", "⊙"),
        ("CircleMinus", "⊖"),
        ("CirclePlus", "⊕"),
        ("CircleTimes", "⊗"),
        ("ClockwiseContourIntegral", "∲"),
        ("CloseCurlyDoubleQuote", "”"),
        ("CloseCurlyQuote", "’"),
        ("Colon", "∷"),
        ("Colone", "⩴"),
        ("Congruent", "≡"),
        ("Conint", "∯"),
        ("ContourIntegral", "∮"),
        ("Copf", "ℂ"),
        ("Coproduct", "∐"),
        ("CounterClockwiseContourIntegral", "∳"),
        ("Cross", "⨯"),
        ("Cscr", "𝒞"),
        ("Cup", "⋓"),
        ("CupCap", "≍"),
        ("DD", "ⅅ"),
        ("DDotrahd", "⤑"),
        ("DJcy", "Ђ"),
        ("DScy", "Ѕ"),
        ("DZcy", "Џ"),
        ("Dagger", "‡"),
        ("Darr", "↡"),
        ("Dashv", "⫤"),
        ("Dcaron", "Ď"),
        ("Dcy", "Д"),
        ("Del", "∇"),
        ("Delta", "Δ"),
        ("Dfr", "𝔇"),
        ("DiacriticalAcute", "´"),
        ("DiacriticalDot", "˙"),
        ("DiacriticalDoubleAcute", "˝"),
        ("DiacriticalGrave", "`"),
        ("DiacriticalTilde", "˜"),
        ("Diamond", "⋄"),
        ("DifferentialD", "ⅆ"),
        ("Dopf", "𝔻"),
        ("Dot", "¨"),
        ("DotDot", "⃜"),
        ("DotEqual", "≐"),
        ("DoubleContourIntegral", "∯"),
        ("DoubleDot", "¨"),
        ("DoubleDownArrow", "⇓"),
        ("DoubleLeftArrow", "⇐"),
        ("DoubleLeftRightArrow", "⇔"),
        ("DoubleLeftTee", "⫤"),
        ("DoubleLongLeftArrow", "⟸"),
        ("DoubleLongLeftRightArrow", "⟺"),
        ("DoubleLongRightArrow", "⟹"),
        ("DoubleRightArrow", "⇒"),
        ("DoubleRightTee", "⊨"),
        ("DoubleUpArrow", "⇑"),
        ("DoubleUpDownArrow", "⇕"),
        ("DoubleVerticalBar", "∥"),
        ("DownArrow", "↓"),
        ("DownArrowBar", "⤓"),
        ("DownArrowUpArrow", "⇵"),
        ("DownBreve", "̑"),
        ("DownLeftRightVector", "⥐"),
        ("DownLeftTeeVector", "⥞"),
        ("DownLeftVector", "↽"),
        ("DownLeftVectorBar", "⥖"),
        ("DownRightTeeVector", "⥟"),
        ("DownRightVector", "⇁"),
        ("DownRightVectorBar", "⥗"),
        ("DownTee", "⊤"),
        ("DownTeeArrow", "↧"),
        ("Downarrow", "⇓"),
        ("Dscr", "𝒟"),
        ("Dstrok", "Đ"),
        ("ENG", "Ŋ"),
        ("ETH", "Ð"),
        ("Eacute", "É"),
        ("Ecaron", "Ě"),
        ("Ecirc", "Ê"),
        ("Ecy", "Э"),
        ("Edot", "Ė"),
        ("Efr", "𝔈"),
        ("Egrave", "È"),
        ("Element", "∈"),
        ("Emacr", "Ē"),
        ("EmptySmallSquare", "◻"),
        ("EmptyVerySmallSquare", "▫"),
        ("Eogon", "Ę"),
        ("Eopf", "𝔼"),
        ("Epsilon", "Ε"),
        ("Equal", "⩵"),
        ("EqualTilde", "≂"),
        ("Equilibrium", "⇌"),
        ("Escr", "ℰ"),
        ("Esim", "⩳"),
        ("Eta", "Η"),
        ("Euml", "Ë"),
        ("Exists", "∃"),
        ("ExponentialE", "ⅇ"),
        ("Fcy", "Ф"),
        ("Ffr", "𝔉"),
        ("FilledSmallSquare", "◼"),
        ("FilledVerySmallSquare", "▪"),
        ("Fopf", "𝔽"),
        ("ForAll", "∀"),
        ("Fouriertrf", "ℱ"),
        ("Fscr", "ℱ"),
        ("GJcy", "Ѓ"),
        ("GT", ">"),
        ("Gamma", "Γ"),
        ("Gammad", "Ϝ"),
        ("Gbreve", "Ğ"),
        ("Gcedil", "Ģ"),
        ("Gcirc", "Ĝ"),
        ("Gcy", "Г"),
        ("Gdot", "Ġ"),
        ("Gfr", "𝔊"),
        ("Gg", "⋙"),
        ("Gopf", "𝔾"),
        ("GreaterEqual", "≥"),
        ("GreaterEqualLess", "⋛"),
        ("GreaterFullEqual", "≧"),
        ("GreaterGreater", "⪢"),
        ("GreaterLess", "≷"),
        ("GreaterSlantEqual", "⩾"),
        ("GreaterTilde", "≳"),
        ("Gscr", "𝒢"),
        ("Gt", "≫"),
        ("HARDcy", "Ъ"),
        ("Hacek", "ˇ"),
        ("Hat", "^"),
        ("Hcirc", "Ĥ"),
        ("Hfr", "ℌ"),
        ("HilbertSpace", "ℋ"),
        ("Hopf", "ℍ"),
        ("HorizontalLine", "─"),
        ("Hscr", "ℋ"),
        ("Hstrok", "Ħ"),
        ("HumpDownHump", "≎"),
        ("HumpEqual", "≏"),
        ("IEcy", "Е"),
        ("IJlig", "Ĳ"),
        ("IOcy", "Ё"),
        ("Iacute", "Í"),
        ("Icirc", "Î"),
        ("Icy", "И"),
        ("Idot", "İ"),
        ("Ifr", "ℑ"),
        ("Igrave", "Ì"),
        ("Im", "ℑ"),
        ("Imacr", "Ī"),
        ("ImaginaryI", "ⅈ"),
        ("Implies", "⇒"),
        ("Int", "∬"),
        ("Integral", "∫"),
        ("Intersection", "⋂"),
        ("InvisibleComma", "\u{2063}"),
        ("InvisibleTimes", "\u{2062}"),
        ("Iogon", "Į"),
        ("Iopf", "𝕀"),
        ("Iota", "Ι"),
        ("Iscr", "ℐ"),
        ("Itilde", "Ĩ"),
        ("Iukcy", "І"),
        ("Iuml", "Ï"),
        ("Jcirc", "Ĵ"),
        ("Jcy", "Й"),
        ("Jfr", "𝔍"),
        ("Jopf", "𝕁"),
        ("Jscr", "𝒥"),
        ("Jsercy", "Ј"),
        ("Jukcy", "Є"),
        ("KHcy", "Х"),
        ("KJcy", "Ќ"),
        ("Kappa", "Κ"),
        ("Kcedil", "Ķ"),
        ("Kcy", "К"),
        ("Kfr", "𝔎"),
        ("Kopf", "𝕂"),
        ("Kscr", "𝒦"),
        ("LJcy", "Љ"),
        ("LT", "<"),
        ("Lacute", "Ĺ"),
        ("Lambda", "Λ"),
        ("Lang", "⟪"),
        ("Laplacetrf", "ℒ"),
        ("Larr", "↞"),
        ("Lcaron", "Ľ"),
        ("Lcedil", "Ļ"),
        ("Lcy", "Л"),
        ("LeftAngleBracket", "⟨"),
        ("LeftArrow", "←"),
        ("LeftArrowBar", "⇤"),
        ("LeftArrowRightArrow", "⇆"),
        ("LeftCeiling", "⌈"),
        ("LeftDoubleBracket", "⟦"),
        ("LeftDownTeeVector", "⥡"),
        ("LeftDownVector", "⇃"),
        ("LeftDownVectorBar", "⥙"),
        ("LeftFloor", "⌊"),
        ("LeftRightArrow", "↔"),
        ("LeftRightVector", "⥎"),
        ("LeftTee", "⊣"),
        ("LeftTeeArrow", "↤"),
        ("LeftTeeVector", "⥚"),
        ("LeftTriangle", "⊲"),
        ("LeftTriangleBar", "⧏"),
        ("LeftTriangleEqual", "⊴"),
        ("LeftUpDownVector", "⥑"),
        ("LeftUpTeeVector", "⥠"),
        ("LeftUpVector", "↿"),
        ("LeftUpVectorBar", "⥘"),
        ("LeftVector", "↼"),
        ("LeftVectorBar", "⥒"),
        ("Leftarrow", "⇐"),
        ("Leftrightarrow", "⇔"),
        ("LessEqualGreater", "⋚"),
        ("LessFullEqual", "≦"),
        ("LessGreater", "≶"),
        ("LessLess", "⪡"),
        ("LessSlantEqual", "⩽"),
        ("LessTilde", "≲"),
        ("Lfr", "𝔏"),
        ("Ll", "⋘"),
        ("Lleftarrow", "⇚"),
        ("Lmidot", "Ŀ"),
        ("LongLeftArrow", "⟵"),
        ("LongLeftRightArrow", "⟷"),
        ("LongRightArrow", "⟶"),
        ("Longleftarrow", "⟸"),
        ("Longleftrightarrow", "⟺"),
        ("Longrightarrow", "⟹"),
        ("Lopf", "𝕃"),
        ("LowerLeftArrow", "↙"),
        ("LowerRightArrow", "↘"),
        ("Lscr", "ℒ"),
        ("Lsh", "↰"),
        ("Lstrok", "Ł"),
        ("Lt", "≪"),
        ("Map", "⤅"),
        ("Mcy", "М"),
        ("MediumSpace", " "),
        ("Mellintrf", "ℳ"),
        ("Mfr", "𝔐"),
        ("MinusPlus", "∓"),
        ("Mopf", "𝕄"),
        ("Mscr", "ℳ"),
        ("Mu", "Μ"),
        ("NJcy", "Њ"),
        ("Nacute", "Ń"),
        ("Ncaron", "Ň"),
        ("Ncedil", "Ņ"),
        ("Ncy", "Н"),
        ("NegativeMediumSpace", "\u{200B}"),
        ("NegativeThickSpace", "\u{200B}"),
        ("NegativeThinSpace", "\u{200B}"),
        ("NegativeVeryThinSpace", "\u{200B}"),
        ("NestedGreaterGreater", "≫"),
        ("NestedLessLess", "≪"),
        ("NewLine", "\n"),
        ("Nfr", "𝔑"),
        ("NoBreak", "\u{2060}"),
        ("NonBreakingSpace", " "),
        ("Nopf", "ℕ"),
        ("Not", "⫬"),
        ("NotCongruent", "≢"),
        ("NotCupCap", "≭"),
        ("NotDoubleVerticalBar", "∦"),
        ("NotElement", "∉"),
        ("NotEqual", "≠"),
        ("NotEqualTilde", "≂̸"),
        ("NotExists", "∄"),
        ("NotGreater", "≯"),
        ("NotGreaterEqual", "≱"),
        ("NotGreaterFullEqual", "≧̸"),
        ("NotGreaterGreater", "≫̸"),
        ("NotGreaterLess", "≹"),
        ("NotGreaterSlantEqual", "⩾̸"),
        ("NotGreaterTilde", "≵"),
        ("NotHumpDownHump", "≎̸"),
        ("NotHumpEqual", "≏̸"),
        ("NotLeftTriangle", "⋪"),
        ("NotLeftTriangleBar", "⧏̸"),
        ("NotLeftTriangleEqual", "⋬"),
        ("NotLess", "≮"),
        ("NotLessEqual", "≰"),
        ("NotLessGreater", "≸"),
        ("NotLessLess", "≪̸"),
        ("NotLessSlantEqual", "⩽̸"),
        ("NotLessTilde", "≴"),
        ("NotNestedGreaterGreater", "⪢̸"),
        ("NotNestedLessLess", "⪡̸"),
        ("NotPrecedes", "⊀"),
        ("NotPrecedesEqual", "⪯̸"),
        ("NotPrecedesSlantEqual", "⋠"),
        ("NotReverseElement", "∌"),
        ("NotRightTriangle", "⋫"),
        ("NotRightTriangleBar", "⧐̸"),
        ("NotRightTriangleEqual", "⋭"),
        ("NotSquareSubset", "⊏̸"),
        ("NotSquareSubsetEqual", "⋢"),
        ("NotSquareSuperset", "⊐̸"),
        ("NotSquareSupersetEqual", "⋣"),
        ("NotSubset", "⊂⃒"),
        ("NotSubsetEqual", "⊈"),
        ("NotSucceeds", "⊁"),
        ("NotSucceedsEqual", "⪰̸"),
        ("NotSucceedsSlantEqual", "⋡"),
        ("NotSucceedsTilde", "≿̸"),
        ("NotSuperset", "⊃⃒"),
        ("NotSupersetEqual", "⊉"),
        ("NotTilde", "≁"),
        ("NotTildeEqual", "≄"),
        ("NotTildeFullEqual", "≇"),
        ("NotTildeTilde", "≉"),
        ("NotVerticalBar", "∤"),
        ("Nscr", "𝒩"),
        ("Ntilde", "Ñ"),
        ("Nu", "Ν"),
        ("OElig", "Œ"),
        ("Oacute", "Ó"),
        ("Ocirc", "Ô"),
        ("Ocy", "О"),
        ("Odblac", "Ő"),
        ("Ofr", "𝔒"),
        ("Ograve", "Ò"),
        ("Omacr", "Ō"),
        ("Omega", "Ω"),
        ("Omicron", "Ο"),
        ("Oopf", "𝕆"),
        ("OpenCurlyDoubleQuote", "“"),
        ("OpenCurlyQuote", "‘"),
        ("Or", "⩔"),
        ("Oscr", "𝒪"),
        ("Oslash", "Ø"),
        ("Otilde", "Õ"),
        ("Otimes", "⨷"),
        ("Ouml", "Ö"),
        ("OverBar", "‾"),
        ("OverBrace", "⏞"),
        ("OverBracket", "⎴"),
        ("OverParenthesis", "⏜"),
        ("PartialD", "∂"),
        ("Pcy", "П"),
        ("Pfr", "𝔓"),
        ("Phi", "Φ"),
        ("Pi", "Π"),
        ("PlusMinus", "±"),
        ("Poincareplane", "ℌ"),
        ("Popf", "ℙ"),
        ("Pr", "⪻"),
        ("Precedes", "≺"),
        ("PrecedesEqual", "⪯"),
        ("PrecedesSlantEqual", "≼"),
        ("PrecedesTilde", "≾"),
        ("Prime", "″"),
        ("Product", "∏"),
        ("Proportion", "∷"),
        ("Proportional", "∝"),
        ("Pscr", "𝒫"),
        ("Psi", "Ψ"),
        ("QUOT", "\""),
        ("Qfr", "𝔔"),
        ("Qopf", "ℚ"),
        ("Qscr", "𝒬"),
        ("RBarr", "⤐"),
        ("REG", "®"),
        ("Racute", "Ŕ"),
        ("Rang", "⟫"),
        ("Rarr", "↠"),
        ("Rarrtl", "⤖"),
        ("Rcaron", "Ř"),
        ("Rcedil", "Ŗ"),
        ("Rcy", "Р"),
        ("Re", "ℜ"),
        ("ReverseElement", "∋"),
        ("ReverseEquilibrium", "⇋"),
        ("ReverseUpEquilibrium", "⥯"),
        ("Rfr", "ℜ"),
        ("Rho", "Ρ"),
        ("RightAngleBracket", "⟩"),
        ("RightArrow", "→"),
        ("RightArrowBar", "⇥"),
        ("RightArrowLeftArrow", "⇄"),
        ("RightCeiling", "⌉"),
        ("RightDoubleBracket", "⟧"),
        ("RightDownTeeVector", "⥝"),
        ("RightDownVector", "⇂"),
        ("RightDownVectorBar", "⥕"),
        ("RightFloor", "⌋"),
        ("RightTee", "⊢"),
        ("RightTeeArrow", "↦"),
        ("RightTeeVector", "⥛"),
        ("RightTriangle", "⊳"),
        ("RightTriangleBar", "⧐"),
        ("RightTriangleEqual", "⊵"),
        ("RightUpDownVector", "⥏"),
        ("RightUpTeeVector", "⥜"),
        ("RightUpVector", "↾"),
        ("RightUpVectorBar", "⥔"),
        ("RightVector", "⇀"),
        ("RightVectorBar", "⥓"),
        ("Rightarrow", "⇒"),
        ("Ropf", "ℝ"),
        ("RoundImplies", "⥰"),
        ("Rrightarrow", "⇛"),
        ("Rscr", "ℛ"),
        ("Rsh", "↱"),
        ("RuleDelayed", "⧴"),
        ("SHCHcy", "Щ"),
        ("SHcy", "Ш"),
        ("SOFTcy", "Ь"),
        ("Sacute", "Ś"),
        ("Sc", "⪼"),
        ("Scaron", "Š"),
        ("Scedil", "Ş"),
        ("Scirc", "Ŝ"),
        ("Scy", "С"),
        ("Sfr", "𝔖"),
        ("ShortDownArrow", "↓"),
        ("ShortLeftArrow", "←"),
        ("ShortRightArrow", "→"),
        ("ShortUpArrow", "↑"),
        ("Sigma", "Σ"),
        ("SmallCircle", "∘"),
        ("Sopf", "𝕊"),
        ("Sqrt", "√"),
        ("Square", "□"),
        ("SquareIntersection", "⊓"),
        ("SquareSubset", "⊏"),
        ("SquareSubsetEqual", "⊑"),
        ("SquareSuperset", "⊐"),
        ("SquareSupersetEqual", "⊒"),
        ("SquareUnion", "⊔"),
        ("Sscr", "𝒮"),
        ("Star", "⋆"),
        ("Sub", "⋐"),
        ("Subset", "⋐"),
        ("SubsetEqual", "⊆"),
        ("Succeeds", "≻"),
        ("SucceedsEqual", "⪰"),
        ("SucceedsSlantEqual", "≽"),
        ("SucceedsTilde", "≿"),
        ("SuchThat", "∋"),
        ("Sum", "∑"),
        ("Sup", "⋑"),
        ("Superset", "⊃"),
        ("SupersetEqual", "⊇"),
        ("Supset", "⋑"),
        ("THORN", "Þ"),
        ("TRADE", "™"),
        ("TSHcy", "Ћ"),
        ("TScy", "Ц"),
        ("Tab", "\t"),
        ("Tau", "Τ"),
        ("Tcaron", "Ť"),
        ("Tcedil", "Ţ"),
        ("Tcy", "Т"),
        ("Tfr", "𝔗"),
        ("Therefore", "∴"),
        ("Theta", "Θ"),
        ("ThickSpace", "  "),
        ("ThinSpace", " "),
        ("Tilde", "∼"),
        ("TildeEqual", "≃"),
        ("TildeFullEqual", "≅"),
        ("TildeTilde", "≈"),
        ("Topf", "𝕋"),
        ("TripleDot", "⃛"),
        ("Tscr", "𝒯"),
        ("Tstrok", "Ŧ"),
        ("Uacute", "Ú"),
        ("Uarr", "↟"),
        ("Uarrocir", "⥉"),
        ("Ubrcy", "Ў"),
        ("Ubreve", "Ŭ"),
        ("Ucirc", "Û"),
        ("Ucy", "У"),
        ("Udblac", "Ű"),
        ("Ufr", "𝔘"),
        ("Ugrave", "Ù"),
        ("Umacr", "Ū"),
        ("UnderBar", "_"),
        ("UnderBrace", "⏟"),
        ("UnderBracket", "⎵"),
        ("UnderParenthesis", "⏝"),
        ("Union", "⋃"),
        ("UnionPlus", "⊎"),
        ("Uogon", "Ų"),
        ("Uopf", "𝕌"),
        ("UpArrow", "↑"),
        ("UpArrowBar", "⤒"),
        ("UpArrowDownArrow", "⇅"),
        ("UpDownArrow", "↕"),
        ("UpEquilibrium", "⥮"),
        ("UpTee", "⊥"),
        ("UpTeeArrow", "↥"),
        ("Uparrow", "⇑"),
        ("Updownarrow", "⇕"),
        ("UpperLeftArrow", "↖"),
        ("UpperRightArrow", "↗"),
        ("Upsi", "ϒ"),
        ("Upsilon", "Υ"),
        ("Uring", "Ů"),
        ("Uscr", "𝒰"),
        ("Utilde", "Ũ"),
        ("Uuml", "Ü"),
        ("VDash", "⊫"),
        ("Vbar", "⫫"),
        ("Vcy", "В"),
        ("Vdash", "⊩"),
        ("Vdashl", "⫦"),
        ("Vee", "⋁"),
        ("Verbar", "‖"),
        ("Vert", "‖"),
        ("VerticalBar", "∣"),
        ("VerticalLine", "|"),
        ("VerticalSeparator", "❘"),
        ("VerticalTilde", "≀"),
        ("VeryThinSpace", " "),
        ("Vfr", "𝔙"),
        ("Vopf", "𝕍"),
        ("Vscr", "𝒱"),
        ("Vvdash", "⊪"),
        ("Wcirc", "Ŵ"),
        ("Wedge", "⋀"),
        ("Wfr", "𝔚"),
        ("Wopf", "𝕎"),
        ("Wscr", "𝒲"),
        ("Xfr", "𝔛"),
        ("Xi", "Ξ"),
        ("Xopf", "𝕏"),
        ("Xscr", "𝒳"),
        ("YAcy", "Я"),
        ("YIcy", "Ї"),
        ("YUcy", "Ю"),
        ("Yacute", "Ý"),
        ("Ycirc", "Ŷ"),
        ("Ycy", "Ы"),
        ("Yfr", "𝔜"),
        ("Yopf", "𝕐"),
        ("Yscr", "𝒴"),
        ("Yuml", "Ÿ"),
        ("ZHcy", "Ж"),
        ("Zacute", "Ź"),
        ("Zcaron", "Ž"),
        ("Zcy", "З"),
        ("Zdot", "Ż"),
        ("ZeroWidthSpace", "\u{200B}"),
        ("Zeta", "Ζ"),
        ("Zfr", "ℨ"),
        ("Zopf", "ℤ"),
        ("Zscr", "𝒵"),
        ("aacute", "á"),
        ("abreve", "ă"),
        ("ac", "∾"),
        ("acE", "∾̳"),
        ("acd", "∿"),
        ("acirc", "â"),
        ("acute", "´"),
        ("acy", "а"),
        ("aelig", "æ"),
        ("af", "\u{2061}"),
        ("afr", "𝔞"),
        ("agrave", "à"),
        ("alefsym", "ℵ"),
        ("aleph", "ℵ"),
        ("alpha", "α"),
        ("amacr", "ā"),
        ("amalg", "⨿"),
        ("amp", "&"),
        ("and", "∧"),
        ("andand", "⩕"),
        ("andd", "⩜"),
        ("andslope", "⩘"),
        ("andv", "⩚"),
        ("ang", "∠"),
        ("ange", "⦤"),
        ("angle", "∠"),
        ("angmsd", "∡"),
        ("angmsdaa", "⦨"),
        ("angmsdab", "⦩"),
        ("angmsdac", "⦪"),
        ("angmsdad", "⦫"),
        ("angmsdae", "⦬"),
        ("angmsdaf", "⦭"),
        ("angmsdag", "⦮"),
        ("angmsdah", "⦯"),
        ("angrt", "∟"),
        ("angrtvb", "⊾"),
        ("angrtvbd", "⦝"),
        ("angsph", "∢"),
        ("angst", "Å"),
        ("angzarr", "⍼"),
        ("aogon", "ą"),
        ("aopf", "𝕒"),
        ("ap", "≈"),
        ("apE", "⩰"),
        ("apacir", "⩯"),
        ("ape", "≊"),
        ("apid", "≋"),
        ("apos", "'"),
        ("approx", "≈"),
        ("approxeq", "≊"),
        ("aring", "å"),
        ("ascr", "𝒶"),
        ("ast", "*"),
        ("asymp", "≈"),
        ("asympeq", "≍"),
        ("atilde", "ã"),
        ("auml", "ä"),
        ("awconint", "∳"),
        ("awint", "⨑"),
        ("bNot", "⫭"),
        ("backcong", "≌"),
        ("backepsilon", "϶"),
        ("backprime", "‵"),
        ("backsim", "∽"),
        ("backsimeq", "⋍"),
        ("barvee", "⊽"),
        ("barwed", "⌅"),
        ("barwedge", "⌅"),
        ("bbrk", "⎵"),
        ("bbrktbrk", "⎶"),
        ("bcong", "≌"),
        ("bcy", "б"),
        ("bdquo", "„"),
        ("becaus", "∵"),
        ("because", "∵"),
        ("bemptyv", "⦰"),
        ("bepsi", "϶"),
        ("bernou", "ℬ"),
        ("beta", "β"),
        ("beth", "ℶ"),
        ("between", "≬"),
        ("bfr", "𝔟"),
        ("bigcap", "⋂"),
        ("bigcirc", "◯"),
        ("bigcup", "⋃"),
        ("bigodot", "⨀"),
        ("bigoplus", "⨁"),
        ("bigotimes", "⨂"),
        ("bigsqcup", "⨆"),
        ("bigstar", "★"),
        ("bigtriangledown", "▽"),
        ("bigtriangleup", "△"),
        ("biguplus", "⨄"),
        ("bigvee", "⋁"),
        ("bigwedge", "⋀"),
        ("bkarow", "⤍"),
        ("blacklozenge", "⧫"),
        ("blacksquare", "▪"),
        ("blacktriangle", "▴"),
        ("blacktriangledown", "▾"),
        ("blacktriangleleft", "◂"),
        ("blacktriangleright", "▸"),
        ("blank", "␣"),
        ("blk12", "▒"),
        ("blk14", "░"),
        ("blk34", "▓"),
        ("block", "█"),
        ("bne", "=⃥"),
        ("bnequiv", "≡⃥"),
        ("bnot", "⌐"),
        ("bopf", "𝕓"),
        ("bot", "⊥"),
        ("bottom", "⊥"),
        ("bowtie", "⋈"),
        ("boxDL", "╗"),
        ("boxDR", "╔"),
        ("boxDl", "╖"),
        ("boxDr", "╓"),
        ("boxH", "═"),
        ("boxHD", "╦"),
        ("boxHU", "╩"),
        ("boxHd", "╤"),
        ("boxHu", "╧"),
        ("boxUL", "╝"),
        ("boxUR", "╚"),
        ("boxUl", "╜"),
        ("boxUr", "╙"),
        ("boxV", "║"),
        ("boxVH", "╬"),
        ("boxVL", "╣"),
        ("boxVR", "╠"),
        ("boxVh", "╫"),
        ("boxVl", "╢"),
        ("boxVr", "╟"),
        ("boxbox", "⧉"),
        ("boxdL", "╕"),
        ("boxdR", "╒"),
        ("boxdl", "┐"),
        ("boxdr", "┌"),
        ("boxh", "─"),
        ("boxhD", "╥"),
        ("boxhU", "╨"),
        ("boxhd", "┬"),
        ("boxhu", "┴"),
        ("boxminus", "⊟"),
        ("boxplus", "⊞"),
        ("boxtimes", "⊠"),
        ("boxuL", "╛"),
        ("boxuR", "╘"),
        ("boxul", "┘"),
        ("boxur", "└"),
        ("boxv", "│"),
        ("boxvH", "╪"),
        ("boxvL", "╡"),
        ("boxvR", "╞"),
        ("boxvh", "┼"),
        ("boxvl", "┤"),
        ("boxvr", "├"),
        ("bprime", "‵"),
        ("breve", "˘"),
        ("brvbar", "¦"),
        ("bscr", "𝒷"),
        ("bsemi", "⁏"),
        ("bsim", "∽"),
        ("bsime", "⋍"),
        ("bsol", "\\"),
        ("bsolb", "⧅"),
        ("bsolhsub", "⟈"),
        ("bull", "•"),
        ("bullet", "•"),
        ("bump", "≎"),
        ("bumpE", "⪮"),
        ("bumpe", "≏"),
        ("bumpeq", "≏"),
        ("cacute", "ć"),
        ("cap", "∩"),
        ("capand", "⩄"),
        ("capbrcup", "⩉"),
        ("capcap", "⩋"),
        ("capcup", "⩇"),
        ("capdot", "⩀"),
        ("caps", "∩︀"),
        ("caret", "⁁"),
        ("caron", "ˇ"),
        ("ccaps", "⩍"),
        ("ccaron", "č"),
        ("ccedil", "ç"),
        ("ccirc", "ĉ"),
        ("ccups", "⩌"),
        ("ccupssm", "⩐"),
        ("cdot", "ċ"),
        ("cedil", "¸"),
        ("cemptyv", "⦲"),
        ("cent", "¢"),
        ("centerdot", "·"),
        ("cfr", "𝔠"),
        ("chcy", "ч"),
        ("check", "✓"),
        ("checkmark", "✓"),
        ("chi", "χ"),
        ("cir", "○"),
        ("cirE", "⧃"),
        ("circ", "ˆ"),
        ("circeq", "≗"),
        ("circlearrowleft", "↺"),
        ("circlearrowright", "↻"),
        ("circledR", "®"),
        ("circledS", "Ⓢ"),
        ("circledast", "⊛"),
        ("circledcirc", "⊚"),
        ("circleddash", "⊝"),
        ("cire", "≗"),
        ("cirfnint", "⨐"),
        ("cirmid", "⫯"),
        ("cirscir", "⧂"),
        ("clubs", "♣"),
        ("clubsuit", "♣"),
        ("colon", ":"),
        ("colone", "≔"),
        ("coloneq", "≔"),
        ("comma", ","),
        ("commat", "@"),
        ("comp", "∁"),
        ("compfn", "∘"),
        ("complement", "∁"),
        ("complexes", "ℂ"),
        ("cong", "≅"),
        ("congdot", "⩭"),
        ("conint", "∮"),
        ("copf", "𝕔"),
        ("coprod", "∐"),
        ("copy", "©"),
        ("copysr", "℗"),
        ("crarr", "↵"),
        ("cross", "✗"),
        ("cscr", "𝒸"),
        ("csub", "⫏"),
        ("csube", "⫑"),
        ("csup", "⫐"),
        ("csupe", "⫒"),
        ("ctdot", "⋯"),
        ("cudarrl", "⤸"),
        ("cudarrr", "⤵"),
        ("cuepr", "⋞"),
        ("cuesc", "⋟"),
        ("cularr", "↶"),
        ("cularrp", "⤽"),
        ("cup", "∪"),
        ("cupbrcap", "⩈"),
        ("cupcap", "⩆"),
        ("cupcup", "⩊"),
        ("cupdot", "⊍"),
        ("cupor", "⩅"),
        ("cups", "∪︀"),
        ("curarr", "↷"),
        ("curarrm", "⤼"),
        ("curlyeqprec", "⋞"),
        ("curlyeqsucc", "⋟"),
        ("curlyvee", "⋎"),
        ("curlywedge", "⋏"),
        ("curren", "¤"),
        ("curvearrowleft", "↶"),
        ("curvearrowright", "↷"),
        ("cuvee", "⋎"),
        ("cuwed", "⋏"),
        ("cwconint", "∲"),
        ("cwint", "∱"),
        ("cylcty", "⌭"),
        ("dArr", "⇓"),
        ("dHar", "⥥"),
        ("dagger", "†"),
        ("daleth", "ℸ"),
        ("darr", "↓"),
        ("dash", "‐"),
        ("dashv", "⊣"),
        ("dbkarow", "⤏"),
        ("dblac", "˝"),
        ("dcaron", "ď"),
        ("dcy", "д"),
        ("dd", "ⅆ"),
        ("ddagger", "‡"),
        ("ddarr", "⇊"),
        ("ddotseq", "⩷"),
        ("deg", "°"),
        ("delta", "δ"),
        ("demptyv", "⦱"),
        ("dfisht", "⥿"),
        ("dfr", "𝔡"),
        ("dharl", "⇃"),
        ("dharr", "⇂"),
        ("diam", "⋄"),
        ("diamond", "⋄"),
        ("diamondsuit", "♦"),
        ("diams", "♦"),
        ("die", "¨"),
        ("digamma", "ϝ"),
        ("disin", "⋲"),
        ("div", "÷"),
        ("divide", "÷"),
        ("divideontimes", "⋇"),
        ("divonx", "⋇"),
        ("djcy", "ђ"),
        ("dlcorn", "⌞"),
        ("dlcrop", "⌍"),
        ("dollar", "$"),
        ("dopf", "𝕕"),
        ("dot", "˙"),
        ("doteq", "≐"),
        ("doteqdot", "≑"),
        ("dotminus", "∸"),
        ("dotplus", "∔"),
        ("dotsquare", "⊡"),
        ("doublebarwedge", "⌆"),
        ("downarrow", "↓"),
        ("downdownarrows", "⇊"),
        ("downharpoonleft", "⇃"),
        ("downharpoonright", "⇂"),
        ("drbkarow", "⤐"),
        ("drcorn", "⌟"),
        ("drcrop", "⌌"),
        ("dscr", "𝒹"),
        ("dscy", "ѕ"),
        ("dsol", "⧶"),
        ("dstrok", "đ"),
        ("dtdot", "⋱"),
        ("dtri", "▿"),
        ("dtrif", "▾"),
        ("duarr", "⇵"),
        ("duhar", "⥯"),
        ("dwangle", "⦦"),
        ("dzcy", "џ"),
        ("dzigrarr", "⟿"),
        ("eDDot", "⩷"),
        ("eDot", "≑"),
        ("eacute", "é"),
        ("easter", "⩮"),
        ("ecaron", "ě"),
        ("ecir", "≖"),
        ("ecirc", "ê"),
        ("ecolon", "≕"),
        ("ecy", "э"),
        ("edot", "ė"),
        ("ee", "ⅇ"),
        ("efDot", "≒"),
        ("efr", "𝔢"),
        ("eg", "⪚"),
        ("egrave", "è"),
        ("egs", "⪖"),
        ("egsdot", "⪘"),
        ("el", "⪙"),
        ("elinters", "⏧"),
        ("ell", "ℓ"),
        ("els", "⪕"),
        ("elsdot", "⪗"),
        ("emacr", "ē"),
        ("empty", "∅"),
        ("emptyset", "∅"),
        ("emptyv", "∅"),
        ("emsp", " "),
        ("emsp13", " "),
        ("emsp14", " "),
        ("eng", "ŋ"),
        ("ensp", " "),
        ("eogon", "ę"),
        ("eopf", "𝕖"),
        ("epar", "⋕"),
        ("eparsl", "⧣"),
        ("eplus", "⩱"),
        ("epsi", "ε"),
        ("epsilon", "ε"),
        ("epsiv", "ϵ"),
        ("eqcirc", "≖"),
        ("eqcolon", "≕"),
        ("eqsim", "≂"),
        ("eqslantgtr", "⪖"),
        ("eqslantless", "⪕"),
        ("equals", "="),
        ("equest", "≟"),
        ("equiv", "≡"),
        ("equivDD", "⩸"),
        ("eqvparsl", "⧥"),
        ("erDot", "≓"),
        ("erarr", "⥱"),
        ("escr", "ℯ"),
        ("esdot", "≐"),
        ("esim", "≂"),
        ("eta", "η"),
        ("eth", "ð"),
        ("euml", "ë"),
        ("euro", "€"),
        ("excl", "!"),
        ("exist", "∃"),
        ("expectation", "ℰ"),
        ("exponentiale", "ⅇ"),
        ("fallingdotseq", "≒"),
        ("fcy", "ф"),
        ("female", "♀"),
        ("ffilig", "ﬃ"),
        ("fflig", "ﬀ"),
        ("ffllig", "ﬄ"),
        ("ffr", "𝔣"),
        ("filig", "ﬁ"),
        ("fjlig", "fj"),
        ("flat", "♭"),
        ("fllig", "ﬂ"),
        ("fltns", "▱"),
        ("fnof", "ƒ"),
        ("fopf", "𝕗"),
        ("forall", "∀"),
        ("fork", "⋔"),
        ("forkv", "⫙"),
        ("fpartint", "⨍"),
        ("frac12", "½"),
        ("frac13", "⅓"),
        ("frac14", "¼"),
        ("frac15", "⅕"),
        ("frac16", "⅙"),
        ("frac18", "⅛"),
        ("frac23", "⅔"),
        ("frac25", "⅖"),
        ("frac34", "¾"),
        ("frac35", "⅗"),
        ("frac38", "⅜"),
        ("frac45", "⅘"),
        ("frac56", "⅚"),
        ("frac58", "⅝"),
        ("frac78", "⅞"),
        ("frasl", "⁄"),
        ("frown", "⌢"),
        ("fscr", "𝒻"),
        ("gE", "≧"),
        ("gEl", "⪌"),
        ("gacute", "ǵ"),
        ("gamma", "γ"),
        ("gammad", "ϝ"),
        ("gap", "⪆"),
        ("gbreve", "ğ"),
        ("gcirc", "ĝ"),
        ("gcy", "г"),
        ("gdot", "ġ"),
        ("ge", "≥"),
        ("gel", "⋛"),
        ("geq", "≥"),
        ("geqq", "≧"),
        ("geqslant", "⩾"),
        ("ges", "⩾"),
        ("gescc", "⪩"),
        ("gesdot", "⪀"),
        ("gesdoto", "⪂"),
        ("gesdotol", "⪄"),
        ("gesl", "⋛︀"),
        ("gesles", "⪔"),
        ("gfr", "𝔤"),
        ("gg", "≫"),
        ("ggg", "⋙"),
        ("gimel", "ℷ"),
        ("gjcy", "ѓ"),
        ("gl", "≷"),
        ("glE", "⪒"),
        ("gla", "⪥"),
        ("glj", "⪤"),
        ("gnE", "≩"),
        ("gnap", "⪊"),
        ("gnapprox", "⪊"),
        ("gne", "⪈"),
        ("gneq", "⪈"),
        ("gneqq", "≩"),
        ("gnsim", "⋧"),
        ("gopf", "𝕘"),
        ("grave", "`"),
        ("gscr", "ℊ"),
        ("gsim", "≳"),
        ("gsime", "⪎"),
        ("gsiml", "⪐"),
        ("gt", ">"),
        ("gtcc", "⪧"),
        ("gtcir", "⩺"),
        ("gtdot", "⋗"),
        ("gtlPar", "⦕"),
        ("gtquest", "⩼"),
        ("gtrapprox", "⪆"),
        ("gtrarr", "⥸"),
        ("gtrdot", "⋗"),
        ("gtreqless", "⋛"),
        ("gtreqqless", "⪌"),
        ("gtrless", "≷"),
        ("gtrsim", "≳"),
        ("gvertneqq", "≩︀"),
        ("gvnE", "≩︀"),
        ("hArr", "⇔"),
        ("hairsp", " "),
        ("half", "½"),
        ("hamilt", "ℋ"),
        ("hardcy", "ъ"),
        ("harr", "↔"),
        ("harrcir", "⥈"),
        ("harrw", "↭"),
        ("hbar", "ℏ"),
        ("hcirc", "ĥ"),
        ("hearts", "♥"),
        ("heartsuit", "♥"),
        ("hellip", "…"),
        ("hercon", "⊹"),
        ("hfr", "𝔥"),
        ("hksearow", "⤥"),
        ("hkswarow", "⤦"),
        ("hoarr", "⇿"),
        ("homtht", "∻"),
        ("hookleftarrow", "↩"),
        ("hookrightarrow", "↪"),
        ("hopf", "𝕙"),
        ("horbar", "―"),
        ("hscr", "𝒽"),
        ("hslash", "ℏ"),
        ("hstrok", "ħ"),
        ("hybull", "⁃"),
        ("hyphen", "‐"),
        ("iacute", "í"),
        ("ic", "\u{2063}"),
        ("icirc", "î"),
        ("icy", "и"),
        ("iecy", "е"),
        ("iexcl", "¡"),
        ("iff", "⇔"),
        ("ifr", "𝔦"),
        ("igrave", "ì"),
        ("ii", "ⅈ"),
        ("iiiint", "⨌"),
        ("iiint", "∭"),
        ("iinfin", "⧜"),
        ("iiota", "℩"),
        ("ijlig", "ĳ"),
        ("imacr", "ī"),
        ("image", "ℑ"),
        ("imagline", "ℐ"),
        ("imagpart", "ℑ"),
        ("imath", "ı"),
        ("imof", "⊷"),
        ("imped", "Ƶ"),
        ("in", "∈"),
        ("incare", "℅"),
        ("infin", "∞"),
        ("infintie", "⧝"),
        ("inodot", "ı"),
        ("int", "∫"),
        ("intcal", "⊺"),
        ("integers", "ℤ"),
        ("intercal", "⊺"),
        ("intlarhk", "⨗"),
        ("intprod", "⨼"),
        ("iocy", "ё"),
        ("iogon", "į"),
        ("iopf", "𝕚"),
        ("iota", "ι"),
        ("iprod", "⨼"),
        ("iquest", "¿"),
        ("iscr", "𝒾"),
        ("isin", "∈"),
        ("isinE", "⋹"),
        ("isindot", "⋵"),
        ("isins", "⋴"),
        ("isinsv", "⋳"),
        ("isinv", "∈"),
        ("it", "\u{2062}"),
        ("itilde", "ĩ"),
        ("iukcy", "і"),
        ("iuml", "ï"),
        ("jcirc", "ĵ"),
        ("jcy", "й"),
        ("jfr", "𝔧"),
        ("jmath", "ȷ"),
        ("jopf", "𝕛"),
        ("jscr", "𝒿"),
        ("jsercy", "ј"),
        ("jukcy", "є"),
        ("kappa", "κ"),
        ("kappav", "ϰ"),
        ("kcedil", "ķ"),
        ("kcy", "к"),
        ("kfr", "𝔨"),
        ("kgreen", "ĸ"),
        ("khcy", "х"),
        ("kjcy", "ќ"),
        ("kopf", "𝕜"),
        ("kscr", "𝓀"),
        ("lAarr", "⇚"),
        ("lArr", "⇐"),
        ("lAtail", "⤛"),
        ("lBarr", "⤎"),
        ("lE", "≦"),
        ("lEg", "⪋"),
        ("lHar", "⥢"),
        ("lacute", "ĺ"),
        ("laemptyv", "⦴"),
        ("lagran", "ℒ"),
        ("lambda", "λ"),
        ("lang", "⟨"),
        ("langd", "⦑"),
        ("langle", "⟨"),
        ("lap", "⪅"),
        ("laquo", "«"),
        ("larr", "←"),
        ("larrb", "⇤"),
        ("larrbfs", "⤟"),
        ("larrfs", "⤝"),
        ("larrhk", "↩"),
        ("larrlp", "↫"),
        ("larrpl", "⤹"),
        ("larrsim", "⥳"),
        ("larrtl", "↢"),
        ("lat", "⪫"),
        ("latail", "⤙"),
        ("late", "⪭"),
        ("lates", "⪭︀"),
        ("lbarr", "⤌"),
        ("lbbrk", "❲"),
        ("lbrace", "{"),
        ("lbrack", "["),
        ("lbrke", "⦋"),
        ("lbrksld", "⦏"),
        ("lbrkslu", "⦍"),
        ("lcaron", "ľ"),
        ("lcedil", "ļ"),
        ("lceil", "⌈"),
        ("lcub", "{"),
        ("lcy", "л"),
        ("ldca", "⤶"),
        ("ldquo", "“"),
        ("ldquor", "„"),
        ("ldrdhar", "⥧"),
        ("ldrushar", "⥋"),
        ("ldsh", "↲"),
        ("le", "≤"),
        ("leftarrow", "←"),
        ("leftarrowtail", "↢"),
        ("leftharpoondown", "↽"),
        ("leftharpoonup", "↼"),
        ("leftleftarrows", "⇇"),
        ("leftrightarrow", "↔"),
        ("leftrightarrows", "⇆"),
        ("leftrightharpoons", "⇋"),
        ("leftrightsquigarrow", "↭"),
        ("leftthreetimes", "⋋"),
        ("leg", "⋚"),
        ("leq", "≤"),
        ("leqq", "≦"),
        ("leqslant", "⩽"),
        ("les", "⩽"),
        ("lescc", "⪨"),
        ("lesdot", "⩿"),
        ("lesdoto", "⪁"),
        ("lesdotor", "⪃"),
        ("lesg", "⋚︀"),
        ("lesges", "⪓"),
        ("lessapprox", "⪅"),
        ("lessdot", "⋖"),
        ("lesseqgtr", "⋚"),
        ("lesseqqgtr", "⪋"),
        ("lessgtr", "≶"),
        ("lesssim", "≲"),
        ("lfisht", "⥼"),
        ("lfloor", "⌊"),
        ("lfr", "𝔩"),
        ("lg", "≶"),
        ("lgE", "⪑"),
        ("lhard", "↽"),
        ("lharu", "↼"),
        ("lharul", "⥪"),
        ("lhblk", "▄"),
        ("ljcy", "љ"),
        ("ll", "≪"),
        ("llarr", "⇇"),
        ("llcorner", "⌞"),
        ("llhard", "⥫"),
        ("lltri", "◺"),
        ("lmidot", "ŀ"),
        ("lmoust", "⎰"),
        ("lmoustache", "⎰"),
        ("lnE", "≨"),
        ("lnap", "⪉"),
        ("lnapprox", "⪉"),
        ("lne", "⪇"),
        ("lneq", "⪇"),
        ("lneqq", "≨"),
        ("lnsim", "⋦"),
        ("loang", "⟬"),
        ("loarr", "⇽"),
        ("lobrk", "⟦"),
        ("longleftarrow", "⟵"),
        ("longleftrightarrow", "⟷"),
        ("longmapsto", "⟼"),
        ("longrightarrow", "⟶"),
        ("looparrowleft", "↫"),
        ("looparrowright", "↬"),
        ("lopar", "⦅"),
        ("lopf", "𝕝"),
        ("loplus", "⨭"),
        ("lotimes", "⨴"),
        ("lowast", "∗"),
        ("lowbar", "_"),
        ("loz", "◊"),
        ("lozenge", "◊"),
        ("lozf", "⧫"),
        ("lpar", "("),
        ("lparlt", "⦓"),
        ("lrarr", "⇆"),
        ("lrcorner", "⌟"),
        ("lrhar", "⇋"),
        ("lrhard", "⥭"),
        ("lrm", "\u{200E}"),
        ("lrtri", "⊿"),
        ("lsaquo", "‹"),
        ("lscr", "𝓁"),
        ("lsh", "↰"),
        ("lsim", "≲"),
        ("lsime", "⪍"),
        ("lsimg", "⪏"),
        ("lsqb", "["),
        ("lsquo", "‘"),
        ("lsquor", "‚"),
        ("lstrok", "ł"),
        ("lt", "<"),
        ("ltcc", "⪦"),
        ("ltcir", "⩹"),
        ("ltdot", "⋖"),
        ("lthree", "⋋"),
        ("ltimes", "⋉"),
        ("ltlarr", "⥶"),
        ("ltquest", "⩻"),
        ("ltrPar", "⦖"),
        ("ltri", "◃"),
        ("ltrie", "⊴"),
        ("ltrif", "◂"),
        ("lurdshar", "⥊"),
        ("luruhar", "⥦"),
        ("lvertneqq", "≨︀"),
        ("lvnE", "≨︀"),
        ("mDDot", "∺"),
        ("macr", "¯"),
        ("male", "♂"),
        ("malt", "✠"),
        ("maltese", "✠"),
        ("map", "↦"),
        ("mapsto", "↦"),
        ("mapstodown", "↧"),
        ("mapstoleft", "↤"),
        ("mapstoup", "↥"),
        ("marker", "▮"),
        ("mcomma", "⨩"),
        ("mcy", "м"),
        ("mdash", "—"),
        ("measuredangle", "∡"),
        ("mfr", "𝔪"),
        ("mho", "℧"),
        ("micro", "µ"),
        ("mid", "∣"),
        ("midast", "*"),
        ("midcir", "⫰"),
        ("middot", "·"),
        ("minus", "−"),
        ("minusb", "⊟"),
        ("minusd", "∸"),
        ("minusdu", "⨪"),
        ("mlcp", "⫛"),
        ("mldr", "…"),
        ("mnplus", "∓"),
        ("models", "⊧"),
        ("mopf", "𝕞"),
        ("mp", "∓"),
        ("mscr", "𝓂"),
        ("mstpos", "∾"),
        ("mu", "μ"),
        ("multimap", "⊸"),
        ("mumap", "⊸"),
        ("nGg", "⋙̸"),
        ("nGt", "≫⃒"),
        ("nGtv", "≫̸"),
        ("nLeftarrow", "⇍"),
        ("nLeftrightarrow", "⇎"),
        ("nLl", "⋘̸"),
        ("nLt", "≪⃒"),
        ("nLtv", "≪̸"),
        ("nRightarrow", "⇏"),
        ("nVDash", "⊯"),
        ("nVdash", "⊮"),
        ("nabla", "∇"),
        ("nacute", "ń"),
        ("nang", "∠⃒"),
        ("nap", "≉"),
        ("napE", "⩰̸"),
        ("napid", "≋̸"),
        ("napos", "ŉ"),
        ("napprox", "≉"),
        ("natur", "♮"),
        ("natural", "♮"),
        ("naturals", "ℕ"),
        ("nbsp", " "),
        ("nbump", "≎̸"),
        ("nbumpe", "≏̸"),
        ("ncap", "⩃"),
        ("ncaron", "ň"),
        ("ncedil", "ņ"),
        ("ncong", "≇"),
        ("ncongdot", "⩭̸"),
        ("ncup", "⩂"),
        ("ncy", "н"),
        ("ndash", "–"),
        ("ne", "≠"),
        ("neArr", "⇗"),
        ("nearhk", "⤤"),
        ("nearr", "↗"),
        ("nearrow", "↗"),
        ("nedot", "≐̸"),
        ("nequiv", "≢"),
        ("nesear", "⤨"),
        ("nesim", "≂̸"),
        ("nexist", "∄"),
        ("nexists", "∄"),
        ("nfr", "𝔫"),
        ("ngE", "≧̸"),
        ("nge", "≱"),
        ("ngeq", "≱"),
        ("ngeqq", "≧̸"),
        ("ngeqslant", "⩾̸"),
        ("nges", "⩾̸"),
        ("ngsim", "≵"),
        ("ngt", "≯"),
        ("ngtr", "≯"),
        ("nhArr", "⇎"),
        ("nharr", "↮"),
        ("nhpar", "⫲"),
        ("ni", "∋"),
        ("nis", "⋼"),
        ("nisd", "⋺"),
        ("niv", "∋"),
        ("njcy", "њ"),
        ("nlArr", "⇍"),
        ("nlE", "≦̸"),
        ("nlarr", "↚"),
        ("nldr", "‥"),
        ("nle", "≰"),
        ("nleftarrow", "↚"),
        ("nleftrightarrow", "↮"),
        ("nleq", "≰"),
        ("nleqq", "≦̸"),
        ("nleqslant", "⩽̸"),
        ("nles", "⩽̸"),
        ("nless", "≮"),
        ("nlsim", "≴"),
        ("nlt", "≮"),
        ("nltri", "⋪"),
        ("nltrie", "⋬"),
        ("nmid", "∤"),
        ("nopf", "𝕟"),
        ("not", "¬"),
        ("notin", "∉"),
        ("notinE", "⋹̸"),
        ("notindot", "⋵̸"),
        ("notinva", "∉"),
        ("notinvb", "⋷"),
        ("notinvc", "⋶"),
        ("notni", "∌"),
        ("notniva", "∌"),
        ("notnivb", "⋾"),
        ("notnivc", "⋽"),
        ("npar", "∦"),
        ("nparallel", "∦"),
        ("nparsl", "⫽⃥"),
        ("npart", "∂̸"),
        ("npolint", "⨔"),
        ("npr", "⊀"),
        ("nprcue", "⋠"),
        ("npre", "⪯̸"),
        ("nprec", "⊀"),
        ("npreceq", "⪯̸"),
        ("nrArr", "⇏"),
        ("nrarr", "↛"),
        ("nrarrc", "⤳̸"),
        ("nrarrw", "↝̸"),
        ("nrightarrow", "↛"),
        ("nrtri", "⋫"),
        ("nrtrie", "⋭"),
        ("nsc", "⊁"),
        ("nsccue", "⋡"),
        ("nsce", "⪰̸"),
        ("nscr", "𝓃"),
        ("nshortmid", "∤"),
        ("nshortparallel", "∦"),
        ("nsim", "≁"),
        ("nsime", "≄"),
        ("nsimeq", "≄"),
        ("nsmid", "∤"),
        ("nspar", "∦"),
        ("nsqsube", "⋢"),
        ("nsqsupe", "⋣"),
        ("nsub", "⊄"),
        ("nsubE", "⫅̸"),
        ("nsube", "⊈"),
        ("nsubset", "⊂⃒"),
        ("nsubseteq", "⊈"),
        ("nsubseteqq", "⫅̸"),
        ("nsucc", "⊁"),
        ("nsucceq", "⪰̸"),
        ("nsup", "⊅"),
        ("nsupE", "⫆̸"),
        ("nsupe", "⊉"),
        ("nsupset", "⊃⃒"),
        ("nsupseteq", "⊉"),
        ("nsupseteqq", "⫆̸"),
        ("ntgl", "≹"),
        ("ntilde", "ñ"),
        ("ntlg", "≸"),
        ("ntriangleleft", "⋪"),
        ("ntrianglelefteq", "⋬"),
        ("ntriangleright", "⋫"),
        ("ntrianglerighteq", "⋭"),
        ("nu", "ν"),
        ("num", "#"),
        ("numero", "№"),
        ("numsp", " "),
        ("nvDash", "⊭"),
        ("nvHarr", "⤄"),
        ("nvap", "≍⃒"),
        ("nvdash", "⊬"),
        ("nvge", "≥⃒"),
        ("nvgt", ">⃒"),
        ("nvinfin", "⧞"),
        ("nvlArr", "⤂"),
        ("nvle", "≤⃒"),
        ("nvlt", "<⃒"),
        ("nvltrie", "⊴⃒"),
        ("nvrArr", "⤃"),
        ("nvrtrie", "⊵⃒"),
        ("nvsim", "∼⃒"),
        ("nwArr", "⇖"),
        ("nwarhk", "⤣"),
        ("nwarr", "↖"),
        ("nwarrow", "↖"),
        ("nwnear", "⤧"),
        ("oS", "Ⓢ"),
        ("oacute", "ó"),
        ("oast", "⊛"),
        ("ocir", "⊚"),
        ("ocirc", "ô"),
        ("ocy", "о"),
        ("odash", "⊝"),
        ("odblac", "ő"),
        ("odiv", "⨸"),
        ("odot", "⊙"),
        ("odsold", "⦼"),
        ("oelig", "œ"),
        ("ofcir", "⦿"),
        ("ofr", "𝔬"),
        ("ogon", "˛"),
        ("ograve", "ò"),
        ("ogt", "⧁"),
        ("ohbar", "⦵"),
        ("ohm", "Ω"),
        ("oint", "∮"),
        ("olarr", "↺"),
        ("olcir", "⦾"),
        ("olcross", "⦻"),
        ("oline", "‾"),
        ("olt", "⧀"),
        ("omacr", "ō"),
        ("omega", "ω"),
        ("omicron", "ο"),
        ("omid", "⦶"),
        ("ominus", "⊖"),
        ("oopf", "𝕠"),
        ("opar", "⦷"),
        ("operp", "⦹"),
        ("oplus", "⊕"),
        ("or", "∨"),
        ("orarr", "↻"),
        ("ord", "⩝"),
        ("order", "ℴ"),
        ("orderof", "ℴ"),
        ("ordf", "ª"),
        ("ordm", "º"),
        ("origof", "⊶"),
        ("oror", "⩖"),
        ("orslope", "⩗"),
        ("orv", "⩛"),
        ("oscr", "ℴ"),
        ("oslash", "ø"),
        ("osol", "⊘"),
        ("otilde", "õ"),
        ("otimes", "⊗"),
        ("otimesas", "⨶"),
        ("ouml", "ö"),
        ("ovbar", "⌽"),
        ("par", "∥"),
        ("para", "¶"),
        ("parallel", "∥"),
        ("parsim", "⫳"),
        ("parsl", "⫽"),
        ("part", "∂"),
        ("pcy", "п"),
        ("percnt", "%"),
        ("period", "."),
        ("permil", "‰"),
        ("perp", "⊥"),
        ("pertenk", "‱"),
        ("pfr", "𝔭"),
        ("phi", "φ"),
        ("phiv", "ϕ"),
        ("phmmat", "ℳ"),
        ("phone", "☎"),
        ("pi", "π"),
        ("pitchfork", "⋔"),
        ("piv", "ϖ"),
        ("planck", "ℏ"),
        ("planckh", "ℎ"),
        ("plankv", "ℏ"),
        ("plus", "+"),
        ("plusacir", "⨣"),
        ("plusb", "⊞"),
        ("pluscir", "⨢"),
        ("plusdo", "∔"),
        ("plusdu", "⨥"),
        ("pluse", "⩲"),
        ("plusmn", "±"),
        ("plussim", "⨦"),
        ("plustwo", "⨧"),
        ("pm", "±"),
        ("pointint", "⨕"),
        ("popf", "𝕡"),
        ("pound", "£"),
        ("pr", "≺"),
        ("prE", "⪳"),
        ("prap", "⪷"),
        ("prcue", "≼"),
        ("pre", "⪯"),
        ("prec", "≺"),
        ("precapprox", "⪷"),
        ("preccurlyeq", "≼"),
        ("preceq", "⪯"),
        ("precnapprox", "⪹"),
        ("precneqq", "⪵"),
        ("precnsim", "⋨"),
        ("precsim", "≾"),
        ("prime", "′"),
        ("primes", "ℙ"),
        ("prnE", "⪵"),
        ("prnap", "⪹"),
        ("prnsim", "⋨"),
        ("prod", "∏"),
        ("profalar", "⌮"),
        ("profline", "⌒"),
        ("profsurf", "⌓"),
        ("prop", "∝"),
        ("propto", "∝"),
        ("prsim", "≾"),
        ("prurel", "⊰"),
        ("pscr", "𝓅"),
        ("psi", "ψ"),
        ("puncsp", " "),
        ("qfr", "𝔮"),
        ("qint", "⨌"),
        ("qopf", "𝕢"),
        ("qprime", "⁗"),
        ("qscr", "𝓆"),
        ("quaternions", "ℍ"),
        ("quatint", "⨖"),
        ("quest", "?"),
        ("questeq", "≟"),
        ("quot", "\""),
        ("rAarr", "⇛"),
        ("rArr", "⇒"),
        ("rAtail", "⤜"),
        ("rBarr", "⤏"),
        ("rHar", "⥤"),
        ("race", "∽̱"),
        ("racute", "ŕ"),
        ("radic", "√"),
        ("raemptyv", "⦳"),
        ("rang", "⟩"),
        ("rangd", "⦒"),
        ("range", "⦥"),
        ("rangle", "⟩"),
        ("raquo", "»"),
        ("rarr", "→"),
        ("rarrap", "⥵"),
        ("rarrb", "⇥"),
        ("rarrbfs", "⤠"),
        ("rarrc", "⤳"),
        ("rarrfs", "⤞"),
        ("rarrhk", "↪"),
        ("rarrlp", "↬"),
        ("rarrpl", "⥅"),
        ("rarrsim", "⥴"),
        ("rarrtl", "↣"),
        ("rarrw", "↝"),
        ("ratail", "⤚"),
        ("ratio", "∶"),
        ("rationals", "ℚ"),
        ("rbarr", "⤍"),
        ("rbbrk", "❳"),
        ("rbrace", "}"),
        ("rbrack", "]"),
        ("rbrke", "⦌"),
        ("rbrksld", "⦎"),
        ("rbrkslu", "⦐"),
        ("rcaron", "ř"),
        ("rcedil", "ŗ"),
        ("rceil", "⌉"),
        ("rcub", "}"),
        ("rcy", "р"),
        ("rdca", "⤷"),
        ("rdldhar", "⥩"),
        ("rdquo", "”"),
        ("rdquor", "”"),
        ("rdsh", "↳"),
        ("real", "ℜ"),
        ("realine", "ℛ"),
        ("realpart", "ℜ"),
        ("reals", "ℝ"),
        ("rect", "▭"),
        ("reg", "®"),
        ("rfisht", "⥽"),
        ("rfloor", "⌋"),
        ("rfr", "𝔯"),
        ("rhard", "⇁"),
        ("rharu", "⇀"),
        ("rharul", "⥬"),
        ("rho", "ρ"),
        ("rhov", "ϱ"),
        ("rightarrow", "→"),
        ("rightarrowtail", "↣"),
        ("rightharpoondown", "⇁"),
        ("rightharpoonup", "⇀"),
        ("rightleftarrows", "⇄"),
        ("rightleftharpoons", "⇌"),
        ("rightrightarrows", "⇉"),
        ("rightsquigarrow", "↝"),
        ("rightthreetimes", "⋌"),
        ("ring", "˚"),
        ("risingdotseq", "≓"),
        ("rlarr", "⇄"),
        ("rlhar", "⇌"),
        ("rlm", "\u{200F}"),
        ("rmoust", "⎱"),
        ("rmoustache", "⎱"),
        ("rnmid", "⫮"),
        ("roang", "⟭"),
        ("roarr", "⇾"),
        ("robrk", "⟧"),
        ("ropar", "⦆"),
        ("ropf", "𝕣"),
        ("roplus", "⨮"),
        ("rotimes", "⨵"),
        ("rpar", ")"),
        ("rpargt", "⦔"),
        ("rppolint", "⨒"),
        ("rrarr", "⇉"),
        ("rsaquo", "›"),
        ("rscr", "𝓇"),
        ("rsh", "↱"),
        ("rsqb", "]"),
        ("rsquo", "’"),
        ("rsquor", "’"),
        ("rthree", "⋌"),
        ("rtimes", "⋊"),
        ("rtri", "▹"),
        ("rtrie", "⊵"),
        ("rtrif", "▸"),
        ("rtriltri", "⧎"),
        ("ruluhar", "⥨"),
        ("rx", "℞"),
        ("sacute", "ś"),
        ("sbquo", "‚"),
        ("sc", "≻"),
        ("scE", "⪴"),
        ("scap", "⪸"),
        ("scaron", "š"),
        ("sccue", "≽"),
        ("sce", "⪰"),
        ("scedil", "ş"),
        ("scirc", "ŝ"),
        ("scnE", "⪶"),
        ("scnap", "⪺"),
        ("scnsim", "⋩"),
        ("scpolint", "⨓"),
        ("scsim", "≿"),
        ("scy", "с"),
        ("sdot", "⋅"),
        ("sdotb", "⊡"),
        ("sdote", "⩦"),
        ("seArr", "⇘"),
        ("searhk", "⤥"),
        ("searr", "↘"),
        ("searrow", "↘"),
        ("sect", "§"),
        ("semi", ";"),
        ("seswar", "⤩"),
        ("setminus", "∖"),
        ("setmn", "∖"),
        ("sext", "✶"),
        ("sfr", "𝔰"),
        ("sfrown", "⌢"),
        ("sharp", "♯"),
        ("shchcy", "щ"),
        ("shcy", "ш"),
        ("shortmid", "∣"),
        ("shortparallel", "∥"),
        ("shy", "\u{AD}"),
        ("sigma", "σ"),
        ("sigmaf", "ς"),
        ("sigmav", "ς"),
        ("sim", "∼"),
        ("simdot", "⩪"),
        ("sime", "≃"),
        ("simeq", "≃"),
        ("simg", "⪞"),
        ("simgE", "⪠"),
        ("siml", "⪝"),
        ("simlE", "⪟"),
        ("simne", "≆"),
        ("simplus", "⨤"),
        ("simrarr", "⥲"),
        ("slarr", "←"),
        ("smallsetminus", "∖"),
        ("smashp", "⨳"),
        ("smeparsl", "⧤"),
        ("smid", "∣"),
        ("smile", "⌣"),
        ("smt", "⪪"),
        ("smte", "⪬"),
        ("smtes", "⪬︀"),
        ("softcy", "ь"),
        ("sol", "/"),
        ("solb", "⧄"),
        ("solbar", "⌿"),
        ("sopf", "𝕤"),
        ("spades", "♠"),
        ("spadesuit", "♠"),
        ("spar", "∥"),
        ("sqcap", "⊓"),
        ("sqcaps", "⊓︀"),
        ("sqcup", "⊔"),
        ("sqcups", "⊔︀"),
        ("sqsub", "⊏"),
        ("sqsube", "⊑"),
        ("sqsubset", "⊏"),
        ("sqsubseteq", "⊑"),
        ("sqsup", "⊐"),
        ("sqsupe", "⊒"),
        ("sqsupset", "⊐"),
        ("sqsupseteq", "⊒"),
        ("squ", "□"),
        ("square", "□"),
        ("squarf", "▪"),
        ("squf", "▪"),
        ("srarr", "→"),
        ("sscr", "𝓈"),
        ("ssetmn", "∖"),
        ("ssmile", "⌣"),
        ("sstarf", "⋆"),
        ("star", "☆"),
        ("starf", "★"),
        ("straightepsilon", "ϵ"),
        ("straightphi", "ϕ"),
        ("strns", "¯"),
        ("sub", "⊂"),
        ("subE", "⫅"),
        ("subdot", "⪽"),
        ("sube", "⊆"),
        ("subedot", "⫃"),
        ("submult", "⫁"),
        ("subnE", "⫋"),
        ("subne", "⊊"),
        ("subplus", "⪿"),
        ("subrarr", "⥹"),
        ("subset", "⊂"),
        ("subseteq", "⊆"),
        ("subseteqq", "⫅"),
        ("subsetneq", "⊊"),
        ("subsetneqq", "⫋"),
        ("subsim", "⫇"),
        ("subsub", "⫕"),
        ("subsup", "⫓"),
        ("succ", "≻"),
        ("succapprox", "⪸"),
        ("succcurlyeq", "≽"),
        ("succeq", "⪰"),
        ("succnapprox", "⪺"),
        ("succneqq", "⪶"),
        ("succnsim", "⋩"),
        ("succsim", "≿"),
        ("sum", "∑"),
        ("sung", "♪"),
        ("sup", "⊃"),
        ("sup1", "¹"),
        ("sup2", "²"),
        ("sup3", "³"),
        ("supE", "⫆"),
        ("supdot", "⪾"),
        ("supdsub", "⫘"),
        ("supe", "⊇"),
        ("supedot", "⫄"),
        ("suphsol", "⟉"),
        ("suphsub", "⫗"),
        ("suplarr", "⥻"),
        ("supmult", "⫂"),
        ("supnE", "⫌"),
        ("supne", "⊋"),
        ("supplus", "⫀"),
        ("supset", "⊃"),
        ("supseteq", "⊇"),
        ("supseteqq", "⫆"),
        ("supsetneq", "⊋"),
        ("supsetneqq", "⫌"),
        ("supsim", "⫈"),
        ("supsub", "⫔"),
        ("supsup", "⫖"),
        ("swArr", "⇙"),
        ("swarhk", "⤦"),
        ("swarr", "↙"),
        ("swarrow", "↙"),
        ("swnwar", "⤪"),
        ("szlig", "ß"),
        ("target", "⌖"),
        ("tau", "τ"),
        ("tbrk", "⎴"),
        ("tcaron", "ť"),
        ("tcedil", "ţ"),
        ("tcy", "т"),
        ("tdot", "⃛"),
        ("telrec", "⌕"),
        ("tfr", "𝔱"),
        ("there4", "∴"),
        ("therefore", "∴"),
        ("theta", "θ"),
        ("thetasym", "ϑ"),
        ("thetav", "ϑ"),
        ("thickapprox", "≈"),
        ("thicksim", "∼"),
        ("thinsp", " "),
        ("thkap", "≈"),
        ("thksim", "∼"),
        ("thorn", "þ"),
        ("tilde", "˜"),
        ("times", "×"),
        ("timesb", "⊠"),
        ("timesbar", "⨱"),
        ("timesd", "⨰"),
        ("tint", "∭"),
        ("toea", "⤨"),
        ("top", "⊤"),
        ("topbot", "⌶"),
        ("topcir", "⫱"),
        ("topf", "𝕥"),
        ("topfork", "⫚"),
        ("tosa", "⤩"),
        ("tprime", "‴"),
        ("trade", "™"),
        ("triangle", "▵"),
        ("triangledown", "▿"),
        ("triangleleft", "◃"),
        ("trianglelefteq", "⊴"),
        ("triangleq", "≜"),
        ("triangleright", "▹"),
        ("trianglerighteq", "⊵"),
        ("tridot", "◬"),
        ("trie", "≜"),
        ("triminus", "⨺"),
        ("triplus", "⨹"),
        ("trisb", "⧍"),
        ("tritime", "⨻"),
        ("trpezium", "⏢"),
        ("tscr", "𝓉"),
        ("tscy", "ц"),
        ("tshcy", "ћ"),
        ("tstrok", "ŧ"),
        ("twixt", "≬"),
        ("twoheadleftarrow", "↞"),
        ("twoheadrightarrow", "↠"),
        ("uArr", "⇑"),
        ("uHar", "⥣"),
        ("uacute", "ú"),
        ("uarr", "↑"),
        ("ubrcy", "ў"),
        ("ubreve", "ŭ"),
        ("ucirc", "û"),
        ("ucy", "у"),
        ("udarr", "⇅"),
        ("udblac", "ű"),
        ("udhar", "⥮"),
        ("ufisht", "⥾"),
        ("ufr", "𝔲"),
        ("ugrave", "ù"),
        ("uharl", "↿"),
        ("uharr", "↾"),
        ("uhblk", "▀"),
        ("ulcorn", "⌜"),
        ("ulcorner", "⌜"),
        ("ulcrop", "⌏"),
        ("ultri", "◸"),
        ("umacr", "ū"),
        ("uml", "¨"),
        ("uogon", "ų"),
        ("uopf", "𝕦"),
        ("uparrow", "↑"),
        ("updownarrow", "↕"),
        ("upharpoonleft", "↿"),
        ("upharpoonright", "↾"),
        ("uplus", "⊎"),
        ("upsi", "υ"),
        ("upsih", "ϒ"),
        ("upsilon", "υ"),
        ("upuparrows", "⇈"),
        ("urcorn", "⌝"),
        ("urcorner", "⌝"),
        ("urcrop", "⌎"),
        ("uring", "ů"),
        ("urtri", "◹"),
        ("uscr", "𝓊"),
        ("utdot", "⋰"),
        ("utilde", "ũ"),
        ("utri", "▵"),
        ("utrif", "▴"),
        ("uuarr", "⇈"),
        ("uuml", "ü"),
        ("uwangle", "⦧"),
        ("vArr", "⇕"),
        ("vBar", "⫨"),
        ("vBarv", "⫩"),
        ("vDash", "⊨"),
        ("vangrt", "⦜"),
        ("varepsilon", "ϵ"),
        ("varkappa", "ϰ"),
        ("varnothing", "∅"),
        ("varphi", "ϕ"),
        ("varpi", "ϖ"),
        ("varpropto", "∝"),
        ("varr", "↕"),
        ("varrho", "ϱ"),
        ("varsigma", "ς"),
        ("varsubsetneq", "⊊︀"),
        ("varsubsetneqq", "⫋︀"),
        ("varsupsetneq", "⊋︀"),
        ("varsupsetneqq", "⫌︀"),
        ("vartheta", "ϑ"),
        ("vartriangleleft", "⊲"),
        ("vartriangleright", "⊳"),
        ("vcy", "в"),
        ("vdash", "⊢"),
        ("vee", "∨"),
        ("veebar", "⊻"),
        ("veeeq", "≚"),
        ("vellip", "⋮"),
        ("verbar", "|"),
        ("vert", "|"),
        ("vfr", "𝔳"),
        ("vltri", "⊲"),
        ("vnsub", "⊂⃒"),
        ("vnsup", "⊃⃒"),
        ("vopf", "𝕧"),
        ("vprop", "∝"),
        ("vrtri", "⊳"),
        ("vscr", "𝓋"),
        ("vsubnE", "⫋︀"),
        ("vsubne", "⊊︀"),
        ("vsupnE", "⫌︀"),
        ("vsupne", "⊋︀"),
        ("vzigzag", "⦚"),
        ("wcirc", "ŵ"),
        ("wedbar", "⩟"),
        ("wedge", "∧"),
        ("wedgeq", "≙"),
        ("weierp", "℘"),
        ("wfr", "𝔴"),
        ("wopf", "𝕨"),
        ("wp", "℘"),
        ("wr", "≀"),
        ("wreath", "≀"),
        ("wscr", "𝓌"),
        ("xcap", "⋂"),
        ("xcirc", "◯"),
        ("xcup", "⋃"),
        ("xdtri", "▽"),
        ("xfr", "𝔵"),
        ("xhArr", "⟺"),
        ("xharr", "⟷"),
        ("xi", "ξ"),
        ("xlArr", "⟸"),
        ("xlarr", "⟵"),
        ("xmap", "⟼"),
        ("xnis", "⋻"),
        ("xodot", "⨀"),
        ("xopf", "𝕩"),
        ("xoplus", "⨁"),
        ("xotime", "⨂"),
        ("xrArr", "⟹"),
        ("xrarr", "⟶"),
        ("xscr", "𝓍"),
        ("xsqcup", "⨆"),
        ("xuplus", "⨄"),
        ("xutri", "△"),
        ("xvee", "⋁"),
        ("xwedge", "⋀"),
        ("yacute", "ý"),
        ("yacy", "я"),
        ("ycirc", "ŷ"),
        ("ycy", "ы"),
        ("yen", "¥"),
        ("yfr", "𝔶"),
        ("yicy", "ї"),
        ("yopf", "𝕪"),
        ("yscr", "𝓎"),
        ("yucy", "ю"),
        ("yuml", "ÿ"),
        ("zacute", "ź"),
        ("zcaron", "ž"),
        ("zcy", "з"),
        ("zdot", "ż"),
        ("zeetrf", "ℨ"),
        ("zeta", "ζ"),
        ("zfr", "𝔷"),
        ("zhcy", "ж"),
        ("zigrarr", "⇝"),
        ("zopf", "𝕫"),
        ("zscr", "𝓏"),
        ("zwj", "\u{200D}"),
        ("zwnj", "\u{200C}"),
    ];

    pub const PUNCT_RANGES: &[(u32, u32)] = &[
        (0x21, 0x2F),
        (0x3A, 0x40),
        (0x5B, 0x60),
        (0x7B, 0x7E),
        (0xA1, 0xA9),
        (0xAB, 0xAC),
        (0xAE, 0xB1),
        (0xB4, 0xB4),
        (0xB6, 0xB8),
        (0xBB, 0xBB),
        (0xBF, 0xBF),
        (0xD7, 0xD7),
        (0xF7, 0xF7),
        (0x2C2, 0x2C5),
        (0x2D2, 0x2DF),
        (0x2E5, 0x2EB),
        (0x2ED, 0x2ED),
        (0x2EF, 0x2FF),
        (0x375, 0x375),
        (0x37E, 0x37E),
        (0x384, 0x385),
        (0x387, 0x387),
        (0x3F6, 0x3F6),
        (0x482, 0x482),
        (0x55A, 0x55F),
        (0x589, 0x58A),
        (0x58D, 0x58F),
        (0x5BE, 0x5BE),
        (0x5C0, 0x5C0),
        (0x5C3, 0x5C3),
        (0x5C6, 0x5C6),
        (0x5F3, 0x5F4),
        (0x606, 0x60F),
        (0x61B, 0x61B),
        (0x61D, 0x61F),
        (0x66A, 0x66D),
        (0x6D4, 0x6D4),
        (0x6DE, 0x6DE),
        (0x6E9, 0x6E9),
        (0x6FD, 0x6FE),
        (0x700, 0x70D),
        (0x7F6, 0x7F9),
        (0x7FE, 0x7FF),
        (0x830, 0x83E),
        (0x85E, 0x85E),
        (0x888, 0x888),
        (0x964, 0x965),
        (0x970, 0x970),
        (0x9F2, 0x9F3),
        (0x9FA, 0x9FB),
        (0x9FD, 0x9FD),
        (0xA76, 0xA76),
        (0xAF0, 0xAF1),
        (0xB70, 0xB70),
        (0xBF3, 0xBFA),
        (0xC77, 0xC77),
        (0xC7F, 0xC7F),
        (0xC84, 0xC84),
        (0xD4F, 0xD4F),
        (0xD79, 0xD79),
        (0xDF4, 0xDF4),
        (0xE3F, 0xE3F),
        (0xE4F, 0xE4F),
        (0xE5A, 0xE5B),
        (0xF01, 0xF17),
        (0xF1A, 0xF1F),
        (0xF34, 0xF34),
        (0xF36, 0xF36),
        (0xF38, 0xF38),
        (0xF3A, 0xF3D),
        (0xF85, 0xF85),
        (0xFBE, 0xFC5),
        (0xFC7, 0xFCC),
        (0xFCE, 0xFDA),
        (0x104A, 0x104F),
        (0x109E, 0x109F),
        (0x10FB, 0x10FB),
        (0x1360, 0x1368),
        (0x1390, 0x1399),
        (0x1400, 0x1400),
        (0x166D, 0x166E),
        (0x169B, 0x169C),
        (0x16EB, 0x16ED),
        (0x1735, 0x1736),
        (0x17D4, 0x17D6),
        (0x17D8, 0x17DB),
        (0x1800, 0x180A),
        (0x1940, 0x1940),
        (0x1944, 0x1945),
        (0x19DE, 0x19FF),
        (0x1A1E, 0x1A1F),
        (0x1AA0, 0x1AA6),
        (0x1AA8, 0x1AAD),
        (0x1B5A, 0x1B6A),
        (0x1B74, 0x1B7E),
        (0x1BFC, 0x1BFF),
        (0x1C3B, 0x1C3F),
        (0x1C7E, 0x1C7F),
        (0x1CC0, 0x1CC7),
        (0x1CD3, 0x1CD3),
        (0x1FBD, 0x1FBD),
        (0x1FBF, 0x1FC1),
        (0x1FCD, 0x1FCF),
        (0x1FDD, 0x1FDF),
        (0x1FED, 0x1FEF),
        (0x1FFD, 0x1FFE),
        (0x2010, 0x2027),
        (0x2030, 0x205E),
        (0x207A, 0x207E),
        (0x208A, 0x208E),
        (0x20A0, 0x20C0),
        (0x2100, 0x2101),
        (0x2103, 0x2106),
        (0x2108, 0x2109),
        (0x2114, 0x2114),
        (0x2116, 0x2118),
        (0x211E, 0x2123),
        (0x2125, 0x2125),
        (0x2127, 0x2127),
        (0x2129, 0x2129),
        (0x212E, 0x212E),
        (0x213A, 0x213B),
        (0x2140, 0x2144),
        (0x214A, 0x214D),
        (0x214F, 0x214F),
        (0x218A, 0x218B),
        (0x2190, 0x2426),
        (0x2440, 0x244A),
        (0x249C, 0x24E9),
        (0x2500, 0x2775),
        (0x2794, 0x2B73),
        (0x2B76, 0x2B95),
        (0x2B97, 0x2BFF),
        (0x2CE5, 0x2CEA),
        (0x2CF9, 0x2CFC),
        (0x2CFE, 0x2CFF),
        (0x2D70, 0x2D70),
        (0x2E00, 0x2E2E),
        (0x2E30, 0x2E5D),
        (0x2E80, 0x2E99),
        (0x2E9B, 0x2EF3),
        (0x2F00, 0x2FD5),
        (0x2FF0, 0x2FFB),
        (0x3001, 0x3004),
        (0x3008, 0x3020),
        (0x3030, 0x3030),
        (0x3036, 0x3037),
        (0x303D, 0x303F),
        (0x309B, 0x309C),
        (0x30A0, 0x30A0),
        (0x30FB, 0x30FB),
        (0x3190, 0x3191),
        (0x3196, 0x319F),
        (0x31C0, 0x31E3),
        (0x3200, 0x321E),
        (0x322A, 0x3247),
        (0x3250, 0x3250),
        (0x3260, 0x327F),
        (0x328A, 0x32B0),
        (0x32C0, 0x33FF),
        (0x4DC0, 0x4DFF),
        (0xA490, 0xA4C6),
        (0xA4FE, 0xA4FF),
        (0xA60D, 0xA60F),
        (0xA673, 0xA673),
        (0xA67E, 0xA67E),
        (0xA6F2, 0xA6F7),
        (0xA700, 0xA716),
        (0xA720, 0xA721),
        (0xA789, 0xA78A),
        (0xA828, 0xA82B),
        (0xA836, 0xA839),
        (0xA874, 0xA877),
        (0xA8CE, 0xA8CF),
        (0xA8F8, 0xA8FA),
        (0xA8FC, 0xA8FC),
        (0xA92E, 0xA92F),
        (0xA95F, 0xA95F),
        (0xA9C1, 0xA9CD),
        (0xA9DE, 0xA9DF),
        (0xAA5C, 0xAA5F),
        (0xAA77, 0xAA79),
        (0xAADE, 0xAADF),
        (0xAAF0, 0xAAF1),
        (0xAB5B, 0xAB5B),
        (0xAB6A, 0xAB6B),
        (0xABEB, 0xABEB),
        (0xFB29, 0xFB29),
        (0xFBB2, 0xFBC2),
        (0xFD3E, 0xFD4F),
        (0xFDCF, 0xFDCF),
        (0xFDFC, 0xFDFF),
        (0xFE10, 0xFE19),
        (0xFE30, 0xFE52),
        (0xFE54, 0xFE66),
        (0xFE68, 0xFE6B),
        (0xFF01, 0xFF0F),
        (0xFF1A, 0xFF20),
        (0xFF3B, 0xFF40),
        (0xFF5B, 0xFF65),
        (0xFFE0, 0xFFE6),
        (0xFFE8, 0xFFEE),
        (0xFFFC, 0xFFFD),
        (0x10100, 0x10102),
        (0x10137, 0x1013F),
        (0x10179, 0x10189),
        (0x1018C, 0x1018E),
        (0x10190, 0x1019C),
        (0x101A0, 0x101A0),
        (0x101D0, 0x101FC),
        (0x1039F, 0x1039F),
        (0x103D0, 0x103D0),
        (0x1056F, 0x1056F),
        (0x10857, 0x10857),
        (0x10877, 0x10878),
        (0x1091F, 0x1091F),
        (0x1093F, 0x1093F),
        (0x10A50, 0x10A58),
        (0x10A7F, 0x10A7F),
        (0x10AC8, 0x10AC8),
        (0x10AF0, 0x10AF6),
        (0x10B39, 0x10B3F),
        (0x10B99, 0x10B9C),
        (0x10EAD, 0x10EAD),
        (0x10F55, 0x10F59),
        (0x10F86, 0x10F89),
        (0x11047, 0x1104D),
        (0x110BB, 0x110BC),
        (0x110BE, 0x110C1),
        (0x11140, 0x11143),
        (0x11174, 0x11175),
        (0x111C5, 0x111C8),
        (0x111CD, 0x111CD),
        (0x111DB, 0x111DB),
        (0x111DD, 0x111DF),
        (0x11238, 0x1123D),
        (0x112A9, 0x112A9),
        (0x1144B, 0x1144F),
        (0x1145A, 0x1145B),
        (0x1145D, 0x1145D),
        (0x114C6, 0x114C6),
        (0x115C1, 0x115D7),
        (0x11641, 0x11643),
        (0x11660, 0x1166C),
        (0x116B9, 0x116B9),
        (0x1173C, 0x1173F),
        (0x1183B, 0x1183B),
        (0x11944, 0x11946),
        (0x119E2, 0x119E2),
        (0x11A3F, 0x11A46),
        (0x11A9A, 0x11A9C),
        (0x11A9E, 0x11AA2),
        (0x11B00, 0x11B09),
        (0x11C41, 0x11C45),
        (0x11C70, 0x11C71),
        (0x11EF7, 0x11EF8),
        (0x11F43, 0x11F4F),
        (0x11FD5, 0x11FF1),
        (0x11FFF, 0x11FFF),
        (0x12470, 0x12474),
        (0x12FF1, 0x12FF2),
        (0x16A6E, 0x16A6F),
        (0x16AF5, 0x16AF5),
        (0x16B37, 0x16B3F),
        (0x16B44, 0x16B45),
        (0x16E97, 0x16E9A),
        (0x16FE2, 0x16FE2),
        (0x1BC9C, 0x1BC9C),
        (0x1BC9F, 0x1BC9F),
        (0x1CF50, 0x1CFC3),
        (0x1D000, 0x1D0F5),
        (0x1D100, 0x1D126),
        (0x1D129, 0x1D164),
        (0x1D16A, 0x1D16C),
        (0x1D183, 0x1D184),
        (0x1D18C, 0x1D1A9),
        (0x1D1AE, 0x1D1EA),
        (0x1D200, 0x1D241),
        (0x1D245, 0x1D245),
        (0x1D300, 0x1D356),
        (0x1D6C1, 0x1D6C1),
        (0x1D6DB, 0x1D6DB),
        (0x1D6FB, 0x1D6FB),
        (0x1D715, 0x1D715),
        (0x1D735, 0x1D735),
        (0x1D74F, 0x1D74F),
        (0x1D76F, 0x1D76F),
        (0x1D789, 0x1D789),
        (0x1D7A9, 0x1D7A9),
        (0x1D7C3, 0x1D7C3),
        (0x1D800, 0x1D9FF),
        (0x1DA37, 0x1DA3A),
        (0x1DA6D, 0x1DA74),
        (0x1DA76, 0x1DA83),
        (0x1DA85, 0x1DA8B),
        (0x1E14F, 0x1E14F),
        (0x1E2FF, 0x1E2FF),
        (0x1E95E, 0x1E95F),
        (0x1ECAC, 0x1ECAC),
        (0x1ECB0, 0x1ECB0),
        (0x1ED2E, 0x1ED2E),
        (0x1EEF0, 0x1EEF1),
        (0x1F000, 0x1F02B),
        (0x1F030, 0x1F093),
        (0x1F0A0, 0x1F0AE),
        (0x1F0B1, 0x1F0BF),
        (0x1F0C1, 0x1F0CF),
        (0x1F0D1, 0x1F0F5),
        (0x1F10D, 0x1F1AD),
        (0x1F1E6, 0x1F202),
        (0x1F210, 0x1F23B),
        (0x1F240, 0x1F248),
        (0x1F250, 0x1F251),
        (0x1F260, 0x1F265),
        (0x1F300, 0x1F6D7),
        (0x1F6DC, 0x1F6EC),
        (0x1F6F0, 0x1F6FC),
        (0x1F700, 0x1F776),
        (0x1F77B, 0x1F7D9),
        (0x1F7E0, 0x1F7EB),
        (0x1F7F0, 0x1F7F0),
        (0x1F800, 0x1F80B),
        (0x1F810, 0x1F847),
        (0x1F850, 0x1F859),
        (0x1F860, 0x1F887),
        (0x1F890, 0x1F8AD),
        (0x1F8B0, 0x1F8B1),
        (0x1F900, 0x1FA53),
        (0x1FA60, 0x1FA6D),
        (0x1FA70, 0x1FA7C),
        (0x1FA80, 0x1FA88),
        (0x1FA90, 0x1FABD),
        (0x1FABF, 0x1FAC5),
        (0x1FACE, 0x1FADB),
        (0x1FAE0, 0x1FAE8),
        (0x1FAF0, 0x1FAF8),
        (0x1FB00, 0x1FB92),
        (0x1FB94, 0x1FBCA),
    ];

    // (`STRIP_RANGES` lives at module top level — shared with the
    // datetime layer; see `is_py_strip_char`.)

    pub const LOWER_EXC: &[(u32, &str)] = &[
        (0x41, "a"),
        (0x42, "b"),
        (0x43, "c"),
        (0x44, "d"),
        (0x45, "e"),
        (0x46, "f"),
        (0x47, "g"),
        (0x48, "h"),
        (0x49, "i"),
        (0x4A, "j"),
        (0x4B, "k"),
        (0x4C, "l"),
        (0x4D, "m"),
        (0x4E, "n"),
        (0x4F, "o"),
        (0x50, "p"),
        (0x51, "q"),
        (0x52, "r"),
        (0x53, "s"),
        (0x54, "t"),
        (0x55, "u"),
        (0x56, "v"),
        (0x57, "w"),
        (0x58, "x"),
        (0x59, "y"),
        (0x5A, "z"),
        (0xC0, "à"),
        (0xC1, "á"),
        (0xC2, "â"),
        (0xC3, "ã"),
        (0xC4, "ä"),
        (0xC5, "å"),
        (0xC6, "æ"),
        (0xC7, "ç"),
        (0xC8, "è"),
        (0xC9, "é"),
        (0xCA, "ê"),
        (0xCB, "ë"),
        (0xCC, "ì"),
        (0xCD, "í"),
        (0xCE, "î"),
        (0xCF, "ï"),
        (0xD0, "ð"),
        (0xD1, "ñ"),
        (0xD2, "ò"),
        (0xD3, "ó"),
        (0xD4, "ô"),
        (0xD5, "õ"),
        (0xD6, "ö"),
        (0xD8, "ø"),
        (0xD9, "ù"),
        (0xDA, "ú"),
        (0xDB, "û"),
        (0xDC, "ü"),
        (0xDD, "ý"),
        (0xDE, "þ"),
        (0x100, "ā"),
        (0x102, "ă"),
        (0x104, "ą"),
        (0x106, "ć"),
        (0x108, "ĉ"),
        (0x10A, "ċ"),
        (0x10C, "č"),
        (0x10E, "ď"),
        (0x110, "đ"),
        (0x112, "ē"),
        (0x114, "ĕ"),
        (0x116, "ė"),
        (0x118, "ę"),
        (0x11A, "ě"),
        (0x11C, "ĝ"),
        (0x11E, "ğ"),
        (0x120, "ġ"),
        (0x122, "ģ"),
        (0x124, "ĥ"),
        (0x126, "ħ"),
        (0x128, "ĩ"),
        (0x12A, "ī"),
        (0x12C, "ĭ"),
        (0x12E, "į"),
        (0x130, "i̇"),
        (0x132, "ĳ"),
        (0x134, "ĵ"),
        (0x136, "ķ"),
        (0x139, "ĺ"),
        (0x13B, "ļ"),
        (0x13D, "ľ"),
        (0x13F, "ŀ"),
        (0x141, "ł"),
        (0x143, "ń"),
        (0x145, "ņ"),
        (0x147, "ň"),
        (0x14A, "ŋ"),
        (0x14C, "ō"),
        (0x14E, "ŏ"),
        (0x150, "ő"),
        (0x152, "œ"),
        (0x154, "ŕ"),
        (0x156, "ŗ"),
        (0x158, "ř"),
        (0x15A, "ś"),
        (0x15C, "ŝ"),
        (0x15E, "ş"),
        (0x160, "š"),
        (0x162, "ţ"),
        (0x164, "ť"),
        (0x166, "ŧ"),
        (0x168, "ũ"),
        (0x16A, "ū"),
        (0x16C, "ŭ"),
        (0x16E, "ů"),
        (0x170, "ű"),
        (0x172, "ų"),
        (0x174, "ŵ"),
        (0x176, "ŷ"),
        (0x178, "ÿ"),
        (0x179, "ź"),
        (0x17B, "ż"),
        (0x17D, "ž"),
        (0x181, "ɓ"),
        (0x182, "ƃ"),
        (0x184, "ƅ"),
        (0x186, "ɔ"),
        (0x187, "ƈ"),
        (0x189, "ɖ"),
        (0x18A, "ɗ"),
        (0x18B, "ƌ"),
        (0x18E, "ǝ"),
        (0x18F, "ə"),
        (0x190, "ɛ"),
        (0x191, "ƒ"),
        (0x193, "ɠ"),
        (0x194, "ɣ"),
        (0x196, "ɩ"),
        (0x197, "ɨ"),
        (0x198, "ƙ"),
        (0x19C, "ɯ"),
        (0x19D, "ɲ"),
        (0x19F, "ɵ"),
        (0x1A0, "ơ"),
        (0x1A2, "ƣ"),
        (0x1A4, "ƥ"),
        (0x1A6, "ʀ"),
        (0x1A7, "ƨ"),
        (0x1A9, "ʃ"),
        (0x1AC, "ƭ"),
        (0x1AE, "ʈ"),
        (0x1AF, "ư"),
        (0x1B1, "ʊ"),
        (0x1B2, "ʋ"),
        (0x1B3, "ƴ"),
        (0x1B5, "ƶ"),
        (0x1B7, "ʒ"),
        (0x1B8, "ƹ"),
        (0x1BC, "ƽ"),
        (0x1C4, "ǆ"),
        (0x1C5, "ǆ"),
        (0x1C7, "ǉ"),
        (0x1C8, "ǉ"),
        (0x1CA, "ǌ"),
        (0x1CB, "ǌ"),
        (0x1CD, "ǎ"),
        (0x1CF, "ǐ"),
        (0x1D1, "ǒ"),
        (0x1D3, "ǔ"),
        (0x1D5, "ǖ"),
        (0x1D7, "ǘ"),
        (0x1D9, "ǚ"),
        (0x1DB, "ǜ"),
        (0x1DE, "ǟ"),
        (0x1E0, "ǡ"),
        (0x1E2, "ǣ"),
        (0x1E4, "ǥ"),
        (0x1E6, "ǧ"),
        (0x1E8, "ǩ"),
        (0x1EA, "ǫ"),
        (0x1EC, "ǭ"),
        (0x1EE, "ǯ"),
        (0x1F1, "ǳ"),
        (0x1F2, "ǳ"),
        (0x1F4, "ǵ"),
        (0x1F6, "ƕ"),
        (0x1F7, "ƿ"),
        (0x1F8, "ǹ"),
        (0x1FA, "ǻ"),
        (0x1FC, "ǽ"),
        (0x1FE, "ǿ"),
        (0x200, "ȁ"),
        (0x202, "ȃ"),
        (0x204, "ȅ"),
        (0x206, "ȇ"),
        (0x208, "ȉ"),
        (0x20A, "ȋ"),
        (0x20C, "ȍ"),
        (0x20E, "ȏ"),
        (0x210, "ȑ"),
        (0x212, "ȓ"),
        (0x214, "ȕ"),
        (0x216, "ȗ"),
        (0x218, "ș"),
        (0x21A, "ț"),
        (0x21C, "ȝ"),
        (0x21E, "ȟ"),
        (0x220, "ƞ"),
        (0x222, "ȣ"),
        (0x224, "ȥ"),
        (0x226, "ȧ"),
        (0x228, "ȩ"),
        (0x22A, "ȫ"),
        (0x22C, "ȭ"),
        (0x22E, "ȯ"),
        (0x230, "ȱ"),
        (0x232, "ȳ"),
        (0x23A, "ⱥ"),
        (0x23B, "ȼ"),
        (0x23D, "ƚ"),
        (0x23E, "ⱦ"),
        (0x241, "ɂ"),
        (0x243, "ƀ"),
        (0x244, "ʉ"),
        (0x245, "ʌ"),
        (0x246, "ɇ"),
        (0x248, "ɉ"),
        (0x24A, "ɋ"),
        (0x24C, "ɍ"),
        (0x24E, "ɏ"),
        (0x370, "ͱ"),
        (0x372, "ͳ"),
        (0x376, "ͷ"),
        (0x37F, "ϳ"),
        (0x386, "ά"),
        (0x388, "έ"),
        (0x389, "ή"),
        (0x38A, "ί"),
        (0x38C, "ό"),
        (0x38E, "ύ"),
        (0x38F, "ώ"),
        (0x391, "α"),
        (0x392, "β"),
        (0x393, "γ"),
        (0x394, "δ"),
        (0x395, "ε"),
        (0x396, "ζ"),
        (0x397, "η"),
        (0x398, "θ"),
        (0x399, "ι"),
        (0x39A, "κ"),
        (0x39B, "λ"),
        (0x39C, "μ"),
        (0x39D, "ν"),
        (0x39E, "ξ"),
        (0x39F, "ο"),
        (0x3A0, "π"),
        (0x3A1, "ρ"),
        (0x3A3, "σ"),
        (0x3A4, "τ"),
        (0x3A5, "υ"),
        (0x3A6, "φ"),
        (0x3A7, "χ"),
        (0x3A8, "ψ"),
        (0x3A9, "ω"),
        (0x3AA, "ϊ"),
        (0x3AB, "ϋ"),
        (0x3CF, "ϗ"),
        (0x3D8, "ϙ"),
        (0x3DA, "ϛ"),
        (0x3DC, "ϝ"),
        (0x3DE, "ϟ"),
        (0x3E0, "ϡ"),
        (0x3E2, "ϣ"),
        (0x3E4, "ϥ"),
        (0x3E6, "ϧ"),
        (0x3E8, "ϩ"),
        (0x3EA, "ϫ"),
        (0x3EC, "ϭ"),
        (0x3EE, "ϯ"),
        (0x3F4, "θ"),
        (0x3F7, "ϸ"),
        (0x3F9, "ϲ"),
        (0x3FA, "ϻ"),
        (0x3FD, "ͻ"),
        (0x3FE, "ͼ"),
        (0x3FF, "ͽ"),
        (0x400, "ѐ"),
        (0x401, "ё"),
        (0x402, "ђ"),
        (0x403, "ѓ"),
        (0x404, "є"),
        (0x405, "ѕ"),
        (0x406, "і"),
        (0x407, "ї"),
        (0x408, "ј"),
        (0x409, "љ"),
        (0x40A, "њ"),
        (0x40B, "ћ"),
        (0x40C, "ќ"),
        (0x40D, "ѝ"),
        (0x40E, "ў"),
        (0x40F, "џ"),
        (0x410, "а"),
        (0x411, "б"),
        (0x412, "в"),
        (0x413, "г"),
        (0x414, "д"),
        (0x415, "е"),
        (0x416, "ж"),
        (0x417, "з"),
        (0x418, "и"),
        (0x419, "й"),
        (0x41A, "к"),
        (0x41B, "л"),
        (0x41C, "м"),
        (0x41D, "н"),
        (0x41E, "о"),
        (0x41F, "п"),
        (0x420, "р"),
        (0x421, "с"),
        (0x422, "т"),
        (0x423, "у"),
        (0x424, "ф"),
        (0x425, "х"),
        (0x426, "ц"),
        (0x427, "ч"),
        (0x428, "ш"),
        (0x429, "щ"),
        (0x42A, "ъ"),
        (0x42B, "ы"),
        (0x42C, "ь"),
        (0x42D, "э"),
        (0x42E, "ю"),
        (0x42F, "я"),
        (0x460, "ѡ"),
        (0x462, "ѣ"),
        (0x464, "ѥ"),
        (0x466, "ѧ"),
        (0x468, "ѩ"),
        (0x46A, "ѫ"),
        (0x46C, "ѭ"),
        (0x46E, "ѯ"),
        (0x470, "ѱ"),
        (0x472, "ѳ"),
        (0x474, "ѵ"),
        (0x476, "ѷ"),
        (0x478, "ѹ"),
        (0x47A, "ѻ"),
        (0x47C, "ѽ"),
        (0x47E, "ѿ"),
        (0x480, "ҁ"),
        (0x48A, "ҋ"),
        (0x48C, "ҍ"),
        (0x48E, "ҏ"),
        (0x490, "ґ"),
        (0x492, "ғ"),
        (0x494, "ҕ"),
        (0x496, "җ"),
        (0x498, "ҙ"),
        (0x49A, "қ"),
        (0x49C, "ҝ"),
        (0x49E, "ҟ"),
        (0x4A0, "ҡ"),
        (0x4A2, "ң"),
        (0x4A4, "ҥ"),
        (0x4A6, "ҧ"),
        (0x4A8, "ҩ"),
        (0x4AA, "ҫ"),
        (0x4AC, "ҭ"),
        (0x4AE, "ү"),
        (0x4B0, "ұ"),
        (0x4B2, "ҳ"),
        (0x4B4, "ҵ"),
        (0x4B6, "ҷ"),
        (0x4B8, "ҹ"),
        (0x4BA, "һ"),
        (0x4BC, "ҽ"),
        (0x4BE, "ҿ"),
        (0x4C0, "ӏ"),
        (0x4C1, "ӂ"),
        (0x4C3, "ӄ"),
        (0x4C5, "ӆ"),
        (0x4C7, "ӈ"),
        (0x4C9, "ӊ"),
        (0x4CB, "ӌ"),
        (0x4CD, "ӎ"),
        (0x4D0, "ӑ"),
        (0x4D2, "ӓ"),
        (0x4D4, "ӕ"),
        (0x4D6, "ӗ"),
        (0x4D8, "ә"),
        (0x4DA, "ӛ"),
        (0x4DC, "ӝ"),
        (0x4DE, "ӟ"),
        (0x4E0, "ӡ"),
        (0x4E2, "ӣ"),
        (0x4E4, "ӥ"),
        (0x4E6, "ӧ"),
        (0x4E8, "ө"),
        (0x4EA, "ӫ"),
        (0x4EC, "ӭ"),
        (0x4EE, "ӯ"),
        (0x4F0, "ӱ"),
        (0x4F2, "ӳ"),
        (0x4F4, "ӵ"),
        (0x4F6, "ӷ"),
        (0x4F8, "ӹ"),
        (0x4FA, "ӻ"),
        (0x4FC, "ӽ"),
        (0x4FE, "ӿ"),
        (0x500, "ԁ"),
        (0x502, "ԃ"),
        (0x504, "ԅ"),
        (0x506, "ԇ"),
        (0x508, "ԉ"),
        (0x50A, "ԋ"),
        (0x50C, "ԍ"),
        (0x50E, "ԏ"),
        (0x510, "ԑ"),
        (0x512, "ԓ"),
        (0x514, "ԕ"),
        (0x516, "ԗ"),
        (0x518, "ԙ"),
        (0x51A, "ԛ"),
        (0x51C, "ԝ"),
        (0x51E, "ԟ"),
        (0x520, "ԡ"),
        (0x522, "ԣ"),
        (0x524, "ԥ"),
        (0x526, "ԧ"),
        (0x528, "ԩ"),
        (0x52A, "ԫ"),
        (0x52C, "ԭ"),
        (0x52E, "ԯ"),
        (0x531, "ա"),
        (0x532, "բ"),
        (0x533, "գ"),
        (0x534, "դ"),
        (0x535, "ե"),
        (0x536, "զ"),
        (0x537, "է"),
        (0x538, "ը"),
        (0x539, "թ"),
        (0x53A, "ժ"),
        (0x53B, "ի"),
        (0x53C, "լ"),
        (0x53D, "խ"),
        (0x53E, "ծ"),
        (0x53F, "կ"),
        (0x540, "հ"),
        (0x541, "ձ"),
        (0x542, "ղ"),
        (0x543, "ճ"),
        (0x544, "մ"),
        (0x545, "յ"),
        (0x546, "ն"),
        (0x547, "շ"),
        (0x548, "ո"),
        (0x549, "չ"),
        (0x54A, "պ"),
        (0x54B, "ջ"),
        (0x54C, "ռ"),
        (0x54D, "ս"),
        (0x54E, "վ"),
        (0x54F, "տ"),
        (0x550, "ր"),
        (0x551, "ց"),
        (0x552, "ւ"),
        (0x553, "փ"),
        (0x554, "ք"),
        (0x555, "օ"),
        (0x556, "ֆ"),
        (0x10A0, "ⴀ"),
        (0x10A1, "ⴁ"),
        (0x10A2, "ⴂ"),
        (0x10A3, "ⴃ"),
        (0x10A4, "ⴄ"),
        (0x10A5, "ⴅ"),
        (0x10A6, "ⴆ"),
        (0x10A7, "ⴇ"),
        (0x10A8, "ⴈ"),
        (0x10A9, "ⴉ"),
        (0x10AA, "ⴊ"),
        (0x10AB, "ⴋ"),
        (0x10AC, "ⴌ"),
        (0x10AD, "ⴍ"),
        (0x10AE, "ⴎ"),
        (0x10AF, "ⴏ"),
        (0x10B0, "ⴐ"),
        (0x10B1, "ⴑ"),
        (0x10B2, "ⴒ"),
        (0x10B3, "ⴓ"),
        (0x10B4, "ⴔ"),
        (0x10B5, "ⴕ"),
        (0x10B6, "ⴖ"),
        (0x10B7, "ⴗ"),
        (0x10B8, "ⴘ"),
        (0x10B9, "ⴙ"),
        (0x10BA, "ⴚ"),
        (0x10BB, "ⴛ"),
        (0x10BC, "ⴜ"),
        (0x10BD, "ⴝ"),
        (0x10BE, "ⴞ"),
        (0x10BF, "ⴟ"),
        (0x10C0, "ⴠ"),
        (0x10C1, "ⴡ"),
        (0x10C2, "ⴢ"),
        (0x10C3, "ⴣ"),
        (0x10C4, "ⴤ"),
        (0x10C5, "ⴥ"),
        (0x10C7, "ⴧ"),
        (0x10CD, "ⴭ"),
        (0x13A0, "ꭰ"),
        (0x13A1, "ꭱ"),
        (0x13A2, "ꭲ"),
        (0x13A3, "ꭳ"),
        (0x13A4, "ꭴ"),
        (0x13A5, "ꭵ"),
        (0x13A6, "ꭶ"),
        (0x13A7, "ꭷ"),
        (0x13A8, "ꭸ"),
        (0x13A9, "ꭹ"),
        (0x13AA, "ꭺ"),
        (0x13AB, "ꭻ"),
        (0x13AC, "ꭼ"),
        (0x13AD, "ꭽ"),
        (0x13AE, "ꭾ"),
        (0x13AF, "ꭿ"),
        (0x13B0, "ꮀ"),
        (0x13B1, "ꮁ"),
        (0x13B2, "ꮂ"),
        (0x13B3, "ꮃ"),
        (0x13B4, "ꮄ"),
        (0x13B5, "ꮅ"),
        (0x13B6, "ꮆ"),
        (0x13B7, "ꮇ"),
        (0x13B8, "ꮈ"),
        (0x13B9, "ꮉ"),
        (0x13BA, "ꮊ"),
        (0x13BB, "ꮋ"),
        (0x13BC, "ꮌ"),
        (0x13BD, "ꮍ"),
        (0x13BE, "ꮎ"),
        (0x13BF, "ꮏ"),
        (0x13C0, "ꮐ"),
        (0x13C1, "ꮑ"),
        (0x13C2, "ꮒ"),
        (0x13C3, "ꮓ"),
        (0x13C4, "ꮔ"),
        (0x13C5, "ꮕ"),
        (0x13C6, "ꮖ"),
        (0x13C7, "ꮗ"),
        (0x13C8, "ꮘ"),
        (0x13C9, "ꮙ"),
        (0x13CA, "ꮚ"),
        (0x13CB, "ꮛ"),
        (0x13CC, "ꮜ"),
        (0x13CD, "ꮝ"),
        (0x13CE, "ꮞ"),
        (0x13CF, "ꮟ"),
        (0x13D0, "ꮠ"),
        (0x13D1, "ꮡ"),
        (0x13D2, "ꮢ"),
        (0x13D3, "ꮣ"),
        (0x13D4, "ꮤ"),
        (0x13D5, "ꮥ"),
        (0x13D6, "ꮦ"),
        (0x13D7, "ꮧ"),
        (0x13D8, "ꮨ"),
        (0x13D9, "ꮩ"),
        (0x13DA, "ꮪ"),
        (0x13DB, "ꮫ"),
        (0x13DC, "ꮬ"),
        (0x13DD, "ꮭ"),
        (0x13DE, "ꮮ"),
        (0x13DF, "ꮯ"),
        (0x13E0, "ꮰ"),
        (0x13E1, "ꮱ"),
        (0x13E2, "ꮲ"),
        (0x13E3, "ꮳ"),
        (0x13E4, "ꮴ"),
        (0x13E5, "ꮵ"),
        (0x13E6, "ꮶ"),
        (0x13E7, "ꮷ"),
        (0x13E8, "ꮸ"),
        (0x13E9, "ꮹ"),
        (0x13EA, "ꮺ"),
        (0x13EB, "ꮻ"),
        (0x13EC, "ꮼ"),
        (0x13ED, "ꮽ"),
        (0x13EE, "ꮾ"),
        (0x13EF, "ꮿ"),
        (0x13F0, "ᏸ"),
        (0x13F1, "ᏹ"),
        (0x13F2, "ᏺ"),
        (0x13F3, "ᏻ"),
        (0x13F4, "ᏼ"),
        (0x13F5, "ᏽ"),
        (0x1C90, "ა"),
        (0x1C91, "ბ"),
        (0x1C92, "გ"),
        (0x1C93, "დ"),
        (0x1C94, "ე"),
        (0x1C95, "ვ"),
        (0x1C96, "ზ"),
        (0x1C97, "თ"),
        (0x1C98, "ი"),
        (0x1C99, "კ"),
        (0x1C9A, "ლ"),
        (0x1C9B, "მ"),
        (0x1C9C, "ნ"),
        (0x1C9D, "ო"),
        (0x1C9E, "პ"),
        (0x1C9F, "ჟ"),
        (0x1CA0, "რ"),
        (0x1CA1, "ს"),
        (0x1CA2, "ტ"),
        (0x1CA3, "უ"),
        (0x1CA4, "ფ"),
        (0x1CA5, "ქ"),
        (0x1CA6, "ღ"),
        (0x1CA7, "ყ"),
        (0x1CA8, "შ"),
        (0x1CA9, "ჩ"),
        (0x1CAA, "ც"),
        (0x1CAB, "ძ"),
        (0x1CAC, "წ"),
        (0x1CAD, "ჭ"),
        (0x1CAE, "ხ"),
        (0x1CAF, "ჯ"),
        (0x1CB0, "ჰ"),
        (0x1CB1, "ჱ"),
        (0x1CB2, "ჲ"),
        (0x1CB3, "ჳ"),
        (0x1CB4, "ჴ"),
        (0x1CB5, "ჵ"),
        (0x1CB6, "ჶ"),
        (0x1CB7, "ჷ"),
        (0x1CB8, "ჸ"),
        (0x1CB9, "ჹ"),
        (0x1CBA, "ჺ"),
        (0x1CBD, "ჽ"),
        (0x1CBE, "ჾ"),
        (0x1CBF, "ჿ"),
        (0x1E00, "ḁ"),
        (0x1E02, "ḃ"),
        (0x1E04, "ḅ"),
        (0x1E06, "ḇ"),
        (0x1E08, "ḉ"),
        (0x1E0A, "ḋ"),
        (0x1E0C, "ḍ"),
        (0x1E0E, "ḏ"),
        (0x1E10, "ḑ"),
        (0x1E12, "ḓ"),
        (0x1E14, "ḕ"),
        (0x1E16, "ḗ"),
        (0x1E18, "ḙ"),
        (0x1E1A, "ḛ"),
        (0x1E1C, "ḝ"),
        (0x1E1E, "ḟ"),
        (0x1E20, "ḡ"),
        (0x1E22, "ḣ"),
        (0x1E24, "ḥ"),
        (0x1E26, "ḧ"),
        (0x1E28, "ḩ"),
        (0x1E2A, "ḫ"),
        (0x1E2C, "ḭ"),
        (0x1E2E, "ḯ"),
        (0x1E30, "ḱ"),
        (0x1E32, "ḳ"),
        (0x1E34, "ḵ"),
        (0x1E36, "ḷ"),
        (0x1E38, "ḹ"),
        (0x1E3A, "ḻ"),
        (0x1E3C, "ḽ"),
        (0x1E3E, "ḿ"),
        (0x1E40, "ṁ"),
        (0x1E42, "ṃ"),
        (0x1E44, "ṅ"),
        (0x1E46, "ṇ"),
        (0x1E48, "ṉ"),
        (0x1E4A, "ṋ"),
        (0x1E4C, "ṍ"),
        (0x1E4E, "ṏ"),
        (0x1E50, "ṑ"),
        (0x1E52, "ṓ"),
        (0x1E54, "ṕ"),
        (0x1E56, "ṗ"),
        (0x1E58, "ṙ"),
        (0x1E5A, "ṛ"),
        (0x1E5C, "ṝ"),
        (0x1E5E, "ṟ"),
        (0x1E60, "ṡ"),
        (0x1E62, "ṣ"),
        (0x1E64, "ṥ"),
        (0x1E66, "ṧ"),
        (0x1E68, "ṩ"),
        (0x1E6A, "ṫ"),
        (0x1E6C, "ṭ"),
        (0x1E6E, "ṯ"),
        (0x1E70, "ṱ"),
        (0x1E72, "ṳ"),
        (0x1E74, "ṵ"),
        (0x1E76, "ṷ"),
        (0x1E78, "ṹ"),
        (0x1E7A, "ṻ"),
        (0x1E7C, "ṽ"),
        (0x1E7E, "ṿ"),
        (0x1E80, "ẁ"),
        (0x1E82, "ẃ"),
        (0x1E84, "ẅ"),
        (0x1E86, "ẇ"),
        (0x1E88, "ẉ"),
        (0x1E8A, "ẋ"),
        (0x1E8C, "ẍ"),
        (0x1E8E, "ẏ"),
        (0x1E90, "ẑ"),
        (0x1E92, "ẓ"),
        (0x1E94, "ẕ"),
        (0x1E9E, "ß"),
        (0x1EA0, "ạ"),
        (0x1EA2, "ả"),
        (0x1EA4, "ấ"),
        (0x1EA6, "ầ"),
        (0x1EA8, "ẩ"),
        (0x1EAA, "ẫ"),
        (0x1EAC, "ậ"),
        (0x1EAE, "ắ"),
        (0x1EB0, "ằ"),
        (0x1EB2, "ẳ"),
        (0x1EB4, "ẵ"),
        (0x1EB6, "ặ"),
        (0x1EB8, "ẹ"),
        (0x1EBA, "ẻ"),
        (0x1EBC, "ẽ"),
        (0x1EBE, "ế"),
        (0x1EC0, "ề"),
        (0x1EC2, "ể"),
        (0x1EC4, "ễ"),
        (0x1EC6, "ệ"),
        (0x1EC8, "ỉ"),
        (0x1ECA, "ị"),
        (0x1ECC, "ọ"),
        (0x1ECE, "ỏ"),
        (0x1ED0, "ố"),
        (0x1ED2, "ồ"),
        (0x1ED4, "ổ"),
        (0x1ED6, "ỗ"),
        (0x1ED8, "ộ"),
        (0x1EDA, "ớ"),
        (0x1EDC, "ờ"),
        (0x1EDE, "ở"),
        (0x1EE0, "ỡ"),
        (0x1EE2, "ợ"),
        (0x1EE4, "ụ"),
        (0x1EE6, "ủ"),
        (0x1EE8, "ứ"),
        (0x1EEA, "ừ"),
        (0x1EEC, "ử"),
        (0x1EEE, "ữ"),
        (0x1EF0, "ự"),
        (0x1EF2, "ỳ"),
        (0x1EF4, "ỵ"),
        (0x1EF6, "ỷ"),
        (0x1EF8, "ỹ"),
        (0x1EFA, "ỻ"),
        (0x1EFC, "ỽ"),
        (0x1EFE, "ỿ"),
        (0x1F08, "ἀ"),
        (0x1F09, "ἁ"),
        (0x1F0A, "ἂ"),
        (0x1F0B, "ἃ"),
        (0x1F0C, "ἄ"),
        (0x1F0D, "ἅ"),
        (0x1F0E, "ἆ"),
        (0x1F0F, "ἇ"),
        (0x1F18, "ἐ"),
        (0x1F19, "ἑ"),
        (0x1F1A, "ἒ"),
        (0x1F1B, "ἓ"),
        (0x1F1C, "ἔ"),
        (0x1F1D, "ἕ"),
        (0x1F28, "ἠ"),
        (0x1F29, "ἡ"),
        (0x1F2A, "ἢ"),
        (0x1F2B, "ἣ"),
        (0x1F2C, "ἤ"),
        (0x1F2D, "ἥ"),
        (0x1F2E, "ἦ"),
        (0x1F2F, "ἧ"),
        (0x1F38, "ἰ"),
        (0x1F39, "ἱ"),
        (0x1F3A, "ἲ"),
        (0x1F3B, "ἳ"),
        (0x1F3C, "ἴ"),
        (0x1F3D, "ἵ"),
        (0x1F3E, "ἶ"),
        (0x1F3F, "ἷ"),
        (0x1F48, "ὀ"),
        (0x1F49, "ὁ"),
        (0x1F4A, "ὂ"),
        (0x1F4B, "ὃ"),
        (0x1F4C, "ὄ"),
        (0x1F4D, "ὅ"),
        (0x1F59, "ὑ"),
        (0x1F5B, "ὓ"),
        (0x1F5D, "ὕ"),
        (0x1F5F, "ὗ"),
        (0x1F68, "ὠ"),
        (0x1F69, "ὡ"),
        (0x1F6A, "ὢ"),
        (0x1F6B, "ὣ"),
        (0x1F6C, "ὤ"),
        (0x1F6D, "ὥ"),
        (0x1F6E, "ὦ"),
        (0x1F6F, "ὧ"),
        (0x1F88, "ᾀ"),
        (0x1F89, "ᾁ"),
        (0x1F8A, "ᾂ"),
        (0x1F8B, "ᾃ"),
        (0x1F8C, "ᾄ"),
        (0x1F8D, "ᾅ"),
        (0x1F8E, "ᾆ"),
        (0x1F8F, "ᾇ"),
        (0x1F98, "ᾐ"),
        (0x1F99, "ᾑ"),
        (0x1F9A, "ᾒ"),
        (0x1F9B, "ᾓ"),
        (0x1F9C, "ᾔ"),
        (0x1F9D, "ᾕ"),
        (0x1F9E, "ᾖ"),
        (0x1F9F, "ᾗ"),
        (0x1FA8, "ᾠ"),
        (0x1FA9, "ᾡ"),
        (0x1FAA, "ᾢ"),
        (0x1FAB, "ᾣ"),
        (0x1FAC, "ᾤ"),
        (0x1FAD, "ᾥ"),
        (0x1FAE, "ᾦ"),
        (0x1FAF, "ᾧ"),
        (0x1FB8, "ᾰ"),
        (0x1FB9, "ᾱ"),
        (0x1FBA, "ὰ"),
        (0x1FBB, "ά"),
        (0x1FBC, "ᾳ"),
        (0x1FC8, "ὲ"),
        (0x1FC9, "έ"),
        (0x1FCA, "ὴ"),
        (0x1FCB, "ή"),
        (0x1FCC, "ῃ"),
        (0x1FD8, "ῐ"),
        (0x1FD9, "ῑ"),
        (0x1FDA, "ὶ"),
        (0x1FDB, "ί"),
        (0x1FE8, "ῠ"),
        (0x1FE9, "ῡ"),
        (0x1FEA, "ὺ"),
        (0x1FEB, "ύ"),
        (0x1FEC, "ῥ"),
        (0x1FF8, "ὸ"),
        (0x1FF9, "ό"),
        (0x1FFA, "ὼ"),
        (0x1FFB, "ώ"),
        (0x1FFC, "ῳ"),
        (0x2126, "ω"),
        (0x212A, "k"),
        (0x212B, "å"),
        (0x2132, "ⅎ"),
        (0x2160, "ⅰ"),
        (0x2161, "ⅱ"),
        (0x2162, "ⅲ"),
        (0x2163, "ⅳ"),
        (0x2164, "ⅴ"),
        (0x2165, "ⅵ"),
        (0x2166, "ⅶ"),
        (0x2167, "ⅷ"),
        (0x2168, "ⅸ"),
        (0x2169, "ⅹ"),
        (0x216A, "ⅺ"),
        (0x216B, "ⅻ"),
        (0x216C, "ⅼ"),
        (0x216D, "ⅽ"),
        (0x216E, "ⅾ"),
        (0x216F, "ⅿ"),
        (0x2183, "ↄ"),
        (0x24B6, "ⓐ"),
        (0x24B7, "ⓑ"),
        (0x24B8, "ⓒ"),
        (0x24B9, "ⓓ"),
        (0x24BA, "ⓔ"),
        (0x24BB, "ⓕ"),
        (0x24BC, "ⓖ"),
        (0x24BD, "ⓗ"),
        (0x24BE, "ⓘ"),
        (0x24BF, "ⓙ"),
        (0x24C0, "ⓚ"),
        (0x24C1, "ⓛ"),
        (0x24C2, "ⓜ"),
        (0x24C3, "ⓝ"),
        (0x24C4, "ⓞ"),
        (0x24C5, "ⓟ"),
        (0x24C6, "ⓠ"),
        (0x24C7, "ⓡ"),
        (0x24C8, "ⓢ"),
        (0x24C9, "ⓣ"),
        (0x24CA, "ⓤ"),
        (0x24CB, "ⓥ"),
        (0x24CC, "ⓦ"),
        (0x24CD, "ⓧ"),
        (0x24CE, "ⓨ"),
        (0x24CF, "ⓩ"),
        (0x2C00, "ⰰ"),
        (0x2C01, "ⰱ"),
        (0x2C02, "ⰲ"),
        (0x2C03, "ⰳ"),
        (0x2C04, "ⰴ"),
        (0x2C05, "ⰵ"),
        (0x2C06, "ⰶ"),
        (0x2C07, "ⰷ"),
        (0x2C08, "ⰸ"),
        (0x2C09, "ⰹ"),
        (0x2C0A, "ⰺ"),
        (0x2C0B, "ⰻ"),
        (0x2C0C, "ⰼ"),
        (0x2C0D, "ⰽ"),
        (0x2C0E, "ⰾ"),
        (0x2C0F, "ⰿ"),
        (0x2C10, "ⱀ"),
        (0x2C11, "ⱁ"),
        (0x2C12, "ⱂ"),
        (0x2C13, "ⱃ"),
        (0x2C14, "ⱄ"),
        (0x2C15, "ⱅ"),
        (0x2C16, "ⱆ"),
        (0x2C17, "ⱇ"),
        (0x2C18, "ⱈ"),
        (0x2C19, "ⱉ"),
        (0x2C1A, "ⱊ"),
        (0x2C1B, "ⱋ"),
        (0x2C1C, "ⱌ"),
        (0x2C1D, "ⱍ"),
        (0x2C1E, "ⱎ"),
        (0x2C1F, "ⱏ"),
        (0x2C20, "ⱐ"),
        (0x2C21, "ⱑ"),
        (0x2C22, "ⱒ"),
        (0x2C23, "ⱓ"),
        (0x2C24, "ⱔ"),
        (0x2C25, "ⱕ"),
        (0x2C26, "ⱖ"),
        (0x2C27, "ⱗ"),
        (0x2C28, "ⱘ"),
        (0x2C29, "ⱙ"),
        (0x2C2A, "ⱚ"),
        (0x2C2B, "ⱛ"),
        (0x2C2C, "ⱜ"),
        (0x2C2D, "ⱝ"),
        (0x2C2E, "ⱞ"),
        (0x2C2F, "ⱟ"),
        (0x2C60, "ⱡ"),
        (0x2C62, "ɫ"),
        (0x2C63, "ᵽ"),
        (0x2C64, "ɽ"),
        (0x2C67, "ⱨ"),
        (0x2C69, "ⱪ"),
        (0x2C6B, "ⱬ"),
        (0x2C6D, "ɑ"),
        (0x2C6E, "ɱ"),
        (0x2C6F, "ɐ"),
        (0x2C70, "ɒ"),
        (0x2C72, "ⱳ"),
        (0x2C75, "ⱶ"),
        (0x2C7E, "ȿ"),
        (0x2C7F, "ɀ"),
        (0x2C80, "ⲁ"),
        (0x2C82, "ⲃ"),
        (0x2C84, "ⲅ"),
        (0x2C86, "ⲇ"),
        (0x2C88, "ⲉ"),
        (0x2C8A, "ⲋ"),
        (0x2C8C, "ⲍ"),
        (0x2C8E, "ⲏ"),
        (0x2C90, "ⲑ"),
        (0x2C92, "ⲓ"),
        (0x2C94, "ⲕ"),
        (0x2C96, "ⲗ"),
        (0x2C98, "ⲙ"),
        (0x2C9A, "ⲛ"),
        (0x2C9C, "ⲝ"),
        (0x2C9E, "ⲟ"),
        (0x2CA0, "ⲡ"),
        (0x2CA2, "ⲣ"),
        (0x2CA4, "ⲥ"),
        (0x2CA6, "ⲧ"),
        (0x2CA8, "ⲩ"),
        (0x2CAA, "ⲫ"),
        (0x2CAC, "ⲭ"),
        (0x2CAE, "ⲯ"),
        (0x2CB0, "ⲱ"),
        (0x2CB2, "ⲳ"),
        (0x2CB4, "ⲵ"),
        (0x2CB6, "ⲷ"),
        (0x2CB8, "ⲹ"),
        (0x2CBA, "ⲻ"),
        (0x2CBC, "ⲽ"),
        (0x2CBE, "ⲿ"),
        (0x2CC0, "ⳁ"),
        (0x2CC2, "ⳃ"),
        (0x2CC4, "ⳅ"),
        (0x2CC6, "ⳇ"),
        (0x2CC8, "ⳉ"),
        (0x2CCA, "ⳋ"),
        (0x2CCC, "ⳍ"),
        (0x2CCE, "ⳏ"),
        (0x2CD0, "ⳑ"),
        (0x2CD2, "ⳓ"),
        (0x2CD4, "ⳕ"),
        (0x2CD6, "ⳗ"),
        (0x2CD8, "ⳙ"),
        (0x2CDA, "ⳛ"),
        (0x2CDC, "ⳝ"),
        (0x2CDE, "ⳟ"),
        (0x2CE0, "ⳡ"),
        (0x2CE2, "ⳣ"),
        (0x2CEB, "ⳬ"),
        (0x2CED, "ⳮ"),
        (0x2CF2, "ⳳ"),
        (0xA640, "ꙁ"),
        (0xA642, "ꙃ"),
        (0xA644, "ꙅ"),
        (0xA646, "ꙇ"),
        (0xA648, "ꙉ"),
        (0xA64A, "ꙋ"),
        (0xA64C, "ꙍ"),
        (0xA64E, "ꙏ"),
        (0xA650, "ꙑ"),
        (0xA652, "ꙓ"),
        (0xA654, "ꙕ"),
        (0xA656, "ꙗ"),
        (0xA658, "ꙙ"),
        (0xA65A, "ꙛ"),
        (0xA65C, "ꙝ"),
        (0xA65E, "ꙟ"),
        (0xA660, "ꙡ"),
        (0xA662, "ꙣ"),
        (0xA664, "ꙥ"),
        (0xA666, "ꙧ"),
        (0xA668, "ꙩ"),
        (0xA66A, "ꙫ"),
        (0xA66C, "ꙭ"),
        (0xA680, "ꚁ"),
        (0xA682, "ꚃ"),
        (0xA684, "ꚅ"),
        (0xA686, "ꚇ"),
        (0xA688, "ꚉ"),
        (0xA68A, "ꚋ"),
        (0xA68C, "ꚍ"),
        (0xA68E, "ꚏ"),
        (0xA690, "ꚑ"),
        (0xA692, "ꚓ"),
        (0xA694, "ꚕ"),
        (0xA696, "ꚗ"),
        (0xA698, "ꚙ"),
        (0xA69A, "ꚛ"),
        (0xA722, "ꜣ"),
        (0xA724, "ꜥ"),
        (0xA726, "ꜧ"),
        (0xA728, "ꜩ"),
        (0xA72A, "ꜫ"),
        (0xA72C, "ꜭ"),
        (0xA72E, "ꜯ"),
        (0xA732, "ꜳ"),
        (0xA734, "ꜵ"),
        (0xA736, "ꜷ"),
        (0xA738, "ꜹ"),
        (0xA73A, "ꜻ"),
        (0xA73C, "ꜽ"),
        (0xA73E, "ꜿ"),
        (0xA740, "ꝁ"),
        (0xA742, "ꝃ"),
        (0xA744, "ꝅ"),
        (0xA746, "ꝇ"),
        (0xA748, "ꝉ"),
        (0xA74A, "ꝋ"),
        (0xA74C, "ꝍ"),
        (0xA74E, "ꝏ"),
        (0xA750, "ꝑ"),
        (0xA752, "ꝓ"),
        (0xA754, "ꝕ"),
        (0xA756, "ꝗ"),
        (0xA758, "ꝙ"),
        (0xA75A, "ꝛ"),
        (0xA75C, "ꝝ"),
        (0xA75E, "ꝟ"),
        (0xA760, "ꝡ"),
        (0xA762, "ꝣ"),
        (0xA764, "ꝥ"),
        (0xA766, "ꝧ"),
        (0xA768, "ꝩ"),
        (0xA76A, "ꝫ"),
        (0xA76C, "ꝭ"),
        (0xA76E, "ꝯ"),
        (0xA779, "ꝺ"),
        (0xA77B, "ꝼ"),
        (0xA77D, "ᵹ"),
        (0xA77E, "ꝿ"),
        (0xA780, "ꞁ"),
        (0xA782, "ꞃ"),
        (0xA784, "ꞅ"),
        (0xA786, "ꞇ"),
        (0xA78B, "ꞌ"),
        (0xA78D, "ɥ"),
        (0xA790, "ꞑ"),
        (0xA792, "ꞓ"),
        (0xA796, "ꞗ"),
        (0xA798, "ꞙ"),
        (0xA79A, "ꞛ"),
        (0xA79C, "ꞝ"),
        (0xA79E, "ꞟ"),
        (0xA7A0, "ꞡ"),
        (0xA7A2, "ꞣ"),
        (0xA7A4, "ꞥ"),
        (0xA7A6, "ꞧ"),
        (0xA7A8, "ꞩ"),
        (0xA7AA, "ɦ"),
        (0xA7AB, "ɜ"),
        (0xA7AC, "ɡ"),
        (0xA7AD, "ɬ"),
        (0xA7AE, "ɪ"),
        (0xA7B0, "ʞ"),
        (0xA7B1, "ʇ"),
        (0xA7B2, "ʝ"),
        (0xA7B3, "ꭓ"),
        (0xA7B4, "ꞵ"),
        (0xA7B6, "ꞷ"),
        (0xA7B8, "ꞹ"),
        (0xA7BA, "ꞻ"),
        (0xA7BC, "ꞽ"),
        (0xA7BE, "ꞿ"),
        (0xA7C0, "ꟁ"),
        (0xA7C2, "ꟃ"),
        (0xA7C4, "ꞔ"),
        (0xA7C5, "ʂ"),
        (0xA7C6, "ᶎ"),
        (0xA7C7, "ꟈ"),
        (0xA7C9, "ꟊ"),
        (0xA7D0, "ꟑ"),
        (0xA7D6, "ꟗ"),
        (0xA7D8, "ꟙ"),
        (0xA7F5, "ꟶ"),
        (0xFF21, "ａ"),
        (0xFF22, "ｂ"),
        (0xFF23, "ｃ"),
        (0xFF24, "ｄ"),
        (0xFF25, "ｅ"),
        (0xFF26, "ｆ"),
        (0xFF27, "ｇ"),
        (0xFF28, "ｈ"),
        (0xFF29, "ｉ"),
        (0xFF2A, "ｊ"),
        (0xFF2B, "ｋ"),
        (0xFF2C, "ｌ"),
        (0xFF2D, "ｍ"),
        (0xFF2E, "ｎ"),
        (0xFF2F, "ｏ"),
        (0xFF30, "ｐ"),
        (0xFF31, "ｑ"),
        (0xFF32, "ｒ"),
        (0xFF33, "ｓ"),
        (0xFF34, "ｔ"),
        (0xFF35, "ｕ"),
        (0xFF36, "ｖ"),
        (0xFF37, "ｗ"),
        (0xFF38, "ｘ"),
        (0xFF39, "ｙ"),
        (0xFF3A, "ｚ"),
        (0x10400, "𐐨"),
        (0x10401, "𐐩"),
        (0x10402, "𐐪"),
        (0x10403, "𐐫"),
        (0x10404, "𐐬"),
        (0x10405, "𐐭"),
        (0x10406, "𐐮"),
        (0x10407, "𐐯"),
        (0x10408, "𐐰"),
        (0x10409, "𐐱"),
        (0x1040A, "𐐲"),
        (0x1040B, "𐐳"),
        (0x1040C, "𐐴"),
        (0x1040D, "𐐵"),
        (0x1040E, "𐐶"),
        (0x1040F, "𐐷"),
        (0x10410, "𐐸"),
        (0x10411, "𐐹"),
        (0x10412, "𐐺"),
        (0x10413, "𐐻"),
        (0x10414, "𐐼"),
        (0x10415, "𐐽"),
        (0x10416, "𐐾"),
        (0x10417, "𐐿"),
        (0x10418, "𐑀"),
        (0x10419, "𐑁"),
        (0x1041A, "𐑂"),
        (0x1041B, "𐑃"),
        (0x1041C, "𐑄"),
        (0x1041D, "𐑅"),
        (0x1041E, "𐑆"),
        (0x1041F, "𐑇"),
        (0x10420, "𐑈"),
        (0x10421, "𐑉"),
        (0x10422, "𐑊"),
        (0x10423, "𐑋"),
        (0x10424, "𐑌"),
        (0x10425, "𐑍"),
        (0x10426, "𐑎"),
        (0x10427, "𐑏"),
        (0x104B0, "𐓘"),
        (0x104B1, "𐓙"),
        (0x104B2, "𐓚"),
        (0x104B3, "𐓛"),
        (0x104B4, "𐓜"),
        (0x104B5, "𐓝"),
        (0x104B6, "𐓞"),
        (0x104B7, "𐓟"),
        (0x104B8, "𐓠"),
        (0x104B9, "𐓡"),
        (0x104BA, "𐓢"),
        (0x104BB, "𐓣"),
        (0x104BC, "𐓤"),
        (0x104BD, "𐓥"),
        (0x104BE, "𐓦"),
        (0x104BF, "𐓧"),
        (0x104C0, "𐓨"),
        (0x104C1, "𐓩"),
        (0x104C2, "𐓪"),
        (0x104C3, "𐓫"),
        (0x104C4, "𐓬"),
        (0x104C5, "𐓭"),
        (0x104C6, "𐓮"),
        (0x104C7, "𐓯"),
        (0x104C8, "𐓰"),
        (0x104C9, "𐓱"),
        (0x104CA, "𐓲"),
        (0x104CB, "𐓳"),
        (0x104CC, "𐓴"),
        (0x104CD, "𐓵"),
        (0x104CE, "𐓶"),
        (0x104CF, "𐓷"),
        (0x104D0, "𐓸"),
        (0x104D1, "𐓹"),
        (0x104D2, "𐓺"),
        (0x104D3, "𐓻"),
        (0x10570, "𐖗"),
        (0x10571, "𐖘"),
        (0x10572, "𐖙"),
        (0x10573, "𐖚"),
        (0x10574, "𐖛"),
        (0x10575, "𐖜"),
        (0x10576, "𐖝"),
        (0x10577, "𐖞"),
        (0x10578, "𐖟"),
        (0x10579, "𐖠"),
        (0x1057A, "𐖡"),
        (0x1057C, "𐖣"),
        (0x1057D, "𐖤"),
        (0x1057E, "𐖥"),
        (0x1057F, "𐖦"),
        (0x10580, "𐖧"),
        (0x10581, "𐖨"),
        (0x10582, "𐖩"),
        (0x10583, "𐖪"),
        (0x10584, "𐖫"),
        (0x10585, "𐖬"),
        (0x10586, "𐖭"),
        (0x10587, "𐖮"),
        (0x10588, "𐖯"),
        (0x10589, "𐖰"),
        (0x1058A, "𐖱"),
        (0x1058C, "𐖳"),
        (0x1058D, "𐖴"),
        (0x1058E, "𐖵"),
        (0x1058F, "𐖶"),
        (0x10590, "𐖷"),
        (0x10591, "𐖸"),
        (0x10592, "𐖹"),
        (0x10594, "𐖻"),
        (0x10595, "𐖼"),
        (0x10C80, "𐳀"),
        (0x10C81, "𐳁"),
        (0x10C82, "𐳂"),
        (0x10C83, "𐳃"),
        (0x10C84, "𐳄"),
        (0x10C85, "𐳅"),
        (0x10C86, "𐳆"),
        (0x10C87, "𐳇"),
        (0x10C88, "𐳈"),
        (0x10C89, "𐳉"),
        (0x10C8A, "𐳊"),
        (0x10C8B, "𐳋"),
        (0x10C8C, "𐳌"),
        (0x10C8D, "𐳍"),
        (0x10C8E, "𐳎"),
        (0x10C8F, "𐳏"),
        (0x10C90, "𐳐"),
        (0x10C91, "𐳑"),
        (0x10C92, "𐳒"),
        (0x10C93, "𐳓"),
        (0x10C94, "𐳔"),
        (0x10C95, "𐳕"),
        (0x10C96, "𐳖"),
        (0x10C97, "𐳗"),
        (0x10C98, "𐳘"),
        (0x10C99, "𐳙"),
        (0x10C9A, "𐳚"),
        (0x10C9B, "𐳛"),
        (0x10C9C, "𐳜"),
        (0x10C9D, "𐳝"),
        (0x10C9E, "𐳞"),
        (0x10C9F, "𐳟"),
        (0x10CA0, "𐳠"),
        (0x10CA1, "𐳡"),
        (0x10CA2, "𐳢"),
        (0x10CA3, "𐳣"),
        (0x10CA4, "𐳤"),
        (0x10CA5, "𐳥"),
        (0x10CA6, "𐳦"),
        (0x10CA7, "𐳧"),
        (0x10CA8, "𐳨"),
        (0x10CA9, "𐳩"),
        (0x10CAA, "𐳪"),
        (0x10CAB, "𐳫"),
        (0x10CAC, "𐳬"),
        (0x10CAD, "𐳭"),
        (0x10CAE, "𐳮"),
        (0x10CAF, "𐳯"),
        (0x10CB0, "𐳰"),
        (0x10CB1, "𐳱"),
        (0x10CB2, "𐳲"),
        (0x118A0, "𑣀"),
        (0x118A1, "𑣁"),
        (0x118A2, "𑣂"),
        (0x118A3, "𑣃"),
        (0x118A4, "𑣄"),
        (0x118A5, "𑣅"),
        (0x118A6, "𑣆"),
        (0x118A7, "𑣇"),
        (0x118A8, "𑣈"),
        (0x118A9, "𑣉"),
        (0x118AA, "𑣊"),
        (0x118AB, "𑣋"),
        (0x118AC, "𑣌"),
        (0x118AD, "𑣍"),
        (0x118AE, "𑣎"),
        (0x118AF, "𑣏"),
        (0x118B0, "𑣐"),
        (0x118B1, "𑣑"),
        (0x118B2, "𑣒"),
        (0x118B3, "𑣓"),
        (0x118B4, "𑣔"),
        (0x118B5, "𑣕"),
        (0x118B6, "𑣖"),
        (0x118B7, "𑣗"),
        (0x118B8, "𑣘"),
        (0x118B9, "𑣙"),
        (0x118BA, "𑣚"),
        (0x118BB, "𑣛"),
        (0x118BC, "𑣜"),
        (0x118BD, "𑣝"),
        (0x118BE, "𑣞"),
        (0x118BF, "𑣟"),
        (0x16E40, "𖹠"),
        (0x16E41, "𖹡"),
        (0x16E42, "𖹢"),
        (0x16E43, "𖹣"),
        (0x16E44, "𖹤"),
        (0x16E45, "𖹥"),
        (0x16E46, "𖹦"),
        (0x16E47, "𖹧"),
        (0x16E48, "𖹨"),
        (0x16E49, "𖹩"),
        (0x16E4A, "𖹪"),
        (0x16E4B, "𖹫"),
        (0x16E4C, "𖹬"),
        (0x16E4D, "𖹭"),
        (0x16E4E, "𖹮"),
        (0x16E4F, "𖹯"),
        (0x16E50, "𖹰"),
        (0x16E51, "𖹱"),
        (0x16E52, "𖹲"),
        (0x16E53, "𖹳"),
        (0x16E54, "𖹴"),
        (0x16E55, "𖹵"),
        (0x16E56, "𖹶"),
        (0x16E57, "𖹷"),
        (0x16E58, "𖹸"),
        (0x16E59, "𖹹"),
        (0x16E5A, "𖹺"),
        (0x16E5B, "𖹻"),
        (0x16E5C, "𖹼"),
        (0x16E5D, "𖹽"),
        (0x16E5E, "𖹾"),
        (0x16E5F, "𖹿"),
        (0x1E900, "𞤢"),
        (0x1E901, "𞤣"),
        (0x1E902, "𞤤"),
        (0x1E903, "𞤥"),
        (0x1E904, "𞤦"),
        (0x1E905, "𞤧"),
        (0x1E906, "𞤨"),
        (0x1E907, "𞤩"),
        (0x1E908, "𞤪"),
        (0x1E909, "𞤫"),
        (0x1E90A, "𞤬"),
        (0x1E90B, "𞤭"),
        (0x1E90C, "𞤮"),
        (0x1E90D, "𞤯"),
        (0x1E90E, "𞤰"),
        (0x1E90F, "𞤱"),
        (0x1E910, "𞤲"),
        (0x1E911, "𞤳"),
        (0x1E912, "𞤴"),
        (0x1E913, "𞤵"),
        (0x1E914, "𞤶"),
        (0x1E915, "𞤷"),
        (0x1E916, "𞤸"),
        (0x1E917, "𞤹"),
        (0x1E918, "𞤺"),
        (0x1E919, "𞤻"),
        (0x1E91A, "𞤼"),
        (0x1E91B, "𞤽"),
        (0x1E91C, "𞤾"),
        (0x1E91D, "𞤿"),
        (0x1E91E, "𞥀"),
        (0x1E91F, "𞥁"),
        (0x1E920, "𞥂"),
        (0x1E921, "𞥃"),
    ];
    pub const UPPER_EXC: &[(u32, &str)] = &[
        (0x61, "A"),
        (0x62, "B"),
        (0x63, "C"),
        (0x64, "D"),
        (0x65, "E"),
        (0x66, "F"),
        (0x67, "G"),
        (0x68, "H"),
        (0x69, "I"),
        (0x6A, "J"),
        (0x6B, "K"),
        (0x6C, "L"),
        (0x6D, "M"),
        (0x6E, "N"),
        (0x6F, "O"),
        (0x70, "P"),
        (0x71, "Q"),
        (0x72, "R"),
        (0x73, "S"),
        (0x74, "T"),
        (0x75, "U"),
        (0x76, "V"),
        (0x77, "W"),
        (0x78, "X"),
        (0x79, "Y"),
        (0x7A, "Z"),
        (0xB5, "Μ"),
        (0xDF, "SS"),
        (0xE0, "À"),
        (0xE1, "Á"),
        (0xE2, "Â"),
        (0xE3, "Ã"),
        (0xE4, "Ä"),
        (0xE5, "Å"),
        (0xE6, "Æ"),
        (0xE7, "Ç"),
        (0xE8, "È"),
        (0xE9, "É"),
        (0xEA, "Ê"),
        (0xEB, "Ë"),
        (0xEC, "Ì"),
        (0xED, "Í"),
        (0xEE, "Î"),
        (0xEF, "Ï"),
        (0xF0, "Ð"),
        (0xF1, "Ñ"),
        (0xF2, "Ò"),
        (0xF3, "Ó"),
        (0xF4, "Ô"),
        (0xF5, "Õ"),
        (0xF6, "Ö"),
        (0xF8, "Ø"),
        (0xF9, "Ù"),
        (0xFA, "Ú"),
        (0xFB, "Û"),
        (0xFC, "Ü"),
        (0xFD, "Ý"),
        (0xFE, "Þ"),
        (0xFF, "Ÿ"),
        (0x101, "Ā"),
        (0x103, "Ă"),
        (0x105, "Ą"),
        (0x107, "Ć"),
        (0x109, "Ĉ"),
        (0x10B, "Ċ"),
        (0x10D, "Č"),
        (0x10F, "Ď"),
        (0x111, "Đ"),
        (0x113, "Ē"),
        (0x115, "Ĕ"),
        (0x117, "Ė"),
        (0x119, "Ę"),
        (0x11B, "Ě"),
        (0x11D, "Ĝ"),
        (0x11F, "Ğ"),
        (0x121, "Ġ"),
        (0x123, "Ģ"),
        (0x125, "Ĥ"),
        (0x127, "Ħ"),
        (0x129, "Ĩ"),
        (0x12B, "Ī"),
        (0x12D, "Ĭ"),
        (0x12F, "Į"),
        (0x131, "I"),
        (0x133, "Ĳ"),
        (0x135, "Ĵ"),
        (0x137, "Ķ"),
        (0x13A, "Ĺ"),
        (0x13C, "Ļ"),
        (0x13E, "Ľ"),
        (0x140, "Ŀ"),
        (0x142, "Ł"),
        (0x144, "Ń"),
        (0x146, "Ņ"),
        (0x148, "Ň"),
        (0x149, "ʼN"),
        (0x14B, "Ŋ"),
        (0x14D, "Ō"),
        (0x14F, "Ŏ"),
        (0x151, "Ő"),
        (0x153, "Œ"),
        (0x155, "Ŕ"),
        (0x157, "Ŗ"),
        (0x159, "Ř"),
        (0x15B, "Ś"),
        (0x15D, "Ŝ"),
        (0x15F, "Ş"),
        (0x161, "Š"),
        (0x163, "Ţ"),
        (0x165, "Ť"),
        (0x167, "Ŧ"),
        (0x169, "Ũ"),
        (0x16B, "Ū"),
        (0x16D, "Ŭ"),
        (0x16F, "Ů"),
        (0x171, "Ű"),
        (0x173, "Ų"),
        (0x175, "Ŵ"),
        (0x177, "Ŷ"),
        (0x17A, "Ź"),
        (0x17C, "Ż"),
        (0x17E, "Ž"),
        (0x17F, "S"),
        (0x180, "Ƀ"),
        (0x183, "Ƃ"),
        (0x185, "Ƅ"),
        (0x188, "Ƈ"),
        (0x18C, "Ƌ"),
        (0x192, "Ƒ"),
        (0x195, "Ƕ"),
        (0x199, "Ƙ"),
        (0x19A, "Ƚ"),
        (0x19E, "Ƞ"),
        (0x1A1, "Ơ"),
        (0x1A3, "Ƣ"),
        (0x1A5, "Ƥ"),
        (0x1A8, "Ƨ"),
        (0x1AD, "Ƭ"),
        (0x1B0, "Ư"),
        (0x1B4, "Ƴ"),
        (0x1B6, "Ƶ"),
        (0x1B9, "Ƹ"),
        (0x1BD, "Ƽ"),
        (0x1BF, "Ƿ"),
        (0x1C5, "Ǆ"),
        (0x1C6, "Ǆ"),
        (0x1C8, "Ǉ"),
        (0x1C9, "Ǉ"),
        (0x1CB, "Ǌ"),
        (0x1CC, "Ǌ"),
        (0x1CE, "Ǎ"),
        (0x1D0, "Ǐ"),
        (0x1D2, "Ǒ"),
        (0x1D4, "Ǔ"),
        (0x1D6, "Ǖ"),
        (0x1D8, "Ǘ"),
        (0x1DA, "Ǚ"),
        (0x1DC, "Ǜ"),
        (0x1DD, "Ǝ"),
        (0x1DF, "Ǟ"),
        (0x1E1, "Ǡ"),
        (0x1E3, "Ǣ"),
        (0x1E5, "Ǥ"),
        (0x1E7, "Ǧ"),
        (0x1E9, "Ǩ"),
        (0x1EB, "Ǫ"),
        (0x1ED, "Ǭ"),
        (0x1EF, "Ǯ"),
        (0x1F0, "J̌"),
        (0x1F2, "Ǳ"),
        (0x1F3, "Ǳ"),
        (0x1F5, "Ǵ"),
        (0x1F9, "Ǹ"),
        (0x1FB, "Ǻ"),
        (0x1FD, "Ǽ"),
        (0x1FF, "Ǿ"),
        (0x201, "Ȁ"),
        (0x203, "Ȃ"),
        (0x205, "Ȅ"),
        (0x207, "Ȇ"),
        (0x209, "Ȉ"),
        (0x20B, "Ȋ"),
        (0x20D, "Ȍ"),
        (0x20F, "Ȏ"),
        (0x211, "Ȑ"),
        (0x213, "Ȓ"),
        (0x215, "Ȕ"),
        (0x217, "Ȗ"),
        (0x219, "Ș"),
        (0x21B, "Ț"),
        (0x21D, "Ȝ"),
        (0x21F, "Ȟ"),
        (0x223, "Ȣ"),
        (0x225, "Ȥ"),
        (0x227, "Ȧ"),
        (0x229, "Ȩ"),
        (0x22B, "Ȫ"),
        (0x22D, "Ȭ"),
        (0x22F, "Ȯ"),
        (0x231, "Ȱ"),
        (0x233, "Ȳ"),
        (0x23C, "Ȼ"),
        (0x23F, "Ȿ"),
        (0x240, "Ɀ"),
        (0x242, "Ɂ"),
        (0x247, "Ɇ"),
        (0x249, "Ɉ"),
        (0x24B, "Ɋ"),
        (0x24D, "Ɍ"),
        (0x24F, "Ɏ"),
        (0x250, "Ɐ"),
        (0x251, "Ɑ"),
        (0x252, "Ɒ"),
        (0x253, "Ɓ"),
        (0x254, "Ɔ"),
        (0x256, "Ɖ"),
        (0x257, "Ɗ"),
        (0x259, "Ə"),
        (0x25B, "Ɛ"),
        (0x25C, "Ɜ"),
        (0x260, "Ɠ"),
        (0x261, "Ɡ"),
        (0x263, "Ɣ"),
        (0x265, "Ɥ"),
        (0x266, "Ɦ"),
        (0x268, "Ɨ"),
        (0x269, "Ɩ"),
        (0x26A, "Ɪ"),
        (0x26B, "Ɫ"),
        (0x26C, "Ɬ"),
        (0x26F, "Ɯ"),
        (0x271, "Ɱ"),
        (0x272, "Ɲ"),
        (0x275, "Ɵ"),
        (0x27D, "Ɽ"),
        (0x280, "Ʀ"),
        (0x282, "Ʂ"),
        (0x283, "Ʃ"),
        (0x287, "Ʇ"),
        (0x288, "Ʈ"),
        (0x289, "Ʉ"),
        (0x28A, "Ʊ"),
        (0x28B, "Ʋ"),
        (0x28C, "Ʌ"),
        (0x292, "Ʒ"),
        (0x29D, "Ʝ"),
        (0x29E, "Ʞ"),
        (0x345, "Ι"),
        (0x371, "Ͱ"),
        (0x373, "Ͳ"),
        (0x377, "Ͷ"),
        (0x37B, "Ͻ"),
        (0x37C, "Ͼ"),
        (0x37D, "Ͽ"),
        (0x390, "Ϊ́"),
        (0x3AC, "Ά"),
        (0x3AD, "Έ"),
        (0x3AE, "Ή"),
        (0x3AF, "Ί"),
        (0x3B0, "Ϋ́"),
        (0x3B1, "Α"),
        (0x3B2, "Β"),
        (0x3B3, "Γ"),
        (0x3B4, "Δ"),
        (0x3B5, "Ε"),
        (0x3B6, "Ζ"),
        (0x3B7, "Η"),
        (0x3B8, "Θ"),
        (0x3B9, "Ι"),
        (0x3BA, "Κ"),
        (0x3BB, "Λ"),
        (0x3BC, "Μ"),
        (0x3BD, "Ν"),
        (0x3BE, "Ξ"),
        (0x3BF, "Ο"),
        (0x3C0, "Π"),
        (0x3C1, "Ρ"),
        (0x3C2, "Σ"),
        (0x3C3, "Σ"),
        (0x3C4, "Τ"),
        (0x3C5, "Υ"),
        (0x3C6, "Φ"),
        (0x3C7, "Χ"),
        (0x3C8, "Ψ"),
        (0x3C9, "Ω"),
        (0x3CA, "Ϊ"),
        (0x3CB, "Ϋ"),
        (0x3CC, "Ό"),
        (0x3CD, "Ύ"),
        (0x3CE, "Ώ"),
        (0x3D0, "Β"),
        (0x3D1, "Θ"),
        (0x3D5, "Φ"),
        (0x3D6, "Π"),
        (0x3D7, "Ϗ"),
        (0x3D9, "Ϙ"),
        (0x3DB, "Ϛ"),
        (0x3DD, "Ϝ"),
        (0x3DF, "Ϟ"),
        (0x3E1, "Ϡ"),
        (0x3E3, "Ϣ"),
        (0x3E5, "Ϥ"),
        (0x3E7, "Ϧ"),
        (0x3E9, "Ϩ"),
        (0x3EB, "Ϫ"),
        (0x3ED, "Ϭ"),
        (0x3EF, "Ϯ"),
        (0x3F0, "Κ"),
        (0x3F1, "Ρ"),
        (0x3F2, "Ϲ"),
        (0x3F3, "Ϳ"),
        (0x3F5, "Ε"),
        (0x3F8, "Ϸ"),
        (0x3FB, "Ϻ"),
        (0x430, "А"),
        (0x431, "Б"),
        (0x432, "В"),
        (0x433, "Г"),
        (0x434, "Д"),
        (0x435, "Е"),
        (0x436, "Ж"),
        (0x437, "З"),
        (0x438, "И"),
        (0x439, "Й"),
        (0x43A, "К"),
        (0x43B, "Л"),
        (0x43C, "М"),
        (0x43D, "Н"),
        (0x43E, "О"),
        (0x43F, "П"),
        (0x440, "Р"),
        (0x441, "С"),
        (0x442, "Т"),
        (0x443, "У"),
        (0x444, "Ф"),
        (0x445, "Х"),
        (0x446, "Ц"),
        (0x447, "Ч"),
        (0x448, "Ш"),
        (0x449, "Щ"),
        (0x44A, "Ъ"),
        (0x44B, "Ы"),
        (0x44C, "Ь"),
        (0x44D, "Э"),
        (0x44E, "Ю"),
        (0x44F, "Я"),
        (0x450, "Ѐ"),
        (0x451, "Ё"),
        (0x452, "Ђ"),
        (0x453, "Ѓ"),
        (0x454, "Є"),
        (0x455, "Ѕ"),
        (0x456, "І"),
        (0x457, "Ї"),
        (0x458, "Ј"),
        (0x459, "Љ"),
        (0x45A, "Њ"),
        (0x45B, "Ћ"),
        (0x45C, "Ќ"),
        (0x45D, "Ѝ"),
        (0x45E, "Ў"),
        (0x45F, "Џ"),
        (0x461, "Ѡ"),
        (0x463, "Ѣ"),
        (0x465, "Ѥ"),
        (0x467, "Ѧ"),
        (0x469, "Ѩ"),
        (0x46B, "Ѫ"),
        (0x46D, "Ѭ"),
        (0x46F, "Ѯ"),
        (0x471, "Ѱ"),
        (0x473, "Ѳ"),
        (0x475, "Ѵ"),
        (0x477, "Ѷ"),
        (0x479, "Ѹ"),
        (0x47B, "Ѻ"),
        (0x47D, "Ѽ"),
        (0x47F, "Ѿ"),
        (0x481, "Ҁ"),
        (0x48B, "Ҋ"),
        (0x48D, "Ҍ"),
        (0x48F, "Ҏ"),
        (0x491, "Ґ"),
        (0x493, "Ғ"),
        (0x495, "Ҕ"),
        (0x497, "Җ"),
        (0x499, "Ҙ"),
        (0x49B, "Қ"),
        (0x49D, "Ҝ"),
        (0x49F, "Ҟ"),
        (0x4A1, "Ҡ"),
        (0x4A3, "Ң"),
        (0x4A5, "Ҥ"),
        (0x4A7, "Ҧ"),
        (0x4A9, "Ҩ"),
        (0x4AB, "Ҫ"),
        (0x4AD, "Ҭ"),
        (0x4AF, "Ү"),
        (0x4B1, "Ұ"),
        (0x4B3, "Ҳ"),
        (0x4B5, "Ҵ"),
        (0x4B7, "Ҷ"),
        (0x4B9, "Ҹ"),
        (0x4BB, "Һ"),
        (0x4BD, "Ҽ"),
        (0x4BF, "Ҿ"),
        (0x4C2, "Ӂ"),
        (0x4C4, "Ӄ"),
        (0x4C6, "Ӆ"),
        (0x4C8, "Ӈ"),
        (0x4CA, "Ӊ"),
        (0x4CC, "Ӌ"),
        (0x4CE, "Ӎ"),
        (0x4CF, "Ӏ"),
        (0x4D1, "Ӑ"),
        (0x4D3, "Ӓ"),
        (0x4D5, "Ӕ"),
        (0x4D7, "Ӗ"),
        (0x4D9, "Ә"),
        (0x4DB, "Ӛ"),
        (0x4DD, "Ӝ"),
        (0x4DF, "Ӟ"),
        (0x4E1, "Ӡ"),
        (0x4E3, "Ӣ"),
        (0x4E5, "Ӥ"),
        (0x4E7, "Ӧ"),
        (0x4E9, "Ө"),
        (0x4EB, "Ӫ"),
        (0x4ED, "Ӭ"),
        (0x4EF, "Ӯ"),
        (0x4F1, "Ӱ"),
        (0x4F3, "Ӳ"),
        (0x4F5, "Ӵ"),
        (0x4F7, "Ӷ"),
        (0x4F9, "Ӹ"),
        (0x4FB, "Ӻ"),
        (0x4FD, "Ӽ"),
        (0x4FF, "Ӿ"),
        (0x501, "Ԁ"),
        (0x503, "Ԃ"),
        (0x505, "Ԅ"),
        (0x507, "Ԇ"),
        (0x509, "Ԉ"),
        (0x50B, "Ԋ"),
        (0x50D, "Ԍ"),
        (0x50F, "Ԏ"),
        (0x511, "Ԑ"),
        (0x513, "Ԓ"),
        (0x515, "Ԕ"),
        (0x517, "Ԗ"),
        (0x519, "Ԙ"),
        (0x51B, "Ԛ"),
        (0x51D, "Ԝ"),
        (0x51F, "Ԟ"),
        (0x521, "Ԡ"),
        (0x523, "Ԣ"),
        (0x525, "Ԥ"),
        (0x527, "Ԧ"),
        (0x529, "Ԩ"),
        (0x52B, "Ԫ"),
        (0x52D, "Ԭ"),
        (0x52F, "Ԯ"),
        (0x561, "Ա"),
        (0x562, "Բ"),
        (0x563, "Գ"),
        (0x564, "Դ"),
        (0x565, "Ե"),
        (0x566, "Զ"),
        (0x567, "Է"),
        (0x568, "Ը"),
        (0x569, "Թ"),
        (0x56A, "Ժ"),
        (0x56B, "Ի"),
        (0x56C, "Լ"),
        (0x56D, "Խ"),
        (0x56E, "Ծ"),
        (0x56F, "Կ"),
        (0x570, "Հ"),
        (0x571, "Ձ"),
        (0x572, "Ղ"),
        (0x573, "Ճ"),
        (0x574, "Մ"),
        (0x575, "Յ"),
        (0x576, "Ն"),
        (0x577, "Շ"),
        (0x578, "Ո"),
        (0x579, "Չ"),
        (0x57A, "Պ"),
        (0x57B, "Ջ"),
        (0x57C, "Ռ"),
        (0x57D, "Ս"),
        (0x57E, "Վ"),
        (0x57F, "Տ"),
        (0x580, "Ր"),
        (0x581, "Ց"),
        (0x582, "Ւ"),
        (0x583, "Փ"),
        (0x584, "Ք"),
        (0x585, "Օ"),
        (0x586, "Ֆ"),
        (0x587, "ԵՒ"),
        (0x10D0, "Ა"),
        (0x10D1, "Ბ"),
        (0x10D2, "Გ"),
        (0x10D3, "Დ"),
        (0x10D4, "Ე"),
        (0x10D5, "Ვ"),
        (0x10D6, "Ზ"),
        (0x10D7, "Თ"),
        (0x10D8, "Ი"),
        (0x10D9, "Კ"),
        (0x10DA, "Ლ"),
        (0x10DB, "Მ"),
        (0x10DC, "Ნ"),
        (0x10DD, "Ო"),
        (0x10DE, "Პ"),
        (0x10DF, "Ჟ"),
        (0x10E0, "Რ"),
        (0x10E1, "Ს"),
        (0x10E2, "Ტ"),
        (0x10E3, "Უ"),
        (0x10E4, "Ფ"),
        (0x10E5, "Ქ"),
        (0x10E6, "Ღ"),
        (0x10E7, "Ყ"),
        (0x10E8, "Შ"),
        (0x10E9, "Ჩ"),
        (0x10EA, "Ც"),
        (0x10EB, "Ძ"),
        (0x10EC, "Წ"),
        (0x10ED, "Ჭ"),
        (0x10EE, "Ხ"),
        (0x10EF, "Ჯ"),
        (0x10F0, "Ჰ"),
        (0x10F1, "Ჱ"),
        (0x10F2, "Ჲ"),
        (0x10F3, "Ჳ"),
        (0x10F4, "Ჴ"),
        (0x10F5, "Ჵ"),
        (0x10F6, "Ჶ"),
        (0x10F7, "Ჷ"),
        (0x10F8, "Ჸ"),
        (0x10F9, "Ჹ"),
        (0x10FA, "Ჺ"),
        (0x10FD, "Ჽ"),
        (0x10FE, "Ჾ"),
        (0x10FF, "Ჿ"),
        (0x13F8, "Ᏸ"),
        (0x13F9, "Ᏹ"),
        (0x13FA, "Ᏺ"),
        (0x13FB, "Ᏻ"),
        (0x13FC, "Ᏼ"),
        (0x13FD, "Ᏽ"),
        (0x1C80, "В"),
        (0x1C81, "Д"),
        (0x1C82, "О"),
        (0x1C83, "С"),
        (0x1C84, "Т"),
        (0x1C85, "Т"),
        (0x1C86, "Ъ"),
        (0x1C87, "Ѣ"),
        (0x1C88, "Ꙋ"),
        (0x1D79, "Ᵹ"),
        (0x1D7D, "Ᵽ"),
        (0x1D8E, "Ᶎ"),
        (0x1E01, "Ḁ"),
        (0x1E03, "Ḃ"),
        (0x1E05, "Ḅ"),
        (0x1E07, "Ḇ"),
        (0x1E09, "Ḉ"),
        (0x1E0B, "Ḋ"),
        (0x1E0D, "Ḍ"),
        (0x1E0F, "Ḏ"),
        (0x1E11, "Ḑ"),
        (0x1E13, "Ḓ"),
        (0x1E15, "Ḕ"),
        (0x1E17, "Ḗ"),
        (0x1E19, "Ḙ"),
        (0x1E1B, "Ḛ"),
        (0x1E1D, "Ḝ"),
        (0x1E1F, "Ḟ"),
        (0x1E21, "Ḡ"),
        (0x1E23, "Ḣ"),
        (0x1E25, "Ḥ"),
        (0x1E27, "Ḧ"),
        (0x1E29, "Ḩ"),
        (0x1E2B, "Ḫ"),
        (0x1E2D, "Ḭ"),
        (0x1E2F, "Ḯ"),
        (0x1E31, "Ḱ"),
        (0x1E33, "Ḳ"),
        (0x1E35, "Ḵ"),
        (0x1E37, "Ḷ"),
        (0x1E39, "Ḹ"),
        (0x1E3B, "Ḻ"),
        (0x1E3D, "Ḽ"),
        (0x1E3F, "Ḿ"),
        (0x1E41, "Ṁ"),
        (0x1E43, "Ṃ"),
        (0x1E45, "Ṅ"),
        (0x1E47, "Ṇ"),
        (0x1E49, "Ṉ"),
        (0x1E4B, "Ṋ"),
        (0x1E4D, "Ṍ"),
        (0x1E4F, "Ṏ"),
        (0x1E51, "Ṑ"),
        (0x1E53, "Ṓ"),
        (0x1E55, "Ṕ"),
        (0x1E57, "Ṗ"),
        (0x1E59, "Ṙ"),
        (0x1E5B, "Ṛ"),
        (0x1E5D, "Ṝ"),
        (0x1E5F, "Ṟ"),
        (0x1E61, "Ṡ"),
        (0x1E63, "Ṣ"),
        (0x1E65, "Ṥ"),
        (0x1E67, "Ṧ"),
        (0x1E69, "Ṩ"),
        (0x1E6B, "Ṫ"),
        (0x1E6D, "Ṭ"),
        (0x1E6F, "Ṯ"),
        (0x1E71, "Ṱ"),
        (0x1E73, "Ṳ"),
        (0x1E75, "Ṵ"),
        (0x1E77, "Ṷ"),
        (0x1E79, "Ṹ"),
        (0x1E7B, "Ṻ"),
        (0x1E7D, "Ṽ"),
        (0x1E7F, "Ṿ"),
        (0x1E81, "Ẁ"),
        (0x1E83, "Ẃ"),
        (0x1E85, "Ẅ"),
        (0x1E87, "Ẇ"),
        (0x1E89, "Ẉ"),
        (0x1E8B, "Ẋ"),
        (0x1E8D, "Ẍ"),
        (0x1E8F, "Ẏ"),
        (0x1E91, "Ẑ"),
        (0x1E93, "Ẓ"),
        (0x1E95, "Ẕ"),
        (0x1E96, "H̱"),
        (0x1E97, "T̈"),
        (0x1E98, "W̊"),
        (0x1E99, "Y̊"),
        (0x1E9A, "Aʾ"),
        (0x1E9B, "Ṡ"),
        (0x1EA1, "Ạ"),
        (0x1EA3, "Ả"),
        (0x1EA5, "Ấ"),
        (0x1EA7, "Ầ"),
        (0x1EA9, "Ẩ"),
        (0x1EAB, "Ẫ"),
        (0x1EAD, "Ậ"),
        (0x1EAF, "Ắ"),
        (0x1EB1, "Ằ"),
        (0x1EB3, "Ẳ"),
        (0x1EB5, "Ẵ"),
        (0x1EB7, "Ặ"),
        (0x1EB9, "Ẹ"),
        (0x1EBB, "Ẻ"),
        (0x1EBD, "Ẽ"),
        (0x1EBF, "Ế"),
        (0x1EC1, "Ề"),
        (0x1EC3, "Ể"),
        (0x1EC5, "Ễ"),
        (0x1EC7, "Ệ"),
        (0x1EC9, "Ỉ"),
        (0x1ECB, "Ị"),
        (0x1ECD, "Ọ"),
        (0x1ECF, "Ỏ"),
        (0x1ED1, "Ố"),
        (0x1ED3, "Ồ"),
        (0x1ED5, "Ổ"),
        (0x1ED7, "Ỗ"),
        (0x1ED9, "Ộ"),
        (0x1EDB, "Ớ"),
        (0x1EDD, "Ờ"),
        (0x1EDF, "Ở"),
        (0x1EE1, "Ỡ"),
        (0x1EE3, "Ợ"),
        (0x1EE5, "Ụ"),
        (0x1EE7, "Ủ"),
        (0x1EE9, "Ứ"),
        (0x1EEB, "Ừ"),
        (0x1EED, "Ử"),
        (0x1EEF, "Ữ"),
        (0x1EF1, "Ự"),
        (0x1EF3, "Ỳ"),
        (0x1EF5, "Ỵ"),
        (0x1EF7, "Ỷ"),
        (0x1EF9, "Ỹ"),
        (0x1EFB, "Ỻ"),
        (0x1EFD, "Ỽ"),
        (0x1EFF, "Ỿ"),
        (0x1F00, "Ἀ"),
        (0x1F01, "Ἁ"),
        (0x1F02, "Ἂ"),
        (0x1F03, "Ἃ"),
        (0x1F04, "Ἄ"),
        (0x1F05, "Ἅ"),
        (0x1F06, "Ἆ"),
        (0x1F07, "Ἇ"),
        (0x1F10, "Ἐ"),
        (0x1F11, "Ἑ"),
        (0x1F12, "Ἒ"),
        (0x1F13, "Ἓ"),
        (0x1F14, "Ἔ"),
        (0x1F15, "Ἕ"),
        (0x1F20, "Ἠ"),
        (0x1F21, "Ἡ"),
        (0x1F22, "Ἢ"),
        (0x1F23, "Ἣ"),
        (0x1F24, "Ἤ"),
        (0x1F25, "Ἥ"),
        (0x1F26, "Ἦ"),
        (0x1F27, "Ἧ"),
        (0x1F30, "Ἰ"),
        (0x1F31, "Ἱ"),
        (0x1F32, "Ἲ"),
        (0x1F33, "Ἳ"),
        (0x1F34, "Ἴ"),
        (0x1F35, "Ἵ"),
        (0x1F36, "Ἶ"),
        (0x1F37, "Ἷ"),
        (0x1F40, "Ὀ"),
        (0x1F41, "Ὁ"),
        (0x1F42, "Ὂ"),
        (0x1F43, "Ὃ"),
        (0x1F44, "Ὄ"),
        (0x1F45, "Ὅ"),
        (0x1F50, "Υ̓"),
        (0x1F51, "Ὑ"),
        (0x1F52, "Υ̓̀"),
        (0x1F53, "Ὓ"),
        (0x1F54, "Υ̓́"),
        (0x1F55, "Ὕ"),
        (0x1F56, "Υ̓͂"),
        (0x1F57, "Ὗ"),
        (0x1F60, "Ὠ"),
        (0x1F61, "Ὡ"),
        (0x1F62, "Ὢ"),
        (0x1F63, "Ὣ"),
        (0x1F64, "Ὤ"),
        (0x1F65, "Ὥ"),
        (0x1F66, "Ὦ"),
        (0x1F67, "Ὧ"),
        (0x1F70, "Ὰ"),
        (0x1F71, "Ά"),
        (0x1F72, "Ὲ"),
        (0x1F73, "Έ"),
        (0x1F74, "Ὴ"),
        (0x1F75, "Ή"),
        (0x1F76, "Ὶ"),
        (0x1F77, "Ί"),
        (0x1F78, "Ὸ"),
        (0x1F79, "Ό"),
        (0x1F7A, "Ὺ"),
        (0x1F7B, "Ύ"),
        (0x1F7C, "Ὼ"),
        (0x1F7D, "Ώ"),
        (0x1F80, "ἈΙ"),
        (0x1F81, "ἉΙ"),
        (0x1F82, "ἊΙ"),
        (0x1F83, "ἋΙ"),
        (0x1F84, "ἌΙ"),
        (0x1F85, "ἍΙ"),
        (0x1F86, "ἎΙ"),
        (0x1F87, "ἏΙ"),
        (0x1F88, "ἈΙ"),
        (0x1F89, "ἉΙ"),
        (0x1F8A, "ἊΙ"),
        (0x1F8B, "ἋΙ"),
        (0x1F8C, "ἌΙ"),
        (0x1F8D, "ἍΙ"),
        (0x1F8E, "ἎΙ"),
        (0x1F8F, "ἏΙ"),
        (0x1F90, "ἨΙ"),
        (0x1F91, "ἩΙ"),
        (0x1F92, "ἪΙ"),
        (0x1F93, "ἫΙ"),
        (0x1F94, "ἬΙ"),
        (0x1F95, "ἭΙ"),
        (0x1F96, "ἮΙ"),
        (0x1F97, "ἯΙ"),
        (0x1F98, "ἨΙ"),
        (0x1F99, "ἩΙ"),
        (0x1F9A, "ἪΙ"),
        (0x1F9B, "ἫΙ"),
        (0x1F9C, "ἬΙ"),
        (0x1F9D, "ἭΙ"),
        (0x1F9E, "ἮΙ"),
        (0x1F9F, "ἯΙ"),
        (0x1FA0, "ὨΙ"),
        (0x1FA1, "ὩΙ"),
        (0x1FA2, "ὪΙ"),
        (0x1FA3, "ὫΙ"),
        (0x1FA4, "ὬΙ"),
        (0x1FA5, "ὭΙ"),
        (0x1FA6, "ὮΙ"),
        (0x1FA7, "ὯΙ"),
        (0x1FA8, "ὨΙ"),
        (0x1FA9, "ὩΙ"),
        (0x1FAA, "ὪΙ"),
        (0x1FAB, "ὫΙ"),
        (0x1FAC, "ὬΙ"),
        (0x1FAD, "ὭΙ"),
        (0x1FAE, "ὮΙ"),
        (0x1FAF, "ὯΙ"),
        (0x1FB0, "Ᾰ"),
        (0x1FB1, "Ᾱ"),
        (0x1FB2, "ᾺΙ"),
        (0x1FB3, "ΑΙ"),
        (0x1FB4, "ΆΙ"),
        (0x1FB6, "Α͂"),
        (0x1FB7, "Α͂Ι"),
        (0x1FBC, "ΑΙ"),
        (0x1FBE, "Ι"),
        (0x1FC2, "ῊΙ"),
        (0x1FC3, "ΗΙ"),
        (0x1FC4, "ΉΙ"),
        (0x1FC6, "Η͂"),
        (0x1FC7, "Η͂Ι"),
        (0x1FCC, "ΗΙ"),
        (0x1FD0, "Ῐ"),
        (0x1FD1, "Ῑ"),
        (0x1FD2, "Ϊ̀"),
        (0x1FD3, "Ϊ́"),
        (0x1FD6, "Ι͂"),
        (0x1FD7, "Ϊ͂"),
        (0x1FE0, "Ῠ"),
        (0x1FE1, "Ῡ"),
        (0x1FE2, "Ϋ̀"),
        (0x1FE3, "Ϋ́"),
        (0x1FE4, "Ρ̓"),
        (0x1FE5, "Ῥ"),
        (0x1FE6, "Υ͂"),
        (0x1FE7, "Ϋ͂"),
        (0x1FF2, "ῺΙ"),
        (0x1FF3, "ΩΙ"),
        (0x1FF4, "ΏΙ"),
        (0x1FF6, "Ω͂"),
        (0x1FF7, "Ω͂Ι"),
        (0x1FFC, "ΩΙ"),
        (0x214E, "Ⅎ"),
        (0x2170, "Ⅰ"),
        (0x2171, "Ⅱ"),
        (0x2172, "Ⅲ"),
        (0x2173, "Ⅳ"),
        (0x2174, "Ⅴ"),
        (0x2175, "Ⅵ"),
        (0x2176, "Ⅶ"),
        (0x2177, "Ⅷ"),
        (0x2178, "Ⅸ"),
        (0x2179, "Ⅹ"),
        (0x217A, "Ⅺ"),
        (0x217B, "Ⅻ"),
        (0x217C, "Ⅼ"),
        (0x217D, "Ⅽ"),
        (0x217E, "Ⅾ"),
        (0x217F, "Ⅿ"),
        (0x2184, "Ↄ"),
        (0x24D0, "Ⓐ"),
        (0x24D1, "Ⓑ"),
        (0x24D2, "Ⓒ"),
        (0x24D3, "Ⓓ"),
        (0x24D4, "Ⓔ"),
        (0x24D5, "Ⓕ"),
        (0x24D6, "Ⓖ"),
        (0x24D7, "Ⓗ"),
        (0x24D8, "Ⓘ"),
        (0x24D9, "Ⓙ"),
        (0x24DA, "Ⓚ"),
        (0x24DB, "Ⓛ"),
        (0x24DC, "Ⓜ"),
        (0x24DD, "Ⓝ"),
        (0x24DE, "Ⓞ"),
        (0x24DF, "Ⓟ"),
        (0x24E0, "Ⓠ"),
        (0x24E1, "Ⓡ"),
        (0x24E2, "Ⓢ"),
        (0x24E3, "Ⓣ"),
        (0x24E4, "Ⓤ"),
        (0x24E5, "Ⓥ"),
        (0x24E6, "Ⓦ"),
        (0x24E7, "Ⓧ"),
        (0x24E8, "Ⓨ"),
        (0x24E9, "Ⓩ"),
        (0x2C30, "Ⰰ"),
        (0x2C31, "Ⰱ"),
        (0x2C32, "Ⰲ"),
        (0x2C33, "Ⰳ"),
        (0x2C34, "Ⰴ"),
        (0x2C35, "Ⰵ"),
        (0x2C36, "Ⰶ"),
        (0x2C37, "Ⰷ"),
        (0x2C38, "Ⰸ"),
        (0x2C39, "Ⰹ"),
        (0x2C3A, "Ⰺ"),
        (0x2C3B, "Ⰻ"),
        (0x2C3C, "Ⰼ"),
        (0x2C3D, "Ⰽ"),
        (0x2C3E, "Ⰾ"),
        (0x2C3F, "Ⰿ"),
        (0x2C40, "Ⱀ"),
        (0x2C41, "Ⱁ"),
        (0x2C42, "Ⱂ"),
        (0x2C43, "Ⱃ"),
        (0x2C44, "Ⱄ"),
        (0x2C45, "Ⱅ"),
        (0x2C46, "Ⱆ"),
        (0x2C47, "Ⱇ"),
        (0x2C48, "Ⱈ"),
        (0x2C49, "Ⱉ"),
        (0x2C4A, "Ⱊ"),
        (0x2C4B, "Ⱋ"),
        (0x2C4C, "Ⱌ"),
        (0x2C4D, "Ⱍ"),
        (0x2C4E, "Ⱎ"),
        (0x2C4F, "Ⱏ"),
        (0x2C50, "Ⱐ"),
        (0x2C51, "Ⱑ"),
        (0x2C52, "Ⱒ"),
        (0x2C53, "Ⱓ"),
        (0x2C54, "Ⱔ"),
        (0x2C55, "Ⱕ"),
        (0x2C56, "Ⱖ"),
        (0x2C57, "Ⱗ"),
        (0x2C58, "Ⱘ"),
        (0x2C59, "Ⱙ"),
        (0x2C5A, "Ⱚ"),
        (0x2C5B, "Ⱛ"),
        (0x2C5C, "Ⱜ"),
        (0x2C5D, "Ⱝ"),
        (0x2C5E, "Ⱞ"),
        (0x2C5F, "Ⱟ"),
        (0x2C61, "Ⱡ"),
        (0x2C65, "Ⱥ"),
        (0x2C66, "Ⱦ"),
        (0x2C68, "Ⱨ"),
        (0x2C6A, "Ⱪ"),
        (0x2C6C, "Ⱬ"),
        (0x2C73, "Ⱳ"),
        (0x2C76, "Ⱶ"),
        (0x2C81, "Ⲁ"),
        (0x2C83, "Ⲃ"),
        (0x2C85, "Ⲅ"),
        (0x2C87, "Ⲇ"),
        (0x2C89, "Ⲉ"),
        (0x2C8B, "Ⲋ"),
        (0x2C8D, "Ⲍ"),
        (0x2C8F, "Ⲏ"),
        (0x2C91, "Ⲑ"),
        (0x2C93, "Ⲓ"),
        (0x2C95, "Ⲕ"),
        (0x2C97, "Ⲗ"),
        (0x2C99, "Ⲙ"),
        (0x2C9B, "Ⲛ"),
        (0x2C9D, "Ⲝ"),
        (0x2C9F, "Ⲟ"),
        (0x2CA1, "Ⲡ"),
        (0x2CA3, "Ⲣ"),
        (0x2CA5, "Ⲥ"),
        (0x2CA7, "Ⲧ"),
        (0x2CA9, "Ⲩ"),
        (0x2CAB, "Ⲫ"),
        (0x2CAD, "Ⲭ"),
        (0x2CAF, "Ⲯ"),
        (0x2CB1, "Ⲱ"),
        (0x2CB3, "Ⲳ"),
        (0x2CB5, "Ⲵ"),
        (0x2CB7, "Ⲷ"),
        (0x2CB9, "Ⲹ"),
        (0x2CBB, "Ⲻ"),
        (0x2CBD, "Ⲽ"),
        (0x2CBF, "Ⲿ"),
        (0x2CC1, "Ⳁ"),
        (0x2CC3, "Ⳃ"),
        (0x2CC5, "Ⳅ"),
        (0x2CC7, "Ⳇ"),
        (0x2CC9, "Ⳉ"),
        (0x2CCB, "Ⳋ"),
        (0x2CCD, "Ⳍ"),
        (0x2CCF, "Ⳏ"),
        (0x2CD1, "Ⳑ"),
        (0x2CD3, "Ⳓ"),
        (0x2CD5, "Ⳕ"),
        (0x2CD7, "Ⳗ"),
        (0x2CD9, "Ⳙ"),
        (0x2CDB, "Ⳛ"),
        (0x2CDD, "Ⳝ"),
        (0x2CDF, "Ⳟ"),
        (0x2CE1, "Ⳡ"),
        (0x2CE3, "Ⳣ"),
        (0x2CEC, "Ⳬ"),
        (0x2CEE, "Ⳮ"),
        (0x2CF3, "Ⳳ"),
        (0x2D00, "Ⴀ"),
        (0x2D01, "Ⴁ"),
        (0x2D02, "Ⴂ"),
        (0x2D03, "Ⴃ"),
        (0x2D04, "Ⴄ"),
        (0x2D05, "Ⴅ"),
        (0x2D06, "Ⴆ"),
        (0x2D07, "Ⴇ"),
        (0x2D08, "Ⴈ"),
        (0x2D09, "Ⴉ"),
        (0x2D0A, "Ⴊ"),
        (0x2D0B, "Ⴋ"),
        (0x2D0C, "Ⴌ"),
        (0x2D0D, "Ⴍ"),
        (0x2D0E, "Ⴎ"),
        (0x2D0F, "Ⴏ"),
        (0x2D10, "Ⴐ"),
        (0x2D11, "Ⴑ"),
        (0x2D12, "Ⴒ"),
        (0x2D13, "Ⴓ"),
        (0x2D14, "Ⴔ"),
        (0x2D15, "Ⴕ"),
        (0x2D16, "Ⴖ"),
        (0x2D17, "Ⴗ"),
        (0x2D18, "Ⴘ"),
        (0x2D19, "Ⴙ"),
        (0x2D1A, "Ⴚ"),
        (0x2D1B, "Ⴛ"),
        (0x2D1C, "Ⴜ"),
        (0x2D1D, "Ⴝ"),
        (0x2D1E, "Ⴞ"),
        (0x2D1F, "Ⴟ"),
        (0x2D20, "Ⴠ"),
        (0x2D21, "Ⴡ"),
        (0x2D22, "Ⴢ"),
        (0x2D23, "Ⴣ"),
        (0x2D24, "Ⴤ"),
        (0x2D25, "Ⴥ"),
        (0x2D27, "Ⴧ"),
        (0x2D2D, "Ⴭ"),
        (0xA641, "Ꙁ"),
        (0xA643, "Ꙃ"),
        (0xA645, "Ꙅ"),
        (0xA647, "Ꙇ"),
        (0xA649, "Ꙉ"),
        (0xA64B, "Ꙋ"),
        (0xA64D, "Ꙍ"),
        (0xA64F, "Ꙏ"),
        (0xA651, "Ꙑ"),
        (0xA653, "Ꙓ"),
        (0xA655, "Ꙕ"),
        (0xA657, "Ꙗ"),
        (0xA659, "Ꙙ"),
        (0xA65B, "Ꙛ"),
        (0xA65D, "Ꙝ"),
        (0xA65F, "Ꙟ"),
        (0xA661, "Ꙡ"),
        (0xA663, "Ꙣ"),
        (0xA665, "Ꙥ"),
        (0xA667, "Ꙧ"),
        (0xA669, "Ꙩ"),
        (0xA66B, "Ꙫ"),
        (0xA66D, "Ꙭ"),
        (0xA681, "Ꚁ"),
        (0xA683, "Ꚃ"),
        (0xA685, "Ꚅ"),
        (0xA687, "Ꚇ"),
        (0xA689, "Ꚉ"),
        (0xA68B, "Ꚋ"),
        (0xA68D, "Ꚍ"),
        (0xA68F, "Ꚏ"),
        (0xA691, "Ꚑ"),
        (0xA693, "Ꚓ"),
        (0xA695, "Ꚕ"),
        (0xA697, "Ꚗ"),
        (0xA699, "Ꚙ"),
        (0xA69B, "Ꚛ"),
        (0xA723, "Ꜣ"),
        (0xA725, "Ꜥ"),
        (0xA727, "Ꜧ"),
        (0xA729, "Ꜩ"),
        (0xA72B, "Ꜫ"),
        (0xA72D, "Ꜭ"),
        (0xA72F, "Ꜯ"),
        (0xA733, "Ꜳ"),
        (0xA735, "Ꜵ"),
        (0xA737, "Ꜷ"),
        (0xA739, "Ꜹ"),
        (0xA73B, "Ꜻ"),
        (0xA73D, "Ꜽ"),
        (0xA73F, "Ꜿ"),
        (0xA741, "Ꝁ"),
        (0xA743, "Ꝃ"),
        (0xA745, "Ꝅ"),
        (0xA747, "Ꝇ"),
        (0xA749, "Ꝉ"),
        (0xA74B, "Ꝋ"),
        (0xA74D, "Ꝍ"),
        (0xA74F, "Ꝏ"),
        (0xA751, "Ꝑ"),
        (0xA753, "Ꝓ"),
        (0xA755, "Ꝕ"),
        (0xA757, "Ꝗ"),
        (0xA759, "Ꝙ"),
        (0xA75B, "Ꝛ"),
        (0xA75D, "Ꝝ"),
        (0xA75F, "Ꝟ"),
        (0xA761, "Ꝡ"),
        (0xA763, "Ꝣ"),
        (0xA765, "Ꝥ"),
        (0xA767, "Ꝧ"),
        (0xA769, "Ꝩ"),
        (0xA76B, "Ꝫ"),
        (0xA76D, "Ꝭ"),
        (0xA76F, "Ꝯ"),
        (0xA77A, "Ꝺ"),
        (0xA77C, "Ꝼ"),
        (0xA77F, "Ꝿ"),
        (0xA781, "Ꞁ"),
        (0xA783, "Ꞃ"),
        (0xA785, "Ꞅ"),
        (0xA787, "Ꞇ"),
        (0xA78C, "Ꞌ"),
        (0xA791, "Ꞑ"),
        (0xA793, "Ꞓ"),
        (0xA794, "Ꞔ"),
        (0xA797, "Ꞗ"),
        (0xA799, "Ꞙ"),
        (0xA79B, "Ꞛ"),
        (0xA79D, "Ꞝ"),
        (0xA79F, "Ꞟ"),
        (0xA7A1, "Ꞡ"),
        (0xA7A3, "Ꞣ"),
        (0xA7A5, "Ꞥ"),
        (0xA7A7, "Ꞧ"),
        (0xA7A9, "Ꞩ"),
        (0xA7B5, "Ꞵ"),
        (0xA7B7, "Ꞷ"),
        (0xA7B9, "Ꞹ"),
        (0xA7BB, "Ꞻ"),
        (0xA7BD, "Ꞽ"),
        (0xA7BF, "Ꞿ"),
        (0xA7C1, "Ꟁ"),
        (0xA7C3, "Ꟃ"),
        (0xA7C8, "Ꟈ"),
        (0xA7CA, "Ꟊ"),
        (0xA7D1, "Ꟑ"),
        (0xA7D7, "Ꟗ"),
        (0xA7D9, "Ꟙ"),
        (0xA7F6, "Ꟶ"),
        (0xAB53, "Ꭓ"),
        (0xAB70, "Ꭰ"),
        (0xAB71, "Ꭱ"),
        (0xAB72, "Ꭲ"),
        (0xAB73, "Ꭳ"),
        (0xAB74, "Ꭴ"),
        (0xAB75, "Ꭵ"),
        (0xAB76, "Ꭶ"),
        (0xAB77, "Ꭷ"),
        (0xAB78, "Ꭸ"),
        (0xAB79, "Ꭹ"),
        (0xAB7A, "Ꭺ"),
        (0xAB7B, "Ꭻ"),
        (0xAB7C, "Ꭼ"),
        (0xAB7D, "Ꭽ"),
        (0xAB7E, "Ꭾ"),
        (0xAB7F, "Ꭿ"),
        (0xAB80, "Ꮀ"),
        (0xAB81, "Ꮁ"),
        (0xAB82, "Ꮂ"),
        (0xAB83, "Ꮃ"),
        (0xAB84, "Ꮄ"),
        (0xAB85, "Ꮅ"),
        (0xAB86, "Ꮆ"),
        (0xAB87, "Ꮇ"),
        (0xAB88, "Ꮈ"),
        (0xAB89, "Ꮉ"),
        (0xAB8A, "Ꮊ"),
        (0xAB8B, "Ꮋ"),
        (0xAB8C, "Ꮌ"),
        (0xAB8D, "Ꮍ"),
        (0xAB8E, "Ꮎ"),
        (0xAB8F, "Ꮏ"),
        (0xAB90, "Ꮐ"),
        (0xAB91, "Ꮑ"),
        (0xAB92, "Ꮒ"),
        (0xAB93, "Ꮓ"),
        (0xAB94, "Ꮔ"),
        (0xAB95, "Ꮕ"),
        (0xAB96, "Ꮖ"),
        (0xAB97, "Ꮗ"),
        (0xAB98, "Ꮘ"),
        (0xAB99, "Ꮙ"),
        (0xAB9A, "Ꮚ"),
        (0xAB9B, "Ꮛ"),
        (0xAB9C, "Ꮜ"),
        (0xAB9D, "Ꮝ"),
        (0xAB9E, "Ꮞ"),
        (0xAB9F, "Ꮟ"),
        (0xABA0, "Ꮠ"),
        (0xABA1, "Ꮡ"),
        (0xABA2, "Ꮢ"),
        (0xABA3, "Ꮣ"),
        (0xABA4, "Ꮤ"),
        (0xABA5, "Ꮥ"),
        (0xABA6, "Ꮦ"),
        (0xABA7, "Ꮧ"),
        (0xABA8, "Ꮨ"),
        (0xABA9, "Ꮩ"),
        (0xABAA, "Ꮪ"),
        (0xABAB, "Ꮫ"),
        (0xABAC, "Ꮬ"),
        (0xABAD, "Ꮭ"),
        (0xABAE, "Ꮮ"),
        (0xABAF, "Ꮯ"),
        (0xABB0, "Ꮰ"),
        (0xABB1, "Ꮱ"),
        (0xABB2, "Ꮲ"),
        (0xABB3, "Ꮳ"),
        (0xABB4, "Ꮴ"),
        (0xABB5, "Ꮵ"),
        (0xABB6, "Ꮶ"),
        (0xABB7, "Ꮷ"),
        (0xABB8, "Ꮸ"),
        (0xABB9, "Ꮹ"),
        (0xABBA, "Ꮺ"),
        (0xABBB, "Ꮻ"),
        (0xABBC, "Ꮼ"),
        (0xABBD, "Ꮽ"),
        (0xABBE, "Ꮾ"),
        (0xABBF, "Ꮿ"),
        (0xFB00, "FF"),
        (0xFB01, "FI"),
        (0xFB02, "FL"),
        (0xFB03, "FFI"),
        (0xFB04, "FFL"),
        (0xFB05, "ST"),
        (0xFB06, "ST"),
        (0xFB13, "ՄՆ"),
        (0xFB14, "ՄԵ"),
        (0xFB15, "ՄԻ"),
        (0xFB16, "ՎՆ"),
        (0xFB17, "ՄԽ"),
        (0xFF41, "Ａ"),
        (0xFF42, "Ｂ"),
        (0xFF43, "Ｃ"),
        (0xFF44, "Ｄ"),
        (0xFF45, "Ｅ"),
        (0xFF46, "Ｆ"),
        (0xFF47, "Ｇ"),
        (0xFF48, "Ｈ"),
        (0xFF49, "Ｉ"),
        (0xFF4A, "Ｊ"),
        (0xFF4B, "Ｋ"),
        (0xFF4C, "Ｌ"),
        (0xFF4D, "Ｍ"),
        (0xFF4E, "Ｎ"),
        (0xFF4F, "Ｏ"),
        (0xFF50, "Ｐ"),
        (0xFF51, "Ｑ"),
        (0xFF52, "Ｒ"),
        (0xFF53, "Ｓ"),
        (0xFF54, "Ｔ"),
        (0xFF55, "Ｕ"),
        (0xFF56, "Ｖ"),
        (0xFF57, "Ｗ"),
        (0xFF58, "Ｘ"),
        (0xFF59, "Ｙ"),
        (0xFF5A, "Ｚ"),
        (0x10428, "𐐀"),
        (0x10429, "𐐁"),
        (0x1042A, "𐐂"),
        (0x1042B, "𐐃"),
        (0x1042C, "𐐄"),
        (0x1042D, "𐐅"),
        (0x1042E, "𐐆"),
        (0x1042F, "𐐇"),
        (0x10430, "𐐈"),
        (0x10431, "𐐉"),
        (0x10432, "𐐊"),
        (0x10433, "𐐋"),
        (0x10434, "𐐌"),
        (0x10435, "𐐍"),
        (0x10436, "𐐎"),
        (0x10437, "𐐏"),
        (0x10438, "𐐐"),
        (0x10439, "𐐑"),
        (0x1043A, "𐐒"),
        (0x1043B, "𐐓"),
        (0x1043C, "𐐔"),
        (0x1043D, "𐐕"),
        (0x1043E, "𐐖"),
        (0x1043F, "𐐗"),
        (0x10440, "𐐘"),
        (0x10441, "𐐙"),
        (0x10442, "𐐚"),
        (0x10443, "𐐛"),
        (0x10444, "𐐜"),
        (0x10445, "𐐝"),
        (0x10446, "𐐞"),
        (0x10447, "𐐟"),
        (0x10448, "𐐠"),
        (0x10449, "𐐡"),
        (0x1044A, "𐐢"),
        (0x1044B, "𐐣"),
        (0x1044C, "𐐤"),
        (0x1044D, "𐐥"),
        (0x1044E, "𐐦"),
        (0x1044F, "𐐧"),
        (0x104D8, "𐒰"),
        (0x104D9, "𐒱"),
        (0x104DA, "𐒲"),
        (0x104DB, "𐒳"),
        (0x104DC, "𐒴"),
        (0x104DD, "𐒵"),
        (0x104DE, "𐒶"),
        (0x104DF, "𐒷"),
        (0x104E0, "𐒸"),
        (0x104E1, "𐒹"),
        (0x104E2, "𐒺"),
        (0x104E3, "𐒻"),
        (0x104E4, "𐒼"),
        (0x104E5, "𐒽"),
        (0x104E6, "𐒾"),
        (0x104E7, "𐒿"),
        (0x104E8, "𐓀"),
        (0x104E9, "𐓁"),
        (0x104EA, "𐓂"),
        (0x104EB, "𐓃"),
        (0x104EC, "𐓄"),
        (0x104ED, "𐓅"),
        (0x104EE, "𐓆"),
        (0x104EF, "𐓇"),
        (0x104F0, "𐓈"),
        (0x104F1, "𐓉"),
        (0x104F2, "𐓊"),
        (0x104F3, "𐓋"),
        (0x104F4, "𐓌"),
        (0x104F5, "𐓍"),
        (0x104F6, "𐓎"),
        (0x104F7, "𐓏"),
        (0x104F8, "𐓐"),
        (0x104F9, "𐓑"),
        (0x104FA, "𐓒"),
        (0x104FB, "𐓓"),
        (0x10597, "𐕰"),
        (0x10598, "𐕱"),
        (0x10599, "𐕲"),
        (0x1059A, "𐕳"),
        (0x1059B, "𐕴"),
        (0x1059C, "𐕵"),
        (0x1059D, "𐕶"),
        (0x1059E, "𐕷"),
        (0x1059F, "𐕸"),
        (0x105A0, "𐕹"),
        (0x105A1, "𐕺"),
        (0x105A3, "𐕼"),
        (0x105A4, "𐕽"),
        (0x105A5, "𐕾"),
        (0x105A6, "𐕿"),
        (0x105A7, "𐖀"),
        (0x105A8, "𐖁"),
        (0x105A9, "𐖂"),
        (0x105AA, "𐖃"),
        (0x105AB, "𐖄"),
        (0x105AC, "𐖅"),
        (0x105AD, "𐖆"),
        (0x105AE, "𐖇"),
        (0x105AF, "𐖈"),
        (0x105B0, "𐖉"),
        (0x105B1, "𐖊"),
        (0x105B3, "𐖌"),
        (0x105B4, "𐖍"),
        (0x105B5, "𐖎"),
        (0x105B6, "𐖏"),
        (0x105B7, "𐖐"),
        (0x105B8, "𐖑"),
        (0x105B9, "𐖒"),
        (0x105BB, "𐖔"),
        (0x105BC, "𐖕"),
        (0x10CC0, "𐲀"),
        (0x10CC1, "𐲁"),
        (0x10CC2, "𐲂"),
        (0x10CC3, "𐲃"),
        (0x10CC4, "𐲄"),
        (0x10CC5, "𐲅"),
        (0x10CC6, "𐲆"),
        (0x10CC7, "𐲇"),
        (0x10CC8, "𐲈"),
        (0x10CC9, "𐲉"),
        (0x10CCA, "𐲊"),
        (0x10CCB, "𐲋"),
        (0x10CCC, "𐲌"),
        (0x10CCD, "𐲍"),
        (0x10CCE, "𐲎"),
        (0x10CCF, "𐲏"),
        (0x10CD0, "𐲐"),
        (0x10CD1, "𐲑"),
        (0x10CD2, "𐲒"),
        (0x10CD3, "𐲓"),
        (0x10CD4, "𐲔"),
        (0x10CD5, "𐲕"),
        (0x10CD6, "𐲖"),
        (0x10CD7, "𐲗"),
        (0x10CD8, "𐲘"),
        (0x10CD9, "𐲙"),
        (0x10CDA, "𐲚"),
        (0x10CDB, "𐲛"),
        (0x10CDC, "𐲜"),
        (0x10CDD, "𐲝"),
        (0x10CDE, "𐲞"),
        (0x10CDF, "𐲟"),
        (0x10CE0, "𐲠"),
        (0x10CE1, "𐲡"),
        (0x10CE2, "𐲢"),
        (0x10CE3, "𐲣"),
        (0x10CE4, "𐲤"),
        (0x10CE5, "𐲥"),
        (0x10CE6, "𐲦"),
        (0x10CE7, "𐲧"),
        (0x10CE8, "𐲨"),
        (0x10CE9, "𐲩"),
        (0x10CEA, "𐲪"),
        (0x10CEB, "𐲫"),
        (0x10CEC, "𐲬"),
        (0x10CED, "𐲭"),
        (0x10CEE, "𐲮"),
        (0x10CEF, "𐲯"),
        (0x10CF0, "𐲰"),
        (0x10CF1, "𐲱"),
        (0x10CF2, "𐲲"),
        (0x118C0, "𑢠"),
        (0x118C1, "𑢡"),
        (0x118C2, "𑢢"),
        (0x118C3, "𑢣"),
        (0x118C4, "𑢤"),
        (0x118C5, "𑢥"),
        (0x118C6, "𑢦"),
        (0x118C7, "𑢧"),
        (0x118C8, "𑢨"),
        (0x118C9, "𑢩"),
        (0x118CA, "𑢪"),
        (0x118CB, "𑢫"),
        (0x118CC, "𑢬"),
        (0x118CD, "𑢭"),
        (0x118CE, "𑢮"),
        (0x118CF, "𑢯"),
        (0x118D0, "𑢰"),
        (0x118D1, "𑢱"),
        (0x118D2, "𑢲"),
        (0x118D3, "𑢳"),
        (0x118D4, "𑢴"),
        (0x118D5, "𑢵"),
        (0x118D6, "𑢶"),
        (0x118D7, "𑢷"),
        (0x118D8, "𑢸"),
        (0x118D9, "𑢹"),
        (0x118DA, "𑢺"),
        (0x118DB, "𑢻"),
        (0x118DC, "𑢼"),
        (0x118DD, "𑢽"),
        (0x118DE, "𑢾"),
        (0x118DF, "𑢿"),
        (0x16E60, "𖹀"),
        (0x16E61, "𖹁"),
        (0x16E62, "𖹂"),
        (0x16E63, "𖹃"),
        (0x16E64, "𖹄"),
        (0x16E65, "𖹅"),
        (0x16E66, "𖹆"),
        (0x16E67, "𖹇"),
        (0x16E68, "𖹈"),
        (0x16E69, "𖹉"),
        (0x16E6A, "𖹊"),
        (0x16E6B, "𖹋"),
        (0x16E6C, "𖹌"),
        (0x16E6D, "𖹍"),
        (0x16E6E, "𖹎"),
        (0x16E6F, "𖹏"),
        (0x16E70, "𖹐"),
        (0x16E71, "𖹑"),
        (0x16E72, "𖹒"),
        (0x16E73, "𖹓"),
        (0x16E74, "𖹔"),
        (0x16E75, "𖹕"),
        (0x16E76, "𖹖"),
        (0x16E77, "𖹗"),
        (0x16E78, "𖹘"),
        (0x16E79, "𖹙"),
        (0x16E7A, "𖹚"),
        (0x16E7B, "𖹛"),
        (0x16E7C, "𖹜"),
        (0x16E7D, "𖹝"),
        (0x16E7E, "𖹞"),
        (0x16E7F, "𖹟"),
        (0x1E922, "𞤀"),
        (0x1E923, "𞤁"),
        (0x1E924, "𞤂"),
        (0x1E925, "𞤃"),
        (0x1E926, "𞤄"),
        (0x1E927, "𞤅"),
        (0x1E928, "𞤆"),
        (0x1E929, "𞤇"),
        (0x1E92A, "𞤈"),
        (0x1E92B, "𞤉"),
        (0x1E92C, "𞤊"),
        (0x1E92D, "𞤋"),
        (0x1E92E, "𞤌"),
        (0x1E92F, "𞤍"),
        (0x1E930, "𞤎"),
        (0x1E931, "𞤏"),
        (0x1E932, "𞤐"),
        (0x1E933, "𞤑"),
        (0x1E934, "𞤒"),
        (0x1E935, "𞤓"),
        (0x1E936, "𞤔"),
        (0x1E937, "𞤕"),
        (0x1E938, "𞤖"),
        (0x1E939, "𞤗"),
        (0x1E93A, "𞤘"),
        (0x1E93B, "𞤙"),
        (0x1E93C, "𞤚"),
        (0x1E93D, "𞤛"),
        (0x1E93E, "𞤜"),
        (0x1E93F, "𞤝"),
        (0x1E940, "𞤞"),
        (0x1E941, "𞤟"),
        (0x1E942, "𞤠"),
        (0x1E943, "𞤡"),
    ];

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    /// `maxNesting` from the `commonmark` preset (the converter's
    /// `options_update` only flips `html`).
    const MAX_NESTING: i64 = 20;

    // -- tokens (`markdown_it/token.py`) --

    /// Attribute values: `start` is an int (`list.py`), everything else
    /// the Tiptap renderer reads is a string.
    #[derive(Debug, Clone, PartialEq)]
    enum AttrVal {
        Str(String),
        Int(i64),
    }

    impl AttrVal {
        /// Python `str(value)`.
        fn py_str(&self) -> String {
            match self {
                AttrVal::Str(text) => text.clone(),
                AttrVal::Int(num) => num.to_string(),
            }
        }
    }

    /// `Token.meta`: only the task-list flags are ever read downstream
    /// (`markdown_converter.py:341-364`); everything else (`label` via
    /// `store_labels`, `checked` via the `tasklists` option) is off.
    /// Shared by `Rc` because `_resolve_task_lists` aliases close tokens
    /// to their open token's dict *before* writing the flags.
    #[derive(Debug, Clone, Default, PartialEq)]
    struct TaskMeta {
        task_list: bool,
        task_item: bool,
        checked: bool,
    }

    /// `Token` (`token.py`): `type` renamed (`type` is a keyword), `attrs`
    /// a vec (read order-free via `attr_get`), `meta` the shared flags.
    #[derive(Debug, Clone)]
    struct Token {
        ttype: String,
        tag: String,
        nesting: i8,
        attrs: Vec<(String, AttrVal)>,
        map: Option<[usize; 2]>,
        level: i64,
        children: Option<Vec<Token>>,
        content: String,
        markup: String,
        info: String,
        meta: Rc<RefCell<TaskMeta>>,
        block: bool,
        hidden: bool,
    }

    impl Token {
        fn new(ttype: &str, tag: &str, nesting: i8) -> Token {
            Token {
                ttype: ttype.to_owned(),
                tag: tag.to_owned(),
                nesting,
                attrs: Vec::new(),
                map: None,
                level: 0,
                children: None,
                content: String::new(),
                markup: String::new(),
                info: String::new(),
                meta: Rc::new(RefCell::new(TaskMeta::default())),
                block: false,
                hidden: false,
            }
        }

        /// `attrGet`: `None` when missing (`attrs.get(name, None)`).
        fn attr_get(&self, name: &str) -> Option<AttrVal> {
            self.attrs
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }

        /// `attrSet`: replace in place, else push.
        fn attr_set(&mut self, name: &str, value: AttrVal) {
            if let Some(slot) = self.attrs.iter_mut().find(|(key, _)| key == name) {
                slot.1 = value;
            } else {
                self.attrs.push((name.to_owned(), value));
            }
        }
    }

    // -- char classes (`markdown_it/common/utils.py`) --

    /// `isSpace` / `isStrSpace`: tab or space only.
    fn is_str_space(ch: char) -> bool {
        ch == '\t' || ch == ' '
    }

    /// `isWhiteSpace`: Zs/Zl/Zp-adjacent set markdown-it hardcodes.
    fn is_white_space(code: u32) -> bool {
        if (0x2000..=0x200A).contains(&code) {
            return true;
        }
        matches!(
            code,
            0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x20 | 0xA0 | 0x1680 | 0x202F | 0x205F | 0x3000
        )
    }

    /// `isMdAsciiPunct`: the 32 ASCII punctuation codepoints.
    fn is_md_ascii_punct(code: u32) -> bool {
        matches!(
            code,
            0x21..=0x2F
                | 0x3A..=0x40
                | 0x5B..=0x60
                | 0x7B..=0x7E
        )
    }

    /// `isPunctChar`: `unicodedata.category` P/S via the generated table.
    fn is_punct_char(ch: char) -> bool {
        let code = ch as u32;
        PUNCT_RANGES
            .binary_search_by(|&(lo, hi)| {
                if code < lo {
                    std::cmp::Ordering::Greater
                } else if code > hi {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }

    /// `str.strip` / `re-\s` membership (verified identical sets).
    fn is_strip_char(ch: char) -> bool {
        super::is_py_strip_char(ch)
    }

    /// `str.strip()` with no args.
    fn py_strip(text: &str) -> &str {
        text.trim_matches(is_strip_char)
    }

    /// `str.lstrip()` with no args.
    fn py_lstrip(text: &str) -> &str {
        text.trim_start_matches(is_strip_char)
    }

    /// `str.lower()` via the full-mapping exception table.
    fn py_lower(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            match LOWER_EXC.binary_search_by_key(&(ch as u32), |&(code, _)| code) {
                Ok(idx) => out.push_str(LOWER_EXC[idx].1),
                Err(_) => out.push(ch),
            }
        }
        out
    }

    /// `str.upper()` via the full-mapping exception table.
    fn py_upper(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            match UPPER_EXC.binary_search_by_key(&(ch as u32), |&(code, _)| code) {
                Ok(idx) => out.push_str(UPPER_EXC[idx].1),
                Err(_) => out.push(ch),
            }
        }
        out
    }

    /// `normalizeReference`: strip, collapse `\s+` to one space,
    /// `.lower().upper()`.
    fn normalize_reference(text: &str) -> String {
        let mut collapsed = String::with_capacity(text.len());
        let mut in_run = false;
        for ch in py_strip(text).chars() {
            if is_strip_char(ch) {
                if !in_run {
                    collapsed.push(' ');
                    in_run = true;
                }
            } else {
                collapsed.push(ch);
                in_run = false;
            }
        }
        py_upper(&py_lower(&collapsed))
    }

    // -- entities + escaping (`common/utils.py`, `common/entities.py`) --

    /// `escapeHtml`: `&` first, then `<>\"` — never single quotes.
    fn escape_html(raw: &str) -> String {
        let mut out = String::with_capacity(raw.len());
        for ch in raw.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                _ => out.push(ch),
            }
        }
        out
    }

    /// `entities[name]`: case-sensitive binary search.
    fn entity_lookup(name: &str) -> Option<&'static str> {
        match ENTITIES.binary_search_by_key(&name, |&(key, _)| key) {
            Ok(idx) => Some(ENTITIES[idx].1),
            Err(_) => None,
        }
    }

    /// `isValidEntityCode`.
    fn is_valid_entity_code(code: u32) -> bool {
        if (0xD800..=0xDFFF).contains(&code) {
            return false;
        }
        if (0xFDD0..=0xFDEF).contains(&code) {
            return false;
        }
        if code & 0xFFFF == 0xFFFF || code & 0xFFFF == 0xFFFE {
            return false;
        }
        if code <= 0x08 {
            return false;
        }
        if code == 0x0B {
            return false;
        }
        if (0x0E..=0x1F).contains(&code) {
            return false;
        }
        if (0x7F..=0x9F).contains(&code) {
            return false;
        }
        code <= 0x10FFFF
    }

    /// `replaceEntityPattern(match, name)`: named, decimal, hex, else the
    /// match unchanged. `name` excludes the `&`/`;`.
    fn replace_entity_pattern(full_match: &str, name: &str) -> String {
        if let Some(chars) = entity_lookup(name) {
            return chars.to_owned();
        }
        let code = if let Some(digits) = name.strip_prefix('#') {
            if let Some(hex) = digits
                .strip_prefix('x')
                .or_else(|| digits.strip_prefix('X'))
            {
                if !hex.is_empty() && hex.len() <= 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    None
                }
            } else if !digits.is_empty()
                && digits.len() <= 8
                && digits.bytes().all(|b| b.is_ascii_digit())
            {
                digits.parse::<u32>().ok()
            } else {
                None
            }
        } else {
            None
        };
        match code {
            // `chr()` cannot fail on a valid code (surrogates excluded).
            Some(code) if is_valid_entity_code(code) => {
                char::from_u32(code).unwrap_or('\u{FFFD}').to_string()
            }
            _ => full_match.to_owned(),
        }
    }

    /// The `UNESCAPE_ALL_RE` backslash class: all 32 ASCII puncts.
    fn is_unescape_punct(ch: char) -> bool {
        is_md_ascii_punct(ch as u32)
    }

    /// `unescapeAll`: one left-to-right pass; `\&...;` names match
    /// `&([a-z#][a-z0-9]{1,31});` case-insensitively with greedy
    /// backtracking, then resolve case-sensitively.
    fn unescape_all(text: &str) -> String {
        if !text.contains('\\') && !text.contains('&') {
            return text.to_owned();
        }
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut pos = 0;
        while pos < chars.len() {
            let ch = chars[pos];
            if ch == '\\' && pos + 1 < chars.len() && is_unescape_punct(chars[pos + 1]) {
                out.push(chars[pos + 1]);
                pos += 2;
                continue;
            }
            if ch == '&' {
                if let Some((len, name)) = match_entity_name(&chars, pos) {
                    let full: String = chars[pos..pos + len].iter().collect();
                    out.push_str(&replace_entity_pattern(&full, &name));
                    pos += len;
                    continue;
                }
            }
            out.push(ch);
            pos += 1;
        }
        out
    }

    /// Match `&([a-z#][a-z0-9]{1,31});` (case-insensitive) at `pos`:
    /// returns the full match length in chars plus the name. Greedy with
    /// backtracking, like the regex engine.
    fn match_entity_name(chars: &[char], pos: usize) -> Option<(usize, String)> {
        let first = *chars.get(pos + 1)?;
        if !(first.is_ascii_alphabetic() || first == '#') {
            return None;
        }
        let mut end = pos + 2;
        while end < chars.len() && end < pos + 2 + 31 && chars[end].is_ascii_alphanumeric() {
            end += 1;
        }
        // Greedy: longest run first, backtrack to a single char.
        let mut len = end - (pos + 2);
        while len >= 1 {
            if chars.get(pos + 2 + len) == Some(&';') {
                let name: String = chars[pos + 1..pos + 2 + len].iter().collect();
                return Some((2 + len + 1, name));
            }
            len -= 1;
        }
        None
    }

    // -- punycode (`markdown_it/_punycode.py`, RFC 3492) --

    /// RFC 3492 `adapt`: bias adaptation. `None` on overflow (the
    /// callers' `suppress(Exception)` keeps the original hostname then).
    fn puny_adapt(mut delta: u64, num_points: u64, first_time: bool) -> Option<u64> {
        delta = if first_time { delta / 700 } else { delta / 2 };
        delta = delta.checked_add(delta / num_points)?;
        let mut k = 0u64;
        while delta > 455 {
            delta /= 35;
            k = k.checked_add(36)?;
        }
        k.checked_add(
            36u64
                .checked_mul(delta)?
                .checked_div(delta.checked_add(38)?)?,
        )
    }

    /// `encode_digit`: 0-25 → `a`-`z`, 26-35 → `0`-`9`.
    fn puny_encode_digit(digit: u32) -> char {
        char::from_u32(digit + 22 + 75 * u32::from(digit < 26)).unwrap_or('?')
    }

    /// `codecs.encode(label, "punycode")`: basic chars copied as-is
    /// (case preserved), the rest delta-coded.
    fn punycode_encode(label: &str) -> Option<String> {
        let input: Vec<u32> = label.chars().map(|ch| ch as u32).collect();
        let mut output = String::new();
        for &code in &input {
            if code < 128 {
                output.push(char::from_u32(code).unwrap_or('?'));
            }
        }
        let basic_len = output.chars().count();
        if basic_len > 0 {
            output.push('-');
        }
        let (mut n, mut delta, mut bias) = (128u64, 0u64, 72u64);
        let mut handled = basic_len as u64;
        while handled < input.len() as u64 {
            let mut m = u64::MAX;
            for &code in &input {
                let code = code as u64;
                if code >= n && code < m {
                    m = code;
                }
            }
            let extra = (m - n).checked_mul(handled + 1).filter(|_| m >= n)?;
            delta = delta.checked_add(extra)?;
            n = m;
            for &code in &input {
                let code = code as u64;
                if code < n {
                    delta = delta.checked_add(1)?;
                }
                if code == n {
                    let mut q = delta;
                    let mut k = 36u64;
                    loop {
                        let threshold = if k <= bias {
                            1
                        } else if k >= bias + 26 {
                            26
                        } else {
                            k - bias
                        };
                        if q < threshold {
                            break;
                        }
                        output.push(puny_encode_digit(
                            (threshold + (q - threshold) % (36 - threshold)) as u32,
                        ));
                        q = (q - threshold) / (36 - threshold);
                        k += 36;
                    }
                    output.push(puny_encode_digit(q as u32));
                    bias = puny_adapt(delta, handled + 1, handled == basic_len as u64)?;
                    delta = 0;
                    handled += 1;
                }
            }
            delta = delta.checked_add(1)?;
            n += 1;
        }
        Some(output)
    }

    /// `digit_decode`: `a`-`z`/`A`-`Z` → 0-25, `0`-`9` → 26-35.
    fn puny_decode_digit(ch: char) -> Option<u32> {
        if ch.is_ascii_alphabetic() {
            Some((ch.to_ascii_lowercase() as u32) - 0x61)
        } else if ch.is_ascii_digit() {
            Some((ch as u32) - 0x30 + 26)
        } else {
            None
        }
    }

    /// `codecs.decode(label, "punycode")`.
    fn punycode_decode(label: &str) -> Option<String> {
        let (basic, extended) = match label.rfind('-') {
            Some(idx) => (&label[..idx], &label[idx + 1..]),
            None => ("", label),
        };
        if !basic.is_ascii() {
            return None;
        }
        let mut output: Vec<u32> = basic.chars().map(|ch| ch as u32).collect();
        let extended: Vec<char> = extended.chars().collect();
        let (mut n, mut bias) = (128u64, 72u64);
        let mut pos = 0usize;
        let mut i = 0u64;
        while pos < extended.len() {
            let old_i = i;
            let mut w = 1u64;
            let mut k = 36u64;
            loop {
                let ch = *extended.get(pos)?;
                pos += 1;
                let digit = puny_decode_digit(ch)? as u64;
                i = i.checked_add(digit.checked_mul(w)?)?;
                let threshold = if k <= bias {
                    1
                } else if k >= bias + 26 {
                    26
                } else {
                    k - bias
                };
                if digit < threshold {
                    break;
                }
                w = w.checked_mul(36 - threshold)?;
                k += 36;
            }
            let out_len = output.len() as u64 + 1;
            bias = puny_adapt(i - old_i, out_len, old_i == 0)?;
            n = n.checked_add(i / out_len)?;
            i %= out_len;
            if n > 0x10FFFF || (0xD800..=0xDFFF).contains(&(n as u32)) {
                return None;
            }
            output.insert(i as usize, n as u32);
            i += 1;
        }
        output
            .into_iter()
            .map(char::from_u32)
            .collect::<Option<String>>()
    }

    /// `map_domain` + `to_ascii`: only the domain part (after the first
    /// `@`) is coded, and — bug-compatibly — `parts[2:]` is dropped.
    fn puny_to_ascii(host: &str) -> Option<String> {
        let parts: Vec<&str> = host.split('@').collect();
        let (mut result, rest) = if parts.len() > 1 {
            (format!("{}@", parts[0]), parts[1])
        } else {
            (String::new(), host)
        };
        let mut first = true;
        for label in rest.split(['.', '\u{3002}', '\u{FF0E}', '\u{FF61}']) {
            if !first {
                result.push('.');
            }
            first = false;
            // `REGEX_NON_ASCII`: any char outside `\0`-`\x7E`.
            if label.chars().any(|ch| ch as u32 > 0x7E) {
                result.push_str("xn--");
                result.push_str(&punycode_encode(label)?);
            } else {
                result.push_str(label);
            }
        }
        Some(result)
    }

    /// `to_unicode`: `xn--` labels decoded (lowercased first).
    fn puny_to_unicode(host: &str) -> Option<String> {
        let parts: Vec<&str> = host.split('@').collect();
        let (mut result, rest) = if parts.len() > 1 {
            (format!("{}@", parts[0]), parts[1])
        } else {
            (String::new(), host)
        };
        let mut first = true;
        for label in rest.split(['.', '\u{3002}', '\u{FF0E}', '\u{FF61}']) {
            if !first {
                result.push('.');
            }
            first = false;
            if let Some(rest) = label.strip_prefix("xn--") {
                result.push_str(&punycode_decode(&rest.to_lowercase())?);
            } else {
                result.push_str(label);
            }
        }
        Some(result)
    }

    // -- mdurl 0.1.2 (`mdurl/_parse.py`, `_format.py`, `_encode.py`, `_decode.py`) --

    const ENCODE_DEFAULT_CHARS: &str = ";/?:@&=+$,-_.!~*'()#";
    const DECODE_DEFAULT_CHARS: &str = ";/?:@&=+$,#";

    /// `mdurl.URL`.
    #[derive(Debug, Clone, Default)]
    struct MdUrl {
        protocol: Option<String>,
        slashes: bool,
        auth: Option<String>,
        port: Option<String>,
        hostname: Option<String>,
        hash: Option<String>,
        search: Option<String>,
        pathname: Option<String>,
    }

    fn is_hostless_protocol(proto: &str) -> bool {
        proto == "javascript" || proto == "javascript:"
    }

    fn is_slashed_protocol(proto: &str) -> bool {
        matches!(
            proto,
            "http"
                | "https"
                | "ftp"
                | "gopher"
                | "file"
                | "http:"
                | "https:"
                | "ftp:"
                | "gopher:"
                | "file:"
        )
    }

    /// `PROTOCOL_PATTERN`: `^([a-z0-9.+-]+:)` case-insensitive.
    fn match_protocol(rest: &str) -> Option<usize> {
        let mut len = 0;
        for ch in rest.chars() {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '+' || ch == '-' {
                len += ch.len_utf8();
            } else {
                break;
            }
        }
        if len > 0 && rest[len..].starts_with(':') {
            Some(len + 1)
        } else {
            None
        }
    }

    /// `^//[^@/]+@[^@/]+` (only consulted when `slashes_denote_host`
    /// is false — kept for shape; our callers always pass true).
    #[allow(dead_code)]
    fn match_auth_host(rest: &str) -> bool {
        let bytes = rest.as_bytes();
        if bytes.len() < 3 || bytes[0] != b'/' || bytes[1] != b'/' {
            return false;
        }
        let mut idx = 2;
        let mut pre = 0;
        while idx < bytes.len() && bytes[idx] != b'@' && bytes[idx] != b'/' {
            pre += 1;
            idx += 1;
        }
        if pre == 0 || idx >= bytes.len() || bytes[idx] != b'@' {
            return false;
        }
        idx += 1;
        let mut post = 0;
        while idx < bytes.len() && bytes[idx] != b'@' && bytes[idx] != b'/' {
            post += 1;
            idx += 1;
        }
        post > 0
    }

    /// `HOSTNAME_PART_PATTERN`: `^[+a-z0-9A-Z_-]{0,63}$`.
    fn is_hostname_part(part: &str) -> bool {
        part.chars().count() <= 63
            && part
                .chars()
                .all(|ch| ch == '+' || ch == '_' || ch == '-' || ch.is_ascii_alphanumeric())
    }

    /// `HOSTNAME_PART_START`: `^([+a-z0-9A-Z_-]{0,63})(.*)$` — greedy
    /// group 1, group 2 the rest (`(.*)$` always matches: link
    /// destinations never contain `\n`, see `parseLinkDestination`).
    fn split_hostname_part(part: &str) -> (String, String) {
        let mut end = 0;
        let mut count = 0;
        for ch in part.chars() {
            if count >= 63 {
                break;
            }
            if ch == '+' || ch == '_' || ch == '-' || ch.is_ascii_alphanumeric() {
                end += ch.len_utf8();
                count += 1;
            } else {
                break;
            }
        }
        (part[..end].to_owned(), part[end..].to_owned())
    }

    /// `PORT_PATTERN.search(host)`: a trailing `:` plus digits. `search`
    /// (not `match`) with `$` makes this the last colon when everything
    /// after it is a digit.
    fn match_port(host: &str) -> Option<usize> {
        let idx = host.rfind(':')?;
        if host[idx + 1..].bytes().all(|b| b.is_ascii_digit()) {
            Some(idx)
        } else {
            None
        }
    }

    impl MdUrl {
        /// `MutableURL.parse` with `slashes_denote_host=True` (both
        /// `normalizeLink`/`normalizeLinkText` pass it, so the
        /// `SIMPLE_PATH` fast path is dead).
        fn parse(url: &str) -> MdUrl {
            let mut parsed = MdUrl::default();
            let rest = py_strip(url);
            let mut proto = String::new();
            let mut lower_proto = String::new();
            let mut rest = rest.to_owned();
            if let Some(len) = match_protocol(&rest) {
                proto = rest[..len].to_owned();
                lower_proto = proto.to_lowercase();
                rest = rest[len..].to_owned();
            }
            // `self.protocol = proto or None` (with the colon).
            parsed.protocol = if proto.is_empty() {
                None
            } else {
                Some(proto.clone())
            };
            // `slashes_denote_host` is always true here.
            let slashes = rest.starts_with("//");
            if slashes && (proto.is_empty() || !is_hostless_protocol(&proto)) {
                rest = rest[2..].to_owned();
                parsed.slashes = true;
            }
            if !is_hostless_protocol(&proto)
                && (slashes || (!proto.is_empty() && !is_slashed_protocol(&proto)))
            {
                let mut host_end: Option<usize> = None;
                for sep in ['/', '?', '#'] {
                    if let Some(found) = rest.find(sep) {
                        host_end = Some(host_end.map_or(found, |best| best.min(found)));
                    }
                }
                // The *condition* scans the last `@` before the host
                // end, but the *split* partitions at the FIRST `@`.
                let at_sign = match host_end {
                    None => rest.rfind('@'),
                    Some(end) => rest[..end.min(rest.len())].rfind('@'),
                };
                if at_sign.is_some() {
                    if let Some(first) = rest.find('@') {
                        parsed.auth = Some(rest[..first].to_owned());
                        rest = rest[first + 1..].to_owned();
                    }
                }
                let mut host_end: Option<usize> = None;
                for sep in [
                    '%', '/', '?', ';', '#', '\'', '{', '}', '|', '\\', '^', '`', '<', '>', '"',
                    ' ', '\r', '\n', '\t',
                ] {
                    if let Some(found) = rest.find(sep) {
                        host_end = Some(host_end.map_or(found, |best| best.min(found)));
                    }
                }
                let mut host_end = host_end.unwrap_or(rest.len());
                if host_end > 0 && rest.as_bytes().get(host_end - 1) == Some(&b':') {
                    host_end -= 1;
                }
                let host = rest[..host_end].to_owned();
                rest = rest[host_end..].to_owned();
                parsed.parse_host(&host);
                if parsed.hostname.as_deref().unwrap_or("").is_empty() {
                    parsed.hostname = Some(String::new());
                }
                let hostname = parsed.hostname.clone().unwrap_or_default();
                let ipv6_hostname = hostname.starts_with('[') && hostname.ends_with(']');
                if !ipv6_hostname {
                    let hostparts: Vec<&str> = hostname.split('.').collect();
                    let mut idx = 0;
                    while idx < hostparts.len() {
                        let part = hostparts[idx];
                        if part.is_empty() {
                            idx += 1;
                            continue;
                        }
                        if !is_hostname_part(part) {
                            let newpart: String = part
                                .chars()
                                .map(|ch| if ch as u32 > 127 { 'x' } else { ch })
                                .collect();
                            if !is_hostname_part(&newpart) {
                                let mut valid_parts: Vec<String> =
                                    hostparts[..idx].iter().map(|s| (*s).to_owned()).collect();
                                let mut not_host: Vec<String> = hostparts[idx + 1..]
                                    .iter()
                                    .map(|s| (*s).to_owned())
                                    .collect();
                                let (head, tail) = split_hostname_part(part);
                                valid_parts.push(head);
                                not_host.insert(0, tail);
                                if !not_host.is_empty() {
                                    rest = format!("{}{}", not_host.join("."), rest);
                                }
                                parsed.hostname = Some(valid_parts.join("."));
                                break;
                            }
                        }
                        idx += 1;
                    }
                }
                if parsed.hostname.as_deref().unwrap_or("").chars().count() > 255 {
                    parsed.hostname = Some(String::new());
                }
                if ipv6_hostname {
                    let hostname = parsed.hostname.clone().unwrap_or_default();
                    let mut chars = hostname.chars();
                    chars.next();
                    chars.next_back();
                    parsed.hostname = Some(chars.collect());
                }
            }
            if let Some(found) = rest.find('#') {
                parsed.hash = Some(rest[found..].to_owned());
                rest = rest[..found].to_owned();
            }
            if let Some(found) = rest.find('?') {
                parsed.search = Some(rest[found..].to_owned());
                rest = rest[..found].to_owned();
            }
            if !rest.is_empty() {
                parsed.pathname = Some(rest);
            }
            if is_slashed_protocol(&lower_proto)
                && !parsed.hostname.as_deref().unwrap_or("").is_empty()
                && parsed.pathname.is_none()
            {
                parsed.pathname = Some(String::new());
            }
            parsed
        }

        fn parse_host(&mut self, host: &str) {
            let mut host = host.to_owned();
            if let Some(idx) = match_port(&host) {
                let port = host[idx..].to_owned();
                if port != ":" {
                    self.port = Some(port[1..].to_owned());
                }
                host = host[..idx].to_owned();
            }
            if !host.is_empty() {
                self.hostname = Some(host);
            }
        }

        /// `mdurl.format`.
        fn format(&self) -> String {
            let mut result = String::new();
            result.push_str(self.protocol.as_deref().unwrap_or(""));
            if self.slashes {
                result.push_str("//");
            }
            if let Some(auth) = self.auth.as_deref() {
                if !auth.is_empty() {
                    result.push_str(auth);
                    result.push('@');
                }
            }
            match self.hostname.as_deref() {
                Some(host) if host.contains(':') => {
                    result.push('[');
                    result.push_str(host);
                    result.push(']');
                }
                Some(host) => result.push_str(host),
                None => {}
            }
            if let Some(port) = self.port.as_deref() {
                if !port.is_empty() {
                    result.push(':');
                    result.push_str(port);
                }
            }
            result.push_str(self.pathname.as_deref().unwrap_or(""));
            result.push_str(self.search.as_deref().unwrap_or(""));
            result.push_str(self.hash.as_deref().unwrap_or(""));
            result
        }
    }

    /// `mdurl.encode` (`ENCODE_DEFAULT_CHARS`, `keep_escaped=True`):
    /// alphanumerics plus the exclude set pass through, valid `%XX`
    /// runs are kept (`i + 2 < l`, strict), the rest is UTF-8
    /// percent-encoded with uppercase hex. The surrogate-half branch
    /// is unreachable: Rust `char`s are never surrogates (and serde
    /// rejects lone surrogates before we ever see them).
    fn mdurl_encode(text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut idx = 0;
        while idx < chars.len() {
            let ch = chars[idx];
            if ch == '%' && idx + 2 < chars.len() {
                let a = chars[idx + 1];
                let b = chars[idx + 2];
                if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() {
                    out.push('%');
                    out.push(a);
                    out.push(b);
                    idx += 3;
                    continue;
                }
            }
            if (ch as u32) < 128 {
                if ch.is_ascii_alphanumeric() || ENCODE_DEFAULT_CHARS.contains(ch) {
                    out.push(ch);
                } else {
                    out.push_str(&format!("%{:02X}", ch as u32));
                }
                idx += 1;
                continue;
            }
            let mut buf = [0u8; 4];
            for byte in ch.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
            idx += 1;
        }
        out
    }

    /// One `(%[a-f0-9]{2})+` run decoded (`repl_func_with_cache`):
    /// strict-UTF-8 like Python's `bytes.decode()` (overlongs rejected —
    /// `str::from_utf8` agrees), excluded bytes re-encoded uppercase.
    fn mdurl_decode_run(run: &str, exclude: &str) -> String {
        let bytes: Vec<u8> = (0..run.len() / 3)
            .map(|n| u8::from_str_radix(&run[n * 3 + 1..n * 3 + 3], 16).unwrap_or(0))
            .collect();
        let mut out = String::new();
        let mut idx = 0;
        while idx < bytes.len() {
            let b1 = bytes[idx];
            if b1 < 0x80 {
                let ch = b1 as char;
                if exclude.contains(ch) {
                    out.push_str(&format!("%{b1:02X}"));
                } else {
                    out.push(ch);
                }
                idx += 1;
                continue;
            }
            // 2-, 3-, 4-byte sequences, with the same bounds the
            // `(i + 3 < l)` / `(i + 6 < l)` / `(i + 9 < l)` checks give
            // in units of `%XX` groups.
            let mut done = false;
            for width in [2usize, 3, 4] {
                if bytes.len() - idx < width {
                    continue;
                }
                let head_ok = match width {
                    2 => b1 & 0xE0 == 0xC0,
                    3 => b1 & 0xF0 == 0xE0,
                    _ => b1 & 0xF8 == 0xF0,
                };
                if !head_ok {
                    continue;
                }
                if bytes[idx + 1..idx + width].iter().all(|b| b & 0xC0 == 0x80) {
                    match std::str::from_utf8(&bytes[idx..idx + width]) {
                        Ok(valid) => out.push_str(valid),
                        Err(_) => {
                            for _ in 0..width {
                                out.push('\u{FFFD}');
                            }
                        }
                    }
                    idx += width;
                    done = true;
                    break;
                }
            }
            if !done {
                // A head byte that fails every width falls through to
                // the next width's check in Python; only when all fail
                // (or the continuation check fails) is U+FFFD emitted.
                // Re-check: Python tries 2, then 3, then 4, and emits
                // U+FFFD only if none matched — exactly this loop.
                out.push('\u{FFFD}');
                idx += 1;
            }
        }
        out
    }

    /// `mdurl.decode`: maximal `%XX` runs (case-insensitive hex),
    /// everything else literal.
    fn mdurl_decode(text: &str, exclude: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut idx = 0;
        while idx < chars.len() {
            if chars[idx] == '%'
                && idx + 2 < chars.len() + 1
                && chars.get(idx + 1).is_some_and(|ch| ch.is_ascii_hexdigit())
                && chars.get(idx + 2).is_some_and(|ch| ch.is_ascii_hexdigit())
            {
                let start = idx;
                while chars.get(idx) == Some(&'%')
                    && chars.get(idx + 1).is_some_and(|ch| ch.is_ascii_hexdigit())
                    && chars.get(idx + 2).is_some_and(|ch| ch.is_ascii_hexdigit())
                {
                    idx += 3;
                }
                let run: String = chars[start..idx].iter().collect();
                out.push_str(&mdurl_decode_run(&run, exclude));
                continue;
            }
            out.push(chars[idx]);
            idx += 1;
        }
        out
    }

    // -- link normalization (`common/normalize_url.py`) --

    /// `normalizeLink`: parse, punycode the host for the recode set
    /// (failures suppressed — the original hostname is kept), encode.
    fn normalize_link(url: &str) -> String {
        let mut parsed = MdUrl::parse(url);
        let recode = parsed.protocol.is_none()
            || matches!(
                parsed.protocol.as_deref().unwrap_or(""),
                "http:" | "https:" | "mailto:"
            );
        if !parsed.hostname.as_deref().unwrap_or("").is_empty() && recode {
            let hostname = parsed.hostname.clone().unwrap_or_default();
            if let Some(ascii) = puny_to_ascii(&hostname) {
                parsed.hostname = Some(ascii);
            }
        }
        mdurl_encode(&parsed.format())
    }

    /// `normalizeLinkText`: parse, unicode the host (failures
    /// suppressed), decode with `DECODE_DEFAULT_CHARS + "%"`.
    fn normalize_link_text(url: &str) -> String {
        let mut parsed = MdUrl::parse(url);
        let recode = parsed.protocol.is_none()
            || matches!(
                parsed.protocol.as_deref().unwrap_or(""),
                "http:" | "https:" | "mailto:"
            );
        if !parsed.hostname.as_deref().unwrap_or("").is_empty() && recode {
            let hostname = parsed.hostname.clone().unwrap_or_default();
            if let Some(unicode) = puny_to_unicode(&hostname) {
                parsed.hostname = Some(unicode);
            }
        }
        mdurl_decode(&parsed.format(), &format!("{DECODE_DEFAULT_CHARS}%"))
    }

    /// `validateLink`: `BAD_PROTO_RE` fails unless `GOOD_DATA_RE` saves it.
    fn validate_link(url: &str) -> bool {
        let lowered = py_lower(py_strip(url));
        let bad = ["vbscript:", "javascript:", "file:", "data:"]
            .iter()
            .any(|prefix| lowered.starts_with(prefix));
        if !bad {
            return true;
        }
        // `GOOD_DATA_RE`: `^data:image\/(gif|png|jpeg|webp);`.
        ["gif", "png", "jpeg", "webp"].iter().any(|kind| {
            lowered.starts_with("data:")
                && lowered["data:".len()..].starts_with(&format!("image/{kind};"))
        })
    }

    // -- block state (`rules_block/state_block.py`) --

    /// A link reference (`reference.py`): `href` + `title` (`map` is
    /// never read downstream).
    #[derive(Debug, Clone)]
    struct Ref {
        href: String,
        title: String,
    }

    /// `StateBlock`: `src` as chars; every offset is a char index.
    struct StateBlock {
        src: Vec<char>,
        tokens: Vec<Token>,
        references: Option<HashMap<String, Ref>>,
        b_marks: Vec<usize>,
        e_marks: Vec<usize>,
        t_shift: Vec<usize>,
        s_count: Vec<i64>,
        bs_count: Vec<i64>,
        blk_indent: i64,
        line: usize,
        line_max: usize,
        tight: bool,
        list_indent: i64,
        parent_type: String,
        level: i64,
    }

    impl StateBlock {
        fn new(src: &str) -> StateBlock {
            let chars: Vec<char> = src.chars().collect();
            let length = chars.len();
            let mut state = StateBlock {
                src: chars,
                tokens: Vec::new(),
                references: None,
                b_marks: Vec::new(),
                e_marks: Vec::new(),
                t_shift: Vec::new(),
                s_count: Vec::new(),
                bs_count: Vec::new(),
                blk_indent: 0,
                line: 0,
                line_max: 0,
                tight: false,
                list_indent: -1,
                parent_type: "root".to_owned(),
                level: 0,
            };
            let (mut start, mut indent, mut offset) = (0usize, 0usize, 0i64);
            let mut indent_found = false;
            // `for pos, character in enumerate(self.src)` — char indices.
            for pos in 0..length {
                let character = state.src[pos];
                if !indent_found {
                    if is_str_space(character) {
                        indent += 1;
                        if character == '\t' {
                            offset += 4 - offset % 4;
                        } else {
                            offset += 1;
                        }
                        continue;
                    } else {
                        indent_found = true;
                    }
                }
                if character == '\n' || pos == length - 1 {
                    let end = if character == '\n' { pos } else { pos + 1 };
                    state.b_marks.push(start);
                    state.e_marks.push(end);
                    state.t_shift.push(indent);
                    state.s_count.push(offset);
                    state.bs_count.push(0);
                    indent_found = false;
                    indent = 0;
                    offset = 0;
                    start = end + 1;
                }
            }
            // Empty sources never reach here (`ParserBlock.parse`
            // returns early); the fake entry simplifies bound checks.
            state.b_marks.push(length);
            state.e_marks.push(length);
            state.t_shift.push(0);
            state.s_count.push(0);
            state.bs_count.push(0);
            state.line_max = state.b_marks.len() - 1;
            state
        }

        fn at(&self, pos: usize) -> Option<char> {
            self.src.get(pos).copied()
        }

        /// `push`: block token; closing pops the level first.
        fn push(&mut self, ttype: &str, tag: &str, nesting: i8) -> usize {
            let mut token = Token::new(ttype, tag, nesting);
            token.block = true;
            if nesting < 0 {
                self.level -= 1;
            }
            token.level = self.level;
            if nesting > 0 {
                self.level += 1;
            }
            self.tokens.push(token);
            self.tokens.len() - 1
        }

        fn is_empty(&self, line: usize) -> bool {
            self.b_marks[line] + self.t_shift[line] >= self.e_marks[line]
        }

        fn skip_empty_lines(&self, mut from: usize) -> usize {
            while from < self.line_max {
                if self.b_marks.get(from).is_some_and(|b| {
                    b + self.t_shift.get(from).copied().unwrap_or(0)
                        < self.e_marks.get(from).copied().unwrap_or(0)
                }) {
                    break;
                }
                from += 1;
            }
            from
        }

        fn skip_spaces(&self, mut pos: usize) -> usize {
            while self.at(pos).is_some_and(is_str_space) {
                pos += 1;
            }
            pos
        }

        fn skip_spaces_back(&self, mut pos: usize, minimum: usize) -> usize {
            if pos <= minimum {
                return pos;
            }
            while pos > minimum {
                pos -= 1;
                if !self.at(pos).is_some_and(is_str_space) {
                    return pos + 1;
                }
            }
            pos
        }

        fn skip_chars_str(&self, mut pos: usize, ch: char) -> usize {
            while self.at(pos) == Some(ch) {
                pos += 1;
            }
            pos
        }

        fn skip_chars_str_back(&self, mut pos: usize, ch: char, minimum: usize) -> usize {
            if pos <= minimum {
                return pos;
            }
            while pos > minimum {
                pos -= 1;
                if self.at(pos) != Some(ch) {
                    return pos + 1;
                }
            }
            pos
        }

        /// `getLines`: cut `[begin, end)`, dedent, keep the last LF only
        /// when asked (or mid-range).
        fn get_lines(&self, begin: usize, end: usize, indent: i64, keep_last_lf: bool) -> String {
            if begin >= end {
                return String::new();
            }
            let mut queue = String::new();
            let mut line = begin;
            while line < end {
                let mut line_indent = 0i64;
                let line_start = self.b_marks[line];
                let mut first = line_start;
                let last = if line + 1 < end || keep_last_lf {
                    self.e_marks[line] + 1
                } else {
                    self.e_marks[line]
                };
                // Unreachable out-of-range reads (empty lines never reach
                // the rules; the fence scan excludes its danger line) are
                // clamped rather than panicking, mirroring the slice below.
                while first < last && line_indent < indent {
                    let ch = self.at(first);
                    match ch {
                        Some(ch) if is_str_space(ch) => {
                            if ch == '\t' {
                                line_indent += 4 - (line_indent + self.bs_count[line]) % 4;
                            } else {
                                line_indent += 1;
                            }
                        }
                        _ if first - line_start < self.t_shift[line] => {
                            line_indent += 1;
                        }
                        _ => break,
                    }
                    first += 1;
                    if first > self.src.len() {
                        break;
                    }
                }
                let from = first.min(self.src.len());
                let to = last.min(self.src.len()).max(from);
                if line_indent > indent {
                    for _ in 0..line_indent - indent {
                        queue.push(' ');
                    }
                }
                queue.extend(self.src[from..to].iter());
                line += 1;
            }
            queue
        }

        /// `is_code_block`: the `code` rule is enabled, 4+ indent.
        fn is_code_block(&self, line: usize) -> bool {
            self.s_count[line] - self.blk_indent >= 4
        }
    }

    // -- block rules --

    type BlockRule = fn(&mut StateBlock, usize, usize, bool) -> bool;

    /// `html_block` with `html=False`: always false, terminator or not.
    fn html_block(_state: &mut StateBlock, _start: usize, _end: usize, _silent: bool) -> bool {
        false
    }

    // `table.py` --

    fn table_get_line(state: &StateBlock, line: usize) -> String {
        // Python slicing clamps (empty lines yield ""); Rust panics.
        let start = state.b_marks[line] + state.t_shift[line];
        let end = state.e_marks[line];
        if start >= end {
            return String::new();
        }
        state.src[start..end.min(state.src.len())].iter().collect()
    }

    /// `escapedSplit`: a pipe preceded immediately by `\` is literal
    /// (single-char `isEscaped` memory — `\\|` never splits either).
    fn escaped_split(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut result = Vec::new();
        let mut current = String::new();
        let mut last_pos = 0;
        let mut is_escaped = false;
        let mut pos = 0;
        while pos < chars.len() {
            let ch = chars[pos];
            if ch == '|' {
                if !is_escaped {
                    current.push_str(&chars[last_pos..pos].iter().collect::<String>());
                    result.push(std::mem::take(&mut current));
                    last_pos = pos + 1;
                } else {
                    current.push_str(&chars[last_pos..pos - 1].iter().collect::<String>());
                    last_pos = pos;
                }
            }
            is_escaped = ch == '\\';
            pos += 1;
        }
        current.push_str(&chars[last_pos..].iter().collect::<String>());
        result.push(current);
        result
    }

    /// `headerLineRe`: `^:?-+:?$`.
    fn is_header_cell(text: &str) -> bool {
        let mut chars = text.chars().peekable();
        if chars.peek() == Some(&':') {
            chars.next();
        }
        let mut dashes = 0;
        while chars.peek() == Some(&'-') {
            chars.next();
            dashes += 1;
        }
        if dashes == 0 {
            return false;
        }
        if chars.peek() == Some(&':') {
            chars.next();
        }
        chars.next().is_none()
    }

    /// GFM table (`rules_block/table.py`).
    fn table(state: &mut StateBlock, start_line: usize, end_line: usize, silent: bool) -> bool {
        if start_line + 2 > end_line {
            return false;
        }
        let next_line = start_line + 1;
        if state.s_count[next_line] < state.blk_indent {
            return false;
        }
        if state.is_code_block(next_line) {
            return false;
        }
        let pos = state.b_marks[next_line] + state.t_shift[next_line];
        if pos >= state.e_marks[next_line] {
            return false;
        }
        let first_ch = state.src[pos];
        if !matches!(first_ch, '|' | '-' | ':') {
            return false;
        }
        if pos + 1 >= state.e_marks[next_line] {
            return false;
        }
        let second_ch = state.src[pos + 1];
        if !matches!(second_ch, '|' | '-' | ':') && !is_str_space(second_ch) {
            return false;
        }
        if first_ch == '-' && is_str_space(second_ch) {
            return false;
        }
        let mut pos = pos + 2;
        while pos < state.e_marks[next_line] {
            let ch = state.src[pos];
            if !matches!(ch, '|' | '-' | ':') && !is_str_space(ch) {
                return false;
            }
            pos += 1;
        }
        let line_text = table_get_line(state, start_line + 1);
        let columns: Vec<&str> = line_text.split('|').collect();
        let mut aligns: Vec<&str> = Vec::new();
        for (idx, column) in columns.iter().enumerate() {
            let trimmed = column.trim();
            if trimmed.is_empty() {
                if idx == 0 || idx == columns.len() - 1 {
                    continue;
                } else {
                    return false;
                }
            }
            if !is_header_cell(trimmed) {
                return false;
            }
            let mut chars = trimmed.chars();
            let first = chars.next();
            let last = trimmed.chars().next_back();
            if last == Some(':') {
                aligns.push(if first == Some(':') {
                    "center"
                } else {
                    "right"
                });
            } else if first == Some(':') {
                aligns.push("left");
            } else {
                aligns.push("");
            }
        }
        // `str.strip()` here — Python-strip, not trim — matters for
        // `\x1c`-style padding; `getLine(...).strip()`.
        let raw_line = table_get_line(state, start_line);
        let line_text = py_strip(&raw_line);
        if !line_text.contains('|') {
            return false;
        }
        if state.is_code_block(start_line) {
            return false;
        }
        let mut columns = escaped_split(line_text);
        if columns.first().is_some_and(|cell| cell.is_empty()) {
            columns.remove(0);
        }
        if columns.last().is_some_and(|cell| cell.is_empty()) {
            columns.pop();
        }
        let column_count = columns.len();
        if column_count == 0 || column_count != aligns.len() {
            return false;
        }
        if silent {
            return true;
        }
        let old_parent = std::mem::replace(&mut state.parent_type, "table".to_owned());
        let token = state.push("table_open", "table", 1);
        state.tokens[token].map = Some([start_line, 0]);
        let token = state.push("thead_open", "thead", 1);
        state.tokens[token].map = Some([start_line, start_line + 1]);
        let token = state.push("tr_open", "tr", 1);
        state.tokens[token].map = Some([start_line, start_line + 1]);
        for (idx, column) in columns.iter().enumerate() {
            let token = state.push("th_open", "th", 1);
            if !aligns[idx].is_empty() {
                state.tokens[token].attrs = vec![(
                    "style".to_owned(),
                    AttrVal::Str(format!("text-align:{}", aligns[idx])),
                )];
            }
            let token = state.push("inline", "", 0);
            state.tokens[token].map = Some([start_line, start_line + 1]);
            state.tokens[token].content = column.trim().to_owned();
            state.tokens[token].children = Some(Vec::new());
            state.push("th_close", "th", -1);
        }
        state.push("tr_close", "tr", -1);
        state.push("thead_close", "thead", -1);
        let mut autocompleted_cells: i64 = 0;
        let mut next_line = start_line + 2;
        let mut tbody_idx: Option<usize> = None;
        while next_line < end_line {
            if state.s_count[next_line] < state.blk_indent {
                break;
            }
            let mut terminate = false;
            for rule in blockquote_chain() {
                if rule(state, next_line, end_line, true) {
                    terminate = true;
                    break;
                }
            }
            if terminate {
                break;
            }
            let raw_line = table_get_line(state, next_line);
            let line_text = py_strip(&raw_line);
            if line_text.is_empty() {
                break;
            }
            if state.is_code_block(next_line) {
                break;
            }
            let mut columns = escaped_split(line_text);
            if columns.first().is_some_and(|cell| cell.is_empty()) {
                columns.remove(0);
            }
            if columns.last().is_some_and(|cell| cell.is_empty()) {
                columns.pop();
            }
            autocompleted_cells += column_count as i64 - columns.len() as i64;
            if autocompleted_cells > 0x10000 {
                break;
            }
            if next_line == start_line + 2 {
                let token = state.push("tbody_open", "tbody", 1);
                state.tokens[token].map = Some([start_line + 2, 0]);
                tbody_idx = Some(token);
            }
            let token = state.push("tr_open", "tr", 1);
            state.tokens[token].map = Some([next_line, next_line + 1]);
            for (idx, align) in aligns.iter().enumerate() {
                let token = state.push("td_open", "td", 1);
                if !align.is_empty() {
                    state.tokens[token].attrs = vec![(
                        "style".to_owned(),
                        AttrVal::Str(format!("text-align:{align}")),
                    )];
                }
                let token = state.push("inline", "", 0);
                state.tokens[token].map = Some([next_line, next_line + 1]);
                state.tokens[token].content = columns
                    .get(idx)
                    .map(|cell| {
                        if cell.is_empty() {
                            String::new()
                        } else {
                            cell.trim().to_owned()
                        }
                    })
                    .unwrap_or_default();
                state.tokens[token].children = Some(Vec::new());
                state.push("td_close", "td", -1);
            }
            state.push("tr_close", "tr", -1);
            next_line += 1;
        }
        if let Some(idx) = tbody_idx {
            state.push("tbody_close", "tbody", -1);
            state.tokens[idx].map = Some([start_line + 2, next_line]);
        }
        state.push("table_close", "table", -1);
        // `tableLines[1] = nextLine`: the table_open token is 6 pushes
        // back... resolved by index instead: find it.
        let open = state
            .tokens
            .iter()
            .rposition(|token| token.ttype == "table_open")
            .unwrap_or(0);
        state.tokens[open].map = Some([start_line, next_line]);
        state.parent_type = old_parent;
        state.line = next_line;
        true
    }

    // `code.py` --

    /// Indented code (`rules_block/code.py`).
    fn code(state: &mut StateBlock, start_line: usize, end_line: usize, _silent: bool) -> bool {
        if !state.is_code_block(start_line) {
            return false;
        }
        let mut last = start_line + 1;
        let mut next_line = start_line + 1;
        while next_line < end_line {
            if state.is_empty(next_line) {
                next_line += 1;
                continue;
            }
            if state.is_code_block(next_line) {
                next_line += 1;
                last = next_line;
                continue;
            }
            break;
        }
        state.line = last;
        let token = state.push("code_block", "code", 0);
        state.tokens[token].content = format!(
            "{}\n",
            state.get_lines(start_line, last, 4 + state.blk_indent, false)
        );
        state.tokens[token].map = Some([start_line, state.line]);
        true
    }

    // `fence.py` (`make_fence_rule` defaults: `~`/backtick, `fence`,
    // at-least matching, no backtick in backtick-info, min 3) --

    /// Fenced code (`rules_block/fence.py`).
    fn fence(state: &mut StateBlock, start_line: usize, end_line: usize, silent: bool) -> bool {
        let mut have_end_marker = false;
        let pos = state.b_marks[start_line] + state.t_shift[start_line];
        let maximum = state.e_marks[start_line];
        if state.is_code_block(start_line) {
            return false;
        }
        if pos + 3 > maximum {
            return false;
        }
        let marker = match state.at(pos) {
            Some(ch) if ch == '~' || ch == '`' => ch,
            _ => return false,
        };
        let mem = pos;
        let pos = state.skip_chars_str(pos, marker);
        let length = pos - mem;
        if length < 3 {
            return false;
        }
        let markup: String = state.src[mem..pos].iter().collect();
        let params: String = state.src[pos..maximum].iter().collect();
        if marker == '`' && params.contains('`') {
            return false;
        }
        if silent {
            return true;
        }
        let mut next_line = start_line;
        loop {
            next_line += 1;
            if next_line >= end_line {
                break;
            }
            let scan = state.b_marks[next_line] + state.t_shift[next_line];
            let maximum = state.e_marks[next_line];
            if scan < maximum && state.s_count[next_line] < state.blk_indent {
                break;
            }
            if state.at(scan) != Some(marker) {
                // `try: if src[pos] != marker: continue; except: break` —
                // an out-of-range read breaks, a mismatch continues.
                if state.at(scan).is_none() {
                    break;
                }
                continue;
            }
            if state.is_code_block(next_line) {
                continue;
            }
            let mem = scan;
            let mut pos = state.skip_chars_str(scan, marker);
            if pos - mem < length {
                continue;
            }
            pos = state.skip_spaces(pos);
            if pos < maximum {
                continue;
            }
            have_end_marker = true;
            break;
        }
        let length = state.s_count[start_line];
        state.line = next_line + usize::from(have_end_marker);
        let token = state.push("fence", "code", 0);
        state.tokens[token].info = params;
        state.tokens[token].content = state.get_lines(start_line + 1, next_line, length, true);
        state.tokens[token].markup = markup;
        state.tokens[token].map = Some([start_line, state.line]);
        true
    }

    // `hr.py` --

    /// Horizontal rule (`rules_block/hr.py`); note the `cnt + 1` markup quirk.
    fn hr(state: &mut StateBlock, start_line: usize, _end: usize, silent: bool) -> bool {
        let mut pos = state.b_marks[start_line] + state.t_shift[start_line];
        let maximum = state.e_marks[start_line];
        if state.is_code_block(start_line) {
            return false;
        }
        let marker = match state.at(pos) {
            Some(ch) => ch,
            None => return false,
        };
        pos += 1;
        if !matches!(marker, '*' | '-' | '_') {
            return false;
        }
        let mut cnt = 1;
        while pos < maximum {
            let ch = state.src[pos];
            pos += 1;
            if ch != marker && !is_str_space(ch) {
                return false;
            }
            if ch == marker {
                cnt += 1;
            }
        }
        if cnt < 3 {
            return false;
        }
        if silent {
            return true;
        }
        state.line = start_line + 1;
        let token = state.push("hr", "hr", 0);
        state.tokens[token].map = Some([start_line, state.line]);
        state.tokens[token].markup = marker.to_string().repeat(cnt + 1);
        true
    }

    // `heading.py` --

    /// ATX heading (`rules_block/heading.py`).
    fn heading(state: &mut StateBlock, start_line: usize, _end: usize, silent: bool) -> bool {
        let mut pos = state.b_marks[start_line] + state.t_shift[start_line];
        let mut maximum = state.e_marks[start_line];
        if state.is_code_block(start_line) {
            return false;
        }
        // Non-empty lines only reach the rules, so `pos` is in range.
        let mut ch = state.at(pos);
        if ch != Some('#') || pos >= maximum {
            return false;
        }
        let mut level = 1;
        pos += 1;
        ch = state.at(pos);
        while ch == Some('#') && pos < maximum && level <= 6 {
            level += 1;
            pos += 1;
            ch = state.at(pos);
        }
        if level > 6 || (pos < maximum && !ch.is_some_and(is_str_space)) {
            return false;
        }
        if silent {
            return true;
        }
        maximum = state.skip_spaces_back(maximum, pos);
        let tmp = state.skip_chars_str_back(maximum, '#', pos);
        if tmp > pos && state.at(tmp - 1).is_some_and(is_str_space) {
            maximum = tmp;
        }
        state.line = start_line + 1;
        let token = state.push("heading_open", &format!("h{level}"), 1);
        state.tokens[token].markup = "########"[..level].to_owned();
        state.tokens[token].map = Some([start_line, state.line]);
        let token = state.push("inline", "", 0);
        state.tokens[token].content = state.src[pos..maximum]
            .iter()
            .collect::<String>()
            .trim()
            .to_owned();
        state.tokens[token].map = Some([start_line, state.line]);
        state.tokens[token].children = Some(Vec::new());
        let token = state.push("heading_close", &format!("h{level}"), -1);
        state.tokens[token].markup = "########"[..level].to_owned();
        true
    }

    // `paragraph.py`, `lheading.py` --

    /// Paragraph (`rules_block/paragraph.py`): note `endLine =
    /// state.lineMax` — the passed end is ignored.
    fn paragraph(state: &mut StateBlock, start_line: usize, _end: usize, _silent: bool) -> bool {
        let mut next_line = start_line + 1;
        let end_line = state.line_max;
        let old_parent = std::mem::replace(&mut state.parent_type, "paragraph".to_owned());
        while next_line < end_line {
            if state.is_empty(next_line) {
                break;
            }
            if state.s_count[next_line] - state.blk_indent > 3 {
                next_line += 1;
                continue;
            }
            if state.s_count[next_line] < 0 {
                next_line += 1;
                continue;
            }
            let mut terminate = false;
            for rule in paragraph_chain() {
                if rule(state, next_line, end_line, true) {
                    terminate = true;
                    break;
                }
            }
            if terminate {
                break;
            }
            next_line += 1;
        }
        let content = state
            .get_lines(start_line, next_line, state.blk_indent, false)
            .trim()
            .to_owned();
        state.line = next_line;
        let token = state.push("paragraph_open", "p", 1);
        state.tokens[token].map = Some([start_line, state.line]);
        let token = state.push("inline", "", 0);
        state.tokens[token].content = content;
        state.tokens[token].map = Some([start_line, state.line]);
        state.tokens[token].children = Some(Vec::new());
        state.push("paragraph_close", "p", -1);
        state.parent_type = old_parent;
        true
    }

    /// Setext heading (`rules_block/lheading.py`).
    fn lheading(state: &mut StateBlock, start_line: usize, end_line: usize, _silent: bool) -> bool {
        let mut level = None;
        let mut marker = '\0';
        let mut next_line = start_line + 1;
        if state.is_code_block(start_line) {
            return false;
        }
        let old_parent = std::mem::replace(&mut state.parent_type, "paragraph".to_owned());
        while next_line < end_line && !state.is_empty(next_line) {
            if state.s_count[next_line] - state.blk_indent > 3 {
                next_line += 1;
                continue;
            }
            if state.s_count[next_line] >= state.blk_indent {
                let pos = state.b_marks[next_line] + state.t_shift[next_line];
                let maximum = state.e_marks[next_line];
                if pos < maximum {
                    let found = state.src[pos];
                    if found == '-' || found == '=' {
                        let mut pos = state.skip_chars_str(pos, found);
                        pos = state.skip_spaces(pos);
                        if pos >= maximum {
                            marker = found;
                            level = Some(if found == '=' { 1 } else { 2 });
                            break;
                        }
                    }
                }
            }
            if state.s_count[next_line] < 0 {
                next_line += 1;
                continue;
            }
            let mut terminate = false;
            for rule in paragraph_chain() {
                if rule(state, next_line, end_line, true) {
                    terminate = true;
                    break;
                }
            }
            if terminate {
                break;
            }
            next_line += 1;
        }
        let Some(level) = level else {
            // Python returns WITHOUT restoring `parentType` here.
            return false;
        };
        let content = state
            .get_lines(start_line, next_line, state.blk_indent, false)
            .trim()
            .to_owned();
        state.line = next_line + 1;
        let token = state.push("heading_open", &format!("h{level}"), 1);
        state.tokens[token].markup = marker.to_string();
        state.tokens[token].map = Some([start_line, state.line]);
        let token = state.push("inline", "", 0);
        state.tokens[token].content = content;
        state.tokens[token].map = Some([start_line, state.line - 1]);
        state.tokens[token].children = Some(Vec::new());
        let token = state.push("heading_close", &format!("h{level}"), -1);
        state.tokens[token].markup = marker.to_string();
        state.parent_type = old_parent;
        true
    }

    // -- rule chains (`parser_block.py:_rules` alt lists, registration order) --

    /// The main chain: table, code, fence, blockquote, hr, list,
    /// reference, html_block (dead `false`), heading, lheading, paragraph.
    fn block_chain() -> [BlockRule; 11] {
        [
            table, code, fence, blockquote, hr, list_block, reference, html_block, heading,
            lheading, paragraph,
        ]
    }

    /// `paragraph` / `reference` chains (same alt membership).
    fn paragraph_chain() -> [BlockRule; 7] {
        [
            table, fence, blockquote, hr, list_block, html_block, heading,
        ]
    }

    /// `reference` chain: same members as `paragraph`.
    fn reference_chain() -> [BlockRule; 7] {
        [
            table, fence, blockquote, hr, list_block, html_block, heading,
        ]
    }

    /// `blockquote` chain.
    fn blockquote_chain() -> [BlockRule; 6] {
        [fence, blockquote, hr, list_block, html_block, heading]
    }

    /// `list` chain.
    fn list_chain() -> [BlockRule; 3] {
        [fence, blockquote, hr]
    }

    /// `ParserBlock.tokenize`.
    fn block_tokenize(state: &mut StateBlock, start_line: usize, end_line: usize) {
        let mut line = start_line;
        let mut has_empty_lines = false;
        while line < end_line {
            line = state.skip_empty_lines(line);
            state.line = line;
            if line >= end_line {
                break;
            }
            if state.s_count[line] < state.blk_indent {
                break;
            }
            if state.level >= MAX_NESTING {
                state.line = end_line;
                break;
            }
            for rule in block_chain() {
                if rule(state, line, end_line, false) {
                    break;
                }
            }
            state.tight = !has_empty_lines;
            line = state.line;
            if line >= 1 && line - 1 < end_line && state.is_empty(line - 1) {
                has_empty_lines = true;
            }
            if line < end_line && state.is_empty(line) {
                has_empty_lines = true;
                line += 1;
                state.line = line;
            }
        }
    }

    // `blockquote.py` (the `alerts` option is off — plain quotes only) --

    /// One `>`-prefixed line's cache surgery (`blockquote.py`): strip
    /// the marker plus one optional space, push the old caches, install
    /// the new ones. The first line and the scan share this (the two
    /// copies in `blockquote.py` are identical). Returns `lastLineEmpty`.
    /// `pos` arrives past the `>`.
    fn blockquote_adjust(
        state: &mut StateBlock,
        line: usize,
        mut pos: usize,
        olds: &mut BlockquoteOlds,
    ) -> bool {
        let max = state.e_marks[line];
        let mut initial = state.s_count[line] + 1;
        let mut offset = initial;
        // `adjustTab` is unbound in Python's `else` arm, but then the
        // first char is non-space and the loop below breaks before
        // reading it — `false` is equivalent.
        let (space_after_marker, adjust_tab) = match state.at(pos) {
            Some(' ') => {
                pos += 1;
                initial += 1;
                offset += 1;
                (true, false)
            }
            Some('\t') => {
                if (state.bs_count[line] + offset) % 4 == 3 {
                    pos += 1;
                    initial += 1;
                    offset += 1;
                    (true, false)
                } else {
                    (true, true)
                }
            }
            _ => (false, false),
        };
        olds.b_marks.push(state.b_marks[line]);
        state.b_marks[line] = pos;
        while pos < max {
            let ch = state.src[pos];
            if is_str_space(ch) {
                if ch == '\t' {
                    offset += 4 - (offset + state.bs_count[line] + i64::from(adjust_tab)) % 4;
                } else {
                    offset += 1;
                }
            } else {
                break;
            }
            pos += 1;
        }
        let last_line_empty = pos >= max;
        olds.bs_count.push(state.bs_count[line]);
        state.bs_count[line] = state.s_count[line] + 1 + i64::from(space_after_marker);
        olds.s_count.push(state.s_count[line]);
        state.s_count[line] = offset - initial;
        olds.t_shift.push(state.t_shift[line]);
        state.t_shift[line] = pos - state.b_marks[line];
        last_line_empty
    }

    /// The saved caches `blockquote` restores afterwards.
    #[derive(Default)]
    struct BlockquoteOlds {
        b_marks: Vec<usize>,
        bs_count: Vec<i64>,
        s_count: Vec<i64>,
        t_shift: Vec<usize>,
    }

    /// Block quote (`rules_block/blockquote.py`).
    fn blockquote(
        state: &mut StateBlock,
        start_line: usize,
        end_line: usize,
        silent: bool,
    ) -> bool {
        let old_line_max = state.line_max;
        let pos = state.b_marks[start_line] + state.t_shift[start_line];
        if state.is_code_block(start_line) {
            return false;
        }
        if state.at(pos) != Some('>') {
            return false;
        }
        if silent {
            return true;
        }
        let mut olds = BlockquoteOlds::default();
        let mut last_line_empty = blockquote_adjust(state, start_line, pos + 1, &mut olds);
        let old_parent = std::mem::replace(&mut state.parent_type, "blockquote".to_owned());
        let mut next_line = start_line + 1;
        while next_line < end_line {
            let is_outdented = state.s_count[next_line] < state.blk_indent;
            let pos = state.b_marks[next_line] + state.t_shift[next_line];
            let max = state.e_marks[next_line];
            if pos >= max {
                break;
            }
            // `pos += 1` runs even when the marker check fails.
            if state.at(pos) == Some('>') && !is_outdented {
                last_line_empty = blockquote_adjust(state, next_line, pos + 1, &mut olds);
                next_line += 1;
                continue;
            }
            if last_line_empty {
                break;
            }
            let mut terminate = false;
            for rule in blockquote_chain() {
                if rule(state, next_line, end_line, true) {
                    terminate = true;
                    break;
                }
            }
            if terminate {
                state.line_max = next_line;
                if state.blk_indent != 0 {
                    olds.b_marks.push(state.b_marks[next_line]);
                    olds.bs_count.push(state.bs_count[next_line]);
                    olds.t_shift.push(state.t_shift[next_line]);
                    olds.s_count.push(state.s_count[next_line]);
                    state.s_count[next_line] -= state.blk_indent;
                }
                break;
            }
            olds.b_marks.push(state.b_marks[next_line]);
            olds.bs_count.push(state.bs_count[next_line]);
            olds.t_shift.push(state.t_shift[next_line]);
            olds.s_count.push(state.s_count[next_line]);
            state.s_count[next_line] = -1;
            next_line += 1;
        }
        let old_indent = state.blk_indent;
        state.blk_indent = 0;
        let token = state.push("blockquote_open", "blockquote", 1);
        state.tokens[token].markup = ">".to_owned();
        state.tokens[token].map = Some([start_line, 0]);
        let open_idx = token;
        block_tokenize(state, start_line, next_line);
        let token = state.push("blockquote_close", "blockquote", -1);
        state.tokens[token].markup = ">".to_owned();
        state.line_max = old_line_max;
        state.parent_type = old_parent;
        state.tokens[open_idx].map = Some([start_line, state.line]);
        for (idx, item) in olds.t_shift.iter().enumerate() {
            state.b_marks[idx + start_line] = olds.b_marks[idx];
            state.t_shift[idx + start_line] = *item;
            state.s_count[idx + start_line] = olds.s_count[idx];
            state.bs_count[idx + start_line] = olds.bs_count[idx];
        }
        state.blk_indent = old_indent;
        true
    }

    // `list.py` (the `tasklists` option is off — the plugin owns todos) --

    /// `skipBulletListMarker`: next pos past `[-+*][\n ]`, else -1.
    fn skip_bullet_list_marker(state: &StateBlock, start_line: usize) -> isize {
        let mut pos = state.b_marks[start_line] + state.t_shift[start_line];
        let maximum = state.e_marks[start_line];
        let marker = match state.at(pos) {
            Some(ch) => ch,
            None => return -1,
        };
        pos += 1;
        if !matches!(marker, '*' | '-' | '+') {
            return -1;
        }
        if pos < maximum && !state.at(pos).is_some_and(is_str_space) {
            return -1;
        }
        pos as isize
    }

    /// `skipOrderedListMarker`: next pos past `\d+[.)][\n ]`, else -1
    /// (2+ chars, ≤9 digits).
    fn skip_ordered_list_marker(state: &StateBlock, start_line: usize) -> isize {
        let start = state.b_marks[start_line] + state.t_shift[start_line];
        let mut pos = start;
        let maximum = state.e_marks[start_line];
        if pos + 1 >= maximum {
            return -1;
        }
        let ch = match state.at(pos) {
            Some(ch) => ch,
            None => return -1,
        };
        pos += 1;
        if !ch.is_ascii_digit() {
            return -1;
        }
        loop {
            if pos >= maximum {
                return -1;
            }
            let ch = state.at(pos).unwrap_or('\0');
            pos += 1;
            if ch.is_ascii_digit() {
                if pos - start >= 10 {
                    return -1;
                }
                continue;
            }
            if ch == ')' || ch == '.' {
                break;
            }
            return -1;
        }
        if pos < maximum && !state.at(pos).is_some_and(is_str_space) {
            return -1;
        }
        pos as isize
    }

    /// `markTightParagraphs`.
    fn mark_tight_paragraphs(state: &mut StateBlock, idx: usize) {
        let level = state.level + 2;
        let mut i = idx + 2;
        let length = state.tokens.len().saturating_sub(2);
        while i < length {
            if state.tokens[i].level == level && state.tokens[i].ttype == "paragraph_open" {
                state.tokens[i + 2].hidden = true;
                state.tokens[i].hidden = true;
                i += 2;
            }
            i += 1;
        }
    }

    /// Lists (`rules_block/list.py`).
    fn list_block(
        state: &mut StateBlock,
        start_line: usize,
        end_line: usize,
        silent: bool,
    ) -> bool {
        let mut tight = true;
        if state.is_code_block(start_line) {
            return false;
        }
        if state.list_indent >= 0
            && state.s_count[start_line] - state.list_indent >= 4
            && state.s_count[start_line] < state.blk_indent
        {
            return false;
        }
        let mut is_terminating_paragraph = false;
        if silent
            && state.parent_type == "paragraph"
            && state.s_count[start_line] >= state.blk_indent
        {
            is_terminating_paragraph = true;
        }
        let mut pos_after_marker = skip_ordered_list_marker(state, start_line);
        let (is_ordered, marker_value);
        if pos_after_marker >= 0 {
            is_ordered = true;
            let start = state.b_marks[start_line] + state.t_shift[start_line];
            marker_value = state.src[start..pos_after_marker as usize - 1]
                .iter()
                .collect::<String>()
                .parse::<i64>()
                .unwrap_or(0);
            if is_terminating_paragraph && marker_value != 1 {
                return false;
            }
        } else {
            pos_after_marker = skip_bullet_list_marker(state, start_line);
            if pos_after_marker >= 0 {
                is_ordered = false;
                marker_value = 0;
            } else {
                return false;
            }
        }
        if is_terminating_paragraph
            && state.skip_spaces(pos_after_marker as usize) >= state.e_marks[start_line]
        {
            return false;
        }
        let marker_char = state.src[pos_after_marker as usize - 1];
        if silent {
            return true;
        }
        let list_tok_idx = state.tokens.len();
        if is_ordered {
            let token = state.push("ordered_list_open", "ol", 1);
            if marker_value != 1 {
                state.tokens[token].attrs = vec![("start".to_owned(), AttrVal::Int(marker_value))];
            }
            state.tokens[token].map = Some([start_line, 0]);
            state.tokens[token].markup = marker_char.to_string();
        } else {
            let token = state.push("bullet_list_open", "ul", 1);
            state.tokens[token].map = Some([start_line, 0]);
            state.tokens[token].markup = marker_char.to_string();
        }
        let mut next_line = start_line;
        let mut start_line = start_line;
        let mut prev_empty_end = false;
        let old_parent = std::mem::replace(&mut state.parent_type, "list".to_owned());
        while next_line < end_line {
            let mut pos = pos_after_marker as usize;
            let maximum = state.e_marks[next_line];
            let initial = state.s_count[next_line] + pos_after_marker as i64
                - (state.b_marks[start_line] + state.t_shift[start_line]) as i64;
            let mut offset = initial;
            while pos < maximum {
                let ch = state.src[pos];
                if ch == '\t' {
                    offset += 4 - (offset + state.bs_count[next_line]) % 4;
                } else if ch == ' ' {
                    offset += 1;
                } else {
                    break;
                }
                pos += 1;
            }
            let content_start = pos;
            let mut indent_after_marker = if content_start >= maximum {
                1
            } else {
                offset - initial
            };
            if indent_after_marker > 4 {
                indent_after_marker = 1;
            }
            let indent = initial + indent_after_marker;
            let token = state.push("list_item_open", "li", 1);
            state.tokens[token].markup = marker_char.to_string();
            state.tokens[token].map = Some([start_line, 0]);
            let item_idx = token;
            if is_ordered {
                let start = state.b_marks[start_line] + state.t_shift[start_line];
                state.tokens[token].info = state.src[start..pos_after_marker as usize - 1]
                    .iter()
                    .collect();
            }
            let old_tight = state.tight;
            let old_t_shift = state.t_shift[start_line];
            let old_s_count = state.s_count[start_line];
            let old_list_indent = state.list_indent;
            state.list_indent = state.blk_indent;
            state.blk_indent = indent;
            state.tight = true;
            state.t_shift[start_line] = content_start - state.b_marks[start_line];
            state.s_count[start_line] = offset;
            if content_start >= maximum && state.is_empty(start_line + 1) {
                state.line = (state.line + 2).min(end_line);
            } else {
                block_tokenize(state, start_line, end_line);
            }
            if !state.tight || prev_empty_end {
                tight = false;
            }
            // `state.line >= start_line` always holds (tokenize only
            // advances); `saturating_sub` keeps the proof local.
            prev_empty_end = state.line.saturating_sub(start_line) > 1
                && state.line >= 1
                && state.is_empty(state.line - 1);
            state.blk_indent = state.list_indent;
            state.list_indent = old_list_indent;
            state.t_shift[start_line] = old_t_shift;
            state.s_count[start_line] = old_s_count;
            state.tight = old_tight;
            let token = state.push("list_item_close", "li", -1);
            state.tokens[token].markup = marker_char.to_string();
            next_line = state.line;
            start_line = state.line;
            if let Some(map) = state.tokens[item_idx].map.as_mut() {
                map[1] = next_line;
            }
            if next_line >= end_line {
                break;
            }
            if state.s_count[next_line] < state.blk_indent {
                break;
            }
            if state.is_code_block(start_line) {
                break;
            }
            let mut terminate = false;
            for rule in list_chain() {
                if rule(state, next_line, end_line, true) {
                    terminate = true;
                    break;
                }
            }
            if terminate {
                break;
            }
            if is_ordered {
                pos_after_marker = skip_ordered_list_marker(state, next_line);
                if pos_after_marker < 0 {
                    break;
                }
            } else {
                pos_after_marker = skip_bullet_list_marker(state, next_line);
                if pos_after_marker < 0 {
                    break;
                }
            }
            if marker_char != state.src[pos_after_marker as usize - 1] {
                break;
            }
        }
        if is_ordered {
            let token = state.push("ordered_list_close", "ol", -1);
            state.tokens[token].markup = marker_char.to_string();
        } else {
            let token = state.push("bullet_list_close", "ul", -1);
            state.tokens[token].markup = marker_char.to_string();
        }
        if let Some(map) = state.tokens[list_tok_idx].map.as_mut() {
            map[1] = next_line;
        }
        state.line = next_line;
        state.parent_type = old_parent;
        if tight {
            mark_tight_paragraphs(state, list_tok_idx);
        }
        true
    }

    // -- link helpers (`helpers/parse_link_*.py`) --

    /// `parseLinkDestination` result.
    struct LinkDest {
        ok: bool,
        pos: usize,
        text: String,
    }

    /// `parseLinkDestination`: `<...>` or bare (parens-balanced to 32).
    /// Positions are char indices into `chars`.
    fn parse_link_destination(chars: &[char], start: usize, maximum: usize) -> LinkDest {
        let mut result = LinkDest {
            ok: false,
            pos: 0,
            text: String::new(),
        };
        let mut pos = start;
        if chars.get(pos) == Some(&'<') {
            pos += 1;
            while pos < maximum {
                let code = chars[pos] as u32;
                if code == 0x0A || code == 0x3C {
                    return result;
                }
                if code == 0x3E {
                    result.pos = pos + 1;
                    result.text = unescape_all(&chars[start + 1..pos].iter().collect::<String>());
                    result.ok = true;
                    return result;
                }
                if code == 0x5C && pos + 1 < maximum {
                    pos += 2;
                    continue;
                }
                pos += 1;
            }
            return result;
        }
        let mut level = 0;
        while pos < maximum {
            let code = chars[pos] as u32;
            if code == 0x20 {
                break;
            }
            if code < 0x20 || code == 0x7F {
                break;
            }
            if code == 0x5C && pos + 1 < maximum {
                if chars[pos + 1] as u32 == 0x20 {
                    break;
                }
                pos += 2;
                continue;
            }
            if code == 0x28 {
                level += 1;
                if level > 32 {
                    return result;
                }
            }
            if code == 0x29 {
                if level == 0 {
                    break;
                }
                level -= 1;
            }
            pos += 1;
        }
        if start == pos || level != 0 {
            return result;
        }
        result.text = unescape_all(&chars[start..pos].iter().collect::<String>());
        result.pos = pos;
        result.ok = true;
        result
    }

    /// `parseLinkTitle` result.
    struct LinkTitle {
        ok: bool,
        can_continue: bool,
        pos: usize,
        text: String,
        marker: char,
    }

    /// `parseLinkTitle`: `"..."`, `'...'`, `(...)`; `prev` continues a
    /// reference title onto the next line.
    fn parse_link_title(
        chars: &[char],
        start: usize,
        maximum: usize,
        prev: Option<&LinkTitle>,
    ) -> LinkTitle {
        let mut pos = start;
        let mut state = LinkTitle {
            ok: false,
            can_continue: false,
            pos: 0,
            text: String::new(),
            marker: '\0',
        };
        let content_start: usize;
        if let Some(prev) = prev {
            state.text = prev.text.clone();
            state.marker = prev.marker;
            content_start = start;
        } else {
            if pos >= maximum {
                return state;
            }
            let marker = chars[pos];
            if marker != '"' && marker != '\'' && marker != '(' {
                return state;
            }
            content_start = start + 1;
            pos += 1;
            state.marker = if marker == '(' { ')' } else { marker };
        }
        while pos < maximum {
            let code = chars[pos];
            if code == state.marker {
                state.pos = pos + 1;
                state.text.push_str(&unescape_all(
                    &chars[content_start..pos].iter().collect::<String>(),
                ));
                state.ok = true;
                return state;
            } else if code == '(' && state.marker == ')' {
                return state;
            } else if code == '\\' && pos + 1 < maximum {
                pos += 1;
            }
            pos += 1;
        }
        state.can_continue = true;
        state.text.push_str(&unescape_all(
            &chars[content_start..pos].iter().collect::<String>(),
        ));
        state
    }

    // `reference.py` (the `inline_definitions` option is off) --

    /// `getNextLine`: the next line's text plus its newline, unless
    /// empty, past the end, or terminated by a `reference`-chain rule.
    fn reference_next_line(state: &mut StateBlock, next_line: usize) -> Option<String> {
        let end_line = state.line_max;
        if next_line >= end_line || state.is_empty(next_line) {
            return None;
        }
        let mut is_continuation = false;
        if state.is_code_block(next_line) {
            is_continuation = true;
        }
        if state.s_count[next_line] < 0 {
            is_continuation = true;
        }
        if !is_continuation {
            let old_parent = std::mem::replace(&mut state.parent_type, "reference".to_owned());
            let mut terminate = false;
            for rule in reference_chain() {
                if rule(state, next_line, end_line, true) {
                    terminate = true;
                    break;
                }
            }
            state.parent_type = old_parent;
            if terminate {
                return None;
            }
        }
        let pos = state.b_marks[next_line] + state.t_shift[next_line];
        let maximum = state.e_marks[next_line];
        // `maximum + 1` explicitly includes the newline (clamped).
        Some(
            state.src[pos..(maximum + 1).min(state.src.len())]
                .iter()
                .collect(),
        )
    }

    /// Link reference definitions (`rules_block/reference.py`).
    fn reference(state: &mut StateBlock, start_line: usize, _end: usize, silent: bool) -> bool {
        let pos = state.b_marks[start_line] + state.t_shift[start_line];
        let maximum = state.e_marks[start_line];
        let mut next_line = start_line + 1;
        if state.is_code_block(start_line) {
            return false;
        }
        if state.at(pos) != Some('[') {
            return false;
        }
        // `maximum + 1` includes the newline (clamped at EOF).
        let mut string: Vec<char> = state.src[pos..(maximum + 1).min(state.src.len())].to_vec();
        let mut maximum = string.len();
        let mut label_end: Option<usize> = None;
        let mut pos = 1;
        while pos < maximum {
            let ch = string[pos] as u32;
            if ch == 0x5B {
                return false;
            } else if ch == 0x5D {
                label_end = Some(pos);
                break;
            } else if ch == 0x0A {
                if let Some(more) = reference_next_line(state, next_line) {
                    string.extend(more.chars());
                    maximum = string.len();
                    next_line += 1;
                }
            } else if ch == 0x5C {
                pos += 1;
                // (Edition 2021: no let-chains; nested instead.)
                if pos < maximum && string[pos] == '\n' {
                    if let Some(more) = reference_next_line(state, next_line) {
                        string.extend(more.chars());
                        maximum = string.len();
                        next_line += 1;
                    }
                }
            }
            pos += 1;
        }
        let Some(label_end) = label_end else {
            return false;
        };
        if string.get(label_end + 1) != Some(&':') {
            return false;
        }
        let mut pos = label_end + 2;
        while pos < maximum {
            let ch = string[pos];
            if ch == '\n' {
                if let Some(more) = reference_next_line(state, next_line) {
                    string.extend(more.chars());
                    maximum = string.len();
                    next_line += 1;
                }
            } else if !is_str_space(ch) {
                break;
            }
            pos += 1;
        }
        let dest = parse_link_destination(&string, pos, maximum);
        if !dest.ok {
            return false;
        }
        let href = normalize_link(&dest.text);
        if !validate_link(&href) {
            return false;
        }
        pos = dest.pos;
        let dest_end_pos = pos;
        let dest_end_line = next_line;
        let start = pos;
        while pos < maximum {
            let ch = string[pos];
            if ch == '\n' {
                if let Some(more) = reference_next_line(state, next_line) {
                    string.extend(more.chars());
                    maximum = string.len();
                    next_line += 1;
                }
            } else if !is_str_space(ch) {
                break;
            }
            pos += 1;
        }
        let mut title = parse_link_title(&string, pos, maximum, None);
        while title.can_continue {
            let Some(more) = reference_next_line(state, next_line) else {
                break;
            };
            string.extend(more.chars());
            pos = maximum;
            maximum = string.len();
            next_line += 1;
            title = parse_link_title(&string, pos, maximum, Some(&title));
        }
        let title_text: String;
        if pos < maximum && start != pos && title.ok {
            title_text = title.text;
            pos = title.pos;
        } else {
            title_text = String::new();
            pos = dest_end_pos;
            next_line = dest_end_line;
        }
        while pos < maximum {
            if !is_str_space(string[pos]) {
                break;
            }
            pos += 1;
        }
        let mut title_text = title_text;
        if pos < maximum && string[pos] != '\n' && !title_text.is_empty() {
            title_text = String::new();
            pos = dest_end_pos;
            next_line = dest_end_line;
            while pos < maximum {
                if !is_str_space(string[pos]) {
                    break;
                }
                pos += 1;
            }
        }
        if pos < maximum && string[pos] != '\n' {
            return false;
        }
        let label = normalize_reference(&string[1..label_end].iter().collect::<String>());
        if label.is_empty() {
            return false;
        }
        if silent {
            return true;
        }
        // `duplicate_refs` is write-only (the renderer never reads
        // `env`), so only the first definition is kept.
        let href_title = Ref {
            href,
            title: title_text,
        };
        state.line = next_line;
        state
            .references
            .get_or_insert_with(HashMap::new)
            .entry(label)
            .or_insert(href_title);
        true
    }

    // -- inline state (`rules_inline/state_inline.py`) --

    /// `Delimiter` (`level` is never read — only the pair walkers use
    /// these, and they don't consult it).
    #[derive(Debug, Clone)]
    struct Delimiter {
        marker: u32,
        length: usize,
        token: usize,
        end: i64,
        open: bool,
        close: bool,
    }

    /// `StateInline`: `linkLevel` is dead without the `linkify` rule;
    /// `pendingLevel` rides along for `pushPending` levels.
    struct StateInline<'refs> {
        src: Vec<char>,
        tokens: Vec<Token>,
        /// `tokens_meta`: one entry per `push` (never per `pushPending`,
        /// mirroring Python); `Some` holds a delimiter-arena index.
        tokens_meta: Vec<Option<usize>>,
        /// Arena of delimiter lists; index 0 is the top-level list.
        delim_lists: Vec<Vec<Delimiter>>,
        current_list: usize,
        prev_lists: Vec<usize>,
        pos: usize,
        pos_max: usize,
        level: i64,
        pending: String,
        pending_level: i64,
        cache: HashMap<usize, usize>,
        backticks: HashMap<usize, usize>,
        backticks_scanned: bool,
        references: &'refs Option<HashMap<String, Ref>>,
    }

    impl<'refs> StateInline<'refs> {
        fn new(src: &str, references: &'refs Option<HashMap<String, Ref>>) -> StateInline<'refs> {
            StateInline {
                src: src.chars().collect(),
                tokens: Vec::new(),
                tokens_meta: Vec::new(),
                delim_lists: vec![Vec::new()],
                current_list: 0,
                prev_lists: Vec::new(),
                pos: 0,
                pos_max: 0,
                level: 0,
                pending: String::new(),
                pending_level: 0,
                cache: HashMap::new(),
                backticks: HashMap::new(),
                backticks_scanned: false,
                references,
            }
        }

        fn at(&self, pos: usize) -> Option<char> {
            self.src.get(pos).copied()
        }

        fn delimiters_mut(&mut self) -> &mut Vec<Delimiter> {
            &mut self.delim_lists[self.current_list]
        }

        /// `pushPending`: flush pending text (no `tokens_meta` entry).
        fn push_pending(&mut self) -> usize {
            let mut token = Token::new("text", "", 0);
            token.content = std::mem::take(&mut self.pending);
            token.level = self.pending_level;
            self.tokens.push(token);
            self.tokens.len() - 1
        }

        /// `push`: flush pending, then the token; opens swap in a fresh
        /// delimiter list recorded in `tokens_meta`, closes restore.
        fn push(&mut self, ttype: &str, tag: &str, nesting: i8) -> usize {
            if !self.pending.is_empty() {
                self.push_pending();
            }
            let mut token = Token::new(ttype, tag, nesting);
            let mut meta = None;
            if nesting < 0 {
                self.level -= 1;
                if let Some(prev) = self.prev_lists.pop() {
                    self.current_list = prev;
                }
            }
            token.level = self.level;
            if nesting > 0 {
                self.level += 1;
                self.prev_lists.push(self.current_list);
                self.delim_lists.push(Vec::new());
                self.current_list = self.delim_lists.len() - 1;
                meta = Some(self.current_list);
            }
            self.pending_level = self.level;
            self.tokens.push(token);
            self.tokens_meta.push(meta);
            self.tokens.len() - 1
        }

        /// `scanDelims`: flanking scan for emphasis-like runs.
        fn scan_delims(&self, start: usize, can_split_word: bool) -> (bool, bool, usize) {
            let marker = self.src[start];
            let last_char = if start > 0 { self.src[start - 1] } else { ' ' };
            let mut pos = start;
            while pos < self.pos_max && self.src[pos] == marker {
                pos += 1;
            }
            let count = pos - start;
            let next_char = if pos < self.pos_max {
                self.src[pos]
            } else {
                ' '
            };
            let is_last_punct = is_md_ascii_punct(last_char as u32) || is_punct_char(last_char);
            let is_next_punct = is_md_ascii_punct(next_char as u32) || is_punct_char(next_char);
            let is_last_ws = is_white_space(last_char as u32);
            let is_next_ws = is_white_space(next_char as u32);
            let left_flanking = !(is_next_ws || (is_next_punct && !(is_last_ws || is_last_punct)));
            let right_flanking = !(is_last_ws || (is_last_punct && !(is_next_ws || is_next_punct)));
            let can_open = left_flanking && (can_split_word || !right_flanking || is_last_punct);
            let can_close = right_flanking && (can_split_word || !left_flanking || is_next_punct);
            (can_open, can_close, count)
        }
    }

    // -- inline rules --

    type InlineRule = fn(&mut StateInline, bool) -> bool;

    /// `html_inline` / `linkify` with `html=False` / disabled: dead.
    fn html_inline_dead(_state: &mut StateInline, _silent: bool) -> bool {
        false
    }

    /// The `text` rule's terminator set (`parser_inline.py`).
    fn is_terminator(ch: char) -> bool {
        matches!(
            ch,
            '\n' | '!'
                | '#'
                | '$'
                | '%'
                | '&'
                | '*'
                | '+'
                | '-'
                | ':'
                | '<'
                | '='
                | '>'
                | '@'
                | '['
                | '\\'
                | ']'
                | '^'
                | '_'
                | '`'
                | '{'
                | '}'
                | '~'
        )
    }

    /// `text` (`rules_inline/text.py`): consume to the next terminator.
    fn inline_text(state: &mut StateInline, silent: bool) -> bool {
        let mut pos = state.pos;
        while pos < state.pos_max && !is_terminator(state.src[pos]) {
            pos += 1;
        }
        if pos == state.pos {
            return false;
        }
        if !silent {
            state.pending.extend(state.src[state.pos..pos].iter());
        }
        state.pos = pos;
        true
    }

    /// `newline` (`rules_inline/newline.py`): two trailing spaces (or
    /// one, swallowing it) decide hard vs soft.
    fn inline_newline(state: &mut StateInline, silent: bool) -> bool {
        let pos = state.pos;
        if state.at(pos) != Some('\n') {
            return false;
        }
        let maximum = state.pos_max;
        if !silent {
            let pending: Vec<char> = state.pending.chars().collect();
            if pending.last() == Some(&' ') && !pending.is_empty() {
                if pending.len() >= 2 && pending[pending.len() - 2] == ' ' {
                    let mut ws = pending.len() - 2;
                    while ws >= 1 && pending[ws - 1] == ' ' {
                        ws -= 1;
                    }
                    state.pending = pending[..ws].iter().collect();
                    state.push("hardbreak", "br", 0);
                } else {
                    state.pending = pending[..pending.len() - 1].iter().collect();
                    state.push("softbreak", "br", 0);
                }
            } else {
                state.push("softbreak", "br", 0);
            }
        }
        let mut pos = pos + 1;
        while pos < maximum && state.at(pos).is_some_and(is_str_space) {
            pos += 1;
        }
        state.pos = pos;
        true
    }

    /// `escape` (`rules_inline/escape.py`): `\\` + punct (the surrogate
    /// splice is unreachable — Rust `char`s are scalar values), or a
    /// hard break before `\n`.
    fn inline_escape(state: &mut StateInline, silent: bool) -> bool {
        let pos = state.pos;
        let maximum = state.pos_max;
        if state.at(pos) != Some('\\') {
            return false;
        }
        let mut pos = pos + 1;
        if pos >= maximum {
            return false;
        }
        if state.at(pos) == Some('\n') {
            if !silent {
                state.push("hardbreak", "br", 0);
            }
            pos += 1;
            while pos < maximum && state.at(pos).is_some_and(is_str_space) {
                pos += 1;
            }
            state.pos = pos;
            return true;
        }
        let escaped = state.src[pos];
        let orig = format!("\\{escaped}");
        if !silent {
            let token = state.push("text_special", "", 0);
            // `_ESCAPED` is exactly the ASCII punct set (all 32):
            // `\\` + anything else stays whole.
            state.tokens[token].content = if is_md_ascii_punct(escaped as u32) {
                escaped.to_string()
            } else {
                orig.clone()
            };
            state.tokens[token].markup = orig;
            state.tokens[token].info = "escape".to_owned();
        }
        state.pos = pos + 1;
        true
    }

    /// ``backtick`` (``rules_inline/backticks.py``): the `` ` ``-cache
    /// (`backticks`/`backticksScanned`) bounds the closer search.
    fn inline_backtick(state: &mut StateInline, silent: bool) -> bool {
        if state.at(state.pos) != Some('`') {
            return false;
        }
        let start = state.pos;
        let mut pos = start + 1;
        let maximum = state.pos_max;
        while pos < maximum && state.at(pos) == Some('`') {
            pos += 1;
        }
        let marker: String = state.src[start..pos].iter().collect();
        let opener_length = pos - start;
        if state.backticks_scanned
            && state.backticks.get(&opener_length).copied().unwrap_or(0) <= start
        {
            if !silent {
                state.pending.push_str(&marker);
            }
            state.pos += opener_length;
            return true;
        }
        // (Deferred: always assigned from `found` before any read.)
        let mut match_start: usize;
        let mut match_end = pos;
        loop {
            let mut found = None;
            let mut scan = match_end;
            while scan < maximum {
                if state.src[scan] == '`' {
                    found = Some(scan);
                    break;
                }
                scan += 1;
            }
            let Some(found) = found else {
                break;
            };
            match_start = found;
            match_end = match_start + 1;
            while match_end < maximum && state.at(match_end) == Some('`') {
                match_end += 1;
            }
            let closer_length = match_end - match_start;
            if closer_length == opener_length {
                if !silent {
                    let token = state.push("code_inline", "code", 0);
                    state.tokens[token].markup = marker;
                    let mut content: String = state.src[pos..match_start]
                        .iter()
                        .collect::<String>()
                        .replace('\n', " ");
                    if content.starts_with(' ')
                        && content.ends_with(' ')
                        && !py_strip(&content).is_empty()
                    {
                        let mut chars: Vec<char> = content.chars().collect();
                        chars.pop();
                        chars.remove(0);
                        content = chars.into_iter().collect();
                    }
                    state.tokens[token].content = content;
                }
                state.pos = match_end;
                return true;
            }
            state.backticks.insert(closer_length, match_start);
        }
        state.backticks_scanned = true;
        if !silent {
            state.pending.push_str(&marker);
        }
        state.pos += opener_length;
        true
    }

    // `autolink.py` --

    /// One `EMAIL_RE` domain label: alnum, then optionally up to 61
    /// alnum/dash chars plus a closing alnum.
    fn is_email_label(segment: &str) -> bool {
        let mut chars = segment.chars();
        match chars.next() {
            Some(ch) if ch.is_ascii_alphanumeric() => {}
            _ => return false,
        }
        let rest: Vec<char> = chars.collect();
        if rest.is_empty() {
            return true;
        }
        rest.len() <= 62
            && rest
                .iter()
                .all(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
            && rest.last().is_some_and(|ch| ch.is_ascii_alphanumeric())
    }

    /// `EMAIL_RE`: local `@` dotted labels, full match.
    fn is_email_url(url: &str) -> bool {
        let Some(at) = url.find('@') else {
            return false;
        };
        // The local class excludes `@`, so the first `@` splits.
        let (local, domain) = (&url[..at], &url[at + 1..]);
        if local.is_empty()
            || !local
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "!#$%&'*+/=?^_`{|}~-.".contains(ch))
        {
            return false;
        }
        if domain.is_empty() {
            return false;
        }
        domain.split('.').all(is_email_label)
    }

    /// `AUTOLINK_RE`: `scheme:rest` — the scheme can't contain `:`, so
    /// the first colon splits; scheme 2-32 chars.
    fn is_autolink_url(url: &str) -> bool {
        let Some(colon) = url.find(':') else {
            return false;
        };
        let (scheme, rest) = (&url[..colon], &url[colon + 1..]);
        let len = scheme.chars().count();
        if !(2..=32).contains(&len) {
            return false;
        }
        let mut chars = scheme.chars();
        if !chars.next().is_some_and(|ch| ch.is_ascii_alphabetic()) {
            return false;
        }
        if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '.' || ch == '-') {
            return false;
        }
        rest.chars().all(|ch| {
            let code = ch as u32;
            ch != '<' && ch != '>' && code > 0x20
        })
    }

    /// Autolinks (`rules_inline/autolink.py`).
    fn inline_autolink(state: &mut StateInline, silent: bool) -> bool {
        if state.at(state.pos) != Some('<') {
            return false;
        }
        let start = state.pos;
        let maximum = state.pos_max;
        let mut pos = state.pos;
        loop {
            pos += 1;
            if pos >= maximum {
                return false;
            }
            let ch = state.src[pos];
            if ch == '<' {
                return false;
            }
            if ch == '>' {
                break;
            }
        }
        let url: String = state.src[start + 1..pos].iter().collect();
        if is_autolink_url(&url) {
            let full_url = normalize_link(&url);
            if !validate_link(&full_url) {
                return false;
            }
            if !silent {
                let token = state.push("link_open", "a", 1);
                state.tokens[token].attrs = vec![("href".to_owned(), AttrVal::Str(full_url))];
                state.tokens[token].markup = "autolink".to_owned();
                state.tokens[token].info = "auto".to_owned();
                let token = state.push("text", "", 0);
                state.tokens[token].content = normalize_link_text(&url);
                let token = state.push("link_close", "a", -1);
                state.tokens[token].markup = "autolink".to_owned();
                state.tokens[token].info = "auto".to_owned();
            }
            state.pos += url.chars().count() + 2;
            return true;
        }
        if is_email_url(&url) {
            let full_url = normalize_link(&format!("mailto:{url}"));
            if !validate_link(&full_url) {
                return false;
            }
            if !silent {
                let token = state.push("link_open", "a", 1);
                state.tokens[token].attrs = vec![("href".to_owned(), AttrVal::Str(full_url))];
                state.tokens[token].markup = "autolink".to_owned();
                state.tokens[token].info = "auto".to_owned();
                let token = state.push("text", "", 0);
                state.tokens[token].content = normalize_link_text(&url);
                let token = state.push("link_close", "a", -1);
                state.tokens[token].markup = "autolink".to_owned();
                state.tokens[token].info = "auto".to_owned();
            }
            state.pos += url.chars().count() + 2;
            return true;
        }
        false
    }

    // `entity.py` --

    /// `DIGITAL_RE`: `^&#((?:x[a-f0-9]{1,6}|[0-9]{1,7}));`
    /// case-insensitive — greedy with backtracking.
    fn match_digit_entity(chars: &[char]) -> Option<(usize, u32)> {
        // Lengths exclude the `&#` (the caller adds 2): `x` + digits
        // + `;`, or digits + `;`.
        if chars.first() == Some(&'x') || chars.first() == Some(&'X') {
            for len in (1..=6).rev() {
                if chars.len() > len
                    && chars[1..len + 1].iter().all(|ch| ch.is_ascii_hexdigit())
                    && chars.get(len + 1) == Some(&';')
                {
                    let hex: String = chars[1..len + 1].iter().collect();
                    return u32::from_str_radix(&hex, 16)
                        .ok()
                        .map(|code| (len + 2, code));
                }
            }
            return None;
        }
        for len in (1..=7).rev() {
            if chars.len() > len
                && chars[..len].iter().all(|ch| ch.is_ascii_digit())
                && chars.get(len) == Some(&';')
            {
                let digits: String = chars[..len].iter().collect();
                return digits.parse::<u32>().ok().map(|code| (len + 1, code));
            }
        }
        None
    }

    /// `NAMED_RE`: `^&([a-z][a-z0-9]{1,31});` case-insensitive —
    /// greedy with backtracking. Only the longest `;`-terminated
    /// name is ever a candidate (an earlier `;` would have ended the
    /// alnum run), so — like Python — the dict lookup runs once, on
    /// that name, case-sensitively.
    fn match_named_entity(chars: &[char]) -> Option<(usize, String)> {
        if !chars.first().is_some_and(|ch| ch.is_ascii_alphabetic()) {
            return None;
        }
        let mut end = 1;
        while end < chars.len() && end < 1 + 31 && chars[end].is_ascii_alphanumeric() {
            end += 1;
        }
        let mut len = end - 1;
        while len >= 1 {
            if chars.get(1 + len) == Some(&';') {
                let name: String = chars[..1 + len].iter().collect();
                if entity_lookup(&name).is_some() {
                    return Some((1 + len + 1, name));
                }
                return None;
            }
            len -= 1;
        }
        None
    }

    /// HTML entities (`rules_inline/entity.py`): invalid codes become
    /// U+FFFD here (unlike `unescapeAll`, which keeps the match).
    fn inline_entity(state: &mut StateInline, silent: bool) -> bool {
        let pos = state.pos;
        let maximum = state.pos_max;
        if state.at(pos) != Some('&') {
            return false;
        }
        if pos + 1 >= maximum {
            return false;
        }
        if state.at(pos + 1) == Some('#') {
            // `DIGITAL_RE.search(state.src[pos:])`: `&#` + body.
            if let Some((len, code)) =
                match_digit_entity(&state.src[pos + 2..maximum.min(state.src.len())])
            {
                if !silent {
                    let token = state.push("text_special", "", 0);
                    state.tokens[token].content = if is_valid_entity_code(code) {
                        char::from_u32(code).unwrap_or('\u{FFFD}').to_string()
                    } else {
                        "\u{FFFD}".to_owned()
                    };
                    state.tokens[token].markup = state.src[pos..pos + 2 + len].iter().collect();
                    state.tokens[token].info = "entity".to_owned();
                }
                state.pos += 2 + len;
                return true;
            }
        } else if let Some((len, name)) =
            match_named_entity(&state.src[pos + 1..maximum.min(state.src.len())])
        {
            if !silent {
                let token = state.push("text_special", "", 0);
                state.tokens[token].content = entity_lookup(&name).unwrap_or("").to_owned();
                state.tokens[token].markup = state.src[pos..pos + 1 + len].iter().collect();
                state.tokens[token].info = "entity".to_owned();
            }
            state.pos += 1 + len;
            return true;
        }
        false
    }

    // `parse_link_label.py` + the inline driver (`parser_inline.py`) --

    /// `parseLinkLabel`: the label end, or -1. `disableNested` fails
    /// the whole scan when a bare `[` nests inside.
    fn parse_link_label(state: &mut StateInline, start: usize, disable_nested: bool) -> isize {
        let mut label_end = -1;
        let old_pos = state.pos;
        let mut found = false;
        state.pos = start + 1;
        let mut level = 1;
        while state.pos < state.pos_max {
            let marker = state.src[state.pos];
            if marker == ']' {
                level -= 1;
                if level == 0 {
                    found = true;
                    break;
                }
            }
            let prev_pos = state.pos;
            inline_skip_token(state);
            if marker == '[' {
                if prev_pos == state.pos - 1 {
                    level += 1;
                } else if disable_nested {
                    state.pos = old_pos;
                    return -1;
                }
            }
        }
        if found {
            label_end = state.pos as isize;
        }
        state.pos = old_pos;
        label_end
    }

    /// The inline chain: text, newline, escape, backticks,
    /// strikethrough, emphasis, link, image, autolink, html (dead),
    /// entity (`linkify` disabled).
    fn inline_chain() -> [InlineRule; 11] {
        [
            inline_text,
            inline_newline,
            inline_escape,
            inline_backtick,
            strikethrough_tokenize,
            emphasis_tokenize,
            inline_link,
            inline_image,
            inline_autolink,
            html_inline_dead,
            inline_entity,
        ]
    }

    /// `ParserInline.skipToken`: validation-mode scan with the pos cache.
    fn inline_skip_token(state: &mut StateInline) {
        let mut ok = false;
        let pos = state.pos;
        if let Some(&cached) = state.cache.get(&pos) {
            state.pos = cached;
            return;
        }
        if state.level < MAX_NESTING {
            for rule in inline_chain() {
                state.level += 1;
                ok = rule(state, true);
                state.level -= 1;
                if ok {
                    break;
                }
            }
        } else {
            state.pos = state.pos_max;
        }
        if !ok {
            state.pos += 1;
        }
        state.cache.insert(pos, state.pos);
    }

    /// `ParserInline.tokenize`.
    fn inline_tokenize(state: &mut StateInline) {
        let mut ok = false;
        let end = state.pos_max;
        while state.pos < end {
            if state.level < MAX_NESTING {
                for rule in inline_chain() {
                    ok = rule(state, false);
                    if ok {
                        break;
                    }
                }
            }
            if ok {
                if state.pos >= end {
                    break;
                }
                continue;
            }
            state.pending.push(state.src[state.pos]);
            state.pos += 1;
        }
        if !state.pending.is_empty() {
            state.push_pending();
        }
    }

    /// `ParserInline.parse` into `tokens`: tokenize plus `rules2`
    /// (`balance_pairs`, `strikethrough`, `emphasis`, `fragments_join`).
    fn inline_parse(
        content: &str,
        references: &Option<HashMap<String, Ref>>,
        tokens: &mut Vec<Token>,
    ) {
        let mut state = StateInline::new(content, references);
        state.pos_max = state.src.len();
        std::mem::swap(&mut state.tokens, tokens);
        inline_tokenize(&mut state);
        link_pairs(&mut state);
        strikethrough_post_process(&mut state);
        emphasis_post_process(&mut state);
        fragments_join(&mut state);
        std::mem::swap(&mut state.tokens, tokens);
    }

    // `link.py`, `image.py` (the `store_labels` option is off) --

    /// Skip link-whitespace: tab/space/`\n`.
    fn skip_link_spaces(chars: &[char], mut pos: usize, maximum: usize) -> usize {
        while pos < maximum {
            let ch = chars[pos];
            if !is_str_space(ch) && ch != '\n' {
                break;
            }
            pos += 1;
        }
        pos
    }

    /// Links (`rules_inline/link.py`).
    fn inline_link(state: &mut StateInline, silent: bool) -> bool {
        let mut href = String::new();
        let mut title = String::new();
        let old_pos = state.pos;
        let maximum = state.pos_max;
        let mut parse_reference = true;
        if state.at(state.pos) != Some('[') {
            return false;
        }
        let label_start = state.pos + 1;
        let label_end = parse_link_label(state, state.pos, true);
        if label_end < 0 {
            return false;
        }
        let label_end = label_end as usize;
        let mut pos = label_end + 1;
        if pos < maximum && state.at(pos) == Some('(') {
            parse_reference = false;
            pos = skip_link_spaces(&state.src, pos + 1, maximum);
            if pos >= maximum {
                return false;
            }
            let dest = parse_link_destination(&state.src, pos, state.pos_max);
            if dest.ok {
                href = normalize_link(&dest.text);
                if validate_link(&href) {
                    pos = dest.pos;
                } else {
                    href = String::new();
                }
                let start = pos;
                pos = skip_link_spaces(&state.src, pos, maximum);
                let title_res = parse_link_title(&state.src, pos, state.pos_max, None);
                if pos < maximum && start != pos && title_res.ok {
                    title = title_res.text;
                    pos = title_res.pos;
                    pos = skip_link_spaces(&state.src, pos, maximum);
                }
            }
            if pos >= maximum || state.at(pos) != Some(')') {
                parse_reference = true;
            }
            pos += 1;
        }
        if parse_reference {
            let Some(references) = state.references else {
                return false;
            };
            let mut label: Option<String> = None;
            if pos < maximum && state.at(pos) == Some('[') {
                let start = pos + 1;
                let found = parse_link_label(state, pos, false);
                if found >= 0 {
                    label = Some(state.src[start..found as usize].iter().collect());
                    pos = found as usize + 1;
                } else {
                    pos = label_end + 1;
                }
            } else {
                pos = label_end + 1;
            }
            let mut label = label.unwrap_or_default();
            if label.is_empty() {
                label = state.src[label_start..label_end].iter().collect();
            }
            let label = normalize_reference(&label);
            let Some(found) = references.get(&label) else {
                state.pos = old_pos;
                return false;
            };
            href = found.href.clone();
            title = found.title.clone();
        }
        if !silent {
            state.pos = label_start;
            state.pos_max = label_end;
            let token = state.push("link_open", "a", 1);
            state.tokens[token].attrs = vec![("href".to_owned(), AttrVal::Str(href))];
            if !title.is_empty() {
                state.tokens[token].attr_set("title", AttrVal::Str(title));
            }
            inline_tokenize(state);
            state.push("link_close", "a", -1);
        }
        state.pos = pos;
        state.pos_max = maximum;
        true
    }

    /// Images (`rules_inline/image.py`).
    fn inline_image(state: &mut StateInline, silent: bool) -> bool {
        let mut href = String::new();
        let mut title = String::new();
        let old_pos = state.pos;
        let maximum = state.pos_max;
        if state.at(state.pos) != Some('!') {
            return false;
        }
        if state.pos + 1 < state.pos_max && state.at(state.pos + 1) != Some('[') {
            return false;
        }
        let label_start = state.pos + 2;
        let label_end = parse_link_label(state, state.pos + 1, false);
        if label_end < 0 {
            return false;
        }
        let label_end = label_end as usize;
        let mut pos = label_end + 1;
        if pos < maximum && state.at(pos) == Some('(') {
            pos = skip_link_spaces(&state.src, pos + 1, maximum);
            if pos >= maximum {
                return false;
            }
            let dest = parse_link_destination(&state.src, pos, state.pos_max);
            if dest.ok {
                href = normalize_link(&dest.text);
                if validate_link(&href) {
                    pos = dest.pos;
                } else {
                    href = String::new();
                }
            }
            let start = pos;
            pos = skip_link_spaces(&state.src, pos, maximum);
            let title_res = parse_link_title(&state.src, pos, state.pos_max, None);
            if pos < maximum && start != pos && title_res.ok {
                title = title_res.text;
                pos = title_res.pos;
                pos = skip_link_spaces(&state.src, pos, maximum);
            }
            if pos >= maximum || state.at(pos) != Some(')') {
                state.pos = old_pos;
                return false;
            }
            pos += 1;
        } else {
            let Some(references) = state.references else {
                return false;
            };
            let mut label: Option<String> = None;
            if pos < maximum && state.at(pos) == Some('[') {
                let start = pos + 1;
                let found = parse_link_label(state, pos, false);
                if found >= 0 {
                    label = Some(state.src[start..found as usize].iter().collect());
                    pos = found as usize + 1;
                } else {
                    pos = label_end + 1;
                }
            } else {
                pos = label_end + 1;
            }
            let mut label = label.unwrap_or_default();
            if label.is_empty() {
                label = state.src[label_start..label_end].iter().collect();
            }
            let label = normalize_reference(&label);
            let Some(found) = references.get(&label) else {
                state.pos = old_pos;
                return false;
            };
            href = found.href.clone();
            title = found.title.clone();
        }
        if !silent {
            let content: String = state.src[label_start..label_end].iter().collect();
            // A nested full `inline.parse` (rules2 included — but NOT
            // the core `text_join`, so `text_special` survives here).
            let mut tokens = Vec::new();
            inline_parse(&content, state.references, &mut tokens);
            let token = state.push("image", "img", 0);
            state.tokens[token].attrs = vec![
                ("src".to_owned(), AttrVal::Str(href)),
                ("alt".to_owned(), AttrVal::Str(String::new())),
            ];
            state.tokens[token].children = if tokens.is_empty() {
                None
            } else {
                Some(tokens)
            };
            state.tokens[token].content = content;
            if !title.is_empty() {
                state.tokens[token].attr_set("title", AttrVal::Str(title));
            }
        }
        state.pos = pos;
        state.pos_max = maximum;
        true
    }

    // `emphasis.py`, `strikethrough.py`, `balance_pairs.py`, `fragments_join.py`
    // (the `strikethrough_single_tilde` option is off: classic `~~` only) --

    /// `emphasis.tokenize`: one text token per marker + delimiters.
    fn emphasis_tokenize(state: &mut StateInline, silent: bool) -> bool {
        if silent {
            return false;
        }
        let start = state.pos;
        let marker = state.src[start];
        if marker != '_' && marker != '*' {
            return false;
        }
        let (can_open, can_close, length) = state.scan_delims(state.pos, marker == '*');
        for _ in 0..length {
            let token = state.push("text", "", 0);
            state.tokens[token].content = marker.to_string();
            let delim = Delimiter {
                marker: marker as u32,
                length,
                token,
                end: -1,
                open: can_open,
                close: can_close,
            };
            state.delimiters_mut().push(delim);
        }
        state.pos += length;
        true
    }

    /// `strikethrough.tokenize`: `~~` pairs (odd runs shed one `~`).
    fn strikethrough_tokenize(state: &mut StateInline, silent: bool) -> bool {
        if silent {
            return false;
        }
        let start = state.pos;
        let ch = state.src[start];
        if ch != '~' {
            return false;
        }
        let (can_open, can_close, scanned) = state.scan_delims(state.pos, true);
        let mut length = scanned;
        if length < 2 {
            return false;
        }
        if length % 2 == 1 {
            let token = state.push("text", "", 0);
            state.tokens[token].content = ch.to_string();
            length -= 1;
        }
        let mut idx = 0;
        while idx < length {
            let token = state.push("text", "", 0);
            state.tokens[token].content = format!("{ch}{ch}");
            let delim = Delimiter {
                marker: ch as u32,
                length: 0,
                token,
                end: -1,
                open: can_open,
                close: can_close,
            };
            state.delimiters_mut().push(delim);
            idx += 2;
        }
        state.pos += scanned;
        true
    }

    /// `processDelimiters`: match openers to closers (the cmark
    /// jump/skip optimization, transliterated straight).
    fn process_delimiters(state: &mut StateInline, list_idx: usize) {
        if state.delim_lists[list_idx].is_empty() {
            return;
        }
        let mut openers_bottom: HashMap<u32, [i64; 6]> = HashMap::new();
        let maximum = state.delim_lists[list_idx].len();
        let mut header_idx = 0;
        let mut last_token_idx: i64 = -2;
        let mut jumps = vec![0i64; maximum];
        let mut closer_idx = 0;
        while closer_idx < maximum {
            // (`closer.length or 0` is dead: lengths are always ints.)
            let (closer_marker, closer_token, closer_length, closer_open, closer_close) = {
                let closer = &state.delim_lists[list_idx][closer_idx];
                (
                    closer.marker,
                    closer.token as i64,
                    closer.length,
                    closer.open,
                    closer.close,
                )
            };
            if state.delim_lists[list_idx][header_idx].marker != closer_marker
                || last_token_idx != closer_token - 1
            {
                header_idx = closer_idx;
            }
            last_token_idx = closer_token;
            if !closer_close {
                closer_idx += 1;
                continue;
            }
            let bottom = openers_bottom.entry(closer_marker).or_insert([-1; 6]);
            let min_opener_idx = bottom[(if closer_open { 3 } else { 0 }) + closer_length % 3];
            let mut opener_idx = header_idx as i64 - jumps[header_idx] - 1;
            let mut new_min_opener_idx = opener_idx;
            while opener_idx > min_opener_idx {
                let opener = state.delim_lists[list_idx][opener_idx as usize].clone();
                if opener.marker != closer_marker {
                    opener_idx -= jumps[opener_idx as usize] + 1;
                    continue;
                }
                if opener.open && opener.end < 0 {
                    let is_odd_match = (opener.close || closer_open)
                        && (opener.length + closer_length).is_multiple_of(3)
                        && (!opener.length.is_multiple_of(3) || !closer_length.is_multiple_of(3));
                    if !is_odd_match {
                        let last_jump = if opener_idx > 0
                            && !state.delim_lists[list_idx][opener_idx as usize - 1].open
                        {
                            jumps[opener_idx as usize - 1] + 1
                        } else {
                            0
                        };
                        jumps[closer_idx] = closer_idx as i64 - opener_idx + last_jump;
                        jumps[opener_idx as usize] = last_jump;
                        state.delim_lists[list_idx][closer_idx].open = false;
                        state.delim_lists[list_idx][opener_idx as usize].end = closer_idx as i64;
                        state.delim_lists[list_idx][opener_idx as usize].close = false;
                        new_min_opener_idx = -1;
                        last_token_idx = -2;
                        break;
                    }
                }
                opener_idx -= jumps[opener_idx as usize] + 1;
            }
            if new_min_opener_idx != -1 {
                // `closer.open` may have changed above — reread it, as
                // Python does (`closer` is the same object there).
                let (open_now, len_now) = {
                    let closer = &state.delim_lists[list_idx][closer_idx];
                    (closer.open, closer.length)
                };
                openers_bottom.entry(closer_marker).or_insert([-1; 6])
                    [(if open_now { 3 } else { 0 }) + len_now % 3] = new_min_opener_idx;
            }
            closer_idx += 1;
        }
    }

    /// `link_pairs` (`balance_pairs`): the top list plus every
    /// link-nested list in `tokens_meta`.
    fn link_pairs(state: &mut StateInline) {
        process_delimiters(state, 0);
        let metas = state.tokens_meta.clone();
        for idx in metas.into_iter().flatten() {
            process_delimiters(state, idx);
        }
    }

    /// `emphasis._postProcess`: matched pairs become `em`/`strong`
    /// (adjacent same-marker pairs merge into `strong`).
    fn emphasis_post_process_list(state: &mut StateInline, list_idx: usize) {
        let mut idx = state.delim_lists[list_idx].len() as i64 - 1;
        while idx >= 0 {
            let start_delim = state.delim_lists[list_idx][idx as usize].clone();
            if start_delim.marker != 0x5F && start_delim.marker != 0x2A {
                idx -= 1;
                continue;
            }
            if start_delim.end == -1 {
                idx -= 1;
                continue;
            }
            let end_delim = state.delim_lists[list_idx][start_delim.end as usize].clone();
            // (Delimiters in one list point at strictly increasing
            // tokens, so `token - 1` can't underflow for `idx > 0`;
            // written addition-first all the same.)
            let is_strong = idx > 0
                && state.delim_lists[list_idx][idx as usize - 1].end == start_delim.end + 1
                && state.delim_lists[list_idx][idx as usize - 1].marker == start_delim.marker
                && state.delim_lists[list_idx][idx as usize - 1].token + 1 == start_delim.token
                && state.delim_lists[list_idx][start_delim.end as usize + 1].token
                    == end_delim.token + 1;
            let ch = char::from_u32(start_delim.marker)
                .unwrap_or('?')
                .to_string();
            let token = start_delim.token;
            state.tokens[token].ttype =
                if is_strong { "strong_open" } else { "em_open" }.to_owned();
            state.tokens[token].tag = if is_strong { "strong" } else { "em" }.to_owned();
            state.tokens[token].nesting = 1;
            state.tokens[token].markup = if is_strong {
                format!("{ch}{ch}")
            } else {
                ch.clone()
            };
            state.tokens[token].content = String::new();
            let token = end_delim.token;
            state.tokens[token].ttype = if is_strong {
                "strong_close"
            } else {
                "em_close"
            }
            .to_owned();
            state.tokens[token].tag = if is_strong { "strong" } else { "em" }.to_owned();
            state.tokens[token].nesting = -1;
            state.tokens[token].markup = if is_strong { format!("{ch}{ch}") } else { ch };
            state.tokens[token].content = String::new();
            if is_strong {
                let prev = state.delim_lists[list_idx][idx as usize - 1].token;
                state.tokens[prev].content = String::new();
                let next = state.delim_lists[list_idx][start_delim.end as usize + 1].token;
                state.tokens[next].content = String::new();
                idx -= 1;
            }
            idx -= 1;
        }
    }

    /// `emphasis.postProcess`.
    fn emphasis_post_process(state: &mut StateInline) {
        emphasis_post_process_list(state, 0);
        let metas = state.tokens_meta.clone();
        for idx in metas.into_iter().flatten() {
            emphasis_post_process_list(state, idx);
        }
    }

    /// `strikethrough._postProcess`: matched pairs become `s` (odd
    /// leading `~` markers shuffle past the `s_close` tags).
    fn strikethrough_post_process_list(state: &mut StateInline, list_idx: usize) {
        let mut lone_markers: Vec<usize> = Vec::new();
        let maximum = state.delim_lists[list_idx].len();
        let mut idx = 0;
        while idx < maximum {
            let start_delim = state.delim_lists[list_idx][idx].clone();
            if start_delim.marker != 0x7E {
                idx += 1;
                continue;
            }
            if start_delim.end == -1 {
                idx += 1;
                continue;
            }
            let end_delim = state.delim_lists[list_idx][start_delim.end as usize].clone();
            let markup = state.tokens[start_delim.token].content.clone();
            let token = start_delim.token;
            state.tokens[token].ttype = "s_open".to_owned();
            state.tokens[token].tag = "s".to_owned();
            state.tokens[token].nesting = 1;
            state.tokens[token].markup = markup.clone();
            state.tokens[token].content = String::new();
            let token = end_delim.token;
            state.tokens[token].ttype = "s_close".to_owned();
            state.tokens[token].tag = "s".to_owned();
            state.tokens[token].nesting = -1;
            state.tokens[token].markup = markup;
            state.tokens[token].content = String::new();
            if state.tokens[end_delim.token - 1].ttype == "text"
                && state.tokens[end_delim.token - 1].content == "~"
            {
                lone_markers.push(end_delim.token - 1);
            }
            idx += 1;
        }
        while let Some(pos) = lone_markers.pop() {
            let mut next = pos + 1;
            while next < state.tokens.len() && state.tokens[next].ttype == "s_close" {
                next += 1;
            }
            next -= 1;
            if pos != next {
                state.tokens.swap(pos, next);
            }
        }
    }

    /// `strikethrough.postProcess`.
    fn strikethrough_post_process(state: &mut StateInline) {
        strikethrough_post_process_list(state, 0);
        let metas = state.tokens_meta.clone();
        for idx in metas.into_iter().flatten() {
            strikethrough_post_process_list(state, idx);
        }
    }

    /// `fragments_join`: re-level every token, merge adjacent text.
    fn fragments_join(state: &mut StateInline) {
        let mut level = 0i64;
        let maximum = state.tokens.len();
        let mut curr = 0;
        let mut last = 0;
        while curr < maximum {
            if state.tokens[curr].nesting < 0 {
                level -= 1;
            }
            state.tokens[curr].level = level;
            if state.tokens[curr].nesting > 0 {
                level += 1;
            }
            if state.tokens[curr].ttype == "text"
                && curr + 1 < maximum
                && state.tokens[curr + 1].ttype == "text"
            {
                let mut parts = vec![state.tokens[curr].content.clone()];
                curr += 1;
                while curr < maximum && state.tokens[curr].ttype == "text" {
                    parts.push(state.tokens[curr].content.clone());
                    curr += 1;
                }
                let merged = state.tokens[curr - 1].clone();
                state.tokens[last] = Token {
                    content: parts.concat(),
                    level,
                    ..merged
                };
                last += 1;
                continue;
            }
            if curr != last {
                let moved = state.tokens[curr].clone();
                state.tokens[last] = moved;
            }
            last += 1;
            curr += 1;
        }
        state.tokens.truncate(last);
    }

    // -- core pipeline (`rules_core/*`, `parser_core.py`) --

    /// `normalize`: `\r\n?` → `\n`, NUL → U+FFFD.
    fn core_normalize(src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        let mut chars = src.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            } else if ch == '\0' {
                out.push('\u{FFFD}');
            } else {
                out.push(ch);
            }
        }
        out
    }

    /// `block` (`ParserBlock.parse`): empty sources yield no tokens.
    fn core_block(src: &str) -> (Vec<Token>, Option<HashMap<String, Ref>>) {
        if src.is_empty() {
            return (Vec::new(), None);
        }
        let mut state = StateBlock::new(src);
        let end = state.line_max;
        block_tokenize(&mut state, 0, end);
        (state.tokens, state.references)
    }

    /// `inline`: every block `inline` token's content is parsed.
    fn core_inline(tokens: &mut [Token], references: &Option<HashMap<String, Ref>>) {
        for token in tokens.iter_mut() {
            if token.ttype == "inline" {
                if token.children.is_none() {
                    token.children = Some(Vec::new());
                }
                if let Some(children) = token.children.as_mut() {
                    inline_parse(&token.content.clone(), references, children);
                }
            }
        }
    }

    /// `text_join`: `text_special` → `text`, adjacent text merged.
    fn core_text_join(tokens: &mut [Token]) {
        for token in tokens.iter_mut() {
            if token.ttype != "inline" {
                continue;
            }
            let children = token.children.take().unwrap_or_default();
            let mut merged: Vec<Token> = Vec::new();
            let mut idx = 0;
            while idx < children.len() {
                let mut child = children[idx].clone();
                if child.ttype == "text_special" {
                    child.ttype = "text".to_owned();
                }
                if child.ttype == "text" && merged.last().is_some_and(|prev| prev.ttype == "text") {
                    let mut parts = vec![merged.last().unwrap().content.clone(), child.content];
                    idx += 1;
                    while idx < children.len() {
                        let mut next = children[idx].clone();
                        if next.ttype == "text_special" {
                            next.ttype = "text".to_owned();
                        }
                        if next.ttype != "text" {
                            break;
                        }
                        parts.push(next.content);
                        idx += 1;
                    }
                    merged.last_mut().unwrap().content = parts.concat();
                } else {
                    merged.push(child);
                    idx += 1;
                }
            }
            token.children = Some(merged);
        }
    }

    // -- tasklists plugin (`mdit_py_plugins/tasklists`, defaults) --

    /// `starts_with_todo_markdown`: `\[[ xX]][ \t\n\v\f\r]+`.
    fn starts_with_todo(content: &str) -> bool {
        let mut chars = content.chars();
        if chars.next() != Some('[') {
            return false;
        }
        match chars.next() {
            Some(' ') | Some('x') | Some('X') => {}
            _ => return false,
        }
        if chars.next() != Some(']') {
            return false;
        }
        let mut count = 0;
        for ch in chars {
            if matches!(ch, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r') {
                count += 1;
            } else {
                break;
            }
        }
        count > 0
    }

    /// `parent_token`: nearest preceding token one level up (`-1`
    /// wraps to the last token, as in Python).
    fn tasklists_parent(tokens: &[Token], index: usize) -> usize {
        let target = tokens[index].level - 1;
        let mut back = 1;
        while back <= index {
            if tokens[index - back].level == target {
                return index - back;
            }
            back += 1;
        }
        tokens.len() - 1
    }

    /// `todoify` + the `github-tasklists` core rule (inserted after
    /// `inline`, before `text_join`).
    fn tasklists_plugin(tokens: &mut [Token]) {
        if tokens.len() < 3 {
            return;
        }
        // `for i in range(2, len(tokens) - 1)`.
        for idx in 2..tokens.len() - 1 {
            let is_todo = tokens[idx].ttype == "inline"
                && tokens[idx - 1].ttype == "paragraph_open"
                && tokens[idx - 2].ttype == "list_item_open"
                && starts_with_todo(&tokens[idx].content.clone());
            if !is_todo {
                continue;
            }
            // `make_checkbox` (defaults: disabled, no labels): only the
            // literal-space spellings arm it; anything else (e.g. a tab)
            // leaves an empty checkbox behind.
            let content = tokens[idx].content.clone();
            let mut checkbox = Token::new("html_inline", "", 0);
            if content.starts_with("[ ] ") {
                checkbox.content =
                    "<input class=\"task-list-item-checkbox\" disabled=\"disabled\" type=\"checkbox\">"
                        .to_owned();
            } else if content.starts_with("[x] ") || content.starts_with("[X] ") {
                checkbox.content = "<input class=\"task-list-item-checkbox\" checked=\"checked\" disabled=\"disabled\" type=\"checkbox\">"
                    .to_owned();
            }
            let children = tokens[idx]
                .children
                .as_mut()
                .expect("inline token without children");
            children.insert(0, checkbox);
            // `children[1].content[3:]` / `content[3:]`: char drops that
            // never panic (Python slicing clamps).
            children[1].content = children[1].content.chars().skip(3).collect();
            tokens[idx].content = tokens[idx].content.chars().skip(3).collect();
            tokens[idx - 2].attr_set("class", AttrVal::Str("task-list-item".to_owned()));
            let parent = tasklists_parent(tokens, idx - 2);
            tokens[parent].attr_set("class", AttrVal::Str("contains-task-list".to_owned()));
        }
    }

    // -- `_resolve_task_lists` (`markdown_converter.py:232-272`) --

    /// What the tasklists plugin injects as the first inline child.
    const TASK_CHECKBOX_MARKER: &str = "class=\"task-list-item-checkbox\"";

    /// `True` when the plugin turned this `list_item_open` into a todo.
    fn is_task_item(tokens: &[Token], item_idx: usize) -> bool {
        if item_idx + 2 >= tokens.len() {
            return false;
        }
        let inline = &tokens[item_idx + 2];
        tokens[item_idx + 1].ttype == "paragraph_open"
            && inline.ttype == "inline"
            && inline
                .children
                .as_ref()
                .is_some_and(|children| !children.is_empty())
            && inline.children.as_ref().unwrap()[0].ttype == "html_inline"
            && inline.children.as_ref().unwrap()[0]
                .content
                .contains(TASK_CHECKBOX_MARKER)
    }

    /// Per-list task resolution: all-todo bullet lists become
    /// `taskList`s, else the markers go back as literal text.
    fn resolve_task_lists(tokens: &mut [Token]) {
        // (`Token`, open-index) per open list; item opens per list.
        let mut list_stack: Vec<(usize, Vec<usize>)> = Vec::new();
        let mut item_stack: Vec<usize> = Vec::new();
        let len = tokens.len();
        for idx in 0..len {
            let ttype = tokens[idx].ttype.clone();
            if ttype == "bullet_list_open" || ttype == "ordered_list_open" {
                list_stack.push((idx, Vec::new()));
            } else if ttype == "bullet_list_close" || ttype == "ordered_list_close" {
                let (open_idx, items) = list_stack.pop().expect("unbalanced lists");
                // Close shares the open's `meta` dict (before flags).
                let shared = tokens[open_idx].meta.clone();
                tokens[idx].meta = shared;
                let flags: Vec<bool> = items
                    .iter()
                    .map(|&item| is_task_item(tokens, item))
                    .collect();
                let as_task_list = tokens[open_idx].ttype == "bullet_list_open"
                    && !items.is_empty()
                    && flags.iter().all(|&flag| flag);
                tokens[open_idx].meta.borrow_mut().task_list = as_task_list;
                for (item_idx, is_task) in items.iter().zip(flags.iter()) {
                    if !is_task {
                        continue;
                    }
                    let inline_idx = item_idx + 2;
                    let checkbox = tokens[inline_idx]
                        .children
                        .as_mut()
                        .expect("task item without children")
                        .remove(0);
                    let checked = checkbox.content.contains("checked=\"checked\"");
                    if as_task_list {
                        tokens[*item_idx].meta.borrow_mut().task_item = true;
                        tokens[*item_idx].meta.borrow_mut().checked = checked;
                        // The plugin strips `[ ]` but leaves the space.
                        if let Some(children) = tokens[inline_idx].children.as_mut() {
                            if children.first().is_some_and(|child| child.ttype == "text") {
                                children[0].content =
                                    py_lstrip(&children[0].content.clone()).to_owned();
                            }
                        }
                    } else {
                        let mut marker = Token::new("text", "", 0);
                        marker.content = if checked { "[x]" } else { "[ ]" }.to_owned();
                        tokens[inline_idx]
                            .children
                            .as_mut()
                            .expect("task item without children")
                            .insert(0, marker);
                    }
                }
            } else if ttype == "list_item_open" {
                if let Some((_, items)) = list_stack.last_mut() {
                    items.push(idx);
                }
                item_stack.push(idx);
            } else if ttype == "list_item_close" {
                let open_idx = item_stack.pop().expect("unbalanced items");
                let shared = tokens[open_idx].meta.clone();
                tokens[idx].meta = shared;
            }
        }
    }

    // -- Tiptap renderer (`markdown_converter.py:275-469`) --

    /// `SAFE_PROTOCOLS` (`content_validator.py:159`).
    fn is_safe_scheme(scheme: &str) -> bool {
        matches!(scheme, "http" | "https" | "mailto" | "tel")
    }

    /// `_is_safe_url`: relative URLs and the allowed schemes only.
    ///
    /// Every input here is `mdurl.encode` output, which percent-encodes
    /// brackets, whitespace and all non-ASCII — so `urlsplit` can never
    /// raise on them (no bracket/normalization failures) and the strip
    /// steps are no-ops. Only the scheme extraction is live: leading
    /// C0/space strip, `\t\r\n` removal, `scheme:` before the first
    /// colon with an ASCII-alpha start and `scheme_chars` body.
    fn is_safe_url(url: &str) -> bool {
        let cleaned: String = url
            .trim_start_matches(|ch: char| (ch as u32) <= 0x20)
            .chars()
            .filter(|ch| !matches!(ch, '\t' | '\r' | '\n'))
            .collect();
        let Some(colon) = cleaned.find(':') else {
            return true;
        };
        if colon == 0 {
            return true;
        }
        let mut chars = cleaned.chars();
        let first = chars.next().unwrap_or('\0');
        if !first.is_ascii() || !first.is_ascii_alphabetic() {
            return true;
        }
        for ch in cleaned[..colon].chars() {
            if !(ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.') {
                return true;
            }
        }
        is_safe_scheme(&cleaned[..colon].to_lowercase())
    }

    /// `renderInlineAsText`: text verbatim, nested image alts, `\n` for
    /// soft breaks — `text_special` (escapes in image alts) is dropped.
    fn render_inline_as_text(tokens: &[Token]) -> String {
        let mut out = String::new();
        for token in tokens {
            if token.ttype == "text" {
                out.push_str(&token.content);
            } else if token.ttype == "image" {
                if let Some(children) = token.children.as_ref() {
                    out.push_str(&render_inline_as_text(children));
                }
            } else if token.ttype == "softbreak" {
                out.push('\n');
            }
        }
        out
    }

    /// `renderAttrs` (only `code_inline` — always attr-less — uses it).
    fn render_attrs(token: &Token) -> String {
        let mut out = String::new();
        for (key, value) in &token.attrs {
            out.push(' ');
            out.push_str(&escape_html(key));
            out.push_str("=\"");
            out.push_str(&escape_html(&value.py_str()));
            out.push('"');
        }
        out
    }

    /// `TiptapHTMLRenderer._lone_image`: a paragraph whose only child
    /// is a safe-src image renders bare (Tiptap images are blocks).
    fn is_lone_image(tokens: &[Token], inline_idx: usize) -> bool {
        let inline = &tokens[inline_idx];
        if !(inline.ttype == "inline"
            && inline.level == 1
            && inline
                .children
                .as_ref()
                .is_some_and(|children| children.len() == 1)
            && inline.children.as_ref().unwrap()[0].ttype == "image")
        {
            return false;
        }
        let src = inline.children.as_ref().unwrap()[0]
            .attr_get("src")
            .map(|value| value.py_str())
            .unwrap_or_default();
        is_safe_url(&src)
    }

    /// `code_inline` (inherited from `RendererHTML`, not overridden).
    fn render_code_inline(tokens: &[Token], idx: usize) -> String {
        let token = &tokens[idx];
        format!(
            "<code{}>{}</code>",
            render_attrs(token),
            escape_html(&token.content)
        )
    }

    /// `TiptapHTMLRenderer.renderToken`: hidden or tagless renders
    /// nothing, else a bare open/close tag (attrs dropped).
    fn render_token(tokens: &[Token], idx: usize) -> String {
        let token = &tokens[idx];
        if token.hidden || token.tag.is_empty() {
            return String::new();
        }
        if token.nesting == -1 {
            format!("</{}>", token.tag)
        } else {
            format!("<{}>", token.tag)
        }
    }

    /// One block token (`render` loop body; `inline` renders children).
    fn render_block_token(tokens: &[Token], idx: usize) -> String {
        let token = &tokens[idx];
        if token.ttype == "inline" {
            if let Some(children) = token.children.as_ref() {
                return render_inline_tokens(children);
            }
            return String::new();
        }
        match token.ttype.as_str() {
            "paragraph_open" => {
                if token.level == 0 && idx + 1 < tokens.len() && is_lone_image(tokens, idx + 1) {
                    String::new()
                } else {
                    "<p>".to_owned()
                }
            }
            "paragraph_close" => {
                if token.level == 0 && idx >= 1 && is_lone_image(tokens, idx - 1) {
                    String::new()
                } else {
                    "</p>".to_owned()
                }
            }
            "hr" => "<div data-type=\"horizontalRule\"><div></div></div>".to_owned(),
            "bullet_list_open" => {
                if token.meta.borrow().task_list {
                    "<ul data-type=\"taskList\">".to_owned()
                } else {
                    "<ul>".to_owned()
                }
            }
            "ordered_list_open" => {
                let start = token.attr_get("start").map(|value| value.py_str());
                match start {
                    Some(start) if start != "1" => {
                        format!("<ol start=\"{}\">", escape_html(&start))
                    }
                    _ => "<ol>".to_owned(),
                }
            }
            "list_item_open" => {
                let meta = token.meta.borrow();
                if !meta.task_item {
                    "<li>".to_owned()
                } else {
                    let checkbox = if meta.checked {
                        "<input type=\"checkbox\" checked=\"checked\">"
                    } else {
                        "<input type=\"checkbox\">"
                    };
                    format!(
                        "<li data-type=\"taskItem\" data-checked=\"{}\"><label>{checkbox}<span></span></label><div>",
                        if meta.checked { "true" } else { "false" }
                    )
                }
            }
            "list_item_close" => {
                if token.meta.borrow().task_item {
                    "</div></li>".to_owned()
                } else {
                    "</li>".to_owned()
                }
            }
            "fence" => {
                let stripped = py_strip(&token.info);
                let language = stripped
                    .split(|ch: char| is_strip_char(ch))
                    .find(|piece| !piece.is_empty())
                    .unwrap_or("");
                let body = escape_html(token.content.strip_suffix('\n').unwrap_or(&token.content));
                if language.is_empty() {
                    format!("<pre><code>{body}</code></pre>")
                } else {
                    format!(
                        "<pre><code class=\"language-{}\">{body}</code></pre>",
                        escape_html(language)
                    )
                }
            }
            "code_block" => format!(
                "<pre><code>{}</code></pre>",
                escape_html(token.content.strip_suffix('\n').unwrap_or(&token.content))
            ),
            "table_open" => "<table><tbody>".to_owned(),
            "table_close" => "</tbody></table>".to_owned(),
            "thead_open" | "thead_close" | "tbody_open" | "tbody_close" => String::new(),
            "th_open" => "<th><p>".to_owned(),
            "th_close" => "</p></th>".to_owned(),
            "td_open" => "<td><p>".to_owned(),
            "td_close" => "</p></td>".to_owned(),
            "html_block" => format!("<p>{}</p>", escape_html(py_strip(&token.content))),
            _ => render_token(tokens, idx),
        }
    }

    /// One inline token (`renderInline` loop body).
    fn render_inline_token(tokens: &[Token], idx: usize) -> String {
        let token = &tokens[idx];
        match token.ttype.as_str() {
            "text" => escape_html(&token.content),
            "softbreak" => " ".to_owned(),
            "hardbreak" => "<br>".to_owned(),
            "html_inline" => escape_html(&token.content),
            "code_inline" => render_code_inline(tokens, idx),
            "link_open" => {
                let href = token
                    .attr_get("href")
                    .map(|value| value.py_str())
                    .unwrap_or_default();
                let mut attrs = String::new();
                if is_safe_url(&href) {
                    attrs.push_str(&format!(" href=\"{}\"", escape_html(&href)));
                }
                if let Some(title) = token.attr_get("title").map(|value| value.py_str()) {
                    if !title.is_empty() {
                        attrs.push_str(&format!(" title=\"{}\"", escape_html(&title)));
                    }
                }
                format!("<a{attrs}>")
            }
            "image" => {
                let src = token
                    .attr_get("src")
                    .map(|value| value.py_str())
                    .unwrap_or_default();
                let alt = render_inline_as_text(token.children.as_deref().unwrap_or(&[]));
                if !is_safe_url(&src) || src.is_empty() {
                    return escape_html(&alt);
                }
                let mut attrs = format!(" src=\"{}\"", escape_html(&src));
                if !alt.is_empty() {
                    attrs.push_str(&format!(" alt=\"{}\"", escape_html(&alt)));
                }
                if let Some(title) = token.attr_get("title").map(|value| value.py_str()) {
                    if !title.is_empty() {
                        attrs.push_str(&format!(" title=\"{}\"", escape_html(&title)));
                    }
                }
                format!("<img{attrs}>")
            }
            _ => render_token(tokens, idx),
        }
    }

    /// `renderInline`.
    fn render_inline_tokens(tokens: &[Token]) -> String {
        let mut out = String::new();
        for idx in 0..tokens.len() {
            out.push_str(&render_inline_token(tokens, idx));
        }
        out
    }

    /// `MarkdownIt.parse` for this configuration: normalize, block,
    /// inline, the tasklists plugin, `text_join`.
    fn parse_markdown(src: &str) -> Vec<Token> {
        let normalized = core_normalize(src);
        let (mut tokens, references) = core_block(&normalized);
        core_inline(&mut tokens, &references);
        tasklists_plugin(&mut tokens);
        core_text_join(&mut tokens);
        tokens
    }

    /// `markdown_to_html` (`markdown_converter.py:444-469`): empty
    /// input renders the empty document; the sanitizer's failures
    /// surface as its `ValueError` text.
    pub fn to_html(markdown: &str) -> Result<String, String> {
        use crate::space::sanitize::{sanitize_html, Sanitize, MAX_HTML_BYTES};
        if markdown.is_empty() || py_strip(markdown).is_empty() {
            return Ok("<p></p>".to_owned());
        }
        let mut tokens = parse_markdown(markdown);
        for token in tokens.iter_mut() {
            // Tiptap list items and table cells always wrap text in a
            // paragraph, so tight-list paragraphs render too.
            if token.ttype == "paragraph_open" || token.ttype == "paragraph_close" {
                token.hidden = false;
            }
        }
        resolve_task_lists(&mut tokens);
        let mut html = String::new();
        for idx in 0..tokens.len() {
            html.push_str(&render_block_token(&tokens, idx));
        }
        if html.is_empty() {
            return Ok("<p></p>".to_owned());
        }
        // `validate_html_content`: the size arm first (its message),
        // then the cleaner (its failure message).
        if html.len() > MAX_HTML_BYTES {
            return Err("HTML content exceeds maximum size limit (10MB)".to_owned());
        }
        match sanitize_html(&html) {
            Sanitize::Clean(cleaned) => {
                if cleaned.is_empty() {
                    Ok("<p></p>".to_owned())
                } else {
                    Ok(cleaned)
                }
            }
            Sanitize::Invalid => Err("Failed to sanitize HTML".to_owned()),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (fx-h-intake replay: curated batteries + differential fuzz hooks)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const MARKDOWN_GOLDENS: &[(&str, &str)] = &[
        ("# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6", "<h1>H1</h1><h2>H2</h2><h3>H3</h3><h4>H4</h4><h5>H5</h5><h6>H6</h6>"),
        ("one\n\ntwo", "<p>one</p><p>two</p>"),
        ("one\ntwo", "<p>one two</p>"),
        ("one  \ntwo", "<p>one<br>two</p>"),
        ("one\\\ntwo", "<p>one<br>two</p>"),
        ("a\n\n---\n\nb", "<p>a</p><div data-type=\"horizontalRule\"><div></div></div><p>b</p>"),
        ("> quoted", "<blockquote><p>quoted</p></blockquote>"),
        ("**b** *i* ~~s~~ `c`", "<p><strong>b</strong> <em>i</em> <s>s</s> <code>c</code></p>"),
        ("[a link](https://example.com/x)", "<p><a href=\"https://example.com/x\" rel=\"noopener noreferrer\">a link</a></p>"),
        ("[bad](javascript:alert(1))", "<p>[bad](javascript:alert(1))</p>"),
        ("[f](ftp://example.com/x)", "<p><a rel=\"noopener noreferrer\">f</a></p>"),
        ("![a cat](https://cdn.example/x.png)", "<img src=\"https://cdn.example/x.png\" alt=\"a cat\">"),
        ("see ![a cat](https://cdn.example/x.png) here", "<p>see <img src=\"https://cdn.example/x.png\" alt=\"a cat\"> here</p>"),
        ("![a cat](data:image/png;base64,AAAA)", "<p>a cat</p>"),
        ("- one\n- two", "<ul><li><p>one</p></li><li><p>two</p></li></ul>"),
        ("- one\n  - nested\n- two", "<ul><li><p>one</p><ul><li><p>nested</p></li></ul></li><li><p>two</p></li></ul>"),
        ("1. a\n2. b", "<ol><li><p>a</p></li><li><p>b</p></li></ol>"),
        ("3. a\n4. b", "<ol start=\"3\"><li><p>a</p></li><li><p>b</p></li></ol>"),
        ("- one\n  1. first\n  2. second", "<ul><li><p>one</p><ol><li><p>first</p></li><li><p>second</p></li></ol></li></ul>"),
        ("- [ ] todo\n- [x] done", "<ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"false\"><label><input type=\"checkbox\"><span></span></label><div><p>todo</p></div></li><li data-type=\"taskItem\" data-checked=\"true\"><label><input type=\"checkbox\" checked=\"checked\"><span></span></label><div><p>done</p></div></li></ul>"),
        ("- [X] done", "<ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"true\"><label><input type=\"checkbox\" checked=\"checked\"><span></span></label><div><p>done</p></div></li></ul>"),
        ("- [x] parent\n  - [ ] child", "<ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"true\"><label><input type=\"checkbox\" checked=\"checked\"><span></span></label><div><p>parent</p><ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"false\"><label><input type=\"checkbox\"><span></span></label><div><p>child</p></div></li></ul></div></li></ul>"),
        ("- [ ] **bold** rest", "<ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"false\"><label><input type=\"checkbox\"><span></span></label><div><p><strong>bold</strong> rest</p></div></li></ul>"),
        ("- [ ] todo\n- plain", "<ul><li><p>[ ] todo</p></li><li><p>plain</p></li></ul>"),
        ("1. [x] done", "<ol><li><p>[x] done</p></li></ol>"),
        ("```python\ndef f():\n    return 1\n```", "<pre><code class=\"language-python\">def f():\n    return 1</code></pre>"),
        ("```\nplain\n```", "<pre><code>plain</code></pre>"),
        ("    indented", "<pre><code>indented</code></pre>"),
        ("```html\n<script>alert(1)</script>\n```", "<pre><code class=\"language-html\">&lt;script&gt;alert(1)&lt;/script&gt;</code></pre>"),
        ("`<b>`", "<p><code>&lt;b&gt;</code></p>"),
        ("| A | B |\n| --- | :-: |\n| 1 | 2 |", "<table><tbody><tr><th><p>A</p></th><th><p>B</p></th></tr><tr><td><p>1</p></td><td><p>2</p></td></tr></tbody></table>"),
        ("| A |\n| --- |", "<table><tbody><tr><th><p>A</p></th></tr></tbody></table>"),
        ("| A |\n| --- |\n| **x** |", "<table><tbody><tr><th><p>A</p></th></tr><tr><td><p><strong>x</strong></p></td></tr></tbody></table>"),
        ("<script>alert(1)</script>", "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>"),
        ("text <b>x</b>", "<p>text &lt;b&gt;x&lt;/b&gt;</p>"),
        ("a & b", "<p>a &amp; b</p>"),
        ("", "<p></p>"),
        ("   ", "<p></p>"),
        ("\n\n", "<p></p>"),
        ("![x](javascript:alert(1))", "<p>![x](javascript:alert(1))</p>"),
        ("![a\\*b](x)", "<img src=\"x\" alt=\"ab\">"),
        ("![](x)", "<img src=\"x\">"),
        ("- [ ]\ttodo", "<ul><li><p>\ttodo</p></li></ul>"),
        ("[x](http://[::1]/)", "<p><a rel=\"noopener noreferrer\">x</a></p>"),
        ("# Cold start\n\n- parent\n  - child\n\nChecklist:\n\n- [ ] fixtures\n- [x] prerequisites\n\n```rust\nfn main() {}\n```\n\n| page | updated_at |\n| --- | --- |\n| wiki-1 | 2026-09-21 |\n", "<h1>Cold start</h1><ul><li><p>parent</p><ul><li><p>child</p></li></ul></li></ul><p>Checklist:</p><ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"false\"><label><input type=\"checkbox\"><span></span></label><div><p>fixtures</p></div></li><li data-type=\"taskItem\" data-checked=\"true\"><label><input type=\"checkbox\" checked=\"checked\"><span></span></label><div><p>prerequisites</p></div></li></ul><pre><code class=\"language-rust\">fn main() {}</code></pre><table><tbody><tr><th><p>page</p></th><th><p>updated_at</p></th></tr><tr><td><p>wiki-1</p></td><td><p>2026-09-21</p></td></tr></tbody></table>"),
    ];

    const DT_GOLDENS: &[(&str, Option<&str>)] = &[
        ("2030-01-01", Some("naive:2030-01-01T00:00:00")),
        (
            "2030-01-01T00:00:00Z",
            Some("aware:2030-01-01T00:00:00+00:00"),
        ),
        (
            "2030-01-01T00:00:00+00:00",
            Some("aware:2030-01-01T00:00:00+00:00"),
        ),
        ("2030-01-01 00:00:00", Some("naive:2030-01-01T00:00:00")),
        ("2030-01-01T00:00:00", Some("naive:2030-01-01T00:00:00")),
        ("2030-01-01T00:00", Some("naive:2030-01-01T00:00:00")),
        (
            "2030-01-01T00:00:00.123456789Z",
            Some("aware:2030-01-01T00:00:00.123456+00:00"),
        ),
        (
            "2030-01-01T00:00:00,5Z",
            Some("aware:2030-01-01T00:00:00.500000+00:00"),
        ),
        ("2030-01-01T25:00:00Z", Some("RAISES")),
        ("2030-13-01", None),
        ("2030-01-32", None),
        ("2030-02-30", None),
        ("2030-1-1", None),
        ("30-01-01", None),
        (
            "2030-01-01T00:00:00+0530",
            Some("aware:2029-12-31T18:30:00+00:00"),
        ),
        (
            "2030-01-01T00:00:00+05:30",
            Some("aware:2029-12-31T18:30:00+00:00"),
        ),
        (
            "2030-01-01T00:00:00-08:00",
            Some("aware:2030-01-01T08:00:00+00:00"),
        ),
        ("2030-01-01T00:00:00+24:00", Some("RAISES")),
        (
            "2030-01-01T00:00:00+00:60",
            Some("aware:2029-12-31T23:00:00+00:00"),
        ),
        ("  2030-01-01  ", None),
        ("2030-01-01T00:00:00Z  ", None),
        ("2030-01-01t00:00:00z", None),
        (
            "2030-01-01T00:00:00.123Z",
            Some("aware:2030-01-01T00:00:00.123000+00:00"),
        ),
        ("Thu, 01 Jan 2030 00:00:00 GMT", None),
        (
            "2030-01-01 00:00:00+00:00",
            Some("aware:2030-01-01T00:00:00+00:00"),
        ),
        ("2024-02-29", Some("naive:2024-02-29T00:00:00")),
        ("2023-02-29", None),
        (
            "0001-01-01T00:00:00Z",
            Some("aware:0001-01-01T00:00:00+00:00"),
        ),
        (
            "9999-12-31T23:59:59Z",
            Some("aware:9999-12-31T23:59:59+00:00"),
        ),
        ("2030-01-01T24:00:00", Some("RAISES")),
        ("2030-01-01T00:60:00", Some("RAISES")),
        ("2030-W01-1", Some("naive:2029-12-31T00:00:00")),
        ("2024-001", None),
        ("", None),
        ("zzz", None),
        ("2030-01-01T", None),
        ("T00:00:00", None),
        ("00:00:00", None),
        ("20300101", Some("naive:2030-01-01T00:00:00")),
        ("2030/01/01", None),
        (
            "2030-01-01T00:00:00.123456+05:30",
            Some("aware:2029-12-31T18:30:00.123456+00:00"),
        ),
        (
            "2030-01-01T00:00:00,123456Z",
            Some("aware:2030-01-01T00:00:00.123456+00:00"),
        ),
        (
            "2030-01-01 00:00:00.5",
            Some("naive:2030-01-01T00:00:00.500000"),
        ),
        (
            "2030-01-01T00:00:00+14:00",
            Some("aware:2029-12-31T10:00:00+00:00"),
        ),
        (
            "2030-01-01T00:00:00-14:00",
            Some("aware:2030-01-01T14:00:00+00:00"),
        ),
        (
            "2030-01-01T00:00:00+23:59",
            Some("aware:2029-12-31T00:01:00+00:00"),
        ),
        (
            "1970-01-01T00:00:00Z",
            Some("aware:1970-01-01T00:00:00+00:00"),
        ),
        (
            "1969-12-31T23:59:59Z",
            Some("aware:1969-12-31T23:59:59+00:00"),
        ),
        (
            "2030-06-15T12:30:45.000001Z",
            Some("aware:2030-06-15T12:30:45.000001+00:00"),
        ),
        ("2030-06-15 12:30", Some("naive:2030-06-15T12:30:00")),
    ];
    const DATE_GOLDENS: &[(&str, Option<&str>)] = &[
        ("2024-01-02", Some("2024-01-02")),
        ("2024-1-2", Some("2024-01-02")),
        ("2024-13-01", Some("RAISES")),
        ("2024-01-32", Some("RAISES")),
        ("24-01-02", None),
        (" 2024-01-02 ", None),
        ("2024-01-02T00:00:00Z", None),
        ("2024-02-29", Some("2024-02-29")),
        ("2023-02-29", Some("RAISES")),
        ("", None),
        ("zzz", None),
        ("20240102", Some("2024-01-02")),
        ("2030-W01-1", Some("2029-12-31")),
        ("0001-01-01", Some("0001-01-01")),
        ("9999-12-31", Some("9999-12-31")),
    ];

    fn utc() -> Tz {
        "UTC".parse().expect("tz")
    }

    fn json_map(pairs: &[(&str, Value)]) -> Map<String, Value> {
        let mut map = Map::with_capacity(pairs.len());
        for (key, value) in pairs {
            map.insert((*key).to_owned(), value.clone());
        }
        map
    }

    fn fail_message(result: FieldResult<impl std::fmt::Debug>) -> String {
        match result {
            Err(FieldFail::Msg(message)) => message,
            Err(FieldFail::Server) => panic!("unexpected server failure"),
            Ok(value) => panic!("expected failure, got {value:?}"),
        }
    }

    // -- routes (fx-h-intake `routes`: `api/urls/intake.py:14-23`) --

    #[test]
    fn collection_and_detail_paths_match_the_router() {
        assert_eq!(
            INTAKE_ISSUES_PATH,
            "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/"
        );
        assert_eq!(
            INTAKE_ISSUE_PATH,
            "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{issue_id}/"
        );
    }

    // -- markdown (`markdown_converter.markdown_to_html`, live goldens) --

    #[test]
    fn markdown_matches_live_python_on_curated_inputs() {
        for (input, expected) in MARKDOWN_GOLDENS {
            let actual = markdown::to_html(input).expect("to_html failed");
            assert_eq!(actual, *expected, "input: {input:?}");
        }
    }

    #[test]
    fn markdown_size_cap_reports_the_sanitizer_message() {
        // 10MB of paragraph text: `validate_html_content` rejects the
        // *output* (`<p>` + body + `</p>`), and `markdown_to_html`
        // surfaces the sanitizer's `ValueError` text.
        let big = "x".repeat(10 * 1024 * 1024);
        assert_eq!(
            markdown::to_html(&big),
            Err("HTML content exceeds maximum size limit (10MB)".to_owned())
        );
        // Just under the cap converts fine.
        let small = "y".repeat(1024);
        assert!(markdown::to_html(&small).unwrap().starts_with("<p>"));
    }

    #[test]
    fn markdown_output_is_sanitize_stable() {
        // `normalize` + `validate` re-sanitize markdown-born HTML that
        // `markdown_to_html` already sanitized; the flow is only sound
        // when the sanitizer is idempotent on its own output.
        for (input, _) in MARKDOWN_GOLDENS {
            let html = markdown::to_html(input).expect("to_html failed");
            match sanitize_html(&html) {
                Sanitize::Clean(cleaned) => assert_eq!(cleaned, html, "input: {input:?}"),
                Sanitize::Invalid => panic!("golden output fails sanitize: {input:?}"),
            }
        }
    }

    // -- dates (`snoozed_till` / start/target gates, live goldens) --

    /// Render a `ParsedDateTime` the way the golden generator encodes
    /// Django's result (`naive:`/`aware:` + ISO, micros trimmed).
    fn canon_parsed(parsed: &ParsedDateTime) -> String {
        match parsed {
            ParsedDateTime::Naive(naive) => {
                format!("naive:{}", naive.format("%Y-%m-%dT%H:%M:%S%.f"))
            }
            ParsedDateTime::Aware(instant) => {
                // Python ISO of a UTC instant ends `+00:00`.
                let base = instant.format("%Y-%m-%dT%H:%M:%S%.f").to_string();
                format!("aware:{base}+00:00")
            }
        }
    }

    /// chrono `%.f` and Python `isoformat` spell fractions
    /// differently (fixed-width vs trimmed) — pad to nanos, trim
    /// zeros, compare. Full precision: a nanos-keeping parser fails
    /// against Django's microsecond truncation.
    fn canon_iso(text: &str) -> String {
        match text.split_once('.') {
            Some((head, tail)) => {
                let (frac, zone) = match tail.find('+') {
                    Some(idx) => (&tail[..idx], &tail[idx..]),
                    None => (tail, ""),
                };
                let padded = format!("{frac:0<9}");
                let frac: String = padded.chars().take(9).collect();
                let trimmed = frac.trim_end_matches('0');
                if trimmed.is_empty() {
                    format!("{head}{zone}")
                } else {
                    format!("{head}.{trimmed}{zone}")
                }
            }
            None => text.to_owned(),
        }
    }

    #[test]
    fn django_datetime_parse_matches_live_python() {
        for (input, expected) in DT_GOLDENS {
            let actual =
                parse_django_datetime(input).map(|parsed| canon_iso(&canon_parsed(&parsed)));
            // Django *raises* on well-formed-but-invalid values; DRF
            // suppresses that into the same 400 as `None`.
            let expected = match *expected {
                Some("RAISES") | None => None,
                Some(text) => Some(canon_iso(text)),
            };
            assert_eq!(actual, expected, "input: {input:?}");
        }
    }

    #[test]
    fn django_date_parse_matches_live_python() {
        for (input, expected) in DATE_GOLDENS {
            let actual = parse_django_date(input).map(|date| date.format("%Y-%m-%d").to_string());
            let expected = match *expected {
                Some("RAISES") | None => None,
                Some(text) => Some(text.to_owned()),
            };
            assert_eq!(actual, expected, "input: {input:?}");
        }
    }

    #[test]
    fn datetime_trailing_whitespace_follows_the_dollar_quirk() {
        // `datetime_re` ends `\s*(tz)?$` (`$` = end or before one
        // trailing `\n`): trailing space is fine *without* a tz, fatal
        // *with* one; a lone trailing newline is always dead weight.
        let timezone = utc();
        let ok = |input: &str| {
            check_datetime(&Value::String(input.to_owned()), &timezone)
                .map(|instant| canon_instant(&instant))
        };
        assert!(ok("2030-01-01T00:00:00  ").is_ok());
        assert!(ok("2030-01-01T00:00:00\n").is_ok());
        assert!(ok("2030-01-01T00:00:00\n\n").is_ok());
        assert!(ok("2030-01-01T00:00:00 \n").is_ok());
        assert!(ok("2030-01-01T00:00:00\t").is_ok());
        assert!(ok("2030-01-01T00:00:00\x0b").is_ok());
        assert!(ok("2030-01-01T00:00:00\r\n").is_ok());
        assert!(ok("2030-01-01T00:00:00 Z").is_ok());
        assert!(ok("2030-01-01T00:00:00  Z").is_ok());
        assert!(ok("2030-01-01T00:00:00\tZ").is_ok());
        assert!(ok("2030-01-01T00:00:00Z\n").is_ok());
        assert_eq!(
            ok("2030-01-01T00:00:00 Z").expect("aware"),
            "2030-01-01T00:00:00+00:00"
        );
        for bad in [
            "2030-01-01T00:00:00 Z ",
            "2030-01-01T00:00:00+00:00 ",
            "2030-01-01T00:00:00Z  ",
            "2030-01-01T00:00:00Z\r\n",
            " 2030-01-01T00:00:00",
        ] {
            assert!(ok(bad).is_err(), "input: {bad:?}");
        }
    }

    #[test]
    fn datetime_failures_carry_the_drf_message() {
        let timezone = utc();
        for input in [
            "zzz",
            "",
            "2030-13-01",
            "2030-01-01T25:00:00Z",
            "2030-01-01T24:00:00",
        ] {
            assert_eq!(
                fail_message(check_datetime(&Value::String(input.to_owned()), &timezone)),
                "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].",
                "input: {input:?}"
            );
        }
        assert_eq!(
            fail_message(check_datetime(&Value::Number(1.into()), &timezone)),
            "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
        );
        assert_eq!(
            fail_message(check_date(&Value::String("zzz".to_owned()))),
            "Date has wrong format. Use one of these formats instead: YYYY-MM-DD."
        );
    }

    // -- DRF scalars (probed against DRF 3.15.2) --

    #[test]
    fn drf_bool_sets_and_coercions() {
        assert_eq!(parse_drf_bool(&Value::Bool(true)), Ok(true));
        assert_eq!(parse_drf_bool(&Value::Bool(false)), Ok(false));
        assert_eq!(parse_drf_bool(&Value::String("TRUE".to_owned())), Ok(true));
        assert_eq!(parse_drf_bool(&Value::String("off".to_owned())), Ok(false));
        assert_eq!(parse_drf_bool(&Value::String("1".to_owned())), Ok(true));
        assert_eq!(parse_drf_bool(&Value::String("0".to_owned())), Ok(false));
        assert_eq!(parse_drf_bool(&Value::from(1)), Ok(true));
        assert_eq!(parse_drf_bool(&Value::from(0)), Ok(false));
        assert_eq!(parse_drf_bool(&Value::from(1.0)), Ok(true));
        assert_eq!(parse_drf_bool(&Value::from(0.0)), Ok(false));
        assert_eq!(
            fail_message(parse_drf_bool(&Value::String("x".to_owned()))),
            "Must be a valid boolean."
        );
        assert_eq!(
            fail_message(parse_drf_bool(&Value::from(2))),
            "Must be a valid boolean."
        );
        assert_eq!(
            fail_message(parse_drf_bool(&Value::from(1.5))),
            "Must be a valid boolean."
        );
    }

    #[test]
    fn drf_int_dot_zero_and_underscores() {
        assert_eq!(parse_drf_int(&Value::from(5)), Ok(5));
        assert_eq!(parse_drf_int(&Value::String("5".to_owned())), Ok(5));
        assert_eq!(parse_drf_int(&Value::String("5.0".to_owned())), Ok(5));
        assert_eq!(parse_drf_int(&Value::from(5.0)), Ok(5));
        assert_eq!(parse_drf_int(&Value::String("  7  ".to_owned())), Ok(7));
        assert_eq!(parse_drf_int(&Value::String("1_0".to_owned())), Ok(10));
        assert_eq!(
            fail_message(parse_drf_int(&Value::String("5.5".to_owned()))),
            "A valid integer is required."
        );
        assert_eq!(
            fail_message(parse_drf_int(&Value::Bool(true))),
            "A valid integer is required."
        );
        assert_eq!(
            fail_message(parse_drf_int(&Value::String("x".to_owned()))),
            "A valid integer is required."
        );
        assert_eq!(
            fail_message(parse_drf_int(&Value::String("x".repeat(1001)))),
            "String value too large."
        );
    }

    #[test]
    fn drf_float_specials_and_underscores() {
        assert_eq!(parse_drf_float(&Value::Bool(true)), Ok(1.0));
        assert_eq!(parse_drf_float(&Value::Bool(false)), Ok(0.0));
        assert_eq!(
            parse_drf_float(&Value::String("inf".to_owned())),
            Ok(f64::INFINITY)
        );
        assert_eq!(
            parse_drf_float(&Value::String("-Infinity".to_owned())),
            Ok(f64::NEG_INFINITY)
        );
        assert!(parse_drf_float(&Value::String("nan".to_owned()))
            .unwrap()
            .is_nan());
        assert_eq!(
            parse_drf_float(&Value::String("1_0.5".to_owned())),
            Ok(10.5)
        );
        assert_eq!(
            fail_message(parse_drf_float(&Value::String("x".to_owned()))),
            "A valid number is required."
        );
        assert_eq!(
            fail_message(parse_drf_float(&Value::Null)),
            "A valid number is required."
        );
    }

    #[test]
    fn choice_lookup_uses_python_str() {
        // `"1"` coerces to `1`; `1.0`/`True` fail with their echo.
        assert_eq!(check_status(&Value::String("1".to_owned())), Ok(1));
        assert_eq!(check_status(&Value::from(1)), Ok(1));
        assert_eq!(
            fail_message(check_status(&Value::from(1.0))),
            "\"1.0\" is not a valid choice."
        );
        assert_eq!(
            fail_message(check_status(&Value::Bool(true))),
            "\"True\" is not a valid choice."
        );
        assert_eq!(
            fail_message(check_status(&Value::from(7))),
            "\"7\" is not a valid choice."
        );
    }

    #[test]
    fn uuid_pk_errors_echo_python_style() {
        // Bools are a type error; bad strings carry Django's UUID
        // message; ints go through `UUID(int=...)` then miss.
        assert_eq!(
            fail_message(parse_uuid_pk(&Value::Bool(true))),
            "Incorrect type. Expected pk value, received bool."
        );
        let message = fail_message(parse_uuid_pk(&Value::String("zzz".to_owned())));
        assert!(message.contains("is not a valid UUID"), "{message}");
        assert!(message.contains("zzz"), "{message}");
        let (uuid, echo) = parse_uuid_pk(&Value::from(1)).expect("int pk parses");
        assert_eq!(uuid, Uuid::from_u128(1));
        assert_eq!(echo, "1");
    }

    // -- descriptions (`normalize_description_input` + gates) --

    #[test]
    fn normalize_precedence_markdown_over_html_over_legacy() {
        // Markdown wins over both; legacy converts only when no HTML
        // was sent; otherwise the input passes through untouched.
        let data = json_map(&[
            (
                "description_markdown",
                Value::String("# From markdown".to_owned()),
            ),
            (
                "description_html",
                Value::String("<p>from html</p>".to_owned()),
            ),
            ("description", Value::String("from legacy".to_owned())),
        ]);
        let normalized = normalize_description_input(&data).expect("normalizes");
        assert!(normalized.from_markdown);
        assert_eq!(
            normalized.data.get("description_html"),
            Some(&Value::String("<h1>From markdown</h1>".to_owned()))
        );
        assert!(!normalized.data.contains_key("description_markdown"));
        assert!(!normalized.data.contains_key("description"));

        let data = json_map(&[
            (
                "description_html",
                Value::String("<p>from html</p>".to_owned()),
            ),
            ("description", Value::String("legacy".to_owned())),
        ]);
        let normalized = normalize_description_input(&data).expect("normalizes");
        assert!(!normalized.from_markdown);
        assert_eq!(
            normalized.data.get("description_html"),
            Some(&Value::String("<p>from html</p>".to_owned()))
        );

        let data = json_map(&[("description", Value::String("# Legacy".to_owned()))]);
        let normalized = normalize_description_input(&data).expect("normalizes");
        assert!(normalized.from_markdown);
        assert_eq!(
            normalized.data.get("description_html"),
            Some(&Value::String("<h1>Legacy</h1>".to_owned()))
        );

        let data = json_map(&[("name", Value::String("x".to_owned()))]);
        let normalized = normalize_description_input(&data).expect("normalizes");
        assert!(!normalized.from_markdown);
        assert_eq!(normalized.data.len(), 1);
    }

    #[test]
    fn normalize_rejects_non_string_markdown() {
        let data = json_map(&[("description_markdown", Value::from(7))]);
        let body = match normalize_description_input(&data) {
            Err(fail) => fail.body(),
            Ok(_) => panic!("expected failure"),
        };
        assert_eq!(
            body,
            Value::Object(json_map(&[(
                "description_markdown",
                Value::String("Must be a string.".to_owned())
            )]))
        );
    }

    #[test]
    fn description_gate_skips_lxml_for_markdown_html() {
        // Markdown-born HTML skips the round-trip (task checkboxes
        // would not survive it); raw HTML goes through lxml.
        let task = "<ul><li><p>x</p></li></ul>";
        assert!(check_description_html(task, true).is_ok());
        assert_eq!(
            check_description_html("<p>x</p>", false).expect("clean"),
            "<p>x</p>"
        );
        assert!(matches!(
            check_description_html("", false),
            Err(DescriptionFail::InvalidHtml)
        ));
    }

    // -- small pure helpers --

    #[test]
    fn falsy_json_matches_python_truthiness() {
        assert!(is_falsy_json(&Value::Null));
        assert!(is_falsy_json(&Value::Bool(false)));
        assert!(is_falsy_json(&Value::String(String::new())));
        assert!(is_falsy_json(&Value::from(0)));
        assert!(is_falsy_json(&Value::from(0.0)));
        assert!(is_falsy_json(&Value::Array(vec![])));
        assert!(!is_falsy_json(&Value::Bool(true)));
        assert!(!is_falsy_json(&Value::String("x".to_owned())));
        assert!(!is_falsy_json(&Value::from(1)));
    }

    #[test]
    fn create_name_coercion() {
        // `CharField`/`TextField.to_python` `str()`s every non-str
        // non-None (bools, floats with `.0`, single-quote container
        // reprs); only length/NUL fail.
        assert_eq!(
            coerce_create_name(&Value::String("x".to_owned())).expect("name"),
            "x"
        );
        assert_eq!(coerce_create_name(&Value::from(7)).expect("name"), "7");
        assert_eq!(
            coerce_create_name(&Value::Bool(true)).expect("name"),
            "True"
        );
        assert_eq!(
            coerce_create_name(&Value::Bool(false)).expect("name"),
            "False"
        );
        assert_eq!(coerce_create_name(&Value::from(5.0)).expect("name"), "5.0");
        assert_eq!(
            coerce_create_name(&Value::from(vec![Value::from(1), Value::from(2)])).expect("name"),
            "[1, 2]"
        );
        assert!(coerce_create_name(&Value::String("x".repeat(256))).is_err());
        assert!(coerce_create_name(&Value::String("x\x00y".to_owned())).is_err());
        assert_eq!(coerce_create_html(None).expect("default"), "<p></p>");
        assert_eq!(
            coerce_create_html(Some(&Value::String("<p>x</p>".to_owned()))).expect("html"),
            "<p>x</p>"
        );
        // Non-str html 500s in save()'s strip_tags (TypeError), before
        // to_python ever runs — verified live for every JSON type.
        assert!(coerce_create_html(Some(&Value::Bool(true))).is_err());
        assert!(coerce_create_html(Some(&Value::from(7))).is_err());
        assert!(coerce_create_html(Some(&Value::from(5.0))).is_err());
        assert!(coerce_create_html(Some(&Value::Null)).is_err());
    }

    #[test]
    fn advisory_key_matches_sha_prefix() {
        // `convert_uuid_to_integer` (`utils/uuid.py:19-26`): the
        // first 8 sha256 *bytes*, signed big-endian.
        let id = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap();
        assert_eq!(advisory_key(&id), {
            use sha2::Digest;
            let digest = sha2::Sha256::digest(id.to_string().as_bytes());
            i64::from_be_bytes([
                digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6],
                digest[7],
            ])
        });
    }

    #[test]
    fn error_bag_appends_in_field_order() {
        // Repeated `field()` calls on one key accumulate; distinct
        // keys keep insertion order (DRF error shape).
        let mut errors = ErrorBag::default();
        errors.field("b", "second".to_owned());
        errors.field("a", "first".to_owned());
        errors.field("b", "third".to_owned());
        let body = errors.body();
        let map = body.as_object().expect("object");
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["b", "a"]);
        assert_eq!(
            map["b"],
            Value::Array(vec![
                Value::String("second".to_owned()),
                Value::String("third".to_owned())
            ])
        );
    }

    // -- differential fuzz hooks (file-driven, `#[ignore]`) --
    //
    // `/tmp/md_gen_fuzz.py` writes JSON-lines `{input, expected}`
    // files from live Python; each hook replays one file and reports
    // the first mismatch (empty output = full agreement):
    //
    //   cargo test -p pidash-api --lib v1_assets::intake::tests::fuzz -- --ignored --nocapture

    fn read_cases(path: &str) -> Vec<(String, String)> {
        let body = std::fs::read_to_string(path).expect("fuzz cases file");
        body.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let case: Value = serde_json::from_str(line).expect("case JSON");
                (
                    case.get("input")
                        .expect("input")
                        .as_str()
                        .expect("str")
                        .to_owned(),
                    case.get("expected")
                        .expect("expected")
                        .as_str()
                        .expect("str")
                        .to_owned(),
                )
            })
            .collect()
    }

    #[test]
    #[ignore]
    fn fuzz_markdown_against_live_python() {
        let cases = read_cases("/tmp/md_cases.txt");
        assert!(!cases.is_empty(), "no fuzz cases generated");
        let mut mismatches = 0;
        for (input, expected) in &cases {
            let actual = match markdown::to_html(input) {
                Ok(html) => html,
                Err(message) => format!("ERROR: {message}"),
            };
            if &actual != expected {
                mismatches += 1;
                if mismatches <= 5 {
                    println!(
                        "MISMATCH input={input:?}\n  expected={expected:?}\n  actual={actual:?}"
                    );
                }
            }
            // Markdown-born HTML must survive the handler's re-sanitize.
            if let Ok(html) = markdown::to_html(input) {
                if let Sanitize::Clean(cleaned) = sanitize_html(&html) {
                    if cleaned != html {
                        println!("UNSTABLE input={input:?}\n  once={html:?}\n  twice={cleaned:?}");
                        mismatches += 1;
                    }
                }
            }
        }
        println!(
            "{}/{} markdown cases agree",
            cases.len() - mismatches.min(cases.len()),
            cases.len()
        );
        assert_eq!(mismatches, 0, "differential mismatches found");
    }

    /// Canonical instant rendering (Python-`isoformat` shaped, full
    /// precision, UTC `+00:00`): nanos beyond Django's micros fail.
    fn canon_instant(instant: &chrono::DateTime<chrono::Utc>) -> String {
        let base = instant.format("%Y-%m-%dT%H:%M:%S").to_string();
        let nanos = instant.timestamp_subsec_nanos();
        if nanos == 0 {
            format!("{base}+00:00")
        } else {
            let frac = format!("{nanos:09}").trim_end_matches('0').to_owned();
            format!("{base}.{frac}+00:00")
        }
    }

    #[test]
    #[ignore]
    fn fuzz_datetime_against_live_django() {
        let cases = read_cases("/tmp/dt_cases.txt");
        assert!(!cases.is_empty(), "no fuzz cases generated");
        let timezone = utc();
        let mut mismatches = 0;
        for (input, expected) in &cases {
            // The generator encodes live `check_datetime` under UTC:
            // an ISO instant or `ERR:<message>`.
            let actual = match check_datetime(&Value::String(input.clone()), &timezone) {
                Ok(instant) => canon_instant(&instant),
                Err(FieldFail::Msg(message)) => format!("ERR:{message}"),
                Err(FieldFail::Server) => "ERR:server".to_owned(),
            };
            if &actual != expected {
                mismatches += 1;
                if mismatches <= 5 {
                    println!(
                        "MISMATCH input={input:?}\n  expected={expected:?}\n  actual={actual:?}"
                    );
                }
            }
        }
        println!(
            "{}/{} datetime cases agree",
            cases.len() - mismatches.min(cases.len()),
            cases.len()
        );
        assert_eq!(mismatches, 0, "differential mismatches found");
    }
}
